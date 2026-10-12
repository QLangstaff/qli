//! Output formatters for the qli polyglot CLI.
//!
//! Formatters are pluggable renderers over one engine output
//! ([`qli_core::AnalysisResult`]): [`Format::Human`] for a TTY,
//! [`Format::Jsonl`] for pipes. Adding a format is one module here plus one
//! `Format` variant — the engine never changes. [`lsp`] is the exception to the
//! `Format` shape: an editor consumes typed protocol values, not bytes on a
//! writer, so it exposes conversions instead of a `write`. SCIP is not a
//! `Format` either: it is one index per project, not a rendering per file, and
//! its protobuf dependency would land on every user of this crate — it lives
//! in `qli-scip`.

use std::io::{self, Write};

use qli_core::AnalysisResult;

mod human;
pub mod jsonl;
pub mod lsp;

/// A selectable output format. `serde` derives let the CLI accept it as a flag
/// value and echo it back in `--help`.
#[derive(Clone, Copy, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Format {
    /// Colored, `file:line:col`-prefixed lines for terminals.
    Human,
    /// One JSON object per finding, one per line — for pipes and `jq`.
    Jsonl,
}

impl Format {
    /// Render `result` for `file` in this format.
    pub fn write(self, w: &mut dyn Write, file: &str, result: &AnalysisResult) -> io::Result<()> {
        match self {
            Format::Human => human::write(w, file, result),
            Format::Jsonl => jsonl::write(w, file, result),
        }
    }
}

#[cfg(test)]
mod tests {
    use qli_core::{Diagnostic, Position, Range, Reference, Severity, Symbol, SymbolKind};

    use super::*;

    fn sample() -> AnalysisResult {
        let range = Range::new(Position::new(0, 4), Position::new(0, 8));
        AnalysisResult {
            diagnostics: vec![Diagnostic::new(range, Severity::Info, "TODO: x").with_code("todo")],
            symbols: vec![Symbol::new(
                "qli-local:a.py:foo#2:4",
                "foo",
                SymbolKind::Function,
                range,
            )],
            references: vec![Reference::new("bar", range)],
        }
    }

    #[test]
    fn human_uses_one_based_positions() {
        let mut buf = Vec::new();
        Format::Human.write(&mut buf, "a.py", &sample()).unwrap();
        let out = String::from_utf8(buf).unwrap();
        // 0-based (0,4) prints as 1:5.
        assert!(out.contains("a.py:1:5:"), "{out}");
        assert!(out.contains("TODO: x"));
        // `def`/`ref` are ANSI-styled (an anstream stream would strip the
        // escapes off a non-TTY; a raw Vec keeps them), so assert only on the
        // unstyled tail of each line.
        assert!(out.contains("function foo"), "{out}");
        assert!(out.contains("bar (unresolved)"), "{out}");
    }

    #[test]
    fn jsonl_is_one_object_per_line() {
        let mut buf = Vec::new();
        Format::Jsonl.write(&mut buf, "a.py", &sample()).unwrap();
        let out = String::from_utf8(buf).unwrap();
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines.len(), 3, "{out}");
        for line in lines {
            let v: serde_json::Value = serde_json::from_str(line).expect("each line is JSON");
            assert_eq!(v["file"], "a.py");
            assert!(v["type"].is_string());
        }
    }
}
