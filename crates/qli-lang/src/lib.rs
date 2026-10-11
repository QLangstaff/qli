//! Language adapters and the analyzer contract for the qli polyglot CLI.
//!
//! This crate sits between the pure engine ([`qli_core`]) and the grammars.
//! It owns the tree-sitter dependency and defines:
//!
//! - [`Language`] — a per-language adapter (id, extensions, grammar, named
//!   queries), usually built from [`TreeSitterLanguage`];
//! - [`ParseTree`] — a parsed source;
//! - [`LanguageRegistry`] — id/extension lookup for `--lang auto`;
//! - [`Analyzer`] + [`AnalysisContext`] — the tree-walking analysis contract
//!   that [`qli-analyzers`](../qli_analyzers/index.html) implements.
//!
//! The `Analyzer` trait lives here rather than in `qli-core` so that naming a
//! tree-sitter-backed [`ParseTree`] in its signature doesn't pull the grammar
//! dependency into the pure engine.

mod analyzer;
mod language;
mod registry;
mod tree_sitter_language;

pub use analyzer::{analyze_with, AnalysisContext, Analyzer};
pub use language::{Language, ParseTree};
pub use registry::LanguageRegistry;
pub use tree_sitter_language::TreeSitterLanguage;
