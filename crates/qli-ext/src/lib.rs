//! Extension discovery, dispatch, and guardrails for the qli CLI.
//!
//! [`discovery`] walks XDG + PATH and builds the group/extension table.
//! [`dispatch::run`] wraps the child spawn in the guard sequence: banner →
//! [`guard::check_requires_env`] → [`guard::run_confirm`] → secrets →
//! [`audit`] start → spawn → audit finish/interrupted. Manifests
//! ([`manifest`]) describe each group; secret resolution is pluggable via
//! [`secrets::SecretsResolver`].
//!
//! ## Diagnostic policy (fail fast, fail loud)
//!
//! Every error has one obvious surface — never silently swallowed. Four
//! tiers, picked by *user impact*, not by *code locality*:
//!
//! 1. **Process-fatal** — bubbled up as `anyhow::Error` from `main`, printed
//!    `error: {msg}` (exit 1). For startup failures and unrecoverable binary
//!    conditions.
//! 2. **Dispatch-fatal** — typed [`dispatch::DispatchError`] variants that
//!    abort one dispatched extension with full context. Surfaced through
//!    `anyhow` so the user sees `error: failed to run X: Y`.
//! 3. **Must-see warning** — `eprintln!("warning: ...")`. Never goes through
//!    `tracing` (which `-q` can silence). Used when behavior visibly
//!    degrades: discovery skipped a group, a signal handler couldn't
//!    install, an audit-finish write failed.
//! 4. **Trace** — `tracing` info/debug/trace. Routine progress only;
//!    silenceable. Use it when a later operation will fail loudly with full
//!    context if this trace event mattered.
//!
//! Rule of thumb: if you write `.ok()` on a `Result` whose failure changes
//! user-visible behavior, you've picked the wrong tier — promote to 3 or 2.
//! Validation belongs at the **earliest boundary** (parse-time over
//! exec-time) so the error points at the source, not the symptom.

pub mod audit;
pub mod discovery;
pub mod dispatch;
pub mod guard;
pub mod manifest;
pub mod secrets;

pub use discovery::{discover, Discovery, Extension, ExtensionOrigin, Group};
pub use dispatch::{DispatchError, DispatchOptions, DispatchSignals};
pub use guard::{tty_confirm, ConfirmPrompt, GuardError, TtyConfirm};
pub use manifest::{Manifest, ManifestError, SecretProvider, SecretSpec, CURRENT_SCHEMA_VERSION};
pub use secrets::{ResolvedSecret, SecretsError, SecretsResolver, TestResolver};
