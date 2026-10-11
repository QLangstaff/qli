//! Positions, ranges, severities, and diagnostics — the vocabulary a
//! diagnostic-emitting analyzer (e.g. TODO/FIXME) produces.

use serde::{Deserialize, Serialize};

/// A zero-based `(line, column)` cursor into a source file.
///
/// Both are zero-based to match tree-sitter and LSP internals. Human-facing
/// formatters ([`qli-outputs`](../qli_outputs/index.html)) add 1 when they
/// print, so a diagnostic on the first line reads `file:1:1`.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Serialize, Deserialize)]
pub struct Position {
    pub line: u32,
    pub column: u32,
}

impl Position {
    #[must_use]
    pub fn new(line: u32, column: u32) -> Self {
        Self { line, column }
    }
}

/// A half-open span `[start, end)` in a source file.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct Range {
    pub start: Position,
    pub end: Position,
}

impl Range {
    #[must_use]
    pub fn new(start: Position, end: Position) -> Self {
        Self { start, end }
    }
}

/// Diagnostic severity, ordered most-to-least severe.
///
/// Serializes lowercase (`"error"`, `"warning"`, …) so jsonl output and LSP
/// mapping agree on the wire form.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Error,
    Warning,
    Info,
    Hint,
}

/// A single finding at a location — the output of a diagnostic analyzer.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct Diagnostic {
    pub range: Range,
    pub severity: Severity,
    pub message: String,
    /// Machine-readable rule code (e.g. `"todo"`), if the analyzer sets one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
}

impl Diagnostic {
    #[must_use]
    pub fn new(range: Range, severity: Severity, message: impl Into<String>) -> Self {
        Self {
            range,
            severity,
            message: message.into(),
            code: None,
        }
    }

    #[must_use]
    pub fn with_code(mut self, code: impl Into<String>) -> Self {
        self.code = Some(code.into());
        self
    }
}
