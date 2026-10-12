//! The in-memory tier of the analysis cache: the most recent result per
//! document, bounded by least-recent use.

use std::collections::HashMap;

use qli_core::AnalysisResult;

use crate::DocumentUri;

/// How many documents keep their latest analysis in memory before the least
/// recently used one is dropped.
pub const DEFAULT_DOCUMENT_CAPACITY: usize = 200;

/// Per-document LRU of `(content_hash, AnalysisResult)`.
///
/// One entry per document URI holds the result for that document's most
/// recently analyzed content. A lookup hits only when the caller's content
/// hash matches, which is what lets an unchanged `didChange` skip analysis.
///
/// Eviction scans for the oldest entry rather than keeping an ordered list:
/// it runs only on an insert past capacity, over at most `capacity + 1`
/// entries.
#[derive(Debug)]
pub struct DocumentCache {
    capacity: usize,
    /// Monotonic use counter; an entry's `last_used` is its value at the
    /// entry's latest hit or insert.
    clock: u64,
    entries: HashMap<DocumentUri, Entry>,
}

#[derive(Debug)]
struct Entry {
    content_hash: String,
    result: AnalysisResult,
    last_used: u64,
}

impl DocumentCache {
    /// A cache holding at most `capacity` documents (at least one).
    #[must_use]
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity: capacity.max(1),
            clock: 0,
            entries: HashMap::new(),
        }
    }

    /// The cached result for `uri` if it was computed from content with this
    /// `content_hash`. A hit marks the document as most recently used.
    pub fn get(&mut self, uri: &DocumentUri, content_hash: &str) -> Option<&AnalysisResult> {
        let entry = self.entries.get_mut(uri)?;
        if entry.content_hash != content_hash {
            return None;
        }
        self.clock += 1;
        entry.last_used = self.clock;
        Some(&entry.result)
    }

    /// Record `result` as the latest analysis of `uri`, replacing any earlier
    /// entry for it, then evict the least recently used document if the cache
    /// is over capacity.
    pub fn insert(&mut self, uri: DocumentUri, content_hash: String, result: AnalysisResult) {
        self.clock += 1;
        self.entries.insert(
            uri,
            Entry {
                content_hash,
                result,
                last_used: self.clock,
            },
        );
        if self.entries.len() > self.capacity {
            let oldest = self
                .entries
                .iter()
                .min_by_key(|(_, entry)| entry.last_used)
                .map(|(uri, _)| uri.clone());
            if let Some(oldest) = oldest {
                self.entries.remove(&oldest);
            }
        }
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

impl Default for DocumentCache {
    fn default() -> Self {
        Self::new(DEFAULT_DOCUMENT_CAPACITY)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn uri(name: &str) -> DocumentUri {
        // tower-lsp-server swap: `Url::parse` becomes `Uri::from_str`.
        DocumentUri::parse(&format!("file:///tmp/{name}")).unwrap()
    }

    #[test]
    fn hit_requires_matching_content_hash() {
        let mut cache = DocumentCache::default();
        cache.insert(uri("a.py"), "h1".to_owned(), AnalysisResult::default());
        assert!(cache.get(&uri("a.py"), "h1").is_some());
        assert!(cache.get(&uri("a.py"), "h2").is_none(), "content changed");
        assert!(cache.get(&uri("b.py"), "h1").is_none(), "other document");
    }

    #[test]
    fn insert_replaces_the_document_entry() {
        let mut cache = DocumentCache::default();
        cache.insert(uri("a.py"), "h1".to_owned(), AnalysisResult::default());
        cache.insert(uri("a.py"), "h2".to_owned(), AnalysisResult::default());
        assert_eq!(cache.len(), 1);
        assert!(cache.get(&uri("a.py"), "h1").is_none());
        assert!(cache.get(&uri("a.py"), "h2").is_some());
    }

    #[test]
    fn evicts_least_recently_used_past_capacity() {
        let mut cache = DocumentCache::new(2);
        cache.insert(uri("a.py"), "h".to_owned(), AnalysisResult::default());
        cache.insert(uri("b.py"), "h".to_owned(), AnalysisResult::default());
        // Touch `a` so `b` is now the oldest.
        assert!(cache.get(&uri("a.py"), "h").is_some());
        cache.insert(uri("c.py"), "h".to_owned(), AnalysisResult::default());
        assert_eq!(cache.len(), 2);
        assert!(cache.get(&uri("b.py"), "h").is_none(), "evicted");
        assert!(cache.get(&uri("a.py"), "h").is_some());
        assert!(cache.get(&uri("c.py"), "h").is_some());
    }

    #[test]
    fn default_capacity_is_the_named_constant() {
        let mut cache = DocumentCache::default();
        for i in 0..=DEFAULT_DOCUMENT_CAPACITY {
            cache.insert(
                uri(&format!("{i}.py")),
                "h".to_owned(),
                AnalysisResult::default(),
            );
        }
        assert_eq!(cache.len(), DEFAULT_DOCUMENT_CAPACITY);
    }
}
