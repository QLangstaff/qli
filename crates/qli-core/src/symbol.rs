//! Symbols and references — the vocabulary the definition/reference analyzer
//! (Phase 2I) produces and that SCIP (Phase 4) and LSP go-to-def (Phase 3)
//! consume.

use serde::{Deserialize, Serialize};

use crate::diagnostic::Range;

/// What kind of thing a [`Symbol`] names.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SymbolKind {
    Function,
    Method,
    Class,
    Variable,
}

/// A definition: a named thing declared at a location in a file.
///
/// `id` is a loose precursor to a SCIP symbol string of the form
/// `<scheme>:<package>:<file>:<symbol-path>`. It only needs to be unique
/// within one [`AnalysisResult`](crate::AnalysisResult), so that file's
/// references can point at it; it is not unique across files (a cached result
/// keeps the path it was first analyzed under). The formal SCIP symbol is
/// derived from `name` and `kind` by `qli-scip` (see its `SCHEME.md`), never
/// parsed out of `id`.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct Symbol {
    pub id: String,
    pub name: String,
    pub kind: SymbolKind,
    /// Span of the symbol's *name*, not its whole body.
    pub range: Range,
}

impl Symbol {
    #[must_use]
    pub fn new(
        id: impl Into<String>,
        name: impl Into<String>,
        kind: SymbolKind,
        range: Range,
    ) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            kind,
            range,
        }
    }
}

/// A usage: a name referenced at a location, optionally resolved to a
/// [`Symbol`]'s `id`.
///
/// Analyzers do lexical, same-file resolution only. A reference whose target
/// isn't a same-file definition is recorded with `resolved: None` — being
/// unresolved is a first-class, reported state, not an error. Linking it to a
/// definition in another file is the project-wide [`resolve`](crate::resolve)
/// pass, which reads these values and never rewrites them.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct Reference {
    pub name: String,
    pub range: Range,
    /// `Some(symbol_id)` when this reference resolves to a same-file
    /// definition; `None` when unresolved.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resolved: Option<String>,
}

impl Reference {
    #[must_use]
    pub fn new(name: impl Into<String>, range: Range) -> Self {
        Self {
            name: name.into(),
            range,
            resolved: None,
        }
    }

    #[must_use]
    pub fn resolved_to(mut self, symbol_id: impl Into<String>) -> Self {
        self.resolved = Some(symbol_id.into());
        self
    }
}
