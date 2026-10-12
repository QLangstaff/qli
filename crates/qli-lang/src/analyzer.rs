//! The `Analyzer` trait and the `AnalysisContext` handed to it.
//!
//! The trait lives here, not in `qli-core`, on purpose: analyzers walk
//! tree-sitter trees, so their signature names [`ParseTree`] — a type that
//! wraps `tree_sitter` and would drag that C dependency into the pure engine.
//! `qli-core` stays the I/O-free vocabulary; the tree-walking contract lives
//! one layer out, next to the grammars it needs.

use qli_core::{AnalysisResult, Source};

use crate::language::{Language, ParseTree};

/// Everything an analyzer needs about one source: the language adapter (for
/// named queries and grammar), the original bytes, and the parsed tree.
///
/// Bundling these in one context keeps the [`Analyzer::analyze`] signature
/// stable as analyzers grow, and lets a frontend parse a file once and run
/// several analyzers over the shared [`ParseTree`].
#[derive(Debug, Clone, Copy)]
pub struct AnalysisContext<'a> {
    pub language: &'a dyn Language,
    pub source: &'a Source,
    pub tree: &'a ParseTree,
}

impl<'a> AnalysisContext<'a> {
    #[must_use]
    pub fn new(language: &'a dyn Language, source: &'a Source, tree: &'a ParseTree) -> Self {
        Self {
            language,
            source,
            tree,
        }
    }

    /// The source bytes, for reading a node's text via its byte span.
    #[must_use]
    pub fn content(&self) -> &'a [u8] {
        &self.source.content
    }
}

/// A polyglot analysis pass. One implementation runs across every language
/// whose adapter provides what it needs (comment nodes, a named query, …).
///
/// `id` and `version` are the analyzer's cache identity: the cache key folds
/// in `version`, so bumping it when behavior changes invalidates stale entries
/// without touching disk (see `qli_core::cache`).
///
/// `Send + Sync` for the same reason as [`Language`]: the LSP server holds
/// the registry across async tasks. Analyzers are stateless passes.
pub trait Analyzer: Send + Sync {
    /// Stable, kebab-or-flat id used on the CLI (`--analyzer <id>`) and as the
    /// cache bucket, e.g. `"todofixme"`.
    fn id(&self) -> &'static str;

    /// Bump when the analyzer's output for the same input would change.
    fn version(&self) -> u32;

    fn analyze(&self, ctx: &AnalysisContext<'_>) -> AnalysisResult;
}

/// Parse `source` with `language` and run `analyzer` over it in one call.
///
/// Convenience for the single-analyzer path; a frontend running several
/// analyzers should [`parse`](Language::parse) once and build an
/// [`AnalysisContext`] per analyzer to share the tree. Returns `None` if the
/// grammar can't be loaded (see [`Language::parse`]).
pub fn analyze_with(
    language: &dyn Language,
    analyzer: &dyn Analyzer,
    source: &Source,
) -> Option<AnalysisResult> {
    let tree = language.parse(source)?;
    let ctx = AnalysisContext::new(language, source, &tree);
    Some(analyzer.analyze(&ctx))
}
