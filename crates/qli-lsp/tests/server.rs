//! Protocol-level tests of the qli language server, driven in-process through
//! `tower-lsp`'s `LspService` (see `common`).
//!
//! Covers the Phase 3 contract: lifecycle, the initialize → didOpen →
//! publishDiagnostics roundtrip over the shared TODO/FIXME fixtures,
//! go-to-definition for Python and TypeScript, UTF-16 columns end to end, and
//! both cache tiers.

mod common;

use std::sync::atomic::Ordering;
use std::sync::Arc;

use common::{counting_analyzers, fixture, fixture_path, locate, MemoryStore, Session};
use qli_lsp::Pipeline;
use serde_json::{json, Value};

fn fixture_uri(relative: &str) -> String {
    format!("file://{}", fixture_path(relative).display())
}

async fn session() -> Session {
    Session::initialized(common::pipeline(&Arc::new(MemoryStore::default()))).await
}

/// `(code, message, start line, start character)` per published diagnostic.
fn summarize(published: &Value) -> Vec<(String, String, u64, u64)> {
    published["diagnostics"]
        .as_array()
        .expect("diagnostics array")
        .iter()
        .map(|d| {
            (
                d["code"].as_str().unwrap().to_owned(),
                d["message"].as_str().unwrap().to_owned(),
                d["range"]["start"]["line"].as_u64().unwrap(),
                d["range"]["start"]["character"].as_u64().unwrap(),
            )
        })
        .collect()
}

#[tokio::test]
async fn initialize_advertises_sync_and_definition() {
    let mut session = Session::new(common::pipeline(&Arc::new(MemoryStore::default())));
    let result = session
        .request("initialize", json!({ "capabilities": {} }))
        .await
        .expect("initialize succeeds");

    let sync = &result["capabilities"]["textDocumentSync"];
    assert_eq!(sync["openClose"], true);
    assert_eq!(sync["change"], 1, "full-document sync");
    assert!(sync["save"].is_object(), "didSave is requested: {sync}");
    assert_eq!(result["capabilities"]["definitionProvider"], true);
    assert_eq!(result["serverInfo"]["name"], "qli");
    assert_eq!(result["serverInfo"]["version"], env!("CARGO_PKG_VERSION"));
}

#[tokio::test]
async fn requests_before_initialize_are_rejected() {
    let mut session = Session::new(common::pipeline(&Arc::new(MemoryStore::default())));
    let err = session
        .request(
            "textDocument/definition",
            json!({
                "textDocument": { "uri": "file:///tmp/a.py" },
                "position": { "line": 0, "character": 0 },
            }),
        )
        .await
        .expect_err("server is not initialized");
    assert!(err.contains("not initialized"), "{err}");
}

#[tokio::test]
async fn shutdown_then_exit_ends_the_session() {
    let mut session = session().await;
    let result = session.request("shutdown", Value::Null).await;
    assert_eq!(result, Ok(Value::Null));
    session.notify("exit", Value::Null).await;
    assert!(session.take_diagnostics().is_empty());
}

#[tokio::test]
async fn open_publishes_todofixme_diagnostics_for_python() {
    let mut session = session().await;
    let uri = fixture_uri("python/sample.py");
    session.open(&uri, &fixture("python/sample.py")).await;

    let published = session.take_one_diagnostics();
    assert_eq!(published["uri"], uri);
    assert_eq!(published["version"], 1);
    // Four markers in comments; the `TODO` inside the string literal is not
    // flagged.
    assert_eq!(
        summarize(&published),
        [
            ("todo", "TODO: wire up the real client", 0, 2),
            ("fixme", "FIXME: this default is a guess", 4, 2),
            ("xxx", "XXX: revisit the off-by-one below", 9, 6),
            (
                "hack",
                "HACK: temporary shim until the API stabilizes",
                14,
                6
            ),
        ]
        .map(|(code, message, line, character)| (
            code.to_owned(),
            message.to_owned(),
            line,
            character
        ))
    );
    let first = &published["diagnostics"][0];
    assert_eq!(first["source"], "qli");
    assert_eq!(first["severity"], 3, "information");
    assert_eq!(first["range"]["end"], json!({ "line": 0, "character": 6 }));
}

#[tokio::test]
async fn open_publishes_todofixme_diagnostics_for_typescript() {
    let mut session = session().await;
    session
        .open(
            &fixture_uri("typescript/sample.ts"),
            &fixture("typescript/sample.ts"),
        )
        .await;

    let codes: Vec<String> = summarize(&session.take_one_diagnostics())
        .into_iter()
        .map(|(code, ..)| code)
        .collect();
    assert_eq!(codes, ["todo", "fixme", "xxx", "hack"]);
}

#[tokio::test]
async fn change_republishes_for_the_new_content_and_version() {
    let mut session = session().await;
    let uri = "file:///tmp/a.py";
    session.open(uri, "x = 1\n").await;
    assert!(summarize(&session.take_one_diagnostics()).is_empty());

    session.change(uri, 2, "x = 1  # FIXME: later\n").await;
    let published = session.take_one_diagnostics();
    assert_eq!(published["version"], 2);
    assert_eq!(
        summarize(&published),
        [("fixme".to_owned(), "FIXME: later".to_owned(), 0, 9)]
    );
}

#[tokio::test]
async fn close_clears_published_diagnostics() {
    let mut session = session().await;
    let uri = "file:///tmp/a.py";
    session.open(uri, "# TODO: x\n").await;
    assert_eq!(summarize(&session.take_one_diagnostics()).len(), 1);

    session.close(uri).await;
    let published = session.take_one_diagnostics();
    assert_eq!(published["uri"], uri);
    assert_eq!(published["diagnostics"], json!([]));
}

#[tokio::test]
async fn unsupported_document_publishes_nothing_and_does_not_error() {
    let mut session = session().await;
    for uri in [
        "file:///tmp/notes.txt",
        "file:///tmp/Makefile",
        "untitled:1",
    ] {
        session
            .open(uri, "# TODO: not a language qli knows\n")
            .await;
        session.change(uri, 2, "# FIXME: still not\n").await;
        session.save(uri).await;
        assert_eq!(session.definition(uri, 0, 3).await, Value::Null);
        session.close(uri).await;
        assert!(session.take_diagnostics().is_empty(), "{uri}");
    }
}

#[tokio::test]
async fn definition_resolves_a_python_reference_to_its_local_definition() {
    let mut session = session().await;
    let uri = fixture_uri("python/sample.py");
    let text = fixture("python/sample.py");
    session.open(&uri, &text).await;

    // The call `foo(DEFAULT_RETRIES)` inside `main` → `def foo(x):`.
    let (call_line, call_character) = locate(&text, "foo(DEFAULT_RETRIES)");
    let (def_line, def_start) = locate(&text, "foo(x)");
    assert_ne!(call_line, def_line);

    let location = session
        .definition(&uri, call_line, call_character + 1)
        .await;
    assert_eq!(location["uri"], uri);
    assert_eq!(
        location["range"],
        json!({
            "start": { "line": def_line, "character": def_start },
            "end": { "line": def_line, "character": def_start + 3 },
        })
    );

    // `bar()` has no definition in this file.
    let (bar_line, bar_character) = locate(&text, "bar()");
    assert_eq!(
        session.definition(&uri, bar_line, bar_character).await,
        Value::Null
    );
}

#[tokio::test]
async fn definition_resolves_a_typescript_reference_to_its_local_definition() {
    let mut session = session().await;
    let uri = fixture_uri("typescript/sample.ts");
    let text = fixture("typescript/sample.ts");
    session.open(&uri, &text).await;

    // The call `foo(DEFAULT_RETRIES);` inside `main` → `function foo(x: …`.
    let (call_line, call_character) = locate(&text, "foo(DEFAULT_RETRIES)");
    let (def_line, def_start) = locate(&text, "foo(x: number)");
    assert_ne!(call_line, def_line);

    let location = session
        .definition(&uri, call_line, call_character + 1)
        .await;
    assert_eq!(location["uri"], uri);
    assert_eq!(
        location["range"],
        json!({
            "start": { "line": def_line, "character": def_start },
            "end": { "line": def_line, "character": def_start + 3 },
        })
    );

    let (bar_line, bar_character) = locate(&text, "bar()");
    assert_eq!(
        session.definition(&uri, bar_line, bar_character).await,
        Value::Null
    );
}

/// Columns on the wire are UTF-16 code units; the engine's are UTF-8 bytes.
/// `é` (2 bytes, 1 unit) and `😀` (4 bytes, 2 units) before the interesting
/// tokens make the two disagree, in both directions.
#[tokio::test]
async fn columns_are_utf16_on_a_non_ascii_line() {
    let mut session = session().await;
    let uri = "file:///tmp/unicode.py";
    let text = "def foo():\n    pass\n\nprint(\"é😀\", foo())  # TODO: ship\n";
    session.open(uri, text).await;

    // Outgoing: the diagnostic's byte column 26 is UTF-16 column 23.
    let line = text.lines().nth(3).unwrap();
    assert_eq!(line.find("TODO"), Some(26), "byte column");
    assert_eq!(
        summarize(&session.take_one_diagnostics()),
        [("todo".to_owned(), "TODO: ship".to_owned(), 3, 23)]
    );

    // Incoming: the `foo` call sits at UTF-16 column 13 (byte column 16).
    assert_eq!(line.find("foo"), Some(16), "byte column");
    let (call_line, call_character) = locate(text, "foo())");
    assert_eq!((call_line, call_character), (3, 13));
    let location = session.definition(uri, call_line, call_character).await;
    assert_eq!(
        location["range"],
        json!({
            "start": { "line": 0, "character": 4 },
            "end": { "line": 0, "character": 7 },
        })
    );
    // A byte column sent as-is lands past the call: 17 is inside `foo` in
    // bytes, but UTF-16 column 17 is the `)` after it.
    assert_eq!(session.definition(uri, 3, 17).await, Value::Null);
}

#[tokio::test]
async fn unchanged_content_change_skips_reanalysis() {
    let (analyzers, runs) = counting_analyzers();
    let analyzer_count = analyzers.ids().count();
    let store = Arc::new(MemoryStore::default());
    let pipeline = Pipeline::new(common::languages(), analyzers, common::store_handle(&store));
    let mut session = Session::initialized(pipeline).await;
    let uri = "file:///tmp/a.py";
    let text = "# TODO: x\n";

    session.open(uri, text).await;
    assert_eq!(runs.load(Ordering::SeqCst), analyzer_count, "cold open");
    let opened = session.take_one_diagnostics();

    // Same bytes, new version: served from the in-memory tier.
    session.change(uri, 2, text).await;
    assert_eq!(
        runs.load(Ordering::SeqCst),
        analyzer_count,
        "unchanged content must not re-run any analyzer"
    );
    let unchanged = session.take_one_diagnostics();
    assert_eq!(unchanged["version"], 2, "still republished");
    assert_eq!(unchanged["diagnostics"], opened["diagnostics"]);

    // A definition request reads the same cached result.
    session.definition(uri, 0, 0).await;
    assert_eq!(runs.load(Ordering::SeqCst), analyzer_count);

    // Different bytes: every analyzer runs once more.
    session.change(uri, 3, "# FIXME: y\n").await;
    assert_eq!(runs.load(Ordering::SeqCst), analyzer_count * 2);
    assert_eq!(summarize(&session.take_one_diagnostics())[0].0, "fixme");
}

#[tokio::test]
async fn persisted_tier_serves_a_cold_start_without_analysis() {
    let store = Arc::new(MemoryStore::default());
    let uri = "file:///tmp/a.py";
    let text = "# TODO: x\n";

    // First session: open persists one entry per analyzer.
    let mut first = Session::initialized(common::pipeline(&store)).await;
    first.open(uri, text).await;
    let warm = first.take_one_diagnostics();
    let persisted = store.len();
    assert_eq!(persisted, qli_analyzers::builtin().ids().count());

    // Second session, fresh memory, same store: nothing is re-analyzed.
    let (analyzers, runs) = counting_analyzers();
    let pipeline = Pipeline::new(common::languages(), analyzers, common::store_handle(&store));
    let mut second = Session::initialized(pipeline).await;
    second.open(uri, text).await;
    assert_eq!(runs.load(Ordering::SeqCst), 0, "cold start hit the store");
    assert_eq!(
        second.take_one_diagnostics()["diagnostics"],
        warm["diagnostics"]
    );
}

#[tokio::test]
async fn edits_persist_on_save_not_on_every_change() {
    let store = Arc::new(MemoryStore::default());
    let mut session = Session::initialized(common::pipeline(&store)).await;
    let uri = "file:///tmp/a.py";

    session.open(uri, "# TODO: x\n").await;
    let after_open = store.len();
    assert!(after_open > 0, "open persists");

    session.change(uri, 2, "# TODO: xy\n").await;
    session.change(uri, 3, "# TODO: xyz\n").await;
    assert_eq!(store.len(), after_open, "keystrokes are not persisted");

    session.save(uri).await;
    assert_eq!(store.len(), after_open * 2, "the saved content is");
    assert_eq!(session.take_diagnostics().len(), 4);
}
