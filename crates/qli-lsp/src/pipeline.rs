//! The shared analysis pipeline: injected languages + analyzers + persisted
//! store, and the parse-once / analyze / merge step over one source.

use std::path::Path;
use std::sync::Arc;

use qli_analyzers::AnalyzerRegistry;
use qli_core::cache::CacheKey;
use qli_core::{AnalysisResult, Source};
use qli_lang::{AnalysisContext, Language, LanguageRegistry};

use crate::store::AnalysisStore;

/// Whether a freshly computed result is written to the persisted store.
///
/// The server persists the states that exist on disk (`didOpen`, `didSave`) —
/// the ones a cold start can hit — and not every intermediate keystroke,
/// which would write one entry per analyzer per edit and never be read again.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Persist {
    Yes,
    No,
}

/// Everything needed to analyze a document, built by the binary and handed
/// to the server.
#[derive(Debug)]
pub struct Pipeline {
    languages: LanguageRegistry,
    analyzers: AnalyzerRegistry,
    store: Arc<dyn AnalysisStore>,
}

impl Pipeline {
    #[must_use]
    pub fn new(
        languages: LanguageRegistry,
        analyzers: AnalyzerRegistry,
        store: Arc<dyn AnalysisStore>,
    ) -> Self {
        Self {
            languages,
            analyzers,
            store,
        }
    }

    /// The language adapter that claims `path`, or `None` for an unsupported
    /// document (see [`LanguageRegistry::for_path`]).
    #[must_use]
    pub fn language_for(&self, path: &Path) -> Option<&dyn Language> {
        self.languages.for_path(path)
    }

    /// Run every registered analyzer over `source` and merge their results.
    ///
    /// Each analyzer is looked up in the persisted store under its own
    /// [`CacheKey`] first — the same keys `qli analyze` writes — so a cold
    /// server reuses what the CLI (or an earlier session) already computed.
    /// The source is parsed at most once, and only if some analyzer misses.
    ///
    /// Returns `None` only if the grammar can't be loaded (see
    /// [`Language::parse`]).
    #[must_use]
    pub fn analyze(
        &self,
        language: &dyn Language,
        source: &Source,
        persist: Persist,
    ) -> Option<AnalysisResult> {
        let mut tree = None;
        let mut merged = AnalysisResult::default();
        for id in self.analyzers.ids() {
            let Some(analyzer) = self.analyzers.get(id) else {
                continue;
            };
            let key = CacheKey::new(
                source,
                language.fingerprint(),
                analyzer.id(),
                analyzer.version(),
            );
            let result = if let Some(hit) = self.store.get(&key) {
                tracing::debug!(path = %source.path.display(), analyzer = analyzer.id(), "cache hit");
                hit
            } else {
                if tree.is_none() {
                    tree = Some(language.parse(source)?);
                }
                let tree = tree.as_ref()?;
                let ctx = AnalysisContext::new(language, source, tree);
                let result = analyzer.analyze(&ctx);
                if persist == Persist::Yes {
                    if let Err(err) = self.store.put(&key, &result) {
                        tracing::debug!(path = %source.path.display(), "cache write failed: {err}");
                    }
                }
                tracing::debug!(path = %source.path.display(), analyzer = analyzer.id(), "cache miss");
                result
            };
            merged.merge(result);
        }
        Some(merged)
    }
}

/// The hash the in-memory tier compares to decide whether content changed.
/// Entries are per document URI and per process, so only the language and the
/// bytes can differ between two versions of one document.
#[must_use]
pub(crate) fn content_hash(language_id: &str, content: &[u8]) -> String {
    qli_core::cache::fingerprint(&[language_id.as_bytes(), content])
}
