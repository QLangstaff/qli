//! LSP rendering: `qli-core` positions, ranges, diagnostics, and locations as
//! `lsp-types` values for the language server (`qli-lsp`).
//!
//! # Column encoding
//!
//! The two sides disagree on what a column is. A [`qli_core::Position`]
//! column is a **UTF-8 byte offset** into its line — tree-sitter's
//! `Point.column` and `qli-analyzers`' `position_within` both count bytes.
//! An LSP `character` is a **UTF-16 code-unit offset** (the protocol default,
//! and the only encoding every client must support). They agree on ASCII and
//! diverge on everything else: `é` is 2 bytes but 1 unit, `😀` is 4 bytes but
//! 2 units. Converting therefore needs the line's text, which is why every
//! conversion goes through a [`LineIndex`] built over the document rather
//! than being a free `From` impl.
//!
//! Lines are split on `\n` only, matching how the engine counts them. A
//! trailing `\r` (CRLF files) is not part of the line for clamping purposes.

// tower-lsp-server swap: `lsp_types` becomes `tower_lsp_server::ls_types` (this alias and the import below).
use lsp_types as lsp;
// tower-lsp-server swap: `lsp_types::` becomes `tower_lsp_server::ls_types::`.
use lsp_types::{DiagnosticSeverity, NumberOrString};
// tower-lsp-server swap: `Url` becomes `Uri` (`tower_lsp_server::ls_types::Uri`).
use lsp_types::Url;
use qli_core::{Diagnostic, Position, Range, Severity};

/// The `source` every published diagnostic carries, shown by editors next to
/// the message.
pub const DIAGNOSTIC_SOURCE: &str = "qli";

/// Byte offsets of each line start in a document, for converting between
/// `qli-core` byte columns and LSP UTF-16 columns in either direction.
///
/// Build one per document version and reuse it for every conversion against
/// that text.
#[derive(Debug, Clone)]
pub struct LineIndex<'a> {
    text: &'a str,
    /// Byte offset of the first byte of each line; always starts with `0`.
    line_starts: Vec<usize>,
}

impl<'a> LineIndex<'a> {
    #[must_use]
    pub fn new(text: &'a str) -> Self {
        let mut line_starts = vec![0];
        line_starts.extend(text.match_indices('\n').map(|(i, _)| i + 1));
        Self { text, line_starts }
    }

    /// The text of a zero-based line without its terminator; empty for a line
    /// past the end of the document.
    fn line(&self, line: u32) -> &'a str {
        let line = line as usize;
        let Some(&start) = self.line_starts.get(line) else {
            return "";
        };
        let end = self
            .line_starts
            .get(line + 1)
            .map_or(self.text.len(), |next| next - 1);
        let text = &self.text[start..end];
        text.strip_suffix('\r').unwrap_or(text)
    }

    /// Engine position (byte column) → LSP position (UTF-16 column).
    ///
    /// A column past the end of the line clamps to the line's end; a column
    /// inside a multi-byte character floors to that character's start.
    #[must_use]
    pub fn position(&self, position: Position) -> lsp::Position {
        let line = self.line(position.line);
        let mut byte = (position.column as usize).min(line.len());
        while !line.is_char_boundary(byte) {
            byte -= 1;
        }
        let character = line[..byte].encode_utf16().count();
        lsp::Position::new(position.line, saturate(character))
    }

    /// LSP position (UTF-16 column) → engine position (byte column), for
    /// hit-testing a cursor against engine ranges.
    ///
    /// A `character` past the end of the line clamps to the line's end (as
    /// the LSP spec requires); one inside a surrogate pair floors to the
    /// start of that character.
    #[must_use]
    pub fn core_position(&self, position: lsp::Position) -> Position {
        let line = self.line(position.line);
        let target = position.character as usize;
        let mut units = 0;
        let mut byte = line.len();
        for (offset, ch) in line.char_indices() {
            if units + ch.len_utf16() > target {
                byte = offset;
                break;
            }
            units += ch.len_utf16();
        }
        Position::new(position.line, saturate(byte))
    }

    #[must_use]
    pub fn range(&self, range: Range) -> lsp::Range {
        lsp::Range::new(self.position(range.start), self.position(range.end))
    }

    #[must_use]
    pub fn diagnostic(&self, diagnostic: &Diagnostic) -> lsp::Diagnostic {
        lsp::Diagnostic {
            range: self.range(diagnostic.range),
            severity: Some(severity(diagnostic.severity)),
            code: diagnostic.code.clone().map(NumberOrString::String),
            source: Some(DIAGNOSTIC_SOURCE.to_owned()),
            message: diagnostic.message.clone(),
            ..lsp::Diagnostic::default()
        }
    }

    /// A `range` of this document, addressed by the document's `uri`.
    // tower-lsp-server swap: `Url` parameter becomes `Uri`.
    #[must_use]
    pub fn location(&self, uri: Url, range: Range) -> lsp::Location {
        lsp::Location::new(uri, self.range(range))
    }
}

#[must_use]
pub fn severity(severity: Severity) -> DiagnosticSeverity {
    match severity {
        Severity::Error => DiagnosticSeverity::ERROR,
        Severity::Warning => DiagnosticSeverity::WARNING,
        Severity::Info => DiagnosticSeverity::INFORMATION,
        Severity::Hint => DiagnosticSeverity::HINT,
    }
}

fn saturate(value: usize) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `é` is 2 bytes / 1 UTF-16 unit; `😀` is 4 bytes / 2 units (a surrogate
    /// pair). Byte and UTF-16 columns of `TODO`: 16 vs 13.
    const NON_ASCII: &str = "x = 1\ns = \"é😀\"  # TODO: ship\n";

    fn todo_range() -> Range {
        let column = u32::try_from(NON_ASCII.lines().nth(1).unwrap().find("TODO").unwrap())
            .expect("fits in u32");
        assert_eq!(column, 16, "fixture drifted");
        Range::new(Position::new(1, column), Position::new(1, column + 4))
    }

    #[test]
    fn ascii_columns_pass_through() {
        let index = LineIndex::new("abc\ndef\n");
        assert_eq!(
            index.position(Position::new(1, 2)),
            lsp::Position::new(1, 2)
        );
        assert_eq!(
            index.core_position(lsp::Position::new(1, 2)),
            Position::new(1, 2)
        );
    }

    #[test]
    fn byte_columns_become_utf16_columns_on_a_non_ascii_line() {
        let index = LineIndex::new(NON_ASCII);
        let range = index.range(todo_range());
        assert_eq!(range.start, lsp::Position::new(1, 13));
        assert_eq!(range.end, lsp::Position::new(1, 17));
    }

    #[test]
    fn utf16_columns_become_byte_columns_on_a_non_ascii_line() {
        let index = LineIndex::new(NON_ASCII);
        assert_eq!(
            index.core_position(lsp::Position::new(1, 13)),
            todo_range().start
        );
        assert_eq!(
            index.core_position(lsp::Position::new(1, 17)),
            todo_range().end
        );
    }

    #[test]
    fn positions_inside_a_character_floor_to_its_start() {
        // Line 1 is `s = "é😀"…`: `é` at bytes 5..7 (unit 5), `😀` at bytes
        // 7..11 (units 6..8).
        let index = LineIndex::new(NON_ASCII);
        assert_eq!(
            index.position(Position::new(1, 6)),
            lsp::Position::new(1, 5),
            "byte inside `é`"
        );
        assert_eq!(
            index.position(Position::new(1, 9)),
            lsp::Position::new(1, 6),
            "byte inside `😀`"
        );
        assert_eq!(
            index.core_position(lsp::Position::new(1, 7)),
            Position::new(1, 7),
            "unit inside the surrogate pair"
        );
    }

    #[test]
    fn out_of_range_positions_clamp() {
        let index = LineIndex::new("ab\r\ncd");
        assert_eq!(
            index.position(Position::new(0, 99)),
            lsp::Position::new(0, 2),
            "past end of a CRLF line clamps before the `\\r`"
        );
        assert_eq!(
            index.core_position(lsp::Position::new(1, 99)),
            Position::new(1, 2)
        );
        assert_eq!(
            index.position(Position::new(9, 3)),
            lsp::Position::new(9, 0),
            "line past the end of the document"
        );
    }

    #[test]
    fn diagnostic_carries_severity_code_and_source() {
        let index = LineIndex::new(NON_ASCII);
        let diagnostic =
            Diagnostic::new(todo_range(), Severity::Info, "TODO: ship").with_code("todo");
        let converted = index.diagnostic(&diagnostic);
        assert_eq!(converted.range.start, lsp::Position::new(1, 13));
        assert_eq!(converted.severity, Some(DiagnosticSeverity::INFORMATION));
        assert_eq!(
            converted.code,
            Some(NumberOrString::String("todo".to_owned()))
        );
        assert_eq!(converted.source.as_deref(), Some(DIAGNOSTIC_SOURCE));
        assert_eq!(converted.message, "TODO: ship");
    }

    #[test]
    fn location_pairs_the_uri_with_the_converted_range() {
        let index = LineIndex::new(NON_ASCII);
        // tower-lsp-server swap: `Url::parse` becomes `Uri::from_str`.
        let uri = Url::parse("file:///tmp/a.py").unwrap();
        let location = index.location(uri.clone(), todo_range());
        assert_eq!(location.uri, uri);
        assert_eq!(location.range.start, lsp::Position::new(1, 13));
    }
}
