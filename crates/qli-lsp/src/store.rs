//! The handle to the persisted tier of the analysis cache.

use qli_core::cache::CacheKey;
use qli_core::AnalysisResult;

/// A content-addressed store of [`AnalysisResult`]s, keyed by
/// [`CacheKey`] — the same key `qli analyze` uses, so the CLI and the server
/// read and write the same entries.
///
/// The store itself is filesystem I/O under an XDG path, so it lives in the
/// binary (`AnalysisCache`); this trait is how it is injected. A disabled
/// store answers every `get` with `None` and accepts every `put` as a no-op.
pub trait AnalysisStore: std::fmt::Debug + Send + Sync {
    /// Look up a result. `None` on a miss or an unreadable entry.
    fn get(&self, key: &CacheKey<'_>) -> Option<AnalysisResult>;

    /// Persist a result. Best-effort: the analysis already succeeded, so a
    /// failure is reported to the caller to log, never fatal.
    fn put(&self, key: &CacheKey<'_>, result: &AnalysisResult) -> std::io::Result<()>;
}
