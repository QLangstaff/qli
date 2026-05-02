//! Group `_manifest.toml` schema and parser.
//!
//! Manifests live at `<extensions-root>/<group>/_manifest.toml` and declare
//! how the dispatcher should treat every script in that group: banners,
//! environment requirements, confirm prompts, audit logging, and secrets
//! injection. Safety lives in the manifest, not in the script — a bash
//! one-liner gets the same protection as a Python program.
//!
//! The `audit_log` value is stored verbatim. `$XDG_STATE_HOME` / tilde
//! expansion is the dispatcher's job (Phase 1F), not the parser's — so
//! the field is a `String`, not a `PathBuf`.

use std::collections::HashMap;
use std::str::FromStr;

use serde::Deserialize;
use thiserror::Error;

/// Highest `schema_version` value this build understands.
pub const CURRENT_SCHEMA_VERSION: u32 = 1;

/// Top-level shape of a group `_manifest.toml`.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub schema_version: u32,
    pub description: String,
    #[serde(default)]
    pub banner: Option<String>,
    #[serde(default)]
    pub requires_env: HashMap<String, String>,
    #[serde(default)]
    pub confirm: bool,
    #[serde(default)]
    pub audit_log: Option<String>,
    #[serde(default)]
    pub secrets: Vec<SecretSpec>,
}

/// A single secret to resolve and inject into the extension's environment.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SecretSpec {
    /// Environment variable name to set in the extension's process.
    pub env: String,
    /// Provider-specific reference (e.g. `op://Vault/Item/field`).
    #[serde(rename = "ref")]
    pub reference: String,
    pub provider: SecretProvider,
}

/// Where a secret value is fetched from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SecretProvider {
    /// Resolved by spawning the 1Password CLI (`op read <ref>`).
    OnePassword,
    /// Read from the dispatcher's own environment via `std::env::var(<ref>)`.
    Env,
}

/// Errors surfaced from `Manifest::from_str`.
#[derive(Debug, Error)]
pub enum ManifestError {
    #[error("invalid manifest TOML: {0}")]
    Toml(#[from] toml::de::Error),
    #[error(
        "manifest schema_version {found} is newer than this qli build supports \
         ({supported}); upgrade qli or downgrade the manifest"
    )]
    SchemaVersionTooNew { found: u32, supported: u32 },
    #[error("manifest schema_version {found} is invalid (this qli build supports {supported})")]
    SchemaVersionInvalid { found: u32, supported: u32 },
    /// `[[secrets]] env` would crash `Command::env` at exec time. Caught at
    /// parse so the error points at the manifest, not at a panic deep in
    /// dispatch.
    #[error("invalid secret env name `{env}`: {reason}")]
    InvalidSecretEnv { env: String, reason: &'static str },
    /// `[[secrets]] ref` is empty or otherwise unusable. Same parse-time
    /// boundary as `InvalidSecretEnv`.
    #[error("invalid secret ref for env `{env}`: {reason}")]
    InvalidSecretRef { env: String, reason: &'static str },
}

impl FromStr for Manifest {
    type Err = ManifestError;

    /// Parse a TOML string into a `Manifest`. Validates `schema_version`
    /// against [`CURRENT_SCHEMA_VERSION`] and every `[[secrets]]` entry.
    fn from_str(input: &str) -> Result<Self, ManifestError> {
        let manifest: Manifest = toml::from_str(input)?;
        if manifest.schema_version > CURRENT_SCHEMA_VERSION {
            return Err(ManifestError::SchemaVersionTooNew {
                found: manifest.schema_version,
                supported: CURRENT_SCHEMA_VERSION,
            });
        }
        if manifest.schema_version < CURRENT_SCHEMA_VERSION {
            return Err(ManifestError::SchemaVersionInvalid {
                found: manifest.schema_version,
                supported: CURRENT_SCHEMA_VERSION,
            });
        }
        for spec in &manifest.secrets {
            validate_secret_spec(spec)?;
        }
        Ok(manifest)
    }
}

fn validate_secret_spec(spec: &SecretSpec) -> Result<(), ManifestError> {
    if spec.env.is_empty() {
        return Err(ManifestError::InvalidSecretEnv {
            env: spec.env.clone(),
            reason: "name must not be empty",
        });
    }
    if spec.env.contains('=') {
        return Err(ManifestError::InvalidSecretEnv {
            env: spec.env.clone(),
            reason: "name must not contain `=` (Command::env would panic)",
        });
    }
    if spec.env.contains('\0') {
        return Err(ManifestError::InvalidSecretEnv {
            env: spec.env.clone(),
            reason: "name must not contain NUL (Command::env would panic)",
        });
    }
    if spec.reference.is_empty() {
        return Err(ManifestError::InvalidSecretRef {
            env: spec.env.clone(),
            reason: "ref must not be empty",
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_minimal_manifest() {
        let toml = r#"
            schema_version = 1
            description = "Personal automation, no guardrails"
        "#;
        let m = Manifest::from_str(toml).expect("minimal manifest should parse");
        assert_eq!(m.schema_version, 1);
        assert_eq!(m.description, "Personal automation, no guardrails");
        assert!(m.banner.is_none());
        assert!(m.requires_env.is_empty());
        assert!(!m.confirm);
        assert!(m.audit_log.is_none());
        assert!(m.secrets.is_empty());
    }

    #[test]
    fn parses_full_manifest_with_both_secret_providers() {
        let toml = r#"
            schema_version = 1
            description = "Production ops"
            banner = "PROD — irreversible; verify before proceeding"
            confirm = true
            audit_log = "$XDG_STATE_HOME/qli/prod-audit.log"

            [requires_env]
            QLI_ENV = "prod"

            [[secrets]]
            env = "OP_TOKEN"
            ref = "op://Personal/CI/token"
            provider = "one_password"

            [[secrets]]
            env = "GITHUB_TOKEN"
            ref = "GITHUB_TOKEN"
            provider = "env"
        "#;
        let m = Manifest::from_str(toml).expect("full manifest should parse");
        assert_eq!(
            m.banner.as_deref(),
            Some("PROD — irreversible; verify before proceeding")
        );
        assert!(m.confirm);
        assert_eq!(
            m.audit_log.as_deref(),
            Some("$XDG_STATE_HOME/qli/prod-audit.log"),
        );
        assert_eq!(
            m.requires_env.get("QLI_ENV").map(String::as_str),
            Some("prod"),
        );
        assert_eq!(m.secrets.len(), 2);
        assert_eq!(m.secrets[0].env, "OP_TOKEN");
        assert_eq!(m.secrets[0].reference, "op://Personal/CI/token");
        assert_eq!(m.secrets[0].provider, SecretProvider::OnePassword);
        assert_eq!(m.secrets[1].provider, SecretProvider::Env);
    }

    #[test]
    fn rejects_schema_version_too_new() {
        let toml = r#"
            schema_version = 2
            description = "from the future"
        "#;
        match Manifest::from_str(toml) {
            Err(ManifestError::SchemaVersionTooNew { found, supported }) => {
                assert_eq!(found, 2);
                assert_eq!(supported, CURRENT_SCHEMA_VERSION);
            }
            other => panic!("expected SchemaVersionTooNew, got {other:?}"),
        }
    }

    #[test]
    fn rejects_schema_version_zero_as_invalid() {
        let toml = r#"
            schema_version = 0
            description = "bogus version"
        "#;
        match Manifest::from_str(toml) {
            Err(ManifestError::SchemaVersionInvalid { found, supported }) => {
                assert_eq!(found, 0);
                assert_eq!(supported, CURRENT_SCHEMA_VERSION);
            }
            other => panic!("expected SchemaVersionInvalid, got {other:?}"),
        }
    }

    #[test]
    fn rejects_missing_schema_version() {
        let toml = r#"
            description = "no version"
        "#;
        let err = Manifest::from_str(toml).expect_err("must reject missing schema_version");
        assert!(matches!(err, ManifestError::Toml(_)), "got {err:?}");
        assert!(
            err.to_string().contains("schema_version"),
            "error should mention schema_version, got: {err}",
        );
    }

    #[test]
    fn rejects_unknown_field() {
        let toml = r#"
            schema_version = 1
            description = "typo'd field"
            audti_log = "/tmp/log"
        "#;
        let err = Manifest::from_str(toml).expect_err("must reject unknown fields");
        assert!(matches!(err, ManifestError::Toml(_)), "got {err:?}");
        assert!(
            err.to_string().contains("audti_log"),
            "error should name the offending field, got: {err}",
        );
    }

    #[test]
    fn rejects_unknown_secret_provider() {
        let toml = r#"
            schema_version = 1
            description = "bad provider"

            [[secrets]]
            env = "FOO"
            ref = "bar"
            provider = "vault"
        "#;
        let err = Manifest::from_str(toml).expect_err("must reject unknown providers");
        assert!(matches!(err, ManifestError::Toml(_)), "got {err:?}");
    }

    #[test]
    fn rejects_pascal_case_provider_value() {
        // Schema is snake_case. PascalCase from older drafts must fail loudly.
        let toml = r#"
            schema_version = 1
            description = "stale casing"

            [[secrets]]
            env = "FOO"
            ref = "bar"
            provider = "OnePassword"
        "#;
        let err = Manifest::from_str(toml).expect_err("PascalCase provider must be rejected");
        assert!(matches!(err, ManifestError::Toml(_)), "got {err:?}");
    }

    #[test]
    fn ref_keyword_field_round_trips() {
        let toml = r#"
            schema_version = 1
            description = "checking the ref rename"

            [[secrets]]
            env = "TOKEN"
            ref = "op://Vault/Item/token"
            provider = "one_password"
        "#;
        let m = Manifest::from_str(toml).expect("ref field should parse via serde rename");
        assert_eq!(m.secrets[0].reference, "op://Vault/Item/token");
    }

    #[test]
    fn rejects_empty_secret_env() {
        let toml = r#"
            schema_version = 1
            description = "blank env"
            [[secrets]]
            env = ""
            ref = "op://x"
            provider = "one_password"
        "#;
        let err = Manifest::from_str(toml).expect_err("empty env must be rejected");
        assert!(
            matches!(err, ManifestError::InvalidSecretEnv { .. }),
            "got {err:?}"
        );
    }

    #[test]
    fn rejects_secret_env_containing_equals() {
        let toml = r#"
            schema_version = 1
            description = "= in env"
            [[secrets]]
            env = "FOO=BAR"
            ref = "op://x"
            provider = "one_password"
        "#;
        let err = Manifest::from_str(toml).expect_err("env with = must be rejected");
        match err {
            ManifestError::InvalidSecretEnv { env, reason } => {
                assert_eq!(env, "FOO=BAR");
                assert!(reason.contains('='), "reason: {reason}");
            }
            other => panic!("expected InvalidSecretEnv, got {other:?}"),
        }
    }

    #[test]
    fn rejects_secret_env_containing_nul() {
        // TOML strings can't contain a literal NUL, but a Unicode-escape can.
        let toml = "schema_version = 1\n\
                    description = \"NUL in env\"\n\
                    [[secrets]]\n\
                    env = \"FOO\\u0000BAR\"\n\
                    ref = \"op://x\"\n\
                    provider = \"one_password\"\n";
        let err = Manifest::from_str(toml).expect_err("env with NUL must be rejected");
        assert!(
            matches!(err, ManifestError::InvalidSecretEnv { .. }),
            "got {err:?}"
        );
    }

    #[test]
    fn rejects_empty_secret_ref() {
        let toml = r#"
            schema_version = 1
            description = "blank ref"
            [[secrets]]
            env = "FOO"
            ref = ""
            provider = "one_password"
        "#;
        let err = Manifest::from_str(toml).expect_err("empty ref must be rejected");
        match err {
            ManifestError::InvalidSecretRef { env, .. } => assert_eq!(env, "FOO"),
            other => panic!("expected InvalidSecretRef, got {other:?}"),
        }
    }
}
