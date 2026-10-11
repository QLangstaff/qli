//! The `TodoFixme` analyzer: flags `TODO`/`FIXME`/`XXX`/`HACK` markers in
//! comments.
//!
//! Matching is restricted to comment nodes **via tree-sitter**, not a regex
//! over raw bytes — so a `TODO` inside a string literal or an identifier like
//! `TODO_LIST` outside a comment doesn't trip it. This is the point of Phase
//! 2H: prove the same analyzer runs polyglot by walking each language's parse
//! tree rather than its text.

use qli_core::{AnalysisResult, Diagnostic, Range, Severity};
use qli_lang::{AnalysisContext, Analyzer};
use regex::Regex;

use crate::util::byte_to_position;

/// Marker keywords, matched on word boundaries and case-sensitively (screaming
/// case is the convention; `todo` in prose shouldn't fire).
const MARKERS: &str = r"\b(TODO|FIXME|XXX|HACK)\b";

#[derive(Debug, Default)]
pub struct TodoFixme;

impl TodoFixme {
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

impl Analyzer for TodoFixme {
    fn id(&self) -> &'static str {
        "todofixme"
    }

    fn version(&self) -> u32 {
        1
    }

    fn analyze(&self, ctx: &AnalysisContext<'_>) -> AnalysisResult {
        let content = ctx.content();
        let re = Regex::new(MARKERS).expect("MARKERS is a valid regex");
        let mut diagnostics = Vec::new();

        for comment in comment_nodes(ctx.tree.root_node()) {
            let range = comment.byte_range();
            let Some(text) = content.get(range.clone()) else {
                continue;
            };
            let Ok(text) = std::str::from_utf8(text) else {
                // Non-UTF-8 comment bytes: skip rather than guess.
                continue;
            };
            for m in re.find_iter(text) {
                let marker = m.as_str();
                let start_byte = range.start + m.start();
                let start = byte_to_position(content, start_byte);
                let end = byte_to_position(content, range.start + m.end());
                let message = summarize(marker, &text[m.start()..]);
                diagnostics.push(
                    Diagnostic::new(Range::new(start, end), Severity::Info, message)
                        .with_code(marker.to_ascii_lowercase()),
                );
            }
        }

        AnalysisResult {
            diagnostics,
            ..Default::default()
        }
    }
}

/// The comment text from the marker to end of line, collapsed to a one-line
/// diagnostic message (e.g. `TODO: wire this up`).
fn summarize(marker: &str, from_marker: &str) -> String {
    let line = from_marker.lines().next().unwrap_or(from_marker);
    // Trim trailing whitespace, then a single-line block-comment terminator
    // (`*/`) that would otherwise leak into the message, then whitespace again.
    let trimmed = line
        .trim_end()
        .strip_suffix("*/")
        .unwrap_or_else(|| line.trim_end())
        .trim_end();
    // Drop the marker itself and any immediately-following `:`/whitespace so
    // the message reads naturally, then re-prefix with the marker.
    let rest = trimmed[marker.len()..].trim_start_matches([':', ' ', '\t']);
    if rest.is_empty() {
        marker.to_string()
    } else {
        format!("{marker}: {rest}")
    }
}

/// Every `comment`-kinded node in the tree, in source order. Both the Python
/// and TypeScript grammars name comments `comment` (line and block alike), so
/// this stays language-agnostic.
fn comment_nodes(root: tree_sitter::Node<'_>) -> Vec<tree_sitter::Node<'_>> {
    let mut out = Vec::new();
    let mut cursor = root.walk();
    let mut stack = vec![root];
    while let Some(node) = stack.pop() {
        if node.kind() == "comment" {
            out.push(node);
        }
        stack.extend(node.children(&mut cursor));
    }
    out.sort_by_key(tree_sitter::Node::start_byte);
    out
}
