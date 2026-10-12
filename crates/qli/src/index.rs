//! `qli index` — walk a project root, run the `DefRefs` analyzer over every
//! supported file, and write one SCIP index for the whole project.
//!
//! Like `qli analyze`, this module is orchestration and I/O only, and shares
//! that command's walk, language detection, and content-hashed cache (so a
//! re-index only re-analyzes files whose bytes changed). Everything after the
//! per-file results — cross-file resolution, the symbol scheme, the protobuf
//! conversion — lives in `qli-core`/`qli-scip`. Resolution runs over the cached
//! results on every invocation; it is never written back into them.

use std::path::{Component, Path, PathBuf};

use anyhow::Context;
use clap::ArgMatches;
use qli_analyzers::DefRefs;
use qli_core::{AnalysisResult, LanguageId};
use qli_scip::{IndexDocument, IndexStats, Project};

use crate::analyze::{
    analyze_source, build_cache, language_registry, resolve_language, walk, DEFAULT_LANG,
};
use crate::exit;

/// One indexed file: what `qli_scip::IndexDocument` borrows from.
struct IndexedFile {
    relative_path: String,
    language: LanguageId,
    result: AnalysisResult,
}

/// `--output` when the caller names none, relative to the working directory.
pub const DEFAULT_OUTPUT: &str = "index.scip";

/// What to index and where to write it: the inputs every frontend of the index
/// engine (`qli index`, the MCP `qli_index` tool) resolves from its own
/// arguments.
#[derive(Clone, Debug)]
pub struct IndexRequest<'a> {
    /// Project root. Walked recursively, respecting `.gitignore`.
    pub root: &'a Path,
    /// File the index is written to.
    pub output: &'a Path,
    /// Language ids to index. Empty: every supported language.
    pub languages: &'a [String],
    /// The `qli` command line this request corresponds to — provenance
    /// recorded in the index, never read back.
    pub tool_arguments: Vec<String>,
}

/// The index [`index`] wrote.
#[derive(Clone, Copy, Debug)]
pub struct IndexSummary {
    /// Size of the written file.
    pub bytes: usize,
    pub stats: IndexStats,
}

/// Run `qli index`. Returns a process exit code.
pub fn run(matches: &ArgMatches) -> anyhow::Result<u8> {
    let output = matches
        .get_one::<PathBuf>("output")
        .expect("defaulted by clap");
    let languages: Vec<String> = matches
        .get_many::<String>("lang")
        .map(|ids| ids.cloned().collect())
        .unwrap_or_default();

    let IndexSummary { bytes, stats } = index(IndexRequest {
        root: matches
            .get_one::<PathBuf>("path")
            .expect("defaulted by clap"),
        output,
        languages: &languages,
        // Lossy on purpose: `args()` would panic on a non-UTF-8 argument, and
        // this is provenance metadata, not something read back.
        tool_arguments: std::env::args_os()
            .skip(1)
            .map(|argument| argument.to_string_lossy().into_owned())
            .collect(),
    })?;

    eprintln!(
        "indexed {} files to {}: {} bytes, {} symbols, {} references \
         ({} same-file, {} cross-file, {} ambiguous, {} unresolved)",
        stats.documents,
        output.display(),
        bytes,
        stats.symbols,
        stats.references,
        stats.same_file,
        stats.cross_file,
        stats.ambiguous,
        stats.unresolved,
    );
    Ok(exit::SUCCESS)
}

/// The index engine: run `DefRefs` over every supported file under the
/// requested root, resolve references across files, and write one SCIP index.
///
/// Fails on an unknown language id, a root that is not a directory, or an
/// output file that cannot be written. Unreadable and unparseable files are
/// skipped with a `warning:` on stderr.
pub fn index(request: IndexRequest<'_>) -> anyhow::Result<IndexSummary> {
    let IndexRequest {
        root,
        output,
        languages: lang_ids,
        tool_arguments,
    } = request;
    let lang_filter: Vec<LanguageId> = lang_ids
        .iter()
        .map(|id| LanguageId::from(id.as_str()))
        .collect();

    let languages = language_registry();
    // Validate the filter once here so a typo fails fast rather than silently
    // producing an empty index.
    for id in &lang_filter {
        if languages.get(id).is_none() {
            let mut known: Vec<String> = languages.ids().map(ToString::to_string).collect();
            known.sort();
            anyhow::bail!(
                "unknown language `{id}`; known languages: {}",
                known.join(", ")
            );
        }
    }

    anyhow::ensure!(
        root.is_dir(),
        "project root is not a directory: {}",
        root.display()
    );
    let absolute_root = root
        .canonicalize()
        .with_context(|| format!("could not resolve project root {}", root.display()))?;
    let root_uri = qli_scip::file_uri(&slash_path(&absolute_root).with_context(|| {
        format!(
            "project root is not valid UTF-8: {}",
            absolute_root.display()
        )
    })?);

    let analyzer = DefRefs::new();
    let cache = build_cache(false);
    let mut files: Vec<IndexedFile> = Vec::new();
    walk(root, |file| {
        let Some(language) = resolve_language(file, DEFAULT_LANG, &languages) else {
            tracing::debug!(path = %file.display(), "skipping: no language adapter");
            return;
        };
        let language_id = language.id();
        if !lang_filter.is_empty() && !lang_filter.contains(&language_id) {
            tracing::debug!(path = %file.display(), "skipping: excluded by --lang");
            return;
        }
        // SCIP document paths are UTF-8 strings relative to the project root.
        let Some(relative_path) = file.strip_prefix(root).ok().and_then(slash_path) else {
            eprintln!(
                "warning: skipping {}: path is not valid UTF-8",
                file.display()
            );
            return;
        };
        let Some(result) = analyze_source(file, language, &analyzer, &cache) else {
            return;
        };
        files.push(IndexedFile {
            relative_path,
            language: language_id,
            result,
        });
    });
    // Enforce the cache size cap once per invocation, as `qli analyze` does.
    cache.enforce_size_cap();

    let documents: Vec<IndexDocument<'_>> = files
        .iter()
        .map(|file| IndexDocument {
            relative_path: &file.relative_path,
            language: &file.language,
            result: &file.result,
        })
        .collect();
    let project = Project {
        root_uri,
        tool_version: env!("CARGO_PKG_VERSION").to_owned(),
        tool_arguments,
        ..Default::default()
    };
    let (index, stats) = qli_scip::build_index(&project, &documents);
    let bytes = qli_scip::encode(&index);
    std::fs::write(output, &bytes)
        .with_context(|| format!("could not write index to {}", output.display()))?;

    Ok(IndexSummary {
        bytes: bytes.len(),
        stats,
    })
}

/// `path` with `/` between its components (the host separator is `\` on
/// Windows). `None` if a component is not valid UTF-8.
fn slash_path(path: &Path) -> Option<String> {
    let mut out = String::new();
    for component in path.components() {
        match component {
            Component::RootDir => out.push('/'),
            Component::CurDir => {}
            other => {
                if !out.is_empty() && !out.ends_with('/') {
                    out.push('/');
                }
                out.push_str(other.as_os_str().to_str()?);
            }
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slash_path_joins_components_with_forward_slashes() {
        assert_eq!(
            slash_path(Path::new("a/b/c.py")).as_deref(),
            Some("a/b/c.py")
        );
        assert_eq!(slash_path(Path::new("./a.py")).as_deref(), Some("a.py"));
        assert_eq!(
            slash_path(Path::new("/root/a.py")).as_deref(),
            Some("/root/a.py")
        );
    }
}
