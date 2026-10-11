//! `qli analyze` — parse source files and run an analyzer, rendering findings
//! in human or jsonl form, cached by content hash.
//!
//! This is the CLI frontend over the shared engine: it builds the language and
//! analyzer registries, resolves `--lang`/`--format`/`--analyzer`, walks the
//! input paths, and drives cache → parse → analyze → format per file. All the
//! actual analysis lives in `qli-core`/`qli-lang`/`qli-analyzers`; this module
//! is orchestration and I/O.

use std::io::{self, IsTerminal, Write};
use std::path::{Path, PathBuf};

use anyhow::Context;
use clap::ArgMatches;
use ignore::WalkBuilder;
use qli_core::cache::CacheKey;
use qli_core::{LanguageId, Source};
use qli_lang::{AnalysisContext, Analyzer, Language, LanguageRegistry};
use qli_outputs::Format;

use crate::analysis_cache::AnalysisCache;
use crate::cli::FormatArg;
use crate::{exit, paths};

/// Run `qli analyze`. Returns a process exit code.
pub fn run(matches: &ArgMatches) -> anyhow::Result<u8> {
    let input_paths: Vec<PathBuf> = matches
        .get_many::<PathBuf>("paths")
        .map(|v| v.cloned().collect())
        .unwrap_or_default();
    let lang_flag = matches
        .get_one::<String>("lang")
        .map_or("auto", String::as_str);
    let analyzer_id = matches
        .get_one::<String>("analyzer")
        .map_or("todofixme", String::as_str);
    let format_flag = matches
        .get_one::<FormatArg>("format")
        .copied()
        .unwrap_or(FormatArg::Auto);
    let no_cache = matches.get_flag("no_cache");

    let languages = language_registry();
    let analyzers = qli_analyzers::builtin();
    let analyzer = analyzers.get(analyzer_id).with_context(|| {
        let known: Vec<&str> = analyzers.ids().collect();
        format!(
            "unknown analyzer `{analyzer_id}`; known analyzers: {}",
            known.join(", ")
        )
    })?;

    // A specific `--lang` overrides per-file detection; validate it once here
    // so a typo fails fast rather than silently skipping every file.
    if lang_flag != "auto" && languages.get(&LanguageId::from(lang_flag)).is_none() {
        let known: Vec<String> = languages.ids().map(ToString::to_string).collect();
        anyhow::bail!(
            "unknown language `{lang_flag}`; known languages: {}",
            known.join(", ")
        );
    }

    let format = resolve_format(format_flag);
    let cache = build_cache(no_cache);

    let mut stdout = anstream::AutoStream::auto(io::stdout().lock());
    let mut missing = false;
    for path in &input_paths {
        // A path the user named explicitly that doesn't exist is an error
        // (exit 1), not a silent skip — unlike unsupported files found *inside*
        // a directory walk, which are skipped quietly by design.
        if !path.exists() {
            eprintln!("error: path does not exist: {}", path.display());
            missing = true;
            continue;
        }
        walk(path, |file| {
            analyze_file(
                file,
                lang_flag,
                &languages,
                analyzer,
                &cache,
                format,
                &mut stdout,
            );
        });
    }
    stdout.flush().ok();

    // Enforce the cache size cap once per invocation, not per write (which
    // would restat the whole tree on every file). No-op on a disabled cache.
    cache.enforce_size_cap();

    Ok(if missing { exit::ERROR } else { exit::SUCCESS })
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

/// The languages the CLI knows: Python plus the two TypeScript grammars.
fn language_registry() -> LanguageRegistry {
    let mut registry = LanguageRegistry::new();
    registry.register(Box::new(qli_lang_python::language()));
    registry.register(Box::new(qli_lang_typescript::typescript()));
    registry.register(Box::new(qli_lang_typescript::tsx()));
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

fn build_cache(no_cache: bool) -> AnalysisCache {
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
fn walk(path: &Path, mut f: impl FnMut(&Path)) {
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

#[allow(clippy::too_many_arguments)]
fn analyze_file(
    file: &Path,
    lang_flag: &str,
    languages: &LanguageRegistry,
    analyzer: &dyn Analyzer,
    cache: &AnalysisCache,
    format: Format,
    out: &mut dyn Write,
) {
    let Some(language) = resolve_language(file, lang_flag, languages) else {
        // Unsupported extension under `auto`: quietly skip, but note it under
        // `-v` so a surprised user can see why a file produced nothing.
        tracing::debug!(path = %file.display(), "skipping: no language adapter");
        return;
    };

    let content = match std::fs::read(file) {
        Ok(c) => c,
        Err(err) => {
            eprintln!("warning: could not read {}: {err}", file.display());
            return;
        }
    };

    let source = Source::new(file.to_path_buf(), content, language.id());
    let key = CacheKey::new(
        source.language.as_str(),
        &source.content,
        analyzer.id(),
        analyzer.version(),
    );

    let result = if let Some(hit) = cache.get(&key) {
        tracing::debug!(path = %file.display(), analyzer = analyzer.id(), "cache hit");
        hit
    } else {
        let Some(tree) = language.parse(&source) else {
            eprintln!("warning: could not parse {}", file.display());
            return;
        };
        let ctx = AnalysisContext::new(language, &source, &tree);
        let result = analyzer.analyze(&ctx);
        if let Err(err) = cache.put(&key, &result) {
            tracing::debug!(path = %file.display(), "cache write failed: {err}");
        }
        tracing::debug!(path = %file.display(), analyzer = analyzer.id(), "cache miss");
        result
    };

    let file_label = file.display().to_string();
    if let Err(err) = format.write(out, &file_label, &result) {
        eprintln!("warning: failed to write output for {file_label}: {err}");
    }
}

/// Pick the language adapter for a file: the `--lang` override if given,
/// otherwise the file extension.
fn resolve_language<'a>(
    file: &Path,
    lang_flag: &str,
    languages: &'a LanguageRegistry,
) -> Option<&'a dyn Language> {
    if lang_flag != "auto" {
        return languages.get(&LanguageId::from(lang_flag));
    }
    let ext = file.extension()?.to_str()?.to_ascii_lowercase();
    languages.by_extension(&ext)
}
