//! Content-hashed cache **key**.
//!
//! Purity keeps the two halves of the cache apart. This module — in the pure
//! engine — computes the key: it folds every input an analysis result depends on
//! — the whole [`Source`] (language, path, bytes), the language adapter's
//! fingerprint, and the analyzer id/version — into a stable content hash and
//! the relative on-disk path an entry lives at. The **store** (reading and writing those
//! files under `$XDG_CACHE_HOME`) is filesystem I/O, so it lives in the `qli`
//! binary, not here.
//!
//! Layout produced by [`CacheKey::relative_path`]:
//! `<analyzer_id>/<analyzer_version>/<hash-prefix>/<hash>.json`. Bucketing by
//! `analyzer_id`/`analyzer_version` makes a stale analyzer version's entries
//! trivially purgeable (delete the directory); the two-hex-char prefix keeps
//! any single directory from ballooning.

use std::path::PathBuf;

use crate::Source;

/// Hex blake3 digest of `parts`. Each part is length-prefixed, so
/// `["ab", "c"]` and `["a", "bc"]` can't alias.
#[must_use]
pub fn fingerprint(parts: &[&[u8]]) -> String {
    let mut hasher = blake3::Hasher::new();
    for part in parts {
        hasher.update(&(part.len() as u64).to_le_bytes());
        hasher.update(part);
    }
    hasher.finalize().to_hex().to_string()
}

/// The inputs that determine whether two analysis runs may share a result.
///
/// Change any field and the computed [`content_hash`](CacheKey::content_hash)
/// or path changes, so the old entry is never read. In particular bumping an
/// analyzer's `analyzer_version` reroutes it to a fresh directory — old entries
/// are ignored without touching disk.
#[derive(Clone, Copy, Debug)]
pub struct CacheKey<'a> {
    /// The analyzed source. All of it is keyed, the path included: analyzers
    /// receive the path and may bake it into their output (`DefRefs` symbol
    /// ids do), so two files with identical bytes must not share an entry.
    pub source: &'a Source,
    /// The language adapter's fingerprint (grammar + queries): an adapter
    /// change alters results without any analyzer changing.
    pub language_fingerprint: &'a str,
    pub analyzer_id: &'a str,
    pub analyzer_version: u32,
}

impl<'a> CacheKey<'a> {
    #[must_use]
    pub fn new(
        source: &'a Source,
        language_fingerprint: &'a str,
        analyzer_id: &'a str,
        analyzer_version: u32,
    ) -> Self {
        Self {
            source,
            language_fingerprint,
            analyzer_id,
            analyzer_version,
        }
    }

    /// Hex blake3 digest folding the source and the language fingerprint. The
    /// analyzer id/version are *not* in the hash — they bucket the path
    /// instead.
    #[must_use]
    pub fn content_hash(&self) -> String {
        fingerprint(&[
            self.source.language.as_str().as_bytes(),
            self.language_fingerprint.as_bytes(),
            self.source.path.as_os_str().as_encoded_bytes(),
            &self.source.content,
        ])
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
    use crate::LanguageId;

    fn source(language: &str, path: &str, content: &[u8]) -> Source {
        Source::new(path.into(), content.to_vec(), LanguageId::from(language))
    }

    fn hash(language: &str, fingerprint: &str, path: &str, content: &[u8]) -> String {
        CacheKey::new(
            &source(language, path, content),
            fingerprint,
            "todofixme",
            1,
        )
        .content_hash()
    }

    #[test]
    fn hash_is_stable() {
        assert_eq!(
            hash("python", "f", "a.py", b"print(1)"),
            hash("python", "f", "a.py", b"print(1)")
        );
    }

    #[test]
    fn every_input_changes_the_hash() {
        let base = hash("python", "f", "a.py", b"x");
        assert_ne!(base, hash("typescript", "f", "a.py", b"x"), "language");
        assert_ne!(base, hash("python", "g", "a.py", b"x"), "fingerprint");
        assert_ne!(base, hash("python", "f", "b.py", b"x"), "path");
        assert_ne!(base, hash("python", "f", "a.py", b"y"), "content");
    }

    #[test]
    fn part_boundaries_are_unambiguous() {
        // Without the length prefixes these would hash identically.
        assert_ne!(fingerprint(&[b"ab", b"c"]), fingerprint(&[b"a", b"bc"]));
        assert_ne!(hash("ab", "c", "p", b"x"), hash("a", "bc", "p", b"x"));
    }

    #[test]
    fn analyzer_version_reroutes_path_without_changing_hash() {
        let source = source("python", "a.py", b"x");
        let v1 = CacheKey::new(&source, "f", "todofixme", 1);
        let v2 = CacheKey::new(&source, "f", "todofixme", 2);
        assert_eq!(v1.content_hash(), v2.content_hash());
        assert_ne!(v1.relative_path(), v2.relative_path());
    }

    #[test]
    fn relative_path_shape() {
        let source = source("python", "a.py", b"x");
        let key = CacheKey::new(&source, "f", "todofixme", 3);
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
