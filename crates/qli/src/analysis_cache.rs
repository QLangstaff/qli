//! Filesystem-backed store for the analysis cache.
//!
//! The *key* (blake3 over language + content, bucketed by analyzer id/version)
//! is computed in the pure engine — [`qli_core::cache::CacheKey`]. This module
//! is the *store*: the disk I/O the engine deliberately doesn't do. It lives in
//! the binary so `qli-core` stays I/O-free.
//!
//! Layout: `<root>/<analyzer_id>/<analyzer_version>/<hash-prefix>/<hash>.json`,
//! where `<root>` is `$XDG_CACHE_HOME/qli/analysis`. Bucketing by
//! analyzer id/version makes a stale version's entries a single directory to
//! drop; a bumped `analyzer_version` simply reads a different, absent path
//! (cache miss) without touching the old files.
//!
//! Eviction (`clean`) walks the tree rather than maintaining a sidecar size
//! index: correct and simple at current scale. If stat-ing the tree ever
//! becomes a hot path, an atomically-updated `index.json` is the documented
//! next step (see plan Phase 2F).

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use qli_core::cache::CacheKey;
use qli_core::AnalysisResult;

/// Default total-size cap before an on-write eviction trims oldest entries.
pub const DEFAULT_SIZE_CAP_BYTES: u64 = 500 * 1024 * 1024;
/// Default age past which `clean` (no flags) drops an entry.
pub const DEFAULT_TTL_DAYS: u64 = 30;

/// A content-addressed store for [`AnalysisResult`]s under a cache root.
///
/// `enabled == false` (the `--no-cache` case) makes every read a miss and
/// every write a no-op, so callers don't special-case it.
#[derive(Debug)]
pub struct AnalysisCache {
    root: PathBuf,
    enabled: bool,
    size_cap: u64,
}

impl AnalysisCache {
    #[must_use]
    pub fn new(root: PathBuf, enabled: bool) -> Self {
        Self {
            root,
            enabled,
            size_cap: DEFAULT_SIZE_CAP_BYTES,
        }
    }

    /// Look up a cached result. `None` on miss, on a disabled cache, or on any
    /// read/parse error (a corrupt entry is treated as absent, not fatal). A
    /// hit "touches" the file's mtime for LRU-ish eviction ordering.
    #[must_use]
    pub fn get(&self, key: &CacheKey) -> Option<AnalysisResult> {
        if !self.enabled {
            return None;
        }
        let path = self.root.join(key.relative_path());
        let bytes = std::fs::read(&path).ok()?;
        let result = serde_json::from_slice(&bytes).ok()?;
        // Touch on hit; ignore failure (read-only cache dir still works).
        let _ = filetime_now(&path);
        Some(result)
    }

    /// Persist a result. No-op on a disabled cache. Best-effort: a write
    /// failure is returned so the caller can warn, but analysis already
    /// succeeded, so it's never fatal.
    ///
    /// Note: this does **not** enforce the size cap — that would stat the whole
    /// tree on every write, making a batch analyze quadratic in file count. The
    /// caller runs [`enforce_size_cap`](Self::enforce_size_cap) once after a
    /// batch instead.
    pub fn put(&self, key: &CacheKey, result: &AnalysisResult) -> std::io::Result<()> {
        if !self.enabled {
            return Ok(());
        }
        let path = self.root.join(key.relative_path());
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let bytes = serde_json::to_vec(result)?;
        std::fs::write(&path, bytes)?;
        Ok(())
    }

    /// Remove cached entries. Returns `(files_removed, bytes_reclaimed)`.
    ///
    /// - `all`: everything under the root.
    /// - `older_than`: entries whose mtime is older than the given age.
    /// - neither: the default policy — older than [`DEFAULT_TTL_DAYS`], then
    ///   enforce the size cap.
    pub fn clean(&self, all: bool, older_than: Option<Duration>) -> (u64, u64) {
        if all {
            return self.remove_matching(|_| true);
        }
        let ttl = older_than.unwrap_or(Duration::from_secs(DEFAULT_TTL_DAYS * 86_400));
        let cutoff = SystemTime::now().checked_sub(ttl);
        let removed = self.remove_matching(|mtime| match cutoff {
            Some(cutoff) => mtime < cutoff,
            None => false,
        });
        if older_than.is_none() {
            self.enforce_size_cap();
        }
        removed
    }

    /// Evict oldest-by-mtime entries until total size is under the cap. Stats
    /// the whole tree once, so call it at most once per batch (after all
    /// [`put`](Self::put)s), never per write. No-op on a disabled cache.
    pub fn enforce_size_cap(&self) {
        if !self.enabled {
            return;
        }
        let mut entries = self.entries();
        let mut total: u64 = entries.iter().map(|e| e.size).sum();
        if total <= self.size_cap {
            return;
        }
        // Oldest first.
        entries.sort_by_key(|e| e.mtime);
        for entry in entries {
            if total <= self.size_cap {
                break;
            }
            if std::fs::remove_file(&entry.path).is_ok() {
                total = total.saturating_sub(entry.size);
            }
        }
    }

    fn remove_matching(&self, pred: impl Fn(SystemTime) -> bool) -> (u64, u64) {
        let mut files = 0;
        let mut bytes = 0;
        for entry in self.entries() {
            if pred(entry.mtime) && std::fs::remove_file(&entry.path).is_ok() {
                files += 1;
                bytes += entry.size;
            }
        }
        (files, bytes)
    }

    /// Every `.json` entry under the root with its size and mtime.
    fn entries(&self) -> Vec<CacheEntry> {
        let mut out = Vec::new();
        collect_entries(&self.root, &mut out);
        out
    }
}

struct CacheEntry {
    path: PathBuf,
    size: u64,
    mtime: SystemTime,
}

fn collect_entries(dir: &Path, out: &mut Vec<CacheEntry>) {
    let Ok(read) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in read.flatten() {
        let path = entry.path();
        let Ok(meta) = entry.metadata() else { continue };
        if meta.is_dir() {
            collect_entries(&path, out);
        } else if path.extension().is_some_and(|e| e == "json") {
            out.push(CacheEntry {
                path,
                size: meta.len(),
                mtime: meta.modified().unwrap_or(SystemTime::UNIX_EPOCH),
            });
        }
    }
}

/// Set a file's mtime to now, for LRU touch-on-hit. Uses
/// [`File::set_modified`] (stable since Rust 1.75) so a hit doesn't rewrite
/// the entry's bytes. Failure is non-fatal — a read-only cache dir still
/// serves hits, it just loses LRU ordering.
fn filetime_now(path: &Path) -> std::io::Result<()> {
    let file = std::fs::File::options().write(true).open(path)?;
    file.set_modified(SystemTime::now())
}

#[cfg(test)]
mod tests {
    use super::*;
    use qli_core::{Diagnostic, LanguageId, Position, Range, Severity, Source};
    use tempfile::TempDir;

    fn source(content: &[u8]) -> Source {
        Source::new("a.py".into(), content.to_vec(), LanguageId::from("python"))
    }

    fn result() -> AnalysisResult {
        AnalysisResult {
            diagnostics: vec![Diagnostic::new(
                Range::new(Position::new(0, 0), Position::new(0, 4)),
                Severity::Info,
                "TODO",
            )],
            ..Default::default()
        }
    }

    #[test]
    fn put_then_get_roundtrips() {
        let tmp = TempDir::new().unwrap();
        let cache = AnalysisCache::new(tmp.path().join("analysis"), true);
        let source_key = source(b"x");
        let key = CacheKey::new(&source_key, "f", "todofixme", 1);
        assert!(cache.get(&key).is_none(), "cold miss");
        cache.put(&key, &result()).unwrap();
        assert_eq!(cache.get(&key), Some(result()), "warm hit");
    }

    #[test]
    fn version_bump_invalidates_without_touching_old() {
        let tmp = TempDir::new().unwrap();
        let cache = AnalysisCache::new(tmp.path().join("analysis"), true);
        let source_v1 = source(b"x");
        let v1 = CacheKey::new(&source_v1, "f", "todofixme", 1);
        cache.put(&v1, &result()).unwrap();
        let source_v2 = source(b"x");
        let v2 = CacheKey::new(&source_v2, "f", "todofixme", 2);
        assert!(cache.get(&v2).is_none(), "bumped version is a miss");
        assert!(cache.get(&v1).is_some(), "old entry untouched on disk");
    }

    #[test]
    fn content_change_invalidates() {
        let tmp = TempDir::new().unwrap();
        let cache = AnalysisCache::new(tmp.path().join("analysis"), true);
        cache
            .put(
                &CacheKey::new(&source(b"x"), "f", "todofixme", 1),
                &result(),
            )
            .unwrap();
        assert!(cache
            .get(&CacheKey::new(&source(b"y"), "f", "todofixme", 1))
            .is_none());
    }

    #[test]
    fn disabled_cache_never_stores() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().join("analysis");
        let cache = AnalysisCache::new(root.clone(), false);
        let source_key = source(b"x");
        let key = CacheKey::new(&source_key, "f", "todofixme", 1);
        cache.put(&key, &result()).unwrap();
        assert!(cache.get(&key).is_none());
        assert!(!root.exists(), "disabled cache writes nothing");
    }

    #[test]
    fn clean_all_empties_the_cache() {
        let tmp = TempDir::new().unwrap();
        let cache = AnalysisCache::new(tmp.path().join("analysis"), true);
        cache
            .put(
                &CacheKey::new(&source(b"a"), "f", "todofixme", 1),
                &result(),
            )
            .unwrap();
        cache
            .put(
                &CacheKey::new(&source(b"b"), "f", "todofixme", 1),
                &result(),
            )
            .unwrap();
        let (files, _) = cache.clean(true, None);
        assert_eq!(files, 2);
        assert!(cache
            .get(&CacheKey::new(&source(b"a"), "f", "todofixme", 1))
            .is_none());
    }
}
