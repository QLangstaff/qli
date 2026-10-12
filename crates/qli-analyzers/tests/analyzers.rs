//! Analyzer behavior over the shared workspace fixtures. Proves the same
//! analyzer runs polyglot (Python + TypeScript + C# + Angular templates) and
//! that def/ref resolution is same-file lexical.

use std::path::PathBuf;

use qli_analyzers::{DefRefs, TodoFixme};
use qli_core::{LanguageId, Severity, Source, SymbolKind};
use qli_lang::{analyze_with, Analyzer, Language};

fn fixture(rel: &str) -> Vec<u8> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures")
        .join(rel);
    std::fs::read(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

fn source(rel: &str, lang: &str) -> Source {
    Source::new(rel.into(), fixture(rel), LanguageId::from(lang))
}

fn run(lang: &dyn Language, analyzer: &dyn Analyzer, source: &Source) -> qli_core::AnalysisResult {
    analyze_with(lang, analyzer, source).expect("grammar loads")
}

#[test]
fn todofixme_finds_all_markers_in_python() {
    let lang = qli_lang_python::language();
    let result = run(
        &lang,
        &TodoFixme::new(),
        &source("python/sample.py", "python"),
    );
    // TODO, FIXME, XXX, HACK — the string literal `TODO` is NOT in a comment.
    assert_eq!(result.diagnostics.len(), 4, "{:#?}", result.diagnostics);
    let codes: Vec<&str> = result
        .diagnostics
        .iter()
        .map(|d| d.code.as_deref().unwrap())
        .collect();
    assert_eq!(codes, ["todo", "fixme", "xxx", "hack"]);
    assert!(result
        .diagnostics
        .iter()
        .all(|d| d.severity == Severity::Info));
}

#[test]
fn todofixme_finds_all_markers_in_typescript() {
    let lang = qli_lang_typescript::typescript();
    let result = run(
        &lang,
        &TodoFixme::new(),
        &source("typescript/sample.ts", "typescript"),
    );
    // Includes the FIXME inside a /* block comment */.
    assert_eq!(result.diagnostics.len(), 4, "{:#?}", result.diagnostics);
    let codes: Vec<&str> = result
        .diagnostics
        .iter()
        .map(|d| d.code.as_deref().unwrap())
        .collect();
    assert_eq!(codes, ["todo", "fixme", "xxx", "hack"]);
}

#[test]
fn todofixme_finds_all_markers_in_csharp() {
    let lang = qli_lang_csharp::language();
    let result = run(
        &lang,
        &TodoFixme::new(),
        &source("csharp/sample.cs", "csharp"),
    );
    // Includes the FIXME inside a /* block comment */ and the XXX inside a
    // /// doc comment; the string literal `TODO` is NOT in a comment.
    assert_eq!(result.diagnostics.len(), 4, "{:#?}", result.diagnostics);
    let codes: Vec<&str> = result
        .diagnostics
        .iter()
        .map(|d| d.code.as_deref().unwrap())
        .collect();
    assert_eq!(codes, ["todo", "fixme", "xxx", "hack"]);
}

#[test]
fn todofixme_ignores_markers_outside_comments() {
    // The Python fixture returns a string containing "TODO"; it must not fire.
    let lang = qli_lang_python::language();
    let result = run(
        &lang,
        &TodoFixme::new(),
        &source("python/sample.py", "python"),
    );
    assert!(
        result.diagnostics.iter().all(|d| d.range.start.line != 17),
        "a string-literal TODO was flagged"
    );
}

#[test]
fn defrefs_extracts_python_defs_and_resolves_same_file() {
    let lang = qli_lang_python::language();
    let result = run(
        &lang,
        &DefRefs::new(),
        &source("python/sample.py", "python"),
    );

    let names: Vec<&str> = result.symbols.iter().map(|s| s.name.as_str()).collect();
    assert!(names.contains(&"foo"));
    assert!(names.contains(&"Widget"));
    assert!(names.contains(&"main"));
    assert!(names.contains(&"DEFAULT_RETRIES"));
    assert!(result
        .symbols
        .iter()
        .any(|s| s.kind == SymbolKind::Class && s.name == "Widget"));

    // foo() resolves to the local def; bar() does not.
    let foo_ref = result
        .references
        .iter()
        .find(|r| r.name == "foo")
        .expect("foo ref");
    assert!(foo_ref.resolved.is_some(), "foo() should resolve same-file");
    let bar_ref = result
        .references
        .iter()
        .find(|r| r.name == "bar")
        .expect("bar ref");
    assert!(bar_ref.resolved.is_none(), "bar() is undefined here");
}

#[test]
fn defrefs_extracts_typescript_defs_and_resolves_same_file() {
    let lang = qli_lang_typescript::typescript();
    let result = run(
        &lang,
        &DefRefs::new(),
        &source("typescript/sample.ts", "typescript"),
    );

    let names: Vec<&str> = result.symbols.iter().map(|s| s.name.as_str()).collect();
    assert!(names.contains(&"foo"));
    assert!(names.contains(&"Widget"));
    assert!(names.contains(&"render"));
    assert!(names.contains(&"DEFAULT_RETRIES"));

    let foo_ref = result
        .references
        .iter()
        .find(|r| r.name == "foo")
        .expect("foo ref");
    assert!(foo_ref.resolved.is_some());
    let bar_ref = result
        .references
        .iter()
        .find(|r| r.name == "bar")
        .expect("bar ref");
    assert!(bar_ref.resolved.is_none());
}

#[test]
fn defrefs_extracts_csharp_defs_and_resolves_same_file() {
    let lang = qli_lang_csharp::language();
    let result = run(
        &lang,
        &DefRefs::new(),
        &source("csharp/sample.cs", "csharp"),
    );

    let names: Vec<&str> = result.symbols.iter().map(|s| s.name.as_str()).collect();
    assert!(names.contains(&"Foo"));
    assert!(names.contains(&"Widget"));
    assert!(names.contains(&"Render"));
    assert!(names.contains(&"DefaultRetries"));
    assert!(result
        .symbols
        .iter()
        .any(|s| s.kind == SymbolKind::Class && s.name == "Widget"));
    assert!(result
        .symbols
        .iter()
        .any(|s| s.kind == SymbolKind::Method && s.name == "Foo"));

    let foo_ref = result
        .references
        .iter()
        .find(|r| r.name == "Foo")
        .expect("Foo ref");
    assert!(foo_ref.resolved.is_some());
    let bar_ref = result
        .references
        .iter()
        .find(|r| r.name == "Bar")
        .expect("Bar ref");
    assert!(bar_ref.resolved.is_none());
    // A member-access call (`new Widget().Render()`) is not a `@ref.call`.
    assert!(!result.references.iter().any(|r| r.name == "Render"));
}

#[test]
fn registry_dispatches_by_id() {
    let registry = qli_analyzers::builtin();
    assert!(registry.get("todofixme").is_some());
    assert!(registry.get("defrefs").is_some());
    assert!(registry.get("nonexistent").is_none());
    let ids: Vec<&str> = registry.ids().collect();
    assert_eq!(ids, ["todofixme", "defrefs"]);
}

#[test]
fn todofixme_finds_all_markers_in_angular_template() {
    let lang = qli_lang_angular::language();
    let result = run(
        &lang,
        &TodoFixme::new(),
        &source("angular/src/app/sample.component.html", "angular"),
    );
    // Includes the XXX on the second line of a multi-line <!-- comment -->;
    // the element text `TODO` is NOT in a comment.
    assert_eq!(result.diagnostics.len(), 4, "{:#?}", result.diagnostics);
    let codes: Vec<&str> = result
        .diagnostics
        .iter()
        .map(|d| d.code.as_deref().unwrap())
        .collect();
    assert_eq!(codes, ["todo", "fixme", "xxx", "hack"]);
    assert!(
        result.diagnostics.iter().all(|d| d.range.start.line != 15),
        "an element-text TODO was flagged"
    );
    // The `-->` terminator must not leak into the message.
    assert_eq!(
        result.diagnostics[0].message,
        "TODO: replace the placeholder heading"
    );
}

#[test]
fn defrefs_extracts_angular_template_defs_and_resolves_same_file() {
    let lang = qli_lang_angular::language();
    let result = run(
        &lang,
        &DefRefs::new(),
        &source("angular/src/app/sample.component.html", "angular"),
    );

    // The variables the template itself binds: `*ngFor` (`let widget`,
    // `let i`), `@let`, `@if (…; as current)`, `@for (item …; let idx)`.
    let names: Vec<&str> = result.symbols.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names, ["widget", "i", "format", "current", "item", "idx"]);
    assert!(result
        .symbols
        .iter()
        .all(|s| s.kind == SymbolKind::Variable));

    // format() resolves to the `@let`; retry() lives on the component class,
    // outside this file.
    let format_ref = result
        .references
        .iter()
        .find(|r| r.name == "format")
        .expect("format ref");
    assert!(format_ref.resolved.is_some());
    let retry_ref = result
        .references
        .iter()
        .find(|r| r.name == "retry")
        .expect("retry ref");
    assert!(retry_ref.resolved.is_none());
}
