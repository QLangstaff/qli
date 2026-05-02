//! Strict XDG-compliant path resolution for qli.
//!
//! Uses `etcetera::base_strategy::Xdg` so paths are XDG-style on every
//! platform — including macOS, where the OS-native convention would be
//! `~/Library/Application Support/qli`. The plan explicitly chose XDG.
//!
//! All paths are `<XDG base>/qli`:
//!
//! | Function       | Default location              | Override env var      |
//! |----------------|-------------------------------|-----------------------|
//! | `config_dir()` | `~/.config/qli/`              | `XDG_CONFIG_HOME`     |
//! | `cache_dir()`  | `~/.cache/qli/`               | `XDG_CACHE_HOME`      |
//! | `state_dir()`  | `~/.local/state/qli/`         | `XDG_STATE_HOME`      |
//! | `data_dir()`   | `~/.local/share/qli/`         | `XDG_DATA_HOME`       |

use std::path::PathBuf;

use anyhow::{Context, Result, anyhow};
use etcetera::BaseStrategy;
use etcetera::base_strategy::Xdg;

const APP: &str = "qli";

fn strategy() -> Result<Xdg> {
    Xdg::new().context("could not determine XDG paths (is HOME set?)")
}

pub fn config_dir() -> Result<PathBuf> {
    Ok(strategy()?.config_dir().join(APP))
}

pub fn cache_dir() -> Result<PathBuf> {
    Ok(strategy()?.cache_dir().join(APP))
}

pub fn data_dir() -> Result<PathBuf> {
    Ok(strategy()?.data_dir().join(APP))
}

pub fn state_dir() -> Result<PathBuf> {
    let state = strategy()?
        .state_dir()
        .ok_or_else(|| anyhow!("no state directory available on this platform"))?;
    Ok(state.join(APP))
}

/// Best-effort: create all four standard directories. Logs warnings on
/// failure rather than erroring — individual operations later (cache write,
/// audit-log append) handle their own errors with full context.
pub fn ensure_all() {
    for (label, result) in [
        ("config", config_dir()),
        ("cache", cache_dir()),
        ("state", state_dir()),
        ("data", data_dir()),
    ] {
        match result {
            Ok(dir) => {
                if let Err(err) = std::fs::create_dir_all(&dir) {
                    tracing::warn!("could not create {label} dir at {}: {err}", dir.display());
                } else {
                    tracing::trace!("ensured {label} dir: {}", dir.display());
                }
            }
            Err(err) => tracing::warn!("could not resolve {label} dir: {err:#}"),
        }
    }
}
