//! The `DefRefs` analyzer: extracts definitions ([`Symbol`]s) and usages
//! ([`Reference`]s), the data SCIP emission (Phase 4) and LSP go-to-def
//! (Phase 3) need.
//!
//! Structure comes from a per-language `"defrefs"` tree-sitter query the
//! language adapter owns (see `qli-lang-python` / `qli-lang-typescript`). The
//! query's capture names are the contract:
//!
//! - `@def.<kind>` on a definition node, paired with `@name` on the bound
//!   identifier — `<kind>` is one of `function`/`method`/`class`/`variable`;
//! - `@ref.call` on a called identifier.
//!
//! Resolution is lexical and same-file only (Phase 2 scope): a reference whose
//! name matches a definition in the same file resolves to it; otherwise it is
//! recorded unresolved. No cross-file resolution, no type inference.

use qli_core::{AnalysisResult, Reference, Symbol, SymbolKind};
use qli_lang::{AnalysisContext, Analyzer};
use std::collections::HashMap;
use streaming_iterator::StreamingIterator;
use tree_sitter::{Query, QueryCursor};

use crate::util::node_range;

#[derive(Debug, Default)]
pub struct DefRefs;

impl DefRefs {
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

impl Analyzer for DefRefs {
    fn id(&self) -> &'static str {
        "defrefs"
    }

    fn version(&self) -> u32 {
        1
    }

    fn analyze(&self, ctx: &AnalysisContext<'_>) -> AnalysisResult {
        // No query for this language → this analyzer simply produces nothing
        // for it. Coverage is per-language and degrades gracefully.
        let Some(query_src) = ctx.language.named_query("defrefs") else {
            return AnalysisResult::default();
        };
        let Ok(query) = Query::new(&ctx.language.ts_language(), query_src) else {
            // A malformed query is a bug in the adapter, not user input; emit
            // nothing rather than panic in the middle of a batch analyze.
            return AnalysisResult::default();
        };

        let content = ctx.content();
        let capture_names = query.capture_names();
        let file = ctx.source.path.display().to_string();

        let mut symbols: Vec<Symbol> = Vec::new();
        // name -> symbol id of the first same-file definition seen.
        let mut defs_by_name: HashMap<String, String> = HashMap::new();
        // References are collected first, then resolved against the full
        // definition set so a call above its definition still resolves.
        let mut pending_refs: Vec<Reference> = Vec::new();

        let mut cursor = QueryCursor::new();
        let mut matches = cursor.matches(&query, ctx.tree.root_node(), content);
        while let Some(m) = matches.next() {
            let mut kind: Option<SymbolKind> = None;
            let mut name_node = None;
            let mut ref_node = None;

            for cap in m.captures {
                let cap_name = capture_names[cap.index as usize];
                match cap_name {
                    "name" => name_node = Some(cap.node),
                    "ref.call" => ref_node = Some(cap.node),
                    other => {
                        if let Some(k) = other.strip_prefix("def.") {
                            kind = symbol_kind(k);
                        }
                    }
                }
            }

            if let (Some(kind), Some(name_node)) = (kind, name_node) {
                let Some(name) = node_text(content, name_node) else {
                    continue;
                };
                let range = node_range(name_node);
                let id = symbol_id(&file, name, range.start.line, range.start.column);
                defs_by_name
                    .entry(name.to_owned())
                    .or_insert_with(|| id.clone());
                symbols.push(Symbol::new(id, name, kind, range));
            } else if let Some(ref_node) = ref_node {
                let Some(name) = node_text(content, ref_node) else {
                    continue;
                };
                pending_refs.push(Reference::new(name, node_range(ref_node)));
            }
        }

        let references = pending_refs
            .into_iter()
            .map(|r| match defs_by_name.get(&r.name) {
                Some(id) => r.clone().resolved_to(id.clone()),
                None => r,
            })
            .collect();

        AnalysisResult {
            symbols,
            references,
            ..Default::default()
        }
    }
}

fn node_text<'a>(content: &'a [u8], node: tree_sitter::Node<'_>) -> Option<&'a str> {
    std::str::from_utf8(content.get(node.byte_range())?).ok()
}

/// Loose precursor to a SCIP symbol string (`<scheme>:<package>:<file>:<path>`).
/// It only needs to be stable and unique within a file, so it includes the
/// definition's line/column. The formal scheme (`qli-scip`'s `SCHEME.md`) is
/// built from the symbol's name and kind, not from this string.
fn symbol_id(file: &str, name: &str, line: u32, column: u32) -> String {
    format!("qli-local:{file}:{name}#{line}:{column}")
}

fn symbol_kind(raw: &str) -> Option<SymbolKind> {
    match raw {
        "function" => Some(SymbolKind::Function),
        "method" => Some(SymbolKind::Method),
        "class" => Some(SymbolKind::Class),
        "variable" => Some(SymbolKind::Variable),
        _ => None,
    }
}
