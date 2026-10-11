//! `LanguageRegistry` — id/extension lookup over registered [`Language`]s.

use std::collections::HashMap;

use qli_core::LanguageId;

use crate::language::Language;

/// A set of language adapters, looked up by id or by file extension.
///
/// Extension lookup drives `--lang auto`: the CLI maps a file's extension to
/// the language whose adapter claims it. First registration of an extension
/// wins; a later collision is ignored (adapters are registered in a fixed
/// order by the binary, so this is deterministic).
#[derive(Debug, Default)]
pub struct LanguageRegistry {
    by_id: HashMap<LanguageId, Box<dyn Language>>,
    by_extension: HashMap<&'static str, LanguageId>,
}

impl LanguageRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Register an adapter. Its extensions are indexed for `--lang auto`;
    /// an extension already claimed by an earlier adapter is left alone.
    pub fn register(&mut self, language: Box<dyn Language>) {
        let id = language.id();
        for ext in language.extensions() {
            self.by_extension.entry(ext).or_insert_with(|| id.clone());
        }
        self.by_id.insert(id, language);
    }

    #[must_use]
    pub fn get(&self, id: &LanguageId) -> Option<&dyn Language> {
        self.by_id.get(id).map(AsRef::as_ref)
    }

    /// Resolve a file extension (without the dot, case-sensitive) to a
    /// registered language. Callers should lowercase the extension first if
    /// they want case-insensitive matching.
    #[must_use]
    pub fn by_extension(&self, ext: &str) -> Option<&dyn Language> {
        let id = self.by_extension.get(ext)?;
        self.get(id)
    }

    /// Every registered language id.
    pub fn ids(&self) -> impl Iterator<Item = &LanguageId> {
        self.by_id.keys()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.by_id.is_empty()
    }
}
