//! Binary-level tests of `qli mcp`: the real process, driven over stdio by
//! `rmcp`'s client.
//!
//! What only the binary can get wrong, and so what is pinned here:
//!
//!   - the tool surface: exactly `qli_analyze`, `qli_index`, `qli_ext_list`,
//!     with the input fields each requires and an output schema;
//!   - each tool answers with **the same data the CLI command prints** — the
//!     expectations are read from `qli analyze` / `qli index` / `qli ext list`
//!     run against the same input, not restated here;
//!   - a request the engine rejects is a tool error carrying the CLI's message,
//!     and the session keeps serving afterwards;
//!   - stdout is MCP frames and **nothing else**, even when startup warns.
//!
//! Hermetic: every test runs in an `XdgSandbox`; there is no network.

#![cfg(unix)]

mod common;

use std::path::{Path, PathBuf};
use std::process::{Output, Stdio};
use std::time::Duration;

use common::XdgSandbox;
use rmcp::model::{CallToolRequestParams, CallToolResult, Tool};
use rmcp::service::{RoleClient, RunningService};
use rmcp::ServiceExt;
use serde_json::{json, Value};
use serial_test::serial;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::{Child, Command};
use tokio::task::JoinHandle;

/// Upper bound on a whole session. Generous: it only matters on failure.
const TIMEOUT: Duration = Duration::from_secs(60);

const SAMPLE: &str = "tests/fixtures/python/sample.py";
const MULTIFILE: &str = "tests/fixtures/multifile";

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root is two levels up from crates/qli")
        .to_path_buf()
}

/// Run a `qli` CLI command from the workspace root — the reference every tool
/// result is compared against.
fn qli(args: &[&str]) -> Output {
    std::process::Command::new(assert_cmd::cargo::cargo_bin("qli"))
        .args(args)
        .current_dir(workspace_root())
        .env_remove("RUST_LOG")
        .env("NO_COLOR", "1")
        .output()
        .expect("run qli")
}

fn stderr_of(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

/// Stage a group directory whose name shadows a built-in, so startup prints a
/// `warning: group … shadows a built-in` line — the kind of output that must
/// reach stderr and never stdout.
fn stage_warning_trigger(sandbox: &XdgSandbox) {
    std::fs::create_dir_all(sandbox.extensions_dir().join("analyze")).expect("create group dir");
}

/// A running `qli mcp` child with an `rmcp` client attached.
///
/// The client does not read the child's stdout directly: a pump task copies
/// every byte the child writes into `stdout` and forwards it to the client, so
/// the test can inspect the raw stream afterwards.
struct Session {
    client: RunningService<RoleClient, ()>,
    child: Child,
    stdout: JoinHandle<Vec<u8>>,
    stderr: JoinHandle<Vec<u8>>,
}

/// Everything the child wrote, once it has exited.
struct Transcript {
    stdout: Vec<u8>,
    stderr: String,
}

impl Session {
    async fn start() -> Self {
        let mut child = Command::new(assert_cmd::cargo::cargo_bin("qli"))
            .arg("mcp")
            .current_dir(workspace_root())
            .env_remove("RUST_LOG")
            .env("NO_COLOR", "1")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .expect("spawn qli mcp");
        let stdin = child.stdin.take().expect("piped stdin");
        let mut child_stdout = child.stdout.take().expect("piped stdout");
        let mut child_stderr = child.stderr.take().expect("piped stderr");

        let (mut to_client, from_server) = tokio::io::duplex(64 * 1024);
        let stdout = tokio::spawn(async move {
            let mut seen = Vec::new();
            let mut chunk = [0_u8; 8192];
            loop {
                let read = child_stdout.read(&mut chunk).await.expect("read stdout");
                if read == 0 {
                    return seen;
                }
                seen.extend_from_slice(&chunk[..read]);
                // The client hanging up first is fine: keep recording.
                let _ = to_client.write_all(&chunk[..read]).await;
            }
        });
        let stderr = tokio::spawn(async move {
            let mut seen = Vec::new();
            child_stderr
                .read_to_end(&mut seen)
                .await
                .expect("read stderr");
            seen
        });

        let client = ().serve((from_server, stdin)).await.expect("MCP initialize handshake");
        Self {
            client,
            child,
            stdout,
            stderr,
        }
    }

    async fn call(&self, tool: &'static str, arguments: Value) -> CallToolResult {
        let Value::Object(arguments) = arguments else {
            panic!("tool arguments must be a JSON object");
        };
        self.client
            .call_tool(CallToolRequestParams::new(tool).with_arguments(arguments))
            .await
            .unwrap_or_else(|err| panic!("tools/call {tool} failed at the protocol level: {err}"))
    }

    /// Close the client's end (which closes the child's stdin), and wait for
    /// the child to exit on its own.
    async fn finish(self) -> Transcript {
        let Self {
            mut client,
            mut child,
            stdout,
            stderr,
        } = self;
        client.close().await.expect("close the MCP client");
        drop(client);
        let status = child.wait().await.expect("wait for qli mcp");
        let transcript = Transcript {
            stdout: stdout.await.expect("stdout pump"),
            stderr: String::from_utf8_lossy(&stderr.await.expect("stderr pump")).into_owned(),
        };
        assert!(
            status.success(),
            "qli mcp exited with {status} after stdin closed\nstderr:\n{}",
            transcript.stderr
        );
        transcript
    }
}

/// Run `body` against a fresh session inside a sandbox, bounded by [`TIMEOUT`].
fn with_session<F>(sandbox_setup: impl FnOnce(&XdgSandbox), body: F) -> Transcript
where
    F: AsyncFnOnce(&Session),
{
    let sandbox = XdgSandbox::new();
    sandbox_setup(&sandbox);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("build test runtime");
    runtime.block_on(async {
        tokio::time::timeout(TIMEOUT, async {
            let session = Session::start().await;
            body(&session).await;
            session.finish().await
        })
        .await
        .expect("MCP session timed out")
    })
}

/// The text of an error result.
fn error_text(result: &CallToolResult) -> &str {
    assert_eq!(
        result.is_error,
        Some(true),
        "expected a tool error: {result:?}"
    );
    assert_eq!(
        result.structured_content, None,
        "a tool error carries no structured content"
    );
    let [content] = result.content.as_slice() else {
        panic!("expected one content block: {result:?}");
    };
    &content.as_text().expect("text content").text
}

/// The structured content of a successful result.
fn structured(result: &CallToolResult) -> &Value {
    assert_eq!(result.is_error, Some(false), "expected success: {result:?}");
    result
        .structured_content
        .as_ref()
        .expect("structured content")
}

fn required_fields(tool: &Tool) -> Vec<&str> {
    tool.input_schema
        .get("required")
        .and_then(Value::as_array)
        .map(|fields| fields.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default()
}

#[test]
#[serial]
fn lists_exactly_the_three_tools_with_their_schemas() {
    with_session(
        |_| {},
        async |session| {
            let server = session.client.peer_info().expect("server info");
            let implementation = server.server_info.as_ref().expect("serverInfo");
            assert_eq!(implementation.name, "qli");
            assert_eq!(implementation.version, env!("CARGO_PKG_VERSION"));

            let mut tools = session.client.list_all_tools().await.expect("tools/list");
            tools.sort_by(|a, b| a.name.cmp(&b.name));
            let names: Vec<&str> = tools.iter().map(|tool| tool.name.as_ref()).collect();
            assert_eq!(names, ["qli_analyze", "qli_ext_list", "qli_index"]);

            let [analyze, ext_list, index] = tools.as_slice() else {
                unreachable!("three tools asserted above");
            };
            assert_eq!(required_fields(analyze), ["paths"]);
            assert_eq!(required_fields(ext_list), [] as [&str; 0]);
            assert_eq!(required_fields(index), ["path"]);

            let properties = |tool: &Tool| -> Vec<String> {
                let mut names: Vec<String> = tool
                    .input_schema
                    .get("properties")
                    .and_then(Value::as_object)
                    .map(|properties| properties.keys().cloned().collect())
                    .unwrap_or_default();
                names.sort();
                names
            };
            assert_eq!(properties(analyze), ["analyzer", "lang", "paths"]);
            assert_eq!(properties(ext_list), [] as [&str; 0]);
            assert_eq!(properties(index), ["lang", "output", "path"]);

            for tool in &tools {
                assert!(
                    tool.description.as_deref().is_some_and(|d| !d.is_empty()),
                    "{} has no description",
                    tool.name
                );
                let output = tool.output_schema.as_ref().unwrap_or_else(|| {
                    panic!("{} declares no output schema", tool.name);
                });
                assert_eq!(output.get("type"), Some(&json!("object")), "{}", tool.name);
                // schemars' Rust-only integer formats make Claude Code log an
                // "unknown format" warning per field.
                for schema in [
                    serde_json::to_string(&*tool.input_schema),
                    serde_json::to_string(&**output),
                ] {
                    let schema = schema.expect("schema serializes");
                    assert!(!schema.contains("\"uint"), "{}: {schema}", tool.name);
                }
            }
        },
    );
}

#[test]
#[serial]
fn analyze_returns_the_records_the_cli_prints() {
    with_session(
        |_| {},
        async |session| {
            let result = session
                .call("qli_analyze", json!({ "paths": [SAMPLE] }))
                .await;
            let records = structured(&result)["records"]
                .as_array()
                .expect("records array");

            // Same records, same order, same shapes as the jsonl stream.
            let cli = qli(&["analyze", "--format", "jsonl", SAMPLE]);
            assert!(cli.status.success(), "{}", stderr_of(&cli));
            let expected: Vec<Value> = String::from_utf8(cli.stdout)
                .unwrap()
                .lines()
                .map(|line| serde_json::from_str(line).expect("jsonl record"))
                .collect();
            assert_eq!(records, &expected);

            // And they are the fixture's four comment markers.
            let found: Vec<(&str, u64, &str)> = records
                .iter()
                .map(|record| {
                    assert_eq!(record["type"], "diagnostic");
                    assert_eq!(record["file"], SAMPLE);
                    (
                        record["code"].as_str().unwrap(),
                        record["range"]["start"]["line"].as_u64().unwrap(),
                        record["message"].as_str().unwrap(),
                    )
                })
                .collect();
            assert_eq!(
                found,
                [
                    ("todo", 0, "TODO: wire up the real client"),
                    ("fixme", 4, "FIXME: this default is a guess"),
                    ("xxx", 9, "XXX: revisit the off-by-one below"),
                    ("hack", 14, "HACK: temporary shim until the API stabilizes"),
                ]
            );

            // The optional parameters reach the engine: `defrefs` reports
            // definitions and references instead.
            let result = session
                .call(
                    "qli_analyze",
                    json!({ "paths": [SAMPLE], "lang": "python", "analyzer": "defrefs" }),
                )
                .await;
            let records = structured(&result)["records"]
                .as_array()
                .expect("records array");
            let cli = qli(&[
                "analyze",
                "--format",
                "jsonl",
                "--lang",
                "python",
                "--analyzer",
                "defrefs",
                SAMPLE,
            ]);
            let expected: Vec<Value> = String::from_utf8(cli.stdout)
                .unwrap()
                .lines()
                .map(|line| serde_json::from_str(line).expect("jsonl record"))
                .collect();
            assert!(!expected.is_empty(), "defrefs finds symbols in the fixture");
            assert_eq!(records, &expected);
        },
    );
}

#[test]
#[serial]
fn index_writes_the_file_and_reports_the_counts_the_cli_prints() {
    let out_dir = tempfile::tempdir().unwrap();
    let output = out_dir.path().join("mcp.scip");
    let cli_output = out_dir.path().join("cli.scip");
    with_session(
        |_| {},
        async |session| {
            let result = session
                .call(
                    "qli_index",
                    json!({ "path": MULTIFILE, "output": output.to_str().unwrap() }),
                )
                .await;
            let summary = structured(&result);

            assert_eq!(summary["path"], output.to_str().unwrap());
            let bytes = std::fs::read(&output).expect("index file exists");
            assert_eq!(summary["bytes"], bytes.len());
            let index = qli_scip::decode(&bytes).expect("valid SCIP index");
            assert_eq!(summary["files"], index.documents.len());
            assert_eq!(summary["files"], 7);
            // Provenance names the equivalent CLI command, not `mcp`.
            assert_eq!(
                index.metadata.tool_info.arguments,
                ["index", MULTIFILE, "--output", output.to_str().unwrap()]
            );

            // The counts are the ones `qli index` prints for the same project.
            let cli = qli(&["index", MULTIFILE, "--output", cli_output.to_str().unwrap()]);
            assert!(cli.status.success(), "{}", stderr_of(&cli));
            let cli_summary = stderr_of(&cli);
            let files = format!("indexed {} files to ", summary["files"]);
            let counts = format!(
                " bytes, {} symbols, {} references \
                 ({} same-file, {} cross-file, {} ambiguous, {} unresolved)\n",
                summary["symbols"],
                summary["references"],
                summary["same_file"],
                summary["cross_file"],
                summary["ambiguous"],
                summary["unresolved"],
            );
            assert!(
                cli_summary.starts_with(&files) && cli_summary.ends_with(&counts),
                "MCP summary {summary} disagrees with the CLI's: {cli_summary}"
            );
            assert!(summary["symbols"].as_u64().unwrap() > 0, "{summary}");
            assert!(summary["cross_file"].as_u64().unwrap() > 0, "{summary}");

            // `lang` narrows the index, as `--lang` does.
            let result = session
                .call(
                    "qli_index",
                    json!({
                        "path": MULTIFILE,
                        "output": output.to_str().unwrap(),
                        "lang": ["python"],
                    }),
                )
                .await;
            let python_only = structured(&result);
            let index = qli_scip::decode(&std::fs::read(&output).unwrap()).unwrap();
            assert_eq!(python_only["files"], index.documents.len());
            assert!(
                python_only["files"].as_u64() < summary["files"].as_u64(),
                "{python_only}"
            );
            assert!(index
                .documents
                .iter()
                .all(|document| document.relative_path.starts_with("python/")));
        },
    );
}

#[test]
#[serial]
fn ext_list_returns_the_extensions_the_cli_lists() {
    with_session(
        |_| {},
        async |session| {
            let result = session.call("qli_ext_list", json!({})).await;
            let extensions = structured(&result)["extensions"]
                .as_array()
                .expect("extensions array");

            let cli = qli(&["ext", "list", "--json"]);
            assert!(cli.status.success(), "{}", stderr_of(&cli));
            let expected: Value = serde_json::from_slice(&cli.stdout).expect("ext list JSON");
            assert_eq!(Some(extensions), expected.as_array());

            // The defaults embedded in the binary.
            for group in ["dev", "prod", "org"] {
                assert!(
                    extensions
                        .iter()
                        .any(|extension| extension["group"] == group
                            && extension["extension"] == "hello"
                            && extension["origin"] == "embedded"
                            && extension["path"]
                                .as_str()
                                .is_some_and(|p| Path::new(p).is_file())),
                    "missing embedded `{group} hello`: {extensions:?}"
                );
            }
        },
    );
}

#[test]
#[serial]
fn engine_failures_are_tool_errors_with_the_cli_message() {
    with_session(
        |_| {},
        async |session| {
            // (tool, arguments, the CLI command that fails the same way)
            let cases: [(&str, Value, Vec<&str>); 5] = [
                (
                    "qli_analyze",
                    json!({ "paths": [SAMPLE], "analyzer": "nope" }),
                    vec!["analyze", "--analyzer", "nope", SAMPLE],
                ),
                (
                    "qli_analyze",
                    json!({ "paths": [SAMPLE], "lang": "klingon" }),
                    vec!["analyze", "--lang", "klingon", SAMPLE],
                ),
                (
                    "qli_analyze",
                    json!({ "paths": ["tests/fixtures/no-such-file.py"] }),
                    vec!["analyze", "tests/fixtures/no-such-file.py"],
                ),
                (
                    "qli_index",
                    json!({ "path": MULTIFILE, "lang": ["klingon"] }),
                    vec!["index", MULTIFILE, "--lang", "klingon"],
                ),
                (
                    "qli_index",
                    json!({ "path": SAMPLE }),
                    vec!["index", SAMPLE],
                ),
            ];
            for (tool, arguments, cli_args) in cases {
                let result = session.call(tool, arguments).await;
                let message = error_text(&result);

                let cli = qli(&cli_args);
                assert_eq!(cli.status.code(), Some(1), "{cli_args:?}");
                assert_eq!(
                    stderr_of(&cli),
                    format!("error: {message}\n"),
                    "{cli_args:?}"
                );
            }

            // Spelled out once, so the comparison above cannot pass on two
            // empty strings.
            let result = session
                .call(
                    "qli_analyze",
                    json!({ "paths": [SAMPLE], "analyzer": "nope" }),
                )
                .await;
            assert_eq!(
                error_text(&result),
                "unknown analyzer `nope`; known analyzers: todofixme, defrefs"
            );

            // Arguments that do not match the schema are a tool error too.
            let result = session
                .client
                .call_tool(CallToolRequestParams::new("qli_analyze"))
                .await
                .expect("tools/call without arguments");
            assert!(
                error_text(&result).contains("missing field `paths`"),
                "{result:?}"
            );

            // The session survived every failure.
            let result = session
                .call("qli_analyze", json!({ "paths": [SAMPLE] }))
                .await;
            assert_eq!(
                structured(&result)["records"].as_array().map(Vec::len),
                Some(4)
            );
        },
    );
}

#[test]
#[serial]
fn stdout_carries_only_protocol_frames() {
    let transcript = with_session(stage_warning_trigger, async |session| {
        session.client.list_all_tools().await.expect("tools/list");
        // A success, a failure, and a call whose engine path warns on stderr
        // (extension discovery re-runs and meets the staged directory).
        session
            .call("qli_analyze", json!({ "paths": [SAMPLE] }))
            .await;
        session
            .call(
                "qli_analyze",
                json!({ "paths": [SAMPLE], "analyzer": "nope" }),
            )
            .await;
        session.call("qli_ext_list", json!({})).await;
    });

    // Positive control: this run really did have something to say outside the
    // protocol, and said it on stderr.
    assert!(
        transcript.stderr.contains("shadows a built-in"),
        "expected the staged startup warning on stderr:\n{}",
        transcript.stderr
    );

    // MCP's stdio transport is one JSON-RPC message per line.
    let stdout = String::from_utf8(transcript.stdout).expect("stdout is UTF-8");
    assert!(stdout.ends_with('\n'), "stdout ends mid-frame: {stdout:?}");
    let frames: Vec<Value> = stdout
        .lines()
        .map(|line| {
            serde_json::from_str(line)
                .unwrap_or_else(|err| panic!("stdout line is not JSON ({err}): {line:?}"))
        })
        .collect();
    for frame in &frames {
        assert_eq!(frame["jsonrpc"], "2.0", "not a JSON-RPC frame: {frame}");
        assert!(
            frame.get("result").is_some() || frame.get("error").is_some(),
            "the server only ever answers; unexpected frame: {frame}"
        );
    }
    // initialize + tools/list + three tools/call.
    assert_eq!(frames.len(), 5, "{stdout}");
}
