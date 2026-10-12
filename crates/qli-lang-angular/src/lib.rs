//! Angular template language adapter for the qli polyglot CLI.
//!
//! Wraps the `tree-sitter-angular` grammar behind [`qli_lang::Language`] via
//! [`qli_lang::TreeSitterLanguage`]. The grammar is a superset of HTML that
//! parses template syntax — structural directives, bindings, interpolation,
//! `@if`/`@for` control flow — **including the embedded expressions**, into
//! its own nodes (`call_expression`, `member_expression`, `pipe_call`, …). So
//! one tree covers markup and expressions alike, and analyzers query it
//! directly: no second parse through the TypeScript adapter.
//!
//! `.html` is not Angular-specific, so the adapter declares `angular.json` as
//! a project marker: `--lang auto` only routes `.html` here inside an Angular
//! workspace (see [`qli_lang::Language::project_markers`]).

use qli_lang::TreeSitterLanguage;

/// The Angular workspace config file; its directory is the workspace root.
const PROJECT_MARKERS: &[&str] = &["angular.json"];

/// Definition/reference query for the `DefRefs` analyzer. The capture-name
/// contract matches the Python adapter's: `@def.*` with `@name`, `@ref.call`.
///
/// Definitions are the variables a template itself introduces: `@let`, the
/// `@for` item and its `let` aliases, `@if (…; as x)`, and the `let`/`as`
/// bindings of a structural directive. A microsyntax key such as `trackBy:`
/// shares the `structural_assignment` node but binds nothing, hence the
/// `special_keyword` anchors.
const DEFREFS_QUERY: &str = r#"
(let_statement
  (assignment_expression
    name: (identifier) @name)) @def.variable

(for_declaration
  name: (identifier) @name) @def.variable

(for_reference
  alias: (assignment_expression
    name: (identifier) @name)) @def.variable

(if_reference
  (identifier) @name) @def.variable

(structural_declaration
  (special_keyword)
  .
  (structural_assignment
    name: (identifier) @name)) @def.variable

(structural_assignment
  .
  (special_keyword)
  name: (identifier) @name) @def.variable

(structural_assignment
  alias: (identifier) @name) @def.variable

((structural_expression
  (special_keyword) @_keyword
  .
  (identifier) @name) @def.variable
  (#eq? @_keyword "as"))

(call_expression
  function: (identifier) @ref.call)
"#;

/// The Angular template [`Language`](qli_lang::Language) adapter (`.html`
/// inside an `angular.json` workspace).
#[must_use]
pub fn language() -> TreeSitterLanguage {
    TreeSitterLanguage::new(
        "angular",
        &["html"],
        tree_sitter_angular::language(),
        &[("defrefs", DEFREFS_QUERY)],
    )
    .with_project_markers(PROJECT_MARKERS)
}

#[cfg(test)]
mod tests {
    use qli_core::{LanguageId, Source};
    use qli_lang::{Language, ParseTree};

    use super::*;

    fn parse(content: &str) -> ParseTree {
        let source = Source::new(
            "t.component.html".into(),
            content.as_bytes().to_vec(),
            LanguageId::from("angular"),
        );
        language().parse(&source).expect("parse")
    }

    /// Every node kind in the tree, depth-first.
    fn kinds(tree: &ParseTree) -> Vec<&'static str> {
        let mut out = Vec::new();
        let mut cursor = tree.root_node().walk();
        let mut stack = vec![tree.root_node()];
        while let Some(node) = stack.pop() {
            out.push(node.kind());
            stack.extend(node.children(&mut cursor));
        }
        out
    }

    /// Asserts `content` parses cleanly AND yields each of `expected` — a
    /// binding the grammar didn't recognize would still parse error-free, as a
    /// plain HTML attribute, so the node kinds are the real proof.
    fn assert_parses_as(content: &str, expected: &[&str]) {
        let tree = parse(content);
        assert!(!tree.root_node().has_error(), "error node in: {content}");
        let kinds = kinds(&tree);
        for kind in expected {
            assert!(kinds.contains(kind), "no `{kind}` node in: {content}");
        }
    }

    /// `(name, is_definition)` for every `@name` / `@ref.call` capture.
    fn defrefs(content: &str) -> Vec<(String, bool)> {
        use tree_sitter::StreamingIterator;

        let tree = parse(content);
        let query = tree_sitter::Query::new(&language().ts_language(), DEFREFS_QUERY)
            .expect("defrefs query compiles");
        let names = query.capture_names();
        let mut cursor = tree_sitter::QueryCursor::new();
        let mut matches = cursor.matches(&query, tree.root_node(), content.as_bytes());
        let mut out = Vec::new();
        while let Some(m) = matches.next() {
            for cap in m.captures {
                let is_definition = match names[cap.index as usize] {
                    "name" => true,
                    "ref.call" => false,
                    _ => continue,
                };
                out.push((content[cap.node.byte_range()].to_owned(), is_definition));
            }
        }
        out.sort();
        out
    }

    fn def(name: &str) -> (String, bool) {
        (name.to_owned(), true)
    }

    fn call(name: &str) -> (String, bool) {
        (name.to_owned(), false)
    }

    #[test]
    fn parses_template_into_nonempty_tree() {
        let tree = parse("<p>{{ title }}</p>\n");
        let root = tree.root_node();
        assert_eq!(root.kind(), "document");
        assert!(root.child_count() > 0, "expected a non-empty tree");
        assert!(!root.has_error(), "well-formed source should not error");
    }

    #[test]
    fn parses_structural_directives() {
        assert_parses_as(
            r#"<p *ngIf="ready; else loading">x</p>"#,
            &["structural_directive", "structural_expression"],
        );
        assert_parses_as(
            r#"<li *ngFor="let item of items; trackBy: track; let i = index">x</li>"#,
            &["structural_directive", "structural_declaration"],
        );
    }

    #[test]
    fn parses_property_binding() {
        assert_parses_as(r#"<a [href]="url">x</a>"#, &["property_binding"]);
    }

    #[test]
    fn parses_event_binding() {
        assert_parses_as(
            r#"<a (click)="go($event)">x</a>"#,
            &["event_binding", "call_expression"],
        );
    }

    #[test]
    fn parses_two_way_binding() {
        assert_parses_as(
            r#"<input [(ngModel)]="model.name" />"#,
            &["two_way_binding", "member_expression"],
        );
    }

    #[test]
    fn parses_interpolation() {
        assert_parses_as(
            "<p>{{ fmt(user.name) | uppercase }}</p>",
            &["interpolation", "call_expression", "pipe_call"],
        );
    }

    #[test]
    fn parses_if_block() {
        assert_parses_as(
            "@if (user(); as u) { <p>{{ u.name }}</p> } @else if (other) { <b>b</b> } @else { <i>i</i> }",
            &["if_statement", "if_reference", "else_if_statement", "else_statement"],
        );
    }

    #[test]
    fn parses_for_block() {
        assert_parses_as(
            "@for (item of items; track item.id; let idx = $index) { <p>{{ item }}</p> } @empty { none }",
            &["for_statement", "for_declaration", "for_reference", "empty_statement"],
        );
    }

    #[test]
    fn claims_html_extension_behind_angular_json() {
        assert_eq!(language().extensions(), &["html"]);
        assert_eq!(language().id(), LanguageId::from("angular"));
        assert_eq!(language().project_markers(), &["angular.json"]);
    }

    #[test]
    fn exposes_defrefs_query() {
        assert!(language().named_query("defrefs").is_some());
        assert!(language().named_query("nonexistent").is_none());
    }

    #[test]
    fn defrefs_captures_control_flow_bindings_and_calls() {
        assert_eq!(
            defrefs("@let total = sum(a, b);"),
            [call("sum"), def("total")]
        );
        assert_eq!(
            defrefs("@if (user(); as u) { {{ u.name }} }"),
            [def("u"), call("user")]
        );
        assert_eq!(
            defrefs("@for (item of load(); track item.id; let idx = $index) { {{ item }} }"),
            [def("idx"), def("item"), call("load")]
        );
    }

    #[test]
    fn defrefs_captures_structural_directive_bindings() {
        // `trackBy` and `index` are microsyntax keys, not bindings.
        assert_eq!(
            defrefs(
                r#"<li *ngFor="let item of items; trackBy: track; index as i; let last = last">x</li>"#
            ),
            [def("i"), def("item"), def("last")]
        );
        // `else loading` names a template, it doesn't bind one.
        assert_eq!(
            defrefs(r#"<p *ngIf="user$ | async as user; else loading">x</p>"#),
            [def("user")]
        );
    }

    #[test]
    fn defrefs_captures_calls_in_bindings_and_interpolation() {
        assert_eq!(
            defrefs(r#"<a (click)="go(item)" [title]="label()">{{ fmt(x) }}</a>"#),
            [call("fmt"), call("go"), call("label")]
        );
    }
}
