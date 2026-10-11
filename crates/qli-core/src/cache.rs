//! Content-hashed cache **key**.
//!
//! Purity keeps the two halves of the cache apart. This module — in the pure
//! engine — computes the key: it folds `(language_id, blake3(content),
//! analyzer_id, analyzer_version)` into a stable content hash and the relative
//! on-disk path an entry lives at. The **store** (reading and writing those
//! files under `$XDG_CACHE_HOME`) is filesystem I/O, so it lives in the `qli`
//! binary, not here.
//!
//! Layout produced by [`CacheKey::relative_path`]:
//! `<analyzer_id>/<analyzer_version>/<hash-prefix>/<hash>.json`. Bucketing by
//! `analyzer_id`/`analyzer_version` makes a stale analyzer version's entries
//! trivially purgeable (delete the directory); the two-hex-char prefix keeps
//! any single directory from ballooning.

use std::path::PathBuf;

/// The inputs that determine whether two analysis runs may share a result.
///
/// Change any field and the computed [`content_hash`](CacheKey::content_hash)
/// or path changes, so the old entry is never read. In particular bumping an
/// analyzer's `analyzer_version` reroutes it to a fresh directory — old entries
/// are ignored without touching disk.
#[derive(Clone, Copy, Debug)]
pub struct CacheKey<'a> {
    pub language_id: &'a str,
    pub content: &'a [u8],
    pub analyzer_id: &'a str,
    pub analyzer_version: u32,
}

impl<'a> CacheKey<'a> {
    #[must_use]
    pub fn new(
        language_id: &'a str,
        content: &'a [u8],
        analyzer_id: &'a str,
        analyzer_version: u32,
    ) -> Self {
        Self {
            language_id,
            content,
            analyzer_id,
            analyzer_version,
        }
    }

    /// Hex blake3 digest folding the language id and the content. The analyzer
    /// id/version are *not* in the hash — they bucket the path instead — but
    /// the language is, so identical bytes analyzed as two languages don't
    /// collide.
    #[must_use]
    pub fn content_hash(&self) -> String {
        let mut hasher = blake3::Hasher::new();
        hasher.update(self.language_id.as_bytes());
        // NUL separator so `("ab", "c")` and `("a", "bc")` can't alias.
        hasher.update(&[0u8]);
        hasher.update(self.content);
        hasher.finalize().to_hex().to_string()
    }

    /// Path of this entry relative to the cache root, e.g.
    /// `todofixme/3/ab/ab12…ef.json`.
    #[must_use]
    pub fn relative_path(&self) -> PathBuf {
        let hash = self.content_hash();
        let prefix = &hash[..2];
        PathBuf::from(self.analyzer_id)
            .join(self.analyzer_version.to_string())
            .join(prefix)
            .join(format!("{hash}.json"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_is_stable() {
        let key = CacheKey::new("python", b"print(1)", "todofixme", 1);
        assert_eq!(key.content_hash(), key.content_hash());
    }

    #[test]
    fn different_content_differs() {
        let a = CacheKey::new("python", b"a", "todofixme", 1).content_hash();
        let b = CacheKey::new("python", b"b", "todofixme", 1).content_hash();
        assert_ne!(a, b);
    }

    #[test]
    fn different_language_differs() {
        let a = CacheKey::new("python", b"x", "todofixme", 1).content_hash();
        let b = CacheKey::new("typescript", b"x", "todofixme", 1).content_hash();
        assert_ne!(a, b);
    }

    #[test]
    fn language_content_boundary_is_unambiguous() {
        // Without the separator these would hash identically.
        let a = CacheKey::new("ab", b"c", "id", 1).content_hash();
        let b = CacheKey::new("a", b"bc", "id", 1).content_hash();
        assert_ne!(a, b);
    }

    #[test]
    fn analyzer_version_reroutes_path_without_changing_hash() {
        let v1 = CacheKey::new("python", b"x", "todofixme", 1);
        let v2 = CacheKey::new("python", b"x", "todofixme", 2);
        assert_eq!(v1.content_hash(), v2.content_hash());
        assert_ne!(v1.relative_path(), v2.relative_path());
    }

    #[test]
    fn relative_path_shape() {
        let key = CacheKey::new("python", b"x", "todofixme", 3);
        let path = key.relative_path();
        let hash = key.content_hash();
        assert_eq!(
            path,
            PathBuf::from("todofixme")
                .join("3")
                .join(&hash[..2])
                .join(format!("{hash}.json"))
        );
    }
}
