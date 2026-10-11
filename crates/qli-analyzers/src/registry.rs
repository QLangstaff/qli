//! `AnalyzerRegistry` — dispatch to an [`Analyzer`] by its `--analyzer <id>`.

use std::collections::HashMap;

use qli_lang::Analyzer;

/// A set of analyzers, dispatched by id. The CLI's `--analyzer <id>` looks up
/// here; registering a new analyzer is the only step to make it selectable.
///
/// `order` preserves registration order so listings (`ids()`) are stable
/// rather than hash-random.
#[derive(Default)]
pub struct AnalyzerRegistry {
    by_id: HashMap<&'static str, Box<dyn Analyzer>>,
    order: Vec<&'static str>,
}

impl AnalyzerRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Register an analyzer under its own [`Analyzer::id`]. A later analyzer
    /// with the same id replaces the earlier one (last-wins); the id keeps its
    /// original position in `order`.
    pub fn register(&mut self, analyzer: Box<dyn Analyzer>) {
        let id = analyzer.id();
        if !self.by_id.contains_key(id) {
            self.order.push(id);
        }
        self.by_id.insert(id, analyzer);
    }

    #[must_use]
    pub fn get(&self, id: &str) -> Option<&dyn Analyzer> {
        self.by_id.get(id).map(AsRef::as_ref)
    }

    /// Registered ids in registration order — for `--help` listings and
    /// error messages ("known analyzers: …").
    pub fn ids(&self) -> impl Iterator<Item = &'static str> + '_ {
        self.order.iter().copied()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.by_id.is_empty()
    }
}

impl std::fmt::Debug for AnalyzerRegistry {
    // `by_id`'s values are `Box<dyn Analyzer>`, which isn't `Debug`; the id
    // list (which mirrors the map's keys) is the useful summary.
    #[allow(clippy::missing_fields_in_debug)]
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AnalyzerRegistry")
            .field("ids", &self.order)
            .finish_non_exhaustive()
    }
}
