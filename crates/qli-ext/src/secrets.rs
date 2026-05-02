//! Secret resolution trait used by the dispatcher.
//!
//! Phase 1F freezes the trait surface; Phase 1G fills in real
//! [`OnePassword`](crate::manifest::SecretProvider::OnePassword) and
//! [`Env`](crate::manifest::SecretProvider::Env) providers. Tests in this
//! crate use a [`TestResolver`] that returns sentinel strings so the
//! "secrets never leak" regression test can drive every guard path.

use std::collections::HashMap;

use thiserror::Error;

use crate::manifest::SecretSpec;

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
}
