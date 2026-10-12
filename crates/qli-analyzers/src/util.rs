//! Shared helpers for turning tree-sitter spans into `qli-core` ranges.

use qli_core::{Position, Range};

/// Convert a `tree_sitter::Point` (zero-based row + byte column) into a
/// [`Position`]. The two representations already agree — this is a lossless
/// field rename — but going through it keeps analyzers off the tree-sitter
/// type in their emitted data.
#[must_use]
pub fn point_to_position(point: tree_sitter::Point) -> Position {
    Position::new(
        u32::try_from(point.row).unwrap_or(u32::MAX),
        u32::try_from(point.column).unwrap_or(u32::MAX),
    )
}

/// The [`Range`] spanning a tree-sitter node.
#[must_use]
pub fn node_range(node: tree_sitter::Node<'_>) -> Range {
    Range::new(
        point_to_position(node.start_position()),
        point_to_position(node.end_position()),
    )
}

/// Position of the byte `offset` into `text`, where `text` starts at `start`
/// (zero-based line, byte column). Used to place a regex match found *inside*
/// a node's text: only the node's own bytes are scanned, so the cost does not
/// grow with the node's distance from the top of the file.
#[must_use]
pub fn position_within(start: Position, text: &[u8], offset: usize) -> Position {
    let before = &text[..offset.min(text.len())];
    let Some(last_newline) = before.iter().rposition(|&b| b == b'\n') else {
        let column = u32::try_from(before.len()).unwrap_or(u32::MAX);
        return Position::new(start.line, start.column.saturating_add(column));
    };
    let lines = before.split(|&b| b == b'\n').count() - 1;
    Position::new(
        start
            .line
            .saturating_add(u32::try_from(lines).unwrap_or(u32::MAX)),
        u32::try_from(before.len() - last_newline - 1).unwrap_or(u32::MAX),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn position_within_first_line_offsets_from_the_start_column() {
        let start = Position::new(7, 4);
        assert_eq!(position_within(start, b"// TODO x", 3), Position::new(7, 7));
    }

    #[test]
    fn position_within_later_line_restarts_the_column() {
        let start = Position::new(7, 4);
        let text = b"/* a\n   FIXME\n*/";
        assert_eq!(position_within(start, text, 8), Position::new(8, 3));
        assert_eq!(position_within(start, text, 14), Position::new(9, 0));
    }
}
