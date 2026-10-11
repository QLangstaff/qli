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

/// Position of an absolute byte offset into `content` (zero-based line, byte
/// column). Used to place a regex match found *inside* a node's text.
#[must_use]
pub fn byte_to_position(content: &[u8], byte: usize) -> Position {
    let end = byte.min(content.len());
    let mut line = 0u32;
    let mut col = 0u32;
    for &b in &content[..end] {
        if b == b'\n' {
            line += 1;
            col = 0;
        } else {
            col += 1;
        }
    }
    Position::new(line, col)
}
