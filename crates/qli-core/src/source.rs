//! The unit of analysis: a `Source` (path + bytes + language) and the
//! `LanguageId` that keys the language registry.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Stable identifier for a language, e.g. `"python"` or `"typescript"`.
///
/// The registry ([`qli-lang`](../qli_lang/index.html)) keys languages on this;
/// analyzers use it to pick per-language behavior. Kept as an owned `String`
/// (not `&'static str`) so it round-trips through serde in cached results.
#[derive(Clone, PartialEq, Eq, Hash, Debug, Serialize, Deserialize)]
pub struct LanguageId(pub String);

impl LanguageId {
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<&str> for LanguageId {
    fn from(s: &str) -> Self {
        Self(s.to_owned())
    }
}

impl From<String> for LanguageId {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl std::fmt::Display for LanguageId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// One file's worth of input to the engine.
///
/// The engine is handed bytes, never a path to read — keeping `qli-core` free
/// of filesystem I/O. Frontends (the CLI, the LSP server) are responsible for
/// reading files and constructing `Source` values.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Source {
    /// Path the content came from. Used only for reporting; the engine never
    /// opens it.
    pub path: PathBuf,
    /// Raw file bytes. `Vec<u8>` (not `String`) because tree-sitter parses
    /// bytes and not every input is valid UTF-8.
    pub content: Vec<u8>,
    /// Which language adapter should parse this source.
    pub language: LanguageId,
}

impl Source {
    #[must_use]
    pub fn new(path: PathBuf, content: Vec<u8>, language: LanguageId) -> Self {
        Self {
            path,
            content,
            language,
        }
    }
}
