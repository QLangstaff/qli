//! `qli lsp` — serve the Language Server Protocol for editor integration.
//!
//! This is the binary half of the LSP frontend: it resolves what a library
//! crate must not (the XDG log path, the on-disk analysis cache, the set of
//! linked grammars) and injects it into `qli-lsp`, which owns the protocol.
//!
//! # Output streams
//!
//! - `--stdio` (default): stdout carries LSP frames and nothing else; logs go
//!   to stderr, which editors surface in their language-server output pane.
//! - `--tcp <port>`: stdout and stderr both stay silent; logs append to
//!   `$XDG_STATE_HOME/qli/lsp.log`.
//!
//! `main` routes here before extension discovery, so none of the startup
//! warnings other subcommands print can reach either stream. The one thing
//! that still writes to stderr in either mode is a failure to start at all
//! (tier 1 of the diagnostic policy in `qli-ext`): `error: …`, exit 1.

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Context;
use qli_core::cache::CacheKey;
use qli_core::AnalysisResult;
use qli_lsp::{AnalysisStore, Pipeline};

use crate::analysis_cache::AnalysisCache;
use crate::{analyze, exit, logging, paths};

/// File name of the `--tcp` log under the qli state dir.
const LOG_FILE: &str = "lsp.log";

/// Run `qli lsp`. Returns a process exit code once the session ends.
pub fn run(tcp: Option<u16>, verbose: u8, quiet: bool) -> anyhow::Result<u8> {
    if tcp.is_some() {
        let path = log_path()?;
        logging::init_file(verbose, quiet, &path)
            .with_context(|| format!("could not open LSP log file at {}", path.display()))?;
    } else {
        logging::init(verbose, quiet);
    }
    paths::ensure_all();

    // The same registry and persisted cache `qli analyze` uses, so the server
    // and the CLI agree on languages and share cache entries.
    let cache = Arc::new(analyze::build_cache(false));
    let store: Arc<dyn AnalysisStore> = Arc::clone(&cache) as _;
    let pipeline = Pipeline::new(
        analyze::language_registry(),
        qli_analyzers::builtin(),
        store,
    );

    match tcp {
        Some(port) => qli_lsp::serve_tcp(pipeline, port)
            .with_context(|| format!("LSP server failed on 127.0.0.1:{port}"))?,
        None => qli_lsp::serve_stdio(pipeline).context("LSP server failed on stdio")?,
    }

    // Once per session, as `qli analyze` does once per invocation.
    cache.enforce_size_cap();
    Ok(exit::SUCCESS)
}

/// `$XDG_STATE_HOME/qli/lsp.log`, with its directory created.
fn log_path() -> anyhow::Result<PathBuf> {
    let dir = paths::state_dir().context("could not resolve XDG state dir for the LSP log")?;
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("could not create state dir at {}", dir.display()))?;
    Ok(dir.join(LOG_FILE))
}

/// The on-disk cache is the server's persisted tier. The trait lives in
/// `qli-lsp` (which can't name a binary-crate type); the impl is pure
/// delegation.
impl AnalysisStore for AnalysisCache {
    fn get(&self, key: &CacheKey<'_>) -> Option<AnalysisResult> {
        AnalysisCache::get(self, key)
    }

    fn put(&self, key: &CacheKey<'_>, result: &AnalysisResult) -> std::io::Result<()> {
        AnalysisCache::put(self, key, result)
    }
}
