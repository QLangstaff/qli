//! The unified output of an analysis pass.

use serde::{Deserialize, Serialize};

use crate::diagnostic::Diagnostic;
use crate::symbol::{Reference, Symbol};

/// Everything one analyzer produced for one source.
///
/// A single result type spans both analyzer flavors: a diagnostic analyzer
/// (TODO/FIXME) fills `diagnostics` and leaves the rest empty; a
/// definition/reference analyzer fills `symbols` + `references`. Frontends and
/// formatters consume whichever fields are populated, so one cache entry and
/// one output path serve both.
#[derive(Clone, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub struct AnalysisResult {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub diagnostics: Vec<Diagnostic>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub symbols: Vec<Symbol>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub references: Vec<Reference>,
}

impl AnalysisResult {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.diagnostics.is_empty() && self.symbols.is_empty() && self.references.is_empty()
    }

    /// Fold another result's findings into this one. Used when a frontend runs
    /// several analyzers over the same source and reports them together.
    pub fn merge(&mut self, other: AnalysisResult) {
        self.diagnostics.extend(other.diagnostics);
        self.symbols.extend(other.symbols);
        self.references.extend(other.references);
    }
}
