//! Polyglot analyzers for the qli code-analysis CLI.
//!
//! Analyzers live in their own crate (not in `qli-core`) so new ones can be
//! added without touching the engine's core types. Each implements
//! [`qli_lang::Analyzer`] — walking a parsed tree and emitting `qli-core`
//! diagnostics, symbols, and references — and registers into an
//! [`AnalyzerRegistry`] under a stable id used on the CLI (`--analyzer <id>`).
//!
//! Two analyzers ship in Phase 2: [`TodoFixme`] (diagnostics) and
//! [`DefRefs`] (definitions + references).

mod defrefs;
mod registry;
mod todofixme;
mod util;

pub use defrefs::DefRefs;
pub use registry::AnalyzerRegistry;
pub use todofixme::TodoFixme;

/// A registry pre-populated with every built-in analyzer.
///
/// This is the set the CLI dispatches `--analyzer <id>` against. The default
/// analyzer (`qli analyze` with no `--analyzer`) is `todofixme`, resolved by
/// the CLI, not encoded here.
#[must_use]
pub fn builtin() -> AnalyzerRegistry {
    let mut registry = AnalyzerRegistry::new();
    registry.register(Box::new(TodoFixme::new()));
    registry.register(Box::new(DefRefs::new()));
    registry
}
