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
/// `<scheme>:<package>:<file>:<symbol-path>`. Phase 4 formalizes the scheme;
/// for now it only needs to be stable and unique within a run so references
/// can point at it.
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
/// Phase 2 does lexical, same-file resolution only. A reference whose target
/// isn't a same-file definition is recorded with `resolved: None` — being
/// unresolved is a first-class, reported state, not an error.
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
