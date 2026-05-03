//! Secret resolution trait used by the dispatcher.
//!
//! Phase 1F froze the trait surface; Phase 1G filled in the real
//! [`OnePassword`](crate::manifest::SecretProvider::OnePassword) and
//! [`Env`](crate::manifest::SecretProvider::Env) providers. Tests in this
//! crate use a [`TestResolver`] that returns sentinel strings so the
//! "secrets never leak" regression test can drive every guard path.
//!
//! Providers fail closed: any resolution error short-circuits
//! [`SecretsResolver::resolve_all`] and the dispatcher aborts before
//! spawning the child. Resolved values are never logged through
//! [`tracing`]; the audit log records only env-var names.

use std::collections::HashMap;
use std::env::VarError;
use std::io;
use std::process::{Command, Output};

use thiserror::Error;

use crate::manifest::{SecretProvider, SecretSpec};

/// A resolved secret pair (env-var name, value).
#[derive(Debug, Clone)]
pub struct ResolvedSecret {
    pub env: String,
    pub value: String,
}

/// Errors a [`SecretsResolver`] may raise. Variants are deliberately broad —
/// callers surface them with manifest context.
#[derive(Debug, Error)]
pub enum SecretsError {
    #[error("could not resolve secret for env `{env}` via {provider}: {message}")]
    Resolution {
        env: String,
        provider: &'static str,
        message: String,
    },
    #[error("provider tool not available for env `{env}`: {message}")]
    ProviderUnavailable {
        env: String,
        provider: &'static str,
        message: String,
    },
}

/// Strategy for fetching secret values.
///
/// Implementations must be deterministic for a given input (the dispatcher
/// resolves all secrets up-front and fails closed on the first error). They
/// must not log resolved values — only references.
pub trait SecretsResolver {
    /// Resolve every secret in `specs`. On any failure, return immediately
    /// — the dispatcher discards everything and aborts before spawning.
    fn resolve_all(&self, specs: &[SecretSpec]) -> Result<Vec<ResolvedSecret>, SecretsError>;
}

/// In-process resolver used in tests. Backed by a fixed map keyed on the
/// secret's `ref` field. Returns [`SecretsError::Resolution`] for any spec
/// whose reference isn't in the map.
#[derive(Debug, Default, Clone)]
pub struct TestResolver {
    by_ref: HashMap<String, String>,
}

impl TestResolver {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn with(mut self, reference: impl Into<String>, value: impl Into<String>) -> Self {
        self.by_ref.insert(reference.into(), value.into());
        self
    }
}

impl SecretsResolver for TestResolver {
    fn resolve_all(&self, specs: &[SecretSpec]) -> Result<Vec<ResolvedSecret>, SecretsError> {
        specs
            .iter()
            .map(|spec| {
                self.by_ref
                    .get(&spec.reference)
                    .map(|value| ResolvedSecret {
                        env: spec.env.clone(),
                        value: value.clone(),
                    })
                    .ok_or_else(|| SecretsError::Resolution {
                        env: spec.env.clone(),
                        provider: "test",
                        message: format!("no fixture for ref `{}`", spec.reference),
                    })
            })
            .collect()
    }
}

/// Production resolver that dispatches each [`SecretSpec`] to the right
/// provider. Used by the `qli` binary; library callers can keep using
/// [`TestResolver`] (or write their own) for unit tests.
#[derive(Debug, Default, Clone, Copy)]
pub struct ProductionResolver;

impl ProductionResolver {
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

impl SecretsResolver for ProductionResolver {
    fn resolve_all(&self, specs: &[SecretSpec]) -> Result<Vec<ResolvedSecret>, SecretsError> {
        specs
            .iter()
            .map(|spec| match spec.provider {
                SecretProvider::OnePassword => resolve_one_password(spec),
                SecretProvider::Env => resolve_env(spec),
            })
            .collect()
    }
}

/// `Env` provider: read `spec.reference` from the dispatcher's environment
/// and bind the value into the child under `spec.env`.
fn resolve_env(spec: &SecretSpec) -> Result<ResolvedSecret, SecretsError> {
    match std::env::var(&spec.reference) {
        Ok(value) => Ok(ResolvedSecret {
            env: spec.env.clone(),
            value,
        }),
        Err(VarError::NotPresent) => Err(SecretsError::Resolution {
            env: spec.env.clone(),
            provider: "env",
            message: format!("env var `{}` is not set", spec.reference),
        }),
        Err(VarError::NotUnicode(_)) => Err(SecretsError::Resolution {
            env: spec.env.clone(),
            provider: "env",
            message: format!("env var `{}` is not valid Unicode", spec.reference),
        }),
    }
}

/// `OnePassword` provider: spawn `op read <reference>` and capture stdout.
fn resolve_one_password(spec: &SecretSpec) -> Result<ResolvedSecret, SecretsError> {
    let result = Command::new("op").arg("read").arg(&spec.reference).output();
    parse_op_output(spec, result)
}

/// Map a captured `op read` invocation result into a [`ResolvedSecret`] or a
/// targeted [`SecretsError`]. Split out from [`resolve_one_password`] so unit
/// tests can construct `io::Result<Output>` values directly and exercise
/// every error branch without a fake `op` binary on PATH.
pub(crate) fn parse_op_output(
    spec: &SecretSpec,
    result: io::Result<Output>,
) -> Result<ResolvedSecret, SecretsError> {
    let output = match result {
        Ok(out) => out,
        Err(err) if err.kind() == io::ErrorKind::NotFound => {
            return Err(SecretsError::ProviderUnavailable {
                env: spec.env.clone(),
                provider: "one_password",
                message: "`op` not found on PATH; install the 1Password CLI \
                          and run `op signin`, then retry"
                    .into(),
            });
        }
        Err(err) => {
            return Err(SecretsError::ProviderUnavailable {
                env: spec.env.clone(),
                provider: "one_password",
                message: format!("could not spawn `op read`: {err}"),
            });
        }
    };

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let trimmed = stderr.trim();
        let hint = "is `op` signed in? run `op signin` and retry";
        let message = if trimmed.is_empty() {
            format!("`op read` failed (status: {}); {hint}", output.status)
        } else {
            format!("`op read` failed: {trimmed} ({hint})")
        };
        return Err(SecretsError::Resolution {
            env: spec.env.clone(),
            provider: "one_password",
            message,
        });
    }

    let Ok(mut value) = String::from_utf8(output.stdout) else {
        return Err(SecretsError::Resolution {
            env: spec.env.clone(),
            provider: "one_password",
            message: "secret value returned by `op read` is not valid UTF-8".into(),
        });
    };
    // `op read` terminates the value with a single trailing newline.
    // Strip exactly that — preserve any other whitespace the secret carries.
    if value.ends_with('\n') {
        value.pop();
        if value.ends_with('\r') {
            value.pop();
        }
    }

    Ok(ResolvedSecret {
        env: spec.env.clone(),
        value,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::SecretProvider;

    fn spec(env: &str, reference: &str) -> SecretSpec {
        SecretSpec {
            env: env.into(),
            reference: reference.into(),
            provider: SecretProvider::Env,
        }
    }

    fn op_spec(env: &str, reference: &str) -> SecretSpec {
        SecretSpec {
            env: env.into(),
            reference: reference.into(),
            provider: SecretProvider::OnePassword,
        }
    }

    #[test]
    fn test_resolver_returns_fixture_values() {
        let r = TestResolver::new()
            .with("ref-a", "AAA")
            .with("ref-b", "BBB");
        let specs = vec![spec("A", "ref-a"), spec("B", "ref-b")];
        let out = r.resolve_all(&specs).unwrap();
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].env, "A");
        assert_eq!(out[0].value, "AAA");
        assert_eq!(out[1].value, "BBB");
    }

    #[test]
    fn test_resolver_errors_on_missing_ref() {
        let resolver = TestResolver::new();
        let err = resolver.resolve_all(&[spec("A", "missing")]).unwrap_err();
        match err {
            SecretsError::Resolution { env, .. } => assert_eq!(env, "A"),
            SecretsError::ProviderUnavailable { .. } => panic!("expected Resolution, got {err:?}"),
        }
    }

    // ----- Env provider --------------------------------------------------
    //
    // Two layers of isolation, both deliberate:
    //
    //   1. Unique env var names per test (`QLI_ENV_PROVIDER_TEST_*`) — easy
    //      to grep for, hard to collide accidentally.
    //   2. `#[serial_test::serial]` — hard serialization across env-mutating
    //      tests in this binary. Phase 1L added this layer because integration
    //      tests under `tests/` (and `tests/common/mod.rs::XdgSandbox`) also
    //      mutate env, and unique-name discipline can't protect against a
    //      careless future test that forgets it.

    #[test]
    #[serial_test::serial]
    fn env_provider_reads_reference_writes_env() {
        // Crucial: env != reference, so a future swap fails this test.
        let var = "QLI_ENV_PROVIDER_TEST_READ";
        std::env::set_var(var, "value-from-host");
        let s = SecretSpec {
            env: "TARGET_ENV".into(),
            reference: var.into(),
            provider: SecretProvider::Env,
        };
        let resolved = resolve_env(&s).unwrap();
        std::env::remove_var(var);
        assert_eq!(resolved.env, "TARGET_ENV");
        assert_eq!(resolved.value, "value-from-host");
    }

    #[test]
    #[serial_test::serial]
    fn env_provider_errors_when_reference_unset() {
        let var = "QLI_ENV_PROVIDER_TEST_MISSING";
        std::env::remove_var(var);
        let s = SecretSpec {
            env: "TARGET_ENV".into(),
            reference: var.into(),
            provider: SecretProvider::Env,
        };
        match resolve_env(&s).unwrap_err() {
            SecretsError::Resolution {
                env,
                provider,
                message,
            } => {
                assert_eq!(env, "TARGET_ENV");
                assert_eq!(provider, "env");
                assert!(message.contains(var), "message: {message}");
            }
            err @ SecretsError::ProviderUnavailable { .. } => {
                panic!("expected Resolution, got {err:?}")
            }
        }
    }

    #[test]
    #[serial_test::serial]
    fn production_resolver_dispatches_per_spec_provider() {
        let var = "QLI_ENV_PROVIDER_TEST_DISPATCH";
        std::env::set_var(var, "DISPATCHED");
        let resolver = ProductionResolver::new();
        let out = resolver
            .resolve_all(&[SecretSpec {
                env: "OUT".into(),
                reference: var.into(),
                provider: SecretProvider::Env,
            }])
            .expect("env provider should resolve");
        std::env::remove_var(var);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].env, "OUT");
        assert_eq!(out[0].value, "DISPATCHED");
    }

    // ----- OnePassword provider -----------------------------------------
    //
    // Tests construct fake `io::Result<Output>` values and feed them to
    // `parse_op_output`. This exercises every error branch without
    // depending on the user's actual `op` install or PATH.
    //
    // `ExitStatus::from_raw` is unix-only — these tests are gated to
    // `#[cfg(unix)]`. The `op` CLI itself is unix-first (macOS / Linux);
    // a Windows port of these tests would need a different status
    // constructor.

    #[cfg(unix)]
    fn ok_status() -> std::process::ExitStatus {
        use std::os::unix::process::ExitStatusExt;
        std::process::ExitStatus::from_raw(0)
    }

    #[cfg(unix)]
    fn fail_status() -> std::process::ExitStatus {
        // Exit 1 — what `op read` returns when not signed in or the ref
        // doesn't resolve.
        use std::os::unix::process::ExitStatusExt;
        std::process::ExitStatus::from_raw(1 << 8)
    }

    #[test]
    #[cfg(unix)]
    fn op_provider_returns_provider_unavailable_when_op_missing() {
        let s = op_spec("TOKEN", "op://Vault/Item/field");
        let result: io::Result<Output> = Err(io::Error::new(io::ErrorKind::NotFound, "no op"));
        match parse_op_output(&s, result).unwrap_err() {
            SecretsError::ProviderUnavailable {
                env,
                provider,
                message,
            } => {
                assert_eq!(env, "TOKEN");
                assert_eq!(provider, "one_password");
                // Both pieces the user needs: which secret failed AND how
                // to fix it.
                assert!(message.contains("op"), "message: {message}");
                assert!(
                    message.contains("signin") || message.contains("install"),
                    "expected install/signin hint, got: {message}",
                );
            }
            err @ SecretsError::Resolution { .. } => {
                panic!("expected ProviderUnavailable, got {err:?}")
            }
        }
    }

    #[test]
    #[cfg(unix)]
    fn op_provider_returns_provider_unavailable_for_other_spawn_errors() {
        // Permission denied (or any non-NotFound) must not be
        // misclassified as "op missing" — it's still ProviderUnavailable
        // but with a different message.
        let s = op_spec("TOKEN", "op://Vault/Item/field");
        let result: io::Result<Output> =
            Err(io::Error::new(io::ErrorKind::PermissionDenied, "denied"));
        match parse_op_output(&s, result).unwrap_err() {
            SecretsError::ProviderUnavailable { message, .. } => {
                assert!(message.contains("could not spawn"), "message: {message}");
                assert!(message.contains("denied"), "message: {message}");
            }
            err @ SecretsError::Resolution { .. } => {
                panic!("expected ProviderUnavailable, got {err:?}")
            }
        }
    }

    #[test]
    #[cfg(unix)]
    fn op_provider_maps_nonzero_exit_to_resolution_with_signin_hint() {
        let s = op_spec("TOKEN", "op://Vault/Item/field");
        let output = Output {
            status: fail_status(),
            stdout: Vec::new(),
            stderr: b"[ERROR] not signed in\n".to_vec(),
        };
        match parse_op_output(&s, Ok(output)).unwrap_err() {
            SecretsError::Resolution {
                env,
                provider,
                message,
            } => {
                assert_eq!(env, "TOKEN");
                assert_eq!(provider, "one_password");
                assert!(message.contains("not signed in"), "message: {message}");
                assert!(
                    message.contains("op signin"),
                    "expected signin hint: {message}"
                );
            }
            err @ SecretsError::ProviderUnavailable { .. } => {
                panic!("expected Resolution, got {err:?}")
            }
        }
    }

    #[test]
    #[cfg(unix)]
    fn op_provider_handles_failure_with_empty_stderr() {
        let s = op_spec("TOKEN", "op://Vault/Item/field");
        let output = Output {
            status: fail_status(),
            stdout: Vec::new(),
            stderr: Vec::new(),
        };
        match parse_op_output(&s, Ok(output)).unwrap_err() {
            SecretsError::Resolution { message, .. } => {
                assert!(message.contains("status"), "message: {message}");
                assert!(
                    message.contains("op signin"),
                    "expected signin hint: {message}"
                );
            }
            err @ SecretsError::ProviderUnavailable { .. } => {
                panic!("expected Resolution, got {err:?}")
            }
        }
    }

    #[test]
    #[cfg(unix)]
    fn op_provider_strips_single_trailing_newline_from_value() {
        let s = op_spec("TOKEN", "op://Vault/Item/field");
        let output = Output {
            status: ok_status(),
            stdout: b"sup3r-secret\n".to_vec(),
            stderr: Vec::new(),
        };
        let resolved = parse_op_output(&s, Ok(output)).unwrap();
        assert_eq!(resolved.env, "TOKEN");
        assert_eq!(resolved.value, "sup3r-secret");
    }

    #[test]
    #[cfg(unix)]
    fn op_provider_strips_crlf_terminator() {
        let s = op_spec("TOKEN", "op://Vault/Item/field");
        let output = Output {
            status: ok_status(),
            stdout: b"sup3r-secret\r\n".to_vec(),
            stderr: Vec::new(),
        };
        let resolved = parse_op_output(&s, Ok(output)).unwrap();
        assert_eq!(resolved.value, "sup3r-secret");
    }

    #[test]
    #[cfg(unix)]
    fn op_provider_preserves_internal_newlines_and_no_terminator() {
        // Multi-line secret with no trailing newline — strip nothing.
        let s = op_spec("TOKEN", "op://Vault/Item/field");
        let output = Output {
            status: ok_status(),
            stdout: b"line-one\nline-two".to_vec(),
            stderr: Vec::new(),
        };
        let resolved = parse_op_output(&s, Ok(output)).unwrap();
        assert_eq!(resolved.value, "line-one\nline-two");
    }

    #[test]
    #[cfg(unix)]
    fn op_provider_rejects_non_utf8_value() {
        let s = op_spec("TOKEN", "op://Vault/Item/field");
        let output = Output {
            status: ok_status(),
            stdout: vec![0xff, 0xfe, 0xfd],
            stderr: Vec::new(),
        };
        match parse_op_output(&s, Ok(output)).unwrap_err() {
            SecretsError::Resolution {
                env,
                provider,
                message,
            } => {
                assert_eq!(env, "TOKEN");
                assert_eq!(provider, "one_password");
                assert!(message.contains("UTF-8"), "message: {message}");
            }
            err @ SecretsError::ProviderUnavailable { .. } => {
                panic!("expected Resolution, got {err:?}")
            }
        }
    }
}
