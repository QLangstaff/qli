//! The `tower-lsp` backend: lifecycle, document sync, diagnostics, and
//! go-to-definition over the shared [`Pipeline`].
//!
//! # Ordering
//!
//! `tower-lsp` polls up to four handler futures concurrently, in arrival
//! order. Every handler here does all of its state work — storing the text,
//! analyzing, reading the cache — synchronously under one lock *before* its
//! first `.await`, so two rapid `didChange`s can't interleave and the later
//! text always wins. The only awaited step is handing finished diagnostics to
//! the client, which carry the document version so a client can discard a
//! stale publish.
//!
//! # Lifecycle
//!
//! `initialize`, `initialized`, and `shutdown` are implemented here. `exit`
//! has no handler to write: `tower-lsp` consumes the notification itself and
//! ends the `Server::serve` future, which is what makes the process exit.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;

use qli_core::{AnalysisResult, Position, Range, Source};
use qli_outputs::lsp::LineIndex;
// tower-lsp-server swap: `tower_lsp::jsonrpc` becomes `tower_lsp_server::jsonrpc`.
use tower_lsp::jsonrpc::Result;
// tower-lsp-server swap: `tower_lsp::lsp_types::` becomes `tower_lsp_server::ls_types::`.
use tower_lsp::lsp_types::{
    Diagnostic, DidChangeTextDocumentParams, DidCloseTextDocumentParams, DidOpenTextDocumentParams,
    DidSaveTextDocumentParams, GotoDefinitionParams, GotoDefinitionResponse, InitializeParams,
    InitializeResult, InitializedParams, Location, OneOf, SaveOptions, ServerCapabilities,
    ServerInfo, TextDocumentSyncCapability, TextDocumentSyncKind, TextDocumentSyncOptions,
    TextDocumentSyncSaveOptions,
};
// tower-lsp-server swap: `tower_lsp::` becomes `tower_lsp_server::`.
use tower_lsp::{Client, LanguageServer};

use crate::document_cache::DocumentCache;
use crate::pipeline::{content_hash, Persist, Pipeline};
use crate::DocumentUri;

/// The name reported in `initialize`'s `serverInfo`.
const SERVER_NAME: &str = "qli";

/// The qli language server.
///
/// Built from a `tower-lsp` [`Client`] and an injected [`Pipeline`]; see the
/// crate docs for why the registries and the store are parameters.
#[derive(Debug)]
pub struct Backend {
    client: Client,
    pipeline: Pipeline,
    state: Mutex<State>,
}

#[derive(Debug, Default)]
struct State {
    /// Open documents, as last synced by the client.
    documents: HashMap<DocumentUri, Document>,
    /// In-memory tier: latest analysis per document, by content hash.
    cache: DocumentCache,
}

#[derive(Debug)]
struct Document {
    text: String,
    version: i32,
}

impl Backend {
    #[must_use]
    pub fn new(client: Client, pipeline: Pipeline) -> Self {
        Self {
            client,
            pipeline,
            state: Mutex::new(State::default()),
        }
    }

    fn lock_state(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().expect("state mutex poisoned")
    }

    /// Record `text` as the current content of `uri`.
    fn set_document(&self, uri: &DocumentUri, text: String, version: i32) {
        self.lock_state()
            .documents
            .insert(uri.clone(), Document { text, version });
    }

    /// Analyze the open document `uri` through both cache tiers and hand the
    /// document and its result to `f`.
    ///
    /// `None` when the document isn't open, no registered language claims its
    /// extension, or its grammar can't be loaded — the caller then does
    /// nothing, which is how an unsupported document stays silent.
    ///
    /// Tier 1 is the in-memory [`DocumentCache`]: unchanged content skips
    /// analysis entirely. Tier 2 is the persisted store inside
    /// [`Pipeline::analyze`]. A [`Persist::Yes`] call goes straight to the
    /// pipeline so the result reaches disk even when memory already has it;
    /// if disk has it too, that is two store reads and no analysis.
    fn with_analysis<R>(
        &self,
        uri: &DocumentUri,
        persist: Persist,
        f: impl FnOnce(&Document, &AnalysisResult) -> R,
    ) -> Option<R> {
        let mut guard = self.lock_state();
        let State { documents, cache } = &mut *guard;
        let document = documents.get(uri)?;
        let path = document_path(uri);
        let language = self.pipeline.language_for(&path)?;
        let language_id = language.id();
        let hash = content_hash(language_id.as_str(), document.text.as_bytes());

        if persist == Persist::Yes || cache.get(uri, &hash).is_none() {
            let source = Source::new(path, document.text.clone().into_bytes(), language_id);
            let Some(result) = self.pipeline.analyze(language, &source, persist) else {
                tracing::warn!(uri = %uri, "could not parse document");
                return None;
            };
            cache.insert(uri.clone(), hash.clone(), result);
        }
        let result = cache.get(uri, &hash)?;
        Some(f(document, result))
    }

    /// The diagnostics to publish for `uri` and the document version they
    /// describe.
    fn diagnostics(&self, uri: &DocumentUri, persist: Persist) -> Option<(Vec<Diagnostic>, i32)> {
        self.with_analysis(uri, persist, |document, result| {
            let index = LineIndex::new(&document.text);
            let diagnostics = result
                .diagnostics
                .iter()
                .map(|diagnostic| index.diagnostic(diagnostic))
                .collect();
            (diagnostics, document.version)
        })
    }

    /// Where the name under `position` is defined, if it resolves to a
    /// definition in the same document.
    fn definition(
        &self,
        uri: &DocumentUri,
        // tower-lsp-server swap: `tower_lsp::lsp_types::` becomes `tower_lsp_server::ls_types::`.
        position: tower_lsp::lsp_types::Position,
    ) -> Option<Location> {
        self.with_analysis(uri, Persist::No, |document, result| {
            let index = LineIndex::new(&document.text);
            let range = definition_range(result, index.core_position(position))?;
            Some(index.location(uri.clone(), range))
        })?
    }

    async fn publish(&self, uri: DocumentUri, persist: Persist) {
        // Analysis happens here, before the first await (see module docs).
        let Some((diagnostics, version)) = self.diagnostics(&uri, persist) else {
            return;
        };
        self.client
            .publish_diagnostics(uri, diagnostics, Some(version))
            .await;
    }
}

/// The filesystem path of a document, for extension-based language detection
/// and for the `Source` the analyzers see. A non-`file:` URI (e.g.
/// `untitled:`) falls back to its path component.
fn document_path(uri: &DocumentUri) -> PathBuf {
    // tower-lsp-server swap: `Url::to_file_path`/`Url::path` become `Uri::to_file_path` (a `Cow<Path>`, via `UriExt`) and `Uri::path().as_str()`.
    uri.to_file_path()
        .unwrap_or_else(|()| PathBuf::from(uri.path()))
}

/// The name range of the definition that the symbol at `at` refers to.
///
/// A reference resolves to its same-file definition (unresolved → `None`); a
/// cursor already on a definition's name resolves to itself, which is what
/// editors expect of go-to-definition at a declaration. The end bound is
/// inclusive because a cursor resting just after an identifier still means
/// that identifier.
fn definition_range(result: &AnalysisResult, at: Position) -> Option<Range> {
    let contains = |range: Range| range.start <= at && at <= range.end;
    if let Some(reference) = result.references.iter().find(|r| contains(r.range)) {
        let id = reference.resolved.as_ref()?;
        return result
            .symbols
            .iter()
            .find(|symbol| &symbol.id == id)
            .map(|symbol| symbol.range);
    }
    result
        .symbols
        .iter()
        .find(|symbol| contains(symbol.range))
        .map(|symbol| symbol.range)
}

// tower-lsp-server swap: drop the attribute; the fork uses native `async fn` in traits.
#[tower_lsp::async_trait]
impl LanguageServer for Backend {
    async fn initialize(&self, _: InitializeParams) -> Result<InitializeResult> {
        Ok(InitializeResult {
            capabilities: ServerCapabilities {
                // Full-document sync: every change carries the whole text, so
                // the server never applies UTF-16 edit ranges itself.
                text_document_sync: Some(TextDocumentSyncCapability::Options(
                    TextDocumentSyncOptions {
                        open_close: Some(true),
                        change: Some(TextDocumentSyncKind::FULL),
                        save: Some(TextDocumentSyncSaveOptions::SaveOptions(
                            SaveOptions::default(),
                        )),
                        ..TextDocumentSyncOptions::default()
                    },
                )),
                definition_provider: Some(OneOf::Left(true)),
                ..ServerCapabilities::default()
            },
            server_info: Some(ServerInfo {
                name: SERVER_NAME.to_owned(),
                version: Some(env!("CARGO_PKG_VERSION").to_owned()),
            }),
        })
    }

    async fn initialized(&self, _: InitializedParams) {
        tracing::info!("LSP session initialized");
    }

    async fn shutdown(&self) -> Result<()> {
        tracing::info!("LSP session shutting down");
        Ok(())
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        let document = params.text_document;
        self.set_document(&document.uri, document.text, document.version);
        self.publish(document.uri, Persist::Yes).await;
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        let uri = params.text_document.uri;
        // Full sync: the last change is the whole document.
        let Some(change) = params.content_changes.into_iter().next_back() else {
            return;
        };
        self.set_document(&uri, change.text, params.text_document.version);
        self.publish(uri, Persist::No).await;
    }

    async fn did_save(&self, params: DidSaveTextDocumentParams) {
        // The saved content is what a later cold start opens: persist it.
        self.publish(params.text_document.uri, Persist::Yes).await;
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        let uri = params.text_document.uri;
        let was_open = self.lock_state().documents.remove(&uri).is_some();
        // Clear what was published for it. The in-memory analysis stays (it
        // is bounded) so reopening unchanged content is free.
        if was_open && self.pipeline.language_for(&document_path(&uri)).is_some() {
            self.client.publish_diagnostics(uri, Vec::new(), None).await;
        }
    }

    async fn goto_definition(
        &self,
        params: GotoDefinitionParams,
    ) -> Result<Option<GotoDefinitionResponse>> {
        let target = params.text_document_position_params;
        Ok(self
            .definition(&target.text_document.uri, target.position)
            .map(GotoDefinitionResponse::Scalar))
    }
}

#[cfg(test)]
mod tests {
    use qli_core::{Reference, Symbol, SymbolKind};

    use super::*;

    fn range(line: u32, start: u32, end: u32) -> Range {
        Range::new(Position::new(line, start), Position::new(line, end))
    }

    fn result() -> AnalysisResult {
        AnalysisResult {
            symbols: vec![Symbol::new(
                "id:foo",
                "foo",
                SymbolKind::Function,
                range(0, 4, 7),
            )],
            references: vec![
                Reference::new("foo", range(5, 4, 7)).resolved_to("id:foo"),
                Reference::new("bar", range(6, 4, 7)),
            ],
            ..AnalysisResult::default()
        }
    }

    #[test]
    fn reference_resolves_to_its_definition() {
        let found = definition_range(&result(), Position::new(5, 5));
        assert_eq!(found, Some(range(0, 4, 7)));
    }

    #[test]
    fn cursor_just_after_the_identifier_still_resolves() {
        let found = definition_range(&result(), Position::new(5, 7));
        assert_eq!(found, Some(range(0, 4, 7)));
    }

    #[test]
    fn unresolved_reference_has_no_definition() {
        assert_eq!(definition_range(&result(), Position::new(6, 5)), None);
    }

    #[test]
    fn definition_name_resolves_to_itself() {
        let found = definition_range(&result(), Position::new(0, 5));
        assert_eq!(found, Some(range(0, 4, 7)));
    }

    #[test]
    fn elsewhere_has_no_definition() {
        assert_eq!(definition_range(&result(), Position::new(3, 0)), None);
    }
}
