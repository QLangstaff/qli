//! In-process test harness for the qli language server.
//!
//! Drives a real [`Backend`] through `tower-lsp`'s [`LspService`] — the same
//! JSON-RPC routing, state machine, and handlers the binary serves — without
//! a transport. Nothing here touches the filesystem beyond reading the shared
//! fixtures under the workspace-root `tests/fixtures/`, and nothing touches
//! `$HOME`/XDG: the persisted tier is an in-memory [`MemoryStore`].

#![allow(dead_code)] // Helpers picked up à la carte by integration tests.

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use futures::{FutureExt, StreamExt};
use qli_analyzers::{AnalyzerRegistry, DefRefs, TodoFixme};
use qli_core::cache::CacheKey;
use qli_core::AnalysisResult;
use qli_lang::{AnalysisContext, Analyzer, LanguageRegistry};
use qli_lsp::{AnalysisStore, Backend, Pipeline};
use serde_json::{json, Value};
use tower::{Service, ServiceExt};
// tower-lsp-server swap: `tower_lsp::jsonrpc` becomes `tower_lsp_server::jsonrpc`.
use tower_lsp::jsonrpc::{Request, Response};
// tower-lsp-server swap: `tower_lsp::` becomes `tower_lsp_server::`.
use tower_lsp::{ClientSocket, LspService};

/// Read a fixture from the workspace-root `tests/fixtures/<relative>`.
pub fn fixture(relative: &str) -> String {
    let path = fixture_path(relative);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

pub fn fixture_path(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root is two levels up from crates/qli-lsp")
        .join("tests/fixtures")
        .join(relative)
}

/// The languages the binary registers today.
pub fn languages() -> LanguageRegistry {
    let mut registry = LanguageRegistry::new();
    registry.register(Box::new(qli_lang_python::language()));
    registry.register(Box::new(qli_lang_typescript::typescript()));
    registry.register(Box::new(qli_lang_typescript::tsx()));
    registry
}

/// The production pipeline shape over an in-memory persisted tier.
pub fn pipeline(store: &Arc<MemoryStore>) -> Pipeline {
    Pipeline::new(languages(), qli_analyzers::builtin(), store_handle(store))
}

pub fn store_handle(store: &Arc<MemoryStore>) -> Arc<dyn AnalysisStore> {
    Arc::clone(store) as _
}

/// An [`AnalysisStore`] kept in memory, keyed like the on-disk cache (by
/// [`CacheKey::relative_path`]). The real store is the binary's
/// `AnalysisCache`, which a library test can't name.
#[derive(Debug, Default)]
pub struct MemoryStore {
    entries: Mutex<HashMap<PathBuf, AnalysisResult>>,
}

impl MemoryStore {
    pub fn len(&self) -> usize {
        self.entries.lock().unwrap().len()
    }
}

impl AnalysisStore for MemoryStore {
    fn get(&self, key: &CacheKey<'_>) -> Option<AnalysisResult> {
        self.entries
            .lock()
            .unwrap()
            .get(&key.relative_path())
            .cloned()
    }

    fn put(&self, key: &CacheKey<'_>, result: &AnalysisResult) -> std::io::Result<()> {
        self.entries
            .lock()
            .unwrap()
            .insert(key.relative_path(), result.clone());
        Ok(())
    }
}

/// Wraps a real analyzer and counts how many times it actually ran — the
/// observable for "the cache skipped re-analysis".
#[derive(Debug)]
pub struct Counting<A> {
    inner: A,
    runs: Arc<AtomicUsize>,
}

impl<A: Analyzer> Analyzer for Counting<A> {
    fn id(&self) -> &'static str {
        self.inner.id()
    }

    fn version(&self) -> u32 {
        self.inner.version()
    }

    fn analyze(&self, ctx: &AnalysisContext<'_>) -> AnalysisResult {
        self.runs.fetch_add(1, Ordering::SeqCst);
        self.inner.analyze(ctx)
    }
}

/// The built-in analyzers, each wrapped in [`Counting`], plus the shared
/// counter of total analyzer runs.
pub fn counting_analyzers() -> (AnalyzerRegistry, Arc<AtomicUsize>) {
    let runs = Arc::new(AtomicUsize::new(0));
    let mut registry = AnalyzerRegistry::new();
    registry.register(Box::new(Counting {
        inner: TodoFixme::new(),
        runs: Arc::clone(&runs),
    }));
    registry.register(Box::new(Counting {
        inner: DefRefs::new(),
        runs: Arc::clone(&runs),
    }));
    (registry, runs)
}

/// One client session against an in-process server.
pub struct Session {
    service: LspService<Backend>,
    socket: ClientSocket,
    /// Server → client messages (e.g. `publishDiagnostics`) not yet consumed.
    inbox: VecDeque<Request>,
    next_id: i64,
}

impl Session {
    pub fn new(pipeline: Pipeline) -> Self {
        let (service, socket) = LspService::new(|client| Backend::new(client, pipeline));
        Self {
            service,
            socket,
            inbox: VecDeque::new(),
            next_id: 0,
        }
    }

    /// A session that has completed the `initialize`/`initialized` handshake.
    pub async fn initialized(pipeline: Pipeline) -> Self {
        let mut session = Self::new(pipeline);
        session
            .request("initialize", json!({ "capabilities": {} }))
            .await
            .expect("initialize succeeds");
        session.notify("initialized", json!({})).await;
        session
    }

    /// Deliver one message and wait for its handler to finish, collecting
    /// whatever the server sends the client meanwhile. The client socket must
    /// be read while the handler runs: a `publishDiagnostics` blocks until
    /// the client side takes it.
    async fn deliver(&mut self, request: Request) -> Option<Response> {
        let call = self
            .service
            .ready()
            .await
            .expect("service is ready")
            .call(request);
        tokio::pin!(call);
        let response = loop {
            tokio::select! {
                response = &mut call => break response.expect("server has not exited"),
                Some(message) = self.socket.next() => self.inbox.push_back(message),
            }
        };
        while let Some(Some(message)) = self.socket.next().now_or_never() {
            self.inbox.push_back(message);
        }
        response
    }

    /// Send a request; `Ok(result)` or `Err(error message)`. `Value::Null`
    /// params are omitted from the message (`shutdown` and `exit` take none).
    pub async fn request(&mut self, method: &'static str, params: Value) -> Result<Value, String> {
        self.next_id += 1;
        let request = with_params(Request::build(method).id(self.next_id), params);
        let response = self
            .deliver(request)
            .await
            .unwrap_or_else(|| panic!("`{method}` returned no response"));
        response.into_parts().1.map_err(|err| err.to_string())
    }

    pub async fn notify(&mut self, method: &'static str, params: Value) {
        let request = with_params(Request::build(method), params);
        let response = self.deliver(request).await;
        assert!(response.is_none(), "`{method}` is a notification");
    }

    pub async fn open(&mut self, uri: &str, text: &str) {
        self.notify(
            "textDocument/didOpen",
            json!({ "textDocument": { "uri": uri, "languageId": "", "version": 1, "text": text } }),
        )
        .await;
    }

    pub async fn change(&mut self, uri: &str, version: i32, text: &str) {
        self.notify(
            "textDocument/didChange",
            json!({
                "textDocument": { "uri": uri, "version": version },
                "contentChanges": [{ "text": text }],
            }),
        )
        .await;
    }

    pub async fn save(&mut self, uri: &str) {
        self.notify(
            "textDocument/didSave",
            json!({ "textDocument": { "uri": uri } }),
        )
        .await;
    }

    pub async fn close(&mut self, uri: &str) {
        self.notify(
            "textDocument/didClose",
            json!({ "textDocument": { "uri": uri } }),
        )
        .await;
    }

    /// `textDocument/definition` at a zero-based `(line, UTF-16 character)`.
    pub async fn definition(&mut self, uri: &str, line: usize, character: usize) -> Value {
        self.request(
            "textDocument/definition",
            json!({
                "textDocument": { "uri": uri },
                "position": { "line": line, "character": character },
            }),
        )
        .await
        .expect("definition request succeeds")
    }

    /// Take every `publishDiagnostics` params object received so far, in
    /// order, asserting the server sent nothing else.
    pub fn take_diagnostics(&mut self) -> Vec<Value> {
        self.inbox
            .drain(..)
            .map(|message| {
                assert_eq!(message.method(), "textDocument/publishDiagnostics");
                message.params().cloned().expect("publish carries params")
            })
            .collect()
    }

    /// The single `publishDiagnostics` the last message produced.
    pub fn take_one_diagnostics(&mut self) -> Value {
        let mut published = self.take_diagnostics();
        assert_eq!(published.len(), 1, "expected one publish: {published:?}");
        published.remove(0)
    }
}

// tower-lsp-server swap: `tower_lsp::jsonrpc` becomes `tower_lsp_server::jsonrpc`.
fn with_params(builder: tower_lsp::jsonrpc::RequestBuilder, params: Value) -> Request {
    if params.is_null() {
        builder.finish()
    } else {
        builder.params(params).finish()
    }
}

/// Zero-based `(line, UTF-16 character)` of the first occurrence of `needle`
/// in `text`, so tests locate identifiers by content instead of hardcoding
/// fixture coordinates.
pub fn locate(text: &str, needle: &str) -> (usize, usize) {
    let (line, content) = text
        .lines()
        .enumerate()
        .find(|(_, content)| content.contains(needle))
        .unwrap_or_else(|| panic!("`{needle}` not found"));
    let byte = content.find(needle).expect("just matched");
    (line, content[..byte].encode_utf16().count())
}
