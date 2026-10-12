//! `qli analyze` — parse source files and run an analyzer, rendering findings
//! in human or jsonl form, cached by content hash.
//!
//! Two layers. [`analyze`] is the engine-facing part every frontend calls: it
//! builds the language and analyzer registries, validates the request, walks
//! the input paths, and drives cache → parse → analyze per file, reporting each
//! outcome as an [`AnalyzeEvent`]. [`run`] is the CLI frontend over it: it
//! resolves `--lang`/`--format`/`--analyzer` and renders the events to
//! stdout/stderr. All the actual analysis lives in
//! `qli-core`/`qli-lang`/`qli-analyzers`; this module is orchestration and I/O.

use std::io::{self, IsTerminal, Write};
use std::path::{Path, PathBuf};

use anyhow::Context;
use clap::ArgMatches;
use ignore::WalkBuilder;
use qli_core::cache::CacheKey;
use qli_core::{AnalysisResult, LanguageId, Source};
use qli_lang::{AnalysisContext, Analyzer, Language, LanguageRegistry};
use qli_outputs::Format;

use crate::analysis_cache::AnalysisCache;
use crate::cli::FormatArg;
use crate::{exit, paths};

/// `--lang` when the caller names none: detect per file.
pub const DEFAULT_LANG: &str = "auto";

/// `--analyzer` when the caller names none.
pub const DEFAULT_ANALYZER: &str = "todofixme";

/// What to analyze and how: the inputs every frontend of the analyze engine
/// (`qli analyze`, the MCP `qli_analyze` tool) resolves from its own arguments.
#[derive(Clone, Copy, Debug)]
pub struct AnalyzeRequest<'a> {
    /// Files or directories. Directories are walked, respecting `.gitignore`.
    pub paths: &'a [PathBuf],
    /// A language id, or [`DEFAULT_LANG`] to detect from each file.
    pub lang: &'a str,
    /// An analyzer id.
    pub analyzer: &'a str,
    /// Skip the analysis cache (no read, no write).
    pub no_cache: bool,
}

/// One thing [`analyze`] found, reported as the walk reaches it.
#[derive(Clone, Copy, Debug)]
pub enum AnalyzeEvent<'a> {
    /// `file` was analyzed.
    Analyzed {
        file: &'a Path,
        result: &'a AnalysisResult,
    },
    /// A path named in the request does not exist. The remaining paths are
    /// still analyzed.
    MissingPath(&'a Path),
}

/// Run `qli analyze`. Returns a process exit code.
pub fn run(matches: &ArgMatches) -> anyhow::Result<u8> {
    let input_paths: Vec<PathBuf> = matches
        .get_many::<PathBuf>("paths")
        .map(|v| v.cloned().collect())
        .unwrap_or_default();
    let format_flag = matches
        .get_one::<FormatArg>("format")
        .copied()
        .unwrap_or(FormatArg::Auto);
    let request = AnalyzeRequest {
        paths: &input_paths,
        lang: matches
            .get_one::<String>("lang")
            .map_or(DEFAULT_LANG, String::as_str),
        analyzer: matches
            .get_one::<String>("analyzer")
            .map_or(DEFAULT_ANALYZER, String::as_str),
        no_cache: matches.get_flag("no_cache"),
    };

    let format = resolve_format(format_flag);
    let mut stdout = anstream::AutoStream::auto(io::stdout().lock());
    let mut missing = false;
    analyze(&request, |event| match event {
        AnalyzeEvent::Analyzed { file, result } => {
            let file_label = file.display().to_string();
            if let Err(err) = format.write(&mut stdout, &file_label, result) {
                eprintln!("warning: failed to write output for {file_label}: {err}");
            }
        }
        // A path the user named explicitly that doesn't exist is an error
        // (exit 1), not a silent skip — unlike unsupported files found *inside*
        // a directory walk, which are skipped quietly by design.
        AnalyzeEvent::MissingPath(path) => {
            eprintln!("error: path does not exist: {}", path.display());
            missing = true;
        }
    })?;
    stdout.flush().ok();

    Ok(if missing { exit::ERROR } else { exit::SUCCESS })
}

/// The analyze engine: run the requested analyzer over every supported file
/// under the requested paths, handing each outcome to `on_event` as the walk
/// reaches it.
///
/// Fails, before anything is analyzed, on an unknown analyzer or language.
/// Unreadable and unparseable files are skipped with a `warning:` on stderr.
pub fn analyze(
    request: &AnalyzeRequest<'_>,
    mut on_event: impl FnMut(AnalyzeEvent<'_>),
) -> anyhow::Result<()> {
    let AnalyzeRequest {
        paths,
        lang,
        analyzer: analyzer_id,
        no_cache,
    } = *request;

    let languages = language_registry();
    let analyzers = qli_analyzers::builtin();
    let analyzer = analyzers.get(analyzer_id).with_context(|| {
        let known: Vec<&str> = analyzers.ids().collect();
        format!(
            "unknown analyzer `{analyzer_id}`; known analyzers: {}",
            known.join(", ")
        )
    })?;

    // A specific language overrides per-file detection; validate it once here
    // so a typo fails fast rather than silently skipping every file.
    if lang != DEFAULT_LANG && languages.get(&LanguageId::from(lang)).is_none() {
        // Sorted: the registry iterates in hash order, which would make the
        // message differ from one run to the next.
        let mut known: Vec<String> = languages.ids().map(ToString::to_string).collect();
        known.sort();
        anyhow::bail!(
            "unknown language `{lang}`; known languages: {}",
            known.join(", ")
        );
    }

    let cache = build_cache(no_cache);
    for path in paths {
        if !path.exists() {
            on_event(AnalyzeEvent::MissingPath(path));
            continue;
        }
        walk(path, |file| {
            let Some(language) = resolve_language(file, lang, &languages) else {
                // Unsupported extension under `auto`: quietly skip, but note it
                // under `-v` so a surprised user can see why a file produced
                // nothing.
                tracing::debug!(path = %file.display(), "skipping: no language adapter");
                return;
            };
            if let Some(result) = analyze_source(file, language, analyzer, &cache) {
                on_event(AnalyzeEvent::Analyzed {
                    file,
                    result: &result,
                });
            }
        });
    }

    // Enforce the cache size cap once per invocation, not per write (which
    // would restat the whole tree on every file). No-op on a disabled cache.
    cache.enforce_size_cap();
    Ok(())
}

/// `qli ext cache clean` — evict cached analysis results.
///
/// Returns `Result` for a uniform dispatch signature with its sibling
/// handlers; cleaning is best-effort and never itself errors.
#[allow(clippy::unnecessary_wraps)]
pub fn cache_clean(matches: &ArgMatches) -> anyhow::Result<u8> {
    let all = matches.get_flag("all");
    let older_than = matches
        .get_one::<u64>("older_than")
        .map(|days| std::time::Duration::from_secs(days * 86_400));
    let cache = build_cache(false);
    let (files, bytes) = cache.clean(all, older_than);
    eprintln!("cleaned analysis cache: removed {files} entries, reclaimed {bytes} bytes");
    Ok(exit::SUCCESS)
}

/// The languages the CLI knows.
pub fn language_registry() -> LanguageRegistry {
    let mut registry = LanguageRegistry::new();
    registry.register(Box::new(qli_lang_python::language()));
    registry.register(Box::new(qli_lang_typescript::typescript()));
    registry.register(Box::new(qli_lang_typescript::tsx()));
    registry.register(Box::new(qli_lang_csharp::language()));
    registry.register(Box::new(qli_lang_angular::language()));
    registry
}

fn resolve_format(flag: FormatArg) -> Format {
    match flag {
        FormatArg::Human => Format::Human,
        FormatArg::Jsonl => Format::Jsonl,
        FormatArg::Auto => {
            if io::stdout().is_terminal() {
                Format::Human
            } else {
                Format::Jsonl
            }
        }
    }
}

pub fn build_cache(no_cache: bool) -> AnalysisCache {
    // A cache dir we can't resolve just disables caching for the run rather
    // than failing analysis.
    let root = paths::cache_dir()
        .map(|c| c.join("analysis"))
        .unwrap_or_default();
    let enabled = !no_cache && !root.as_os_str().is_empty();
    AnalysisCache::new(root, enabled)
}

/// Invoke `f` on `path` if it's a file, or on each walked file if it's a
/// directory. Directory walks respect `.gitignore` (via the `ignore` crate).
pub(crate) fn walk(path: &Path, mut f: impl FnMut(&Path)) {
    if path.is_dir() {
        for entry in WalkBuilder::new(path).build() {
            match entry {
                Ok(entry) if entry.file_type().is_some_and(|t| t.is_file()) => f(entry.path()),
                Ok(_) => {}
                Err(err) => eprintln!("warning: {err}"),
            }
        }
    } else {
        f(path);
    }
}

/// Read `file` and produce `analyzer`'s result for it: from the cache when the
/// content was analyzed before, otherwise by parsing and analyzing (and caching
/// the result). `None` — after a `warning:` on stderr — when the file cannot be
/// read or parsed. Shared by every command that analyzes files.
pub(crate) fn analyze_source(
    file: &Path,
    language: &dyn Language,
    analyzer: &dyn Analyzer,
    cache: &AnalysisCache,
) -> Option<AnalysisResult> {
    let content = match std::fs::read(file) {
        Ok(c) => c,
        Err(err) => {
            eprintln!("warning: could not read {}: {err}", file.display());
            return None;
        }
    };

    let source = Source::new(file.to_path_buf(), content, language.id());
    let key = CacheKey::new(
        &source,
        language.fingerprint(),
        analyzer.id(),
        analyzer.version(),
    );

    if let Some(hit) = cache.get(&key) {
        tracing::debug!(path = %file.display(), analyzer = analyzer.id(), "cache hit");
        return Some(hit);
    }
    let Some(tree) = language.parse(&source) else {
        eprintln!("warning: could not parse {}", file.display());
        return None;
    };
    let ctx = AnalysisContext::new(language, &source, &tree);
    let result = analyzer.analyze(&ctx);
    if let Err(err) = cache.put(&key, &result) {
        tracing::debug!(path = %file.display(), "cache write failed: {err}");
    }
    tracing::debug!(path = %file.display(), analyzer = analyzer.id(), "cache miss");
    Some(result)
}

/// Pick the language adapter for a file: the `--lang` override if given,
/// otherwise the file extension — gated, for an adapter that declares
/// [`project_markers`](Language::project_markers), on the file living inside
/// such a project.
pub(crate) fn resolve_language<'a>(
    file: &Path,
    lang_flag: &str,
    languages: &'a LanguageRegistry,
) -> Option<&'a dyn Language> {
    if lang_flag != DEFAULT_LANG {
        return languages.get(&LanguageId::from(lang_flag));
    }
    languages.for_path(file)
}
