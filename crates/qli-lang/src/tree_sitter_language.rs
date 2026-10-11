//! `TreeSitterLanguage` — a ready-made [`Language`] implementation that per-language
//! crates configure rather than hand-writing the trait.

use qli_core::LanguageId;

use crate::language::Language;

/// A [`Language`] built by supplying a grammar, id, extensions, and named
/// queries. Per-language crates construct one of these instead of implementing
/// [`Language`] from scratch, so the only per-language code that varies is
/// data: the grammar handle and the query strings.
#[derive(Clone)]
pub struct TreeSitterLanguage {
    id: LanguageId,
    extensions: &'static [&'static str],
    language: tree_sitter::Language,
    /// `(name, query-source)` pairs, e.g. `[("defrefs", "(function… ")]`.
    queries: &'static [(&'static str, &'static str)],
}

impl TreeSitterLanguage {
    #[must_use]
    pub fn new(
        id: impl Into<LanguageId>,
        extensions: &'static [&'static str],
        language: tree_sitter::Language,
        queries: &'static [(&'static str, &'static str)],
    ) -> Self {
        Self {
            id: id.into(),
            extensions,
            language,
            queries,
        }
    }
}

impl std::fmt::Debug for TreeSitterLanguage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // `tree_sitter::Language` isn't Debug-friendly, so it's summarized by
        // ABI rather than shown directly; queries are summarized by name.
        let query_names: Vec<&str> = self.queries.iter().map(|(name, _)| *name).collect();
        f.debug_struct("TreeSitterLanguage")
            .field("id", &self.id)
            .field("extensions", &self.extensions)
            .field("grammar_abi", &self.language.abi_version())
            .field("queries", &query_names)
            .finish()
    }
}

impl Language for TreeSitterLanguage {
    fn id(&self) -> LanguageId {
        self.id.clone()
    }

    fn extensions(&self) -> &'static [&'static str] {
        self.extensions
    }

    fn ts_language(&self) -> tree_sitter::Language {
        self.language.clone()
    }

    fn named_query(&self, name: &str) -> Option<&'static str> {
        self.queries
            .iter()
            .find(|(n, _)| *n == name)
            .map(|(_, q)| *q)
    }
}
