//! Pre-spawn guards: banner, `requires_env`, confirm prompt.
//!
//! Each function corresponds to one numbered step in the Phase 1F dispatch
//! sequence. They are split out so unit tests can exercise each gate
//! independently and so failure paths return typed errors that the
//! dispatcher surfaces uniformly.

use std::io::IsTerminal;

use thiserror::Error;

use crate::manifest::Manifest;

/// Errors raised by pre-spawn guard checks.
#[derive(Debug, Error)]
pub enum GuardError {
    #[error("missing required env var `{key}` (manifest expects `{expected}`); set it with: export {key}={expected}")]
    EnvMissing { key: String, expected: String },
    #[error("env var `{key}` = `{actual}` does not match manifest requirement `{expected}`")]
    EnvMismatch {
        key: String,
        expected: String,
        actual: String,
    },
    #[error("`{group}` requires confirmation but stdin is not a TTY; pass --yes to run non-interactively")]
    NonInteractiveRefuse { group: String },
    #[error("user declined to proceed with `{group} {extension}`")]
    UserDeclined { group: String, extension: String },
}

/// Print the manifest banner to stderr, if any. Step 1 of the guard chain.
pub fn print_banner(manifest: &Manifest) {
    if let Some(banner) = &manifest.banner {
        eprintln!("{banner}");
    }
}

/// Step 2: enforce every `requires_env` entry.
pub fn check_requires_env(manifest: &Manifest) -> Result<(), GuardError> {
    for (key, expected) in &manifest.requires_env {
        match std::env::var(key) {
            Ok(actual) if actual == *expected => {}
            Ok(actual) => {
                return Err(GuardError::EnvMismatch {
                    key: key.clone(),
                    expected: expected.clone(),
                    actual,
                })
            }
            Err(_) => {
                return Err(GuardError::EnvMissing {
                    key: key.clone(),
                    expected: expected.clone(),
                })
            }
        }
    }
    Ok(())
}

/// Step 3: ask for confirmation if the manifest demands it.
///
/// `assume_yes` short-circuits the prompt (used by `--yes`). When stdin is
/// not a TTY and `assume_yes` is false, the dispatcher refuses rather than
/// silently proceeding.
///
/// `prompt` is injected so tests can drive a deterministic answer. Production
/// callers pass [`tty_confirm`] which uses [`dialoguer::Confirm`].
pub fn run_confirm(
    manifest: &Manifest,
    group: &str,
    extension: &str,
    assume_yes: bool,
    prompt: &dyn ConfirmPrompt,
) -> Result<(), GuardError> {
    if !manifest.confirm {
        return Ok(());
    }
    if assume_yes {
        return Ok(());
    }
    if !std::io::stdin().is_terminal() {
        return Err(GuardError::NonInteractiveRefuse {
            group: group.into(),
        });
    }
    let message = format!("Run `qli {group} {extension}`?");
    if prompt.ask(&message)? {
        Ok(())
    } else {
        Err(GuardError::UserDeclined {
            group: group.into(),
            extension: extension.into(),
        })
    }
}

/// Confirm-prompt strategy. Production code uses [`tty_confirm`]; tests pass
/// a stubbed implementation that returns a pre-set answer without touching
/// stdin/stderr.
pub trait ConfirmPrompt {
    /// Ask the user the question and return `true` for affirmative.
    /// Returning `Err` aborts the dispatch with a guard error.
    fn ask(&self, message: &str) -> Result<bool, GuardError>;
}

/// TTY-backed implementation of [`ConfirmPrompt`] using `dialoguer`.
#[derive(Debug, Default)]
pub struct TtyConfirm;

impl ConfirmPrompt for TtyConfirm {
    fn ask(&self, message: &str) -> Result<bool, GuardError> {
        // dialoguer prints to stderr and reads from stdin — both correct
        // for a CLI tool whose stdout is reserved for data.
        match dialoguer::Confirm::new()
            .with_prompt(message)
            .default(false)
            .interact()
        {
            Ok(answer) => Ok(answer),
            // dialoguer maps EOF / closed stdin to an error. Treat that as a
            // decline so the dispatcher refuses instead of surfacing an
            // I/O error. The non-TTY path is already gated above; this
            // branch handles "TTY went away mid-prompt".
            Err(_) => Ok(false),
        }
    }
}

/// Convenience constructor used by the dispatcher.
#[must_use]
pub fn tty_confirm() -> TtyConfirm {
    TtyConfirm
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest_with(banner: Option<&str>, confirm: bool, env: &[(&str, &str)]) -> Manifest {
        Manifest {
            schema_version: 1,
            description: "test".into(),
            banner: banner.map(str::to_owned),
            requires_env: env
                .iter()
                .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
                .collect(),
            confirm,
            audit_log: None,
            secrets: Vec::new(),
        }
    }

    #[test]
    fn check_requires_env_passes_when_match() {
        std::env::set_var("QLI_TEST_GUARD_OK", "yes");
        let m = manifest_with(None, false, &[("QLI_TEST_GUARD_OK", "yes")]);
        check_requires_env(&m).unwrap();
        std::env::remove_var("QLI_TEST_GUARD_OK");
    }

    #[test]
    fn check_requires_env_errors_when_missing() {
        std::env::remove_var("QLI_TEST_GUARD_MISSING");
        let m = manifest_with(None, false, &[("QLI_TEST_GUARD_MISSING", "yes")]);
        let err = check_requires_env(&m).unwrap_err();
        assert!(matches!(err, GuardError::EnvMissing { .. }));
    }

    #[test]
    fn check_requires_env_errors_when_mismatched() {
        std::env::set_var("QLI_TEST_GUARD_MISMATCH", "no");
        let m = manifest_with(None, false, &[("QLI_TEST_GUARD_MISMATCH", "yes")]);
        let err = check_requires_env(&m).unwrap_err();
        assert!(matches!(err, GuardError::EnvMismatch { .. }));
        std::env::remove_var("QLI_TEST_GUARD_MISMATCH");
    }

    struct YesPrompt;
    impl ConfirmPrompt for YesPrompt {
        fn ask(&self, _message: &str) -> Result<bool, GuardError> {
            Ok(true)
        }
    }

    struct NoPrompt;
    impl ConfirmPrompt for NoPrompt {
        fn ask(&self, _message: &str) -> Result<bool, GuardError> {
            Ok(false)
        }
    }

    #[test]
    fn run_confirm_skipped_when_disabled() {
        let m = manifest_with(None, false, &[]);
        run_confirm(&m, "dev", "hello", false, &YesPrompt).unwrap();
    }

    #[test]
    fn run_confirm_skipped_when_assume_yes() {
        let m = manifest_with(None, true, &[]);
        // NoPrompt would fail; --yes must short-circuit before asking.
        run_confirm(&m, "dev", "hello", true, &NoPrompt).unwrap();
    }

    #[test]
    fn run_confirm_declines_propagate() {
        // This test runs only when stdin is a TTY (i.e., locally, not in CI).
        // When stdin is not a TTY, the function returns NonInteractiveRefuse
        // before consulting the prompt, so we accept either decline path.
        let m = manifest_with(None, true, &[]);
        let err = run_confirm(&m, "dev", "hello", false, &NoPrompt).unwrap_err();
        assert!(
            matches!(
                err,
                GuardError::UserDeclined { .. } | GuardError::NonInteractiveRefuse { .. },
            ),
            "unexpected variant: {err:?}",
        );
    }
}
