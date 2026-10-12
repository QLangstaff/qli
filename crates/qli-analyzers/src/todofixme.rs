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

use crate::util::{point_to_position, position_within};

/// Marker keywords, matched on word boundaries and case-sensitively (screaming
/// case is the convention; `todo` in prose shouldn't fire).
const MARKERS: &str = r"\b(TODO|FIXME|XXX|HACK)\b";

/// Block-comment terminators that can close a comment on the marker's own
/// line (`/* TODO: x */`, `<!-- TODO: x -->`).
const BLOCK_TERMINATORS: &[&str] = &["*/", "-->"];

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
        // 2: `summarize` strips the HTML `-->` terminator as well as `*/`.
        2
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
            let comment_start = point_to_position(comment.start_position());
            for m in re.find_iter(text) {
                let marker = m.as_str();
                let start = position_within(comment_start, text.as_bytes(), m.start());
                let end = position_within(comment_start, text.as_bytes(), m.end());
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
    // that would otherwise leak into the message, then whitespace again.
    let line = line.trim_end();
    let trimmed = BLOCK_TERMINATORS
        .iter()
        .find_map(|terminator| line.strip_suffix(terminator))
        .unwrap_or(line)
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

/// Every `comment`-kinded node in the tree, in source order. The Python,
/// TypeScript, C#, and Angular grammars all name comments `comment` (line,
/// block, doc, and `<!-- -->` alike), so this stays language-agnostic.
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
