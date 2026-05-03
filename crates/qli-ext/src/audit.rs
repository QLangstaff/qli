//! Audit log records for the dispatcher.
//!
//! Each dispatched extension produces two log records (start + finish, or
//! start + interrupted). Records are JSON Lines: one [`AuditEvent`] per
//! line, atomically appended to the manifest's `audit_log` path.
//!
//! The schema is stable at the field level for Phase 1 (pre-1.0): consumers
//! that parse JSONL can treat any unknown future fields as additive. Renames
//! or removals will bump the manifest schema version.
//!
//! Secrets never appear in this stream. The `Start` event carries
//! `env_var_names` (the *names* of injected secret env vars) so an audit
//! reader can confirm which secrets were in scope without seeing values.

use std::collections::HashMap;
use std::fs::OpenOptions;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::Serialize;
use thiserror::Error;

/// One JSONL record in an audit log.
#[derive(Debug, Serialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum AuditEvent {
    /// Recorded right before the extension is spawned, after secrets are
    /// resolved. `env_var_names` lists the names of secret env vars about to
    /// be set in the child — never the values.
    Start {
        timestamp: DateTime<Utc>,
        user: String,
        group: String,
        extension: String,
        args: Vec<String>,
        env_var_names: Vec<String>,
    },
    /// Recorded after the child exits normally (or with a non-zero status
    /// the child chose itself).
    Finish {
        timestamp: DateTime<Utc>,
        group: String,
        extension: String,
        exit_code: i32,
        duration_ms: u128,
    },
    /// Recorded when the dispatcher observes a SIGINT/SIGTERM and tears the
    /// child down. `signal` is the conventional name (`SIGINT` / `SIGTERM`).
    Interrupted {
        timestamp: DateTime<Utc>,
        group: String,
        extension: String,
        signal: String,
        exit_code: i32,
        duration_ms: u128,
    },
}

/// Error raised while resolving or writing an audit log path.
#[derive(Debug, Error)]
pub enum AuditError {
    #[error("could not expand audit_log path {literal:?}: {source}")]
    Expand {
        literal: String,
        #[source]
        source: shellexpand::LookupError<std::env::VarError>,
    },
    #[error("could not create audit log directory {path:?}: {source}")]
    CreateDir {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("could not write audit log {path:?}: {source}")]
    Write {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("could not serialize audit event: {0}")]
    Serialize(#[from] serde_json::Error),
}

/// Expand a manifest `audit_log` literal into a concrete path.
///
/// Supports `~` and `$VAR` (or `${VAR}`) expansion. `defaults` is consulted
/// only when an env var is unset in the process environment — typically the
/// dispatcher passes the resolved XDG defaults so a manifest written as
/// `"$XDG_STATE_HOME/qli/prod-audit.log"` works even if the user hasn't
/// explicitly exported `XDG_STATE_HOME`.
pub fn expand_path<S: ::std::hash::BuildHasher>(
    literal: &str,
    defaults: &HashMap<String, String, S>,
) -> Result<PathBuf, AuditError> {
    let lookup = |name: &str| -> Result<Option<String>, std::env::VarError> {
        match std::env::var(name) {
            Ok(v) if !v.is_empty() => Ok(Some(v)),
            Ok(_) | Err(std::env::VarError::NotPresent) => match defaults.get(name) {
                Some(d) => Ok(Some(d.clone())),
                // No fallback either: error so an unexpanded `$VAR` doesn't
                // become a literal path component (would leak `$VAR` into
                // the filesystem layout, never what the manifest meant).
                None => Err(std::env::VarError::NotPresent),
            },
            Err(e) => Err(e),
        }
    };
    let expanded =
        shellexpand::full_with_context(literal, dirs::home_dir, lookup).map_err(|e| {
            AuditError::Expand {
                literal: literal.to_owned(),
                source: e,
            }
        })?;
    Ok(PathBuf::from(expanded.into_owned()))
}

/// Append an event as one JSON line to `path`. Creates parent directories as
/// needed.
///
/// Concurrent dispatchers must not interleave lines. Unix `O_APPEND` is
/// per-`write` atomic only up to `PIPE_BUF` (4096 on Linux, 512 on macOS),
/// and a record with several secret env names + a long arg list can exceed
/// that. So on Unix we additionally take an exclusive `flock` for the write.
/// The kernel releases the lock when the file descriptor closes at the end
/// of this function.
pub fn append(path: &Path, event: &AuditEvent) -> Result<(), AuditError> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent).map_err(|source| AuditError::CreateDir {
                path: parent.to_path_buf(),
                source,
            })?;
        }
    }
    let mut line = serde_json::to_vec(event)?;
    line.push(b'\n');
    let file = OpenOptions::new()
        .append(true)
        .create(true)
        .open(path)
        .map_err(|source| AuditError::Write {
            path: path.to_path_buf(),
            source,
        })?;
    write_locked(path, file, &line)
}

#[cfg(unix)]
fn write_locked(path: &Path, file: std::fs::File, line: &[u8]) -> Result<(), AuditError> {
    use nix::fcntl::{Flock, FlockArg};
    let mut guard =
        Flock::lock(file, FlockArg::LockExclusive).map_err(|(_, errno)| AuditError::Write {
            path: path.to_path_buf(),
            source: std::io::Error::from_raw_os_error(errno as i32),
        })?;
    guard.write_all(line).map_err(|source| AuditError::Write {
        path: path.to_path_buf(),
        source,
    })?;
    Ok(())
    // `guard` drops here → kernel releases the lock and closes the fd.
}

#[cfg(not(unix))]
fn write_locked(path: &Path, mut file: std::fs::File, line: &[u8]) -> Result<(), AuditError> {
    file.write_all(line).map_err(|source| AuditError::Write {
        path: path.to_path_buf(),
        source,
    })
}

/// Best-effort current-user discovery. Reads `USER` (Unix) or `USERNAME`
/// (Windows-style) from the process environment, falling back to `"unknown"`.
/// Audit records are not authentication; this is only a hint.
#[must_use]
pub fn current_user() -> String {
    std::env::var("USER")
        .or_else(|_| std::env::var("USERNAME"))
        .unwrap_or_else(|_| "unknown".into())
}

mod dirs {
    //! Tiny stub so `shellexpand::full_with_context` has a tilde resolver
    //! without dragging in the `dirs` / `directories` crates. We mirror their
    //! behaviour: read `$HOME` on Unix, `%USERPROFILE%` on Windows.
    //! `shellexpand` requires `AsRef<str>`, so we return `String`; non-UTF-8
    //! home directories yield `None` (rare but well-defined).

    pub fn home_dir() -> Option<String> {
        #[cfg(unix)]
        {
            std::env::var("HOME").ok().filter(|s| !s.is_empty())
        }
        #[cfg(windows)]
        {
            std::env::var("USERPROFILE").ok().filter(|s| !s.is_empty())
        }
        #[cfg(not(any(unix, windows)))]
        {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;

    #[test]
    #[serial]
    fn expand_uses_process_env_first() {
        let mut defaults = HashMap::new();
        defaults.insert("QLI_TEST_AUDIT_VAR".into(), "from-defaults".into());
        // Set the env var so it overrides defaults.
        std::env::set_var("QLI_TEST_AUDIT_VAR", "from-env");
        let p = expand_path("$QLI_TEST_AUDIT_VAR/file.log", &defaults).unwrap();
        assert_eq!(p, PathBuf::from("from-env/file.log"));
        std::env::remove_var("QLI_TEST_AUDIT_VAR");
    }

    #[test]
    #[serial]
    fn expand_falls_back_to_defaults_when_env_unset() {
        let mut defaults = HashMap::new();
        defaults.insert("QLI_TEST_AUDIT_UNSET".into(), "from-defaults".into());
        std::env::remove_var("QLI_TEST_AUDIT_UNSET");
        let p = expand_path("$QLI_TEST_AUDIT_UNSET/file.log", &defaults).unwrap();
        assert_eq!(p, PathBuf::from("from-defaults/file.log"));
    }

    #[test]
    #[serial]
    fn expand_errors_on_unset_var_with_no_default() {
        let defaults = HashMap::new();
        std::env::remove_var("QLI_TEST_AUDIT_MISSING");
        let err = expand_path("$QLI_TEST_AUDIT_MISSING/x", &defaults).unwrap_err();
        assert!(matches!(err, AuditError::Expand { .. }), "got {err:?}");
    }

    #[test]
    fn expand_handles_literal_path_unchanged() {
        let defaults = HashMap::new();
        let p = expand_path("/var/log/qli/audit.log", &defaults).unwrap();
        assert_eq!(p, PathBuf::from("/var/log/qli/audit.log"));
    }

    #[test]
    fn append_writes_one_jsonl_line_per_event() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("nested/audit.log");
        let event = AuditEvent::Start {
            timestamp: DateTime::<Utc>::default(),
            user: "tester".into(),
            group: "dev".into(),
            extension: "hello".into(),
            args: vec!["--flag".into()],
            env_var_names: vec!["TOKEN".into()],
        };
        append(&path, &event).unwrap();
        append(
            &path,
            &AuditEvent::Finish {
                timestamp: DateTime::<Utc>::default(),
                group: "dev".into(),
                extension: "hello".into(),
                exit_code: 0,
                duration_ms: 12,
            },
        )
        .unwrap();
        let body = std::fs::read_to_string(&path).unwrap();
        let lines: Vec<&str> = body.lines().collect();
        assert_eq!(lines.len(), 2);
        let first: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
        assert_eq!(first["event"], "start");
        assert_eq!(first["env_var_names"][0], "TOKEN");
        let second: serde_json::Value = serde_json::from_str(lines[1]).unwrap();
        assert_eq!(second["event"], "finish");
        assert_eq!(second["exit_code"], 0);
    }

    #[test]
    fn interrupted_event_serializes_with_signal_field() {
        let event = AuditEvent::Interrupted {
            timestamp: DateTime::<Utc>::default(),
            group: "prod".into(),
            extension: "deploy".into(),
            signal: "SIGINT".into(),
            exit_code: 130,
            duration_ms: 3,
        };
        let v: serde_json::Value =
            serde_json::from_str(&serde_json::to_string(&event).unwrap()).unwrap();
        assert_eq!(v["event"], "interrupted");
        assert_eq!(v["signal"], "SIGINT");
        assert_eq!(v["exit_code"], 130);
    }
}
