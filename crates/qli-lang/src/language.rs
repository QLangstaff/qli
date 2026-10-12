//! The `Language` adapter trait and the `ParseTree` it produces.

use qli_core::Source;

/// A parsed source file: the tree-sitter syntax tree.
///
/// Analyzers walk this together with the original [`Source`] bytes (node spans
/// are byte offsets into that content). The wrapper keeps the concrete
/// `tree_sitter` type out of analyzer call sites that only need the root node,
/// while still exposing [`inner`](ParseTree::inner) for query-based analyzers.
#[derive(Debug)]
pub struct ParseTree {
    tree: tree_sitter::Tree,
}

impl ParseTree {
    #[must_use]
    pub fn new(tree: tree_sitter::Tree) -> Self {
        Self { tree }
    }

    #[must_use]
    pub fn root_node(&self) -> tree_sitter::Node<'_> {
        self.tree.root_node()
    }

    /// The underlying tree, for analyzers that run tree-sitter queries.
    #[must_use]
    pub fn inner(&self) -> &tree_sitter::Tree {
        &self.tree
    }
}

/// A language adapter: knows its id and file extensions, wraps a tree-sitter
/// grammar, and carries the per-language tree-sitter queries analyzers ask for
/// by name.
///
/// Per-language crates (`qli-lang-python`, `qli-lang-typescript`, …) implement
/// this — usually by delegating to [`TreeSitterLanguage`](crate::TreeSitterLanguage).
/// Grammar quirks (TypeScript's two grammars, C#'s preprocessor) are absorbed
/// here so the engine above stays language-agnostic.
///
/// `Send + Sync` because a long-lived frontend (the LSP server) shares one
/// registry across async tasks; adapters are immutable data, so this costs
/// nothing.
pub trait Language: std::fmt::Debug + Send + Sync {
    /// Stable language id, e.g. `"python"`. Matches [`Source::language`].
    fn id(&self) -> qli_core::LanguageId;

    /// File extensions this adapter claims, without the dot (`["py"]`,
    /// `["ts", "tsx"]`). Used for `--lang auto` detection.
    fn extensions(&self) -> &'static [&'static str];

    /// Project marker files that gate `--lang auto` for this adapter's
    /// extensions (e.g. `["angular.json"]`): the file is only treated as this
    /// language when one of its ancestor directories contains a marker. Empty
    /// (the default) means the extension alone identifies the language.
    ///
    /// This is for extensions shared with files the adapter must not claim —
    /// `.html` is an Angular template only inside an Angular workspace. The
    /// adapter just declares the names;
    /// [`LanguageRegistry::for_path`](crate::LanguageRegistry::for_path)
    /// probes the filesystem, and an explicit `--lang <id>` bypasses the gate.
    fn project_markers(&self) -> &'static [&'static str] {
        &[]
    }

    /// The tree-sitter grammar for this language.
    fn ts_language(&self) -> tree_sitter::Language;

    /// A digest of everything in this adapter that shapes analysis results:
    /// the grammar and the named queries. It is part of the cache key
    /// ([`qli_core::cache::CacheKey`]), so changing a query or bumping a
    /// grammar invalidates cached results without any analyzer version bump.
    fn fingerprint(&self) -> &str;

    /// A named tree-sitter query committed to this adapter (e.g. `"defrefs"`).
    ///
    /// Returning `None` means the adapter doesn't provide that query, so an
    /// analyzer depending on it simply produces nothing for this language —
    /// analyzer coverage is per-language and degrades gracefully. Keeping the
    /// query strings behind a name (rather than in the analyzer) is what lets
    /// each adapter own its own grammar-specific queries.
    fn named_query(&self, _name: &str) -> Option<&'static str> {
        None
    }

    /// Parse `source` into a [`ParseTree`]. Returns `None` only if tree-sitter
    /// fails to construct a parser for the grammar (e.g. ABI mismatch) — a
    /// configuration bug, not a per-file syntax error (tree-sitter produces a
    /// tree with error nodes for malformed input rather than failing).
    fn parse(&self, source: &Source) -> Option<ParseTree> {
        let mut parser = tree_sitter::Parser::new();
        parser.set_language(&self.ts_language()).ok()?;
        let tree = parser.parse(&source.content, None)?;
        Some(ParseTree::new(tree))
    }
}
