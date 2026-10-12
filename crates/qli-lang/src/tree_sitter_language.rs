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
    /// Marker files gating extension detection; see
    /// [`Language::project_markers`]. Empty unless set via
    /// [`with_project_markers`](Self::with_project_markers).
    project_markers: &'static [&'static str],
    /// See [`Language::fingerprint`]; computed once in [`new`](Self::new).
    fingerprint: String,
}

impl TreeSitterLanguage {
    #[must_use]
    pub fn new(
        id: impl Into<LanguageId>,
        extensions: &'static [&'static str],
        language: tree_sitter::Language,
        queries: &'static [(&'static str, &'static str)],
    ) -> Self {
        // The crate version stands in for the grammar's own version, which
        // tree-sitter doesn't expose uniformly: every release re-keys the
        // cache. The ABI and node-kind count catch a grammar swap in between.
        let abi = language.abi_version().to_le_bytes();
        let node_kinds = language.node_kind_count().to_le_bytes();
        let mut parts: Vec<&[u8]> = vec![env!("CARGO_PKG_VERSION").as_bytes(), &abi, &node_kinds];
        for (name, query) in queries {
            parts.push(name.as_bytes());
            parts.push(query.as_bytes());
        }
        let fingerprint = qli_core::cache::fingerprint(&parts);
        Self {
            id: id.into(),
            extensions,
            language,
            queries,
            project_markers: &[],
            fingerprint,
        }
    }

    /// Require one of `markers` in an ancestor directory before `--lang auto`
    /// treats a file with this adapter's extensions as this language.
    #[must_use]
    pub fn with_project_markers(mut self, markers: &'static [&'static str]) -> Self {
        self.project_markers = markers;
        self
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
            .field("project_markers", &self.project_markers)
            .field("fingerprint", &self.fingerprint)
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

    fn project_markers(&self) -> &'static [&'static str] {
        self.project_markers
    }

    fn ts_language(&self) -> tree_sitter::Language {
        self.language.clone()
    }

    fn fingerprint(&self) -> &str {
        &self.fingerprint
    }

    fn named_query(&self, name: &str) -> Option<&'static str> {
        self.queries
            .iter()
            .find(|(n, _)| *n == name)
            .map(|(_, q)| *q)
    }
}
