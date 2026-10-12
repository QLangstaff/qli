//! `qli mcp` — serve the Model Context Protocol so an AI assistant can call
//! qli's engines as tools.
//!
//! This is a frontend like `qli analyze` or `qli lsp`: each tool resolves its
//! parameters into the same typed request the CLI command builds and calls the
//! same engine function ([`analyze::analyze`], [`index::index`],
//! [`ext::records`]). The protocol — JSON-RPC framing, the `initialize`
//! handshake, tool listing, schema generation — is the `rmcp` SDK's.
//!
//! # Output streams
//!
//! stdout carries MCP frames and nothing else; logs and the engines'
//! `warning:` lines go to stderr, which MCP clients keep in a server log.
//!
//! # Tool failures
//!
//! A request the engine rejects (unknown analyzer, unknown language, missing
//! path) is a tool result with `isError: true` carrying the message the CLI
//! prints after `error: ` — the model reads it and can correct itself. It is
//! never a JSON-RPC error, and it never ends the session.

use std::path::PathBuf;

use anyhow::Context;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::service::ServerInitializeError;
use rmcp::transport::stdio;
use rmcp::{schemars, tool, tool_handler, tool_router, Json, ServerHandler, ServiceExt};
use serde::{Deserialize, Serialize};

use crate::analyze::{self, AnalyzeEvent, AnalyzeRequest};
use crate::exit;
use crate::ext::{self, ExtensionRecord};
use crate::index::{self, IndexRequest, IndexSummary};

/// Run `qli mcp`. Returns a process exit code once the client closes stdin.
pub fn run() -> anyhow::Result<u8> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .context("could not start the MCP server runtime")?;
    let served = runtime.block_on(async {
        let service = match Server.serve(stdio()).await {
            Ok(service) => service,
            // A client that hangs up before `initialize` ended the session
            // like any other closed stdin; there is nothing to report.
            Err(ServerInitializeError::ConnectionClosed(_)) => return Ok(()),
            Err(err) => {
                return Err(err).context("MCP session failed to initialize on stdio");
            }
        };
        service.waiting().await.context("MCP server task failed")?;
        anyhow::Ok(())
    });
    // tokio reads stdin on a blocking thread that can't be cancelled; a plain
    // drop would wait on it forever.
    runtime.shutdown_background();
    served?;
    Ok(exit::SUCCESS)
}

/// The MCP server. Stateless: every tool call builds what it needs, exactly as
/// one CLI invocation does.
#[derive(Clone, Copy, Debug)]
struct Server;

/// Parameters of `qli_analyze`.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
struct AnalyzeParams {
    /// Files or directories to analyze. Directories are walked recursively
    /// (respecting `.gitignore`); unsupported files are skipped. Prefer
    /// absolute paths: a relative path resolves against the server's working
    /// directory.
    paths: Vec<String>,
    /// Language id to parse every file as (for example `python`,
    /// `typescript`, `tsx`, `csharp`, `angular`). Omit, or pass `auto`, to
    /// detect the language of each file.
    lang: Option<String>,
    /// Analyzer to run: `todofixme` (TODO/FIXME/XXX/HACK markers in comments;
    /// the default) or `defrefs` (definitions and references).
    analyzer: Option<String>,
}

/// Result of `qli_analyze`.
#[derive(Debug, Serialize, schemars::JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
struct AnalyzeOutput {
    /// One record per finding, exactly as `qli analyze --format jsonl` emits
    /// them. Every record has `file` and `type`: a `diagnostic` (`range`,
    /// `severity`, `message`, `code`), a `symbol` (a definition), or a
    /// `reference`. Positions are 0-based.
    records: Vec<serde_json::Value>,
}

/// Parameters of `qli_index`.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
struct IndexParams {
    /// Project root to index. Walked recursively (respecting `.gitignore`);
    /// unsupported files are skipped. Prefer an absolute path: a relative one
    /// resolves against the server's working directory.
    path: String,
    /// File to write the SCIP index to; an existing file is overwritten.
    /// Default: `index.scip` in the server's working directory.
    output: Option<String>,
    /// Only index these language ids. Default: every supported language.
    lang: Option<Vec<String>>,
}

/// Result of `qli_index`.
/// Drop schemars' Rust-specific integer formats (`"uint"`, `"uint64"`, …).
/// They are not JSON Schema formats, and Claude Code logs an "unknown format"
/// warning per field for each one.
fn json_schema_formats_only(schema: &mut schemars::Schema) {
    use schemars::transform::{RecursiveTransform, Transform};

    RecursiveTransform(|schema: &mut schemars::Schema| {
        let rust_specific = schema
            .get("format")
            .and_then(|format| format.as_str())
            .is_some_and(|format| format.starts_with("uint"));
        if rust_specific {
            schema.remove("format");
        }
    })
    .transform(schema);
}

#[derive(Debug, Serialize, schemars::JsonSchema)]
#[schemars(crate = "rmcp::schemars", transform = json_schema_formats_only)]
struct IndexOutput {
    /// Absolute path of the SCIP index file that was written.
    path: String,
    /// Size of the index file in bytes.
    bytes: usize,
    /// Files indexed.
    files: usize,
    /// Definitions.
    symbols: usize,
    /// All references, resolved or not: the sum of the four counts below.
    references: usize,
    /// References resolved to a definition in the same file.
    same_file: usize,
    /// References resolved to a definition in another file.
    cross_file: usize,
    /// References left unresolved because several files define the name.
    ambiguous: usize,
    /// References no definition matches.
    unresolved: usize,
}

/// Result of `qli_ext_list`.
#[derive(Debug, Serialize, schemars::JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
struct ExtListOutput {
    /// Every discovered extension, ordered by group and then by name — the
    /// same entries `qli ext list --json` prints.
    extensions: Vec<ExtensionRecord>,
}

#[tool_router]
impl Server {
    #[tool(
        name = "qli_analyze",
        description = "Analyze source files with qli and return the findings as structured \
            records. Use it to find TODO/FIXME/XXX/HACK comment markers (analyzer `todofixme`, \
            the default) or to list the definitions and references in Python, TypeScript/TSX, \
            C#, and Angular template files (analyzer `defrefs`) without reading the files \
            yourself. Accepts files and directories. Read-only apart from qli's own analysis \
            cache.",
        annotations(
            title = "Analyze source files",
            read_only_hint = true,
            open_world_hint = false
        )
    )]
    async fn analyze(
        Parameters(params): Parameters<AnalyzeParams>,
    ) -> Result<Json<AnalyzeOutput>, String> {
        blocking(move || {
            let paths: Vec<PathBuf> = params.paths.iter().map(PathBuf::from).collect();
            let request = AnalyzeRequest {
                paths: &paths,
                lang: params.lang.as_deref().unwrap_or(analyze::DEFAULT_LANG),
                analyzer: params
                    .analyzer
                    .as_deref()
                    .unwrap_or(analyze::DEFAULT_ANALYZER),
                no_cache: false,
            };
            let mut records = Vec::new();
            let mut missing = Vec::new();
            analyze::analyze(&request, |event| match event {
                AnalyzeEvent::Analyzed { file, result } => {
                    let file_label = file.display().to_string();
                    records.extend(qli_outputs::jsonl::records(&file_label, result));
                }
                AnalyzeEvent::MissingPath(path) => {
                    missing.push(format!("path does not exist: {}", path.display()));
                }
            })?;
            anyhow::ensure!(missing.is_empty(), "{}", missing.join("\n"));
            Ok(AnalyzeOutput { records })
        })
        .await
    }

    #[tool(
        name = "qli_index",
        description = "Index a whole project with qli into one SCIP index file: every \
            definition and reference in its Python, TypeScript/TSX, C#, and Angular template \
            files, with references resolved across files. Use it when the user asks for a SCIP \
            index or a code-navigation index of a project. Writes the index file (overwriting \
            an existing one) and returns its path, size, and symbol and reference counts; the \
            index itself is binary and is not returned.",
        annotations(
            title = "Index a project to SCIP",
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    async fn index(
        Parameters(params): Parameters<IndexParams>,
    ) -> Result<Json<IndexOutput>, String> {
        blocking(move || {
            let root = PathBuf::from(params.path);
            let output = PathBuf::from(params.output.as_deref().unwrap_or(index::DEFAULT_OUTPUT));
            let languages = params.lang.unwrap_or_default();
            // Provenance: the `qli` command line this call is equivalent to,
            // not the server's own (`mcp`).
            let mut tool_arguments = vec![
                "index".to_owned(),
                root.display().to_string(),
                "--output".to_owned(),
                output.display().to_string(),
            ];
            if !languages.is_empty() {
                tool_arguments.extend(["--lang".to_owned(), languages.join(",")]);
            }
            let IndexSummary { bytes, stats } = index::index(IndexRequest {
                root: &root,
                output: &output,
                languages: &languages,
                tool_arguments,
            })?;
            Ok(IndexOutput {
                path: std::path::absolute(&output)
                    .unwrap_or(output)
                    .display()
                    .to_string(),
                bytes,
                files: stats.documents,
                symbols: stats.symbols,
                references: stats.references,
                same_file: stats.same_file,
                cross_file: stats.cross_file,
                ambiguous: stats.ambiguous,
                unresolved: stats.unresolved,
            })
        })
        .await
    }

    #[tool(
        name = "qli_ext_list",
        description = "List the qli extensions installed on this machine: user-defined scripts \
            grouped under `dev`, `prod`, `org`, and so on, each with its group, name, origin, \
            and resolved path. Use it when the user asks which qli extensions they have or \
            where one lives. It only lists them; it never runs one.",
        annotations(
            title = "List qli extensions",
            read_only_hint = true,
            open_world_hint = false
        )
    )]
    async fn ext_list() -> Result<Json<ExtListOutput>, String> {
        blocking(|| {
            // Discovered per call, not once at startup: the server outlives
            // any one listing, and extensions are installed while it runs.
            let discovery = ext::discover();
            for warning in &discovery.warnings {
                tracing::warn!("{warning}");
            }
            Ok(ExtListOutput {
                extensions: ext::records(&discovery),
            })
        })
        .await
    }
}

#[tool_handler(
    name = "qli",
    instructions = "qli is a polyglot code-analysis CLI. `qli_analyze` reports TODO/FIXME \
        markers or definitions and references in source files; `qli_index` writes a SCIP index \
        of a whole project; `qli_ext_list` lists the user's qli extensions. Pass absolute \
        paths: a relative path resolves against the server's working directory."
)]
impl ServerHandler for Server {}

/// Run one engine call off the protocol thread, so a long analysis never
/// stalls the session, and render its outcome as a tool result: the value as
/// structured content, or a failure as the message the CLI prints after
/// `error: `. A panic in the engine is a failed call, not a dead server.
async fn blocking<T, F>(call: F) -> Result<Json<T>, String>
where
    T: Send + 'static,
    F: FnOnce() -> anyhow::Result<T> + Send + 'static,
{
    match tokio::task::spawn_blocking(call).await {
        Ok(Ok(value)) => Ok(Json(value)),
        Ok(Err(err)) => Err(format!("{err:#}")),
        Err(err) => Err(format!("internal error: {err}")),
    }
}
