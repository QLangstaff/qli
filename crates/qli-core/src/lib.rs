//! Core engine vocabulary for the qli polyglot CLI.
//!
//! This crate is the shared, **pure** vocabulary that every frontend (CLI,
//! LSP, SCIP) and every analyzer speaks: positions, ranges, diagnostics,
//! symbols, references, and the `Source`/`AnalysisResult` boundary types, plus
//! the content-hashed [`cache`] key and the project-wide reference
//! [`resolve`] pass.
//!
//! # Purity invariant
//!
//! `qli-core` performs no I/O and holds no global state. It is handed bytes
//! (never a path to open) and returns data (never printed output). This is
//! what lets the CLI, the LSP server, and the SCIP indexer share one engine
//! without contortion — see the plan's "engine purity is non-negotiable"
//! decision. The invariant is guarded by
//! [`tests/dependency_purity.rs`](../../tests/dependency_purity.rs), which
//! fails CI if a disallowed dependency creeps in. `serde` (serialization) and
//! `blake3` (cache hashing) are the only permitted direct dependencies; both
//! are pure. Tree-sitter and every grammar live one layer out, in
//! [`qli-lang`](../qli_lang/index.html).

mod diagnostic;
mod result;
mod source;
mod symbol;

pub mod cache;
pub mod resolve;

pub use diagnostic::{Diagnostic, Position, Range, Severity};
pub use result::AnalysisResult;
pub use source::{LanguageId, Source};
pub use symbol::{Reference, Symbol, SymbolKind};
