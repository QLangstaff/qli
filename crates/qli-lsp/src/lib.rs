//! Language Server Protocol frontend for the qli polyglot CLI.
//!
//! This crate is the editor-facing frontend over the shared engine: the same
//! `qli-core` types, `qli-lang` adapters, and `qli-analyzers` passes that
//! `qli analyze` drives, served over LSP via `tower-lsp`.
//!
//! - [`Pipeline`] — the injected languages + analyzers + persisted store, and
//!   the parse-once / analyze / merge step over one document;
//! - [`Backend`] — the `tower-lsp` `LanguageServer`: lifecycle, document
//!   sync, `publishDiagnostics`, and `textDocument/definition`;
//! - [`DocumentCache`] — the in-memory tier of the two-tier cache;
//! - [`AnalysisStore`] — the handle to the persisted tier;
//! - [`serve_stdio`] / [`serve_tcp`] — blocking entry points for `qli lsp`.
//!
//! # Injection, not ownership
//!
//! Like every library crate in the workspace, this one receives its state as
//! parameters: the binary builds the `LanguageRegistry`, the
//! `AnalyzerRegistry`, and the on-disk cache (it owns XDG path resolution and
//! the list of linked grammars) and hands them to [`Pipeline::new`]. Nothing
//! here resolves a path or names a language.
//!
//! # Output discipline
//!
//! In stdio mode stdout carries LSP frames and nothing else. This crate never
//! prints; everything it has to say goes through `tracing`, and the binary
//! decides where that lands (stderr for `--stdio`, a file for `--tcp`).
//!
//! # `tower-lsp-server` migration
//!
//! `tower-lsp` 0.20.0 is a deliberate choice. Every site that changes in a
//! move to the `tower-lsp-server` fork carries a comment starting
//! `tower-lsp-server swap` and a colon; grepping `crates/` for that marker
//! lists the whole migration surface (and nothing else — which is why this
//! paragraph doesn't spell the marker out).

mod document_cache;
mod pipeline;
mod serve;
mod server;
mod store;

pub use document_cache::{DocumentCache, DEFAULT_DOCUMENT_CAPACITY};
pub use pipeline::{Persist, Pipeline};
pub use serve::{serve_stdio, serve_tcp};
pub use server::Backend;
pub use store::AnalysisStore;

/// The identity of an open document, as the client names it.
///
/// Every use of the URI type in this crate goes through this alias, so the
/// type half of the migration is this one line.
// tower-lsp-server swap: `Url` becomes `Uri` (`tower_lsp_server::ls_types::Uri`).
pub type DocumentUri = tower_lsp::lsp_types::Url;
