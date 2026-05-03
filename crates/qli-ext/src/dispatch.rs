//! Extension dispatch with the Phase 1F guard sequence wrapped around the
//! child spawn.
//!
//! The eight steps the plan specifies (banner → `requires_env` → confirm →
//! secrets → audit-start → spawn → wait → audit-finish) are executed in
//! order; each step gates the next. Failures before spawn surface as
//! [`DispatchError`] without ever reaching the child.
//!
//! `Command::spawn` (not `exec`) is deliberate: the dispatcher must outlive
//! the child to write the post-run audit entry, propagate exit codes, and —
//! on SIGINT/SIGTERM mid-run — forward the signal to the child via the
//! [`DispatchSignals`] handle the binary installs into its global signal
//! handler.

use std::collections::HashMap;
use std::ffi::OsStr;
use std::io;
use std::path::PathBuf;
use std::process::{Command, ExitStatus};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use chrono::Utc;
use thiserror::Error;

use crate::audit::{self, AuditError, AuditEvent};
use crate::discovery::{Extension, Group};
use crate::guard::{self, ConfirmPrompt, GuardError};
use crate::secrets::{ResolvedSecret, SecretsError, SecretsResolver};

/// Tunables and dependencies the dispatcher needs from the binary.
pub struct DispatchOptions<'a> {
    /// Skip the confirm prompt (the user passed `--yes`).
    pub assume_yes: bool,
    /// Resolve every `secrets[*]` entry in the manifest up-front.
    pub resolver: &'a dyn SecretsResolver,
    /// How to ask the user when `confirm = true` and stdin is a TTY.
    pub confirm: &'a dyn ConfirmPrompt,
    /// Shared interrupt + child-PID slot the binary's signal handler updates.
    pub signals: Arc<DispatchSignals>,
    /// Fallback values for env vars referenced in `audit_log` (typically the
    /// resolved XDG defaults so manifests can use `$XDG_STATE_HOME` even
    /// when the user hasn't exported it).
    pub audit_path_defaults: HashMap<String, String>,
}

impl std::fmt::Debug for DispatchOptions<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DispatchOptions")
            .field("assume_yes", &self.assume_yes)
            .field("audit_path_defaults", &self.audit_path_defaults)
            .finish_non_exhaustive()
    }
}

/// Shared between the dispatcher and the binary's ctrlc handler.
///
/// The handler calls [`DispatchSignals::on_signal`] when SIGINT/SIGTERM
/// fires; the dispatcher registers/clears the running child's PID and reads
/// `was_interrupted()` after the child exits to decide between `Finish` and
/// `Interrupted` audit events.
#[derive(Debug, Default)]
pub struct DispatchSignals {
    interrupted: AtomicBool,
    child_pid: Mutex<Option<u32>>,
}

impl DispatchSignals {
    #[must_use]
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    /// Called from the binary's signal handler. Marks the run as interrupted
    /// and forwards SIGTERM to the child if one is registered. Forwarding is
    /// a no-op on non-Unix targets (signal semantics differ on Windows; the
    /// foundation plan stays Unix-first for now).
    pub fn on_signal(&self) {
        self.interrupted.store(true, Ordering::SeqCst);
        if let Some(pid) = *self.lock_pid() {
            forward_terminate(pid);
        }
    }

    fn lock_pid(&self) -> std::sync::MutexGuard<'_, Option<u32>> {
        self.child_pid.lock().expect("child_pid mutex poisoned")
    }

    fn set_child(&self, pid: u32) {
        *self.lock_pid() = Some(pid);
    }

    fn clear_child(&self) {
        *self.lock_pid() = None;
    }

    fn was_interrupted(&self) -> bool {
        self.interrupted.load(Ordering::SeqCst)
    }
}

/// Top-level error from [`run`]. Each variant maps to a specific guard step
/// or post-spawn failure mode so callers can render targeted diagnostics.
#[derive(Debug, Error)]
pub enum DispatchError {
    #[error(transparent)]
    Guard(#[from] GuardError),
    #[error(transparent)]
    Secrets(#[from] SecretsError),
    #[error(transparent)]
    Audit(#[from] AuditError),
    /// A resolved secret carries data that would crash `Command::env`. The
    /// value itself is deliberately omitted from the message — only the
    /// env-var name and a description of *why* it's bad.
    #[error("resolved secret for env `{env}` is invalid: {reason}")]
    SecretValueInvalid { env: String, reason: &'static str },
    #[error("could not spawn `{path}`: {source}")]
    Spawn {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("error waiting on `{path}`: {source}")]
    Wait {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
}

/// Run an extension under the full guard sequence.
///
/// Returns the child's exit code (host conventions: signal exits → 128+sig
/// on Unix). Pre-spawn failures return the corresponding [`DispatchError`]
/// variant — the caller is responsible for mapping that to a process exit
/// code.
pub fn run<I, S>(
    group: &Group,
    extension: &Extension,
    args: I,
    opts: &DispatchOptions<'_>,
) -> Result<i32, DispatchError>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let args: Vec<std::ffi::OsString> = args.into_iter().map(|s| s.as_ref().to_owned()).collect();

    // 1. Banner.
    guard::print_banner(&group.manifest);

    // 2. requires_env.
    guard::check_requires_env(&group.manifest)?;

    // 3. Confirm (gated *before* secrets so we don't hit `op` for an abort).
    guard::run_confirm(
        &group.manifest,
        &group.name,
        &extension.name,
        opts.assume_yes,
        opts.confirm,
    )?;

    // 4. Resolve secrets up-front, fail closed.
    let resolved = opts.resolver.resolve_all(&group.manifest.secrets)?;

    // 5. Audit start.
    let audit_path = group
        .manifest
        .audit_log
        .as_deref()
        .map(|s| audit::expand_path(s, &opts.audit_path_defaults))
        .transpose()?;
    if let Some(path) = &audit_path {
        audit::append(
            path,
            &AuditEvent::Start {
                timestamp: Utc::now(),
                user: audit::current_user(),
                group: group.name.clone(),
                extension: extension.name.clone(),
                args: args
                    .iter()
                    .map(|a| a.to_string_lossy().into_owned())
                    .collect(),
                env_var_names: resolved.iter().map(|r| r.env.clone()).collect(),
            },
        )?;
    }

    // 6 + 7. Spawn, register PID for signal forwarding, wait.
    let started = Instant::now();
    let mut command = Command::new(&extension.path);
    command.args(&args);
    apply_secret_env(&mut command, &resolved)?;
    let mut child = command.spawn().map_err(|source| DispatchError::Spawn {
        path: extension.path.clone(),
        source,
    })?;
    opts.signals.set_child(child.id());
    // The `Command` (and so the resolved values it stores) lives until end of
    // function; `resolved` is one of two copies, not the only one. We do NOT
    // promise zeroization here — secrets handling is a defense-in-depth task
    // tracked separately.
    drop(resolved);
    let status = child.wait().map_err(|source| DispatchError::Wait {
        path: extension.path.clone(),
        source,
    });
    opts.signals.clear_child();
    let status: ExitStatus = status?;
    let duration_ms = started.elapsed().as_millis();
    let code = exit_code(status);

    // 8. Audit finish (or interrupted, if the signal handler tagged the run).
    if let Some(path) = &audit_path {
        let event = if opts.signals.was_interrupted() {
            AuditEvent::Interrupted {
                timestamp: Utc::now(),
                group: group.name.clone(),
                extension: extension.name.clone(),
                signal: signal_label(status),
                exit_code: code,
                duration_ms,
            }
        } else {
            AuditEvent::Finish {
                timestamp: Utc::now(),
                group: group.name.clone(),
                extension: extension.name.clone(),
                exit_code: code,
                duration_ms,
            }
        };
        // A failed finish/interrupted write is reported as a warning so the
        // child's exit code still propagates — the child has already run; we
        // don't want to fabricate an error from a logging glitch.
        if let Err(err) = audit::append(path, &event) {
            eprintln!("warning: failed to write audit-finish entry: {err}");
        }
    }

    Ok(code)
}

fn apply_secret_env(
    command: &mut Command,
    resolved: &[ResolvedSecret],
) -> Result<(), DispatchError> {
    for secret in resolved {
        // The manifest parser rejects bad env names; the env variant is for
        // values returned from a resolver (provider data we didn't author).
        // `Command::env` panics on a value containing NUL — fail with a typed
        // error pointing at the offending env name instead.
        if secret.value.contains('\0') {
            return Err(DispatchError::SecretValueInvalid {
                env: secret.env.clone(),
                reason: "value contains NUL — the secret provider returned malformed data",
            });
        }
        command.env(&secret.env, &secret.value);
    }
    Ok(())
}

#[cfg(unix)]
fn exit_code(status: ExitStatus) -> i32 {
    use std::os::unix::process::ExitStatusExt;
    if let Some(code) = status.code() {
        code
    } else if let Some(sig) = status.signal() {
        128 + sig
    } else {
        1
    }
}

#[cfg(not(unix))]
fn exit_code(status: ExitStatus) -> i32 {
    status.code().unwrap_or(1)
}

#[cfg(unix)]
fn signal_label(status: ExitStatus) -> String {
    use std::os::unix::process::ExitStatusExt;
    match status.signal() {
        Some(2) => "SIGINT".into(),
        Some(15) => "SIGTERM".into(),
        Some(other) => format!("SIG{other}"),
        None => "interrupted".into(),
    }
}

#[cfg(not(unix))]
fn signal_label(_status: ExitStatus) -> String {
    "interrupted".into()
}

#[cfg(unix)]
fn forward_terminate(pid: u32) {
    // Pids are non-negative; on Unix `pid_t` is `i32` and the kernel will
    // never hand back a u32 with the high bit set. `try_into` rejects the
    // pathological case rather than silently wrapping.
    let Ok(raw) = i32::try_from(pid) else {
        return;
    };
    let target = nix::unistd::Pid::from_raw(raw);
    // Best-effort: child may already have exited (ESRCH), or we may lack
    // permission. Either way, the dispatcher's main thread will reach
    // `child.wait()` shortly and tear things down.
    let _ = nix::sys::signal::kill(target, nix::sys::signal::Signal::SIGTERM);
}

#[cfg(not(unix))]
fn forward_terminate(_pid: u32) {
    // Windows signal forwarding requires console-control routing; deferred
    // until a Windows port is in scope.
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::Manifest;
    use crate::secrets::TestResolver;

    struct AlwaysYes;
    impl ConfirmPrompt for AlwaysYes {
        fn ask(&self, _message: &str) -> Result<bool, GuardError> {
            Ok(true)
        }
    }

    fn manifest(
        confirm: bool,
        env: &[(&str, &str)],
        audit_log: Option<&str>,
        secrets: Vec<crate::manifest::SecretSpec>,
    ) -> Manifest {
        Manifest {
            schema_version: 1,
            description: "test".into(),
            banner: None,
            requires_env: env
                .iter()
                .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
                .collect(),
            confirm,
            audit_log: audit_log.map(str::to_owned),
            secrets,
        }
    }

    fn group(manifest: Manifest) -> Group {
        Group {
            name: "dev".into(),
            manifest_path: PathBuf::from("/dev/null/_manifest.toml"),
            manifest,
            extensions: std::collections::BTreeMap::new(),
        }
    }

    fn extension(path: PathBuf) -> Extension {
        Extension {
            name: "hello".into(),
            group: "dev".into(),
            path,
            origin: crate::discovery::ExtensionOrigin::Xdg,
        }
    }

    fn opts<'a>(
        resolver: &'a TestResolver,
        confirm: &'a AlwaysYes,
        signals: Arc<DispatchSignals>,
    ) -> DispatchOptions<'a> {
        DispatchOptions {
            assume_yes: false,
            resolver,
            confirm,
            signals,
            audit_path_defaults: HashMap::new(),
        }
    }

    #[test]
    #[cfg(unix)]
    fn happy_path_runs_extension_and_writes_audit() {
        let tmp = tempfile::tempdir().unwrap();
        let script = tmp.path().join("hello");
        std::fs::write(&script, "#!/bin/sh\nexit 0\n").unwrap();
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&script).unwrap().permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&script, perms).unwrap();
        }
        let audit_path = tmp.path().join("audit.log");
        let g = group(manifest(false, &[], audit_path.to_str(), Vec::new()));
        let e = extension(script);
        let resolver = TestResolver::new();
        let confirm = AlwaysYes;
        let signals = DispatchSignals::new();
        let o = opts(&resolver, &confirm, signals);
        let code = run(&g, &e, std::iter::empty::<&str>(), &o).unwrap();
        assert_eq!(code, 0);
        let body = std::fs::read_to_string(&audit_path).unwrap();
        let lines: Vec<&str> = body.lines().collect();
        assert_eq!(lines.len(), 2, "got: {body}");
        assert!(lines[0].contains("\"event\":\"start\""));
        assert!(lines[1].contains("\"event\":\"finish\""));
        assert!(lines[1].contains("\"exit_code\":0"));
    }

    #[test]
    #[cfg(unix)]
    #[serial_test::serial]
    fn requires_env_blocks_spawn() {
        // Unique env var name per test (defense in depth) plus
        // `#[serial]` for hard isolation against any future env-mutating
        // sibling test in this binary.
        std::env::remove_var("QLI_DISPATCH_TEST_REQ");
        let tmp = tempfile::tempdir().unwrap();
        let script = tmp.path().join("hello");
        std::fs::write(&script, "#!/bin/sh\nexit 0\n").unwrap();
        let g = group(manifest(
            false,
            &[("QLI_DISPATCH_TEST_REQ", "yes")],
            None,
            Vec::new(),
        ));
        let e = extension(script);
        let resolver = TestResolver::new();
        let confirm = AlwaysYes;
        let signals = DispatchSignals::new();
        let o = opts(&resolver, &confirm, signals);
        let err = run(&g, &e, std::iter::empty::<&str>(), &o).unwrap_err();
        assert!(matches!(
            err,
            DispatchError::Guard(GuardError::EnvMissing { .. })
        ));
    }

    #[test]
    #[cfg(unix)]
    fn secrets_propagate_to_child_env_but_not_audit() {
        let tmp = tempfile::tempdir().unwrap();
        let script = tmp.path().join("dump");
        // Print the env var the manifest injects to a sentinel file.
        let dump_path = tmp.path().join("child-env");
        let body = format!(
            "#!/bin/sh\nprintf '%s' \"$INJECTED\" > {}\nexit 0\n",
            dump_path.display()
        );
        std::fs::write(&script, body).unwrap();
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&script).unwrap().permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&script, perms).unwrap();
        }
        let audit_path = tmp.path().join("audit.log");
        let secret = crate::manifest::SecretSpec {
            env: "INJECTED".into(),
            reference: "ref-x".into(),
            provider: crate::manifest::SecretProvider::Env,
        };
        let g = group(manifest(false, &[], audit_path.to_str(), vec![secret]));
        let e = extension(script);
        let resolver = TestResolver::new().with("ref-x", "SECRET_SENTINEL_AAA");
        let confirm = AlwaysYes;
        let signals = DispatchSignals::new();
        let o = opts(&resolver, &confirm, signals);
        let code = run(&g, &e, std::iter::empty::<&str>(), &o).unwrap();
        assert_eq!(code, 0);
        let dumped = std::fs::read_to_string(&dump_path).unwrap();
        assert_eq!(dumped, "SECRET_SENTINEL_AAA", "child should receive secret");
        let audit_body = std::fs::read_to_string(&audit_path).unwrap();
        assert!(
            !audit_body.contains("SECRET_SENTINEL_AAA"),
            "audit log must not contain secret value: {audit_body}",
        );
        assert!(
            audit_body.contains("\"INJECTED\""),
            "env var name must be recorded"
        );
    }

    #[test]
    #[cfg(unix)]
    fn signal_forwarding_writes_interrupted_audit_and_exits_with_signal_code() {
        let tmp = tempfile::tempdir().unwrap();
        let script = tmp.path().join("sleeper");
        // Long-running child. SIGTERM should reach it via the dispatcher's
        // forward path; if forwarding breaks, this test will hang for ~60s
        // before the runner's timeout fires, which makes the failure obvious.
        std::fs::write(&script, "#!/bin/sh\nsleep 60\nexit 0\n").unwrap();
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&script).unwrap().permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&script, perms).unwrap();
        }
        let audit_path = tmp.path().join("audit.log");
        let g = group(manifest(false, &[], audit_path.to_str(), Vec::new()));
        let e = extension(script);
        let resolver = TestResolver::new();
        let confirm = AlwaysYes;
        let signals = DispatchSignals::new();
        let trigger = Arc::clone(&signals);

        let handle = std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(200));
            // Fire what a real Ctrl+C / SIGTERM handler would do.
            trigger.on_signal();
        });

        let o = opts(&resolver, &confirm, signals);
        let code = run(&g, &e, std::iter::empty::<&str>(), &o).unwrap();
        handle.join().unwrap();

        // Child was killed by SIGTERM (signo 15) → exit_code = 128 + 15.
        assert_eq!(code, 143, "expected SIGTERM exit code 143, got {code}");
        let audit_body = std::fs::read_to_string(&audit_path).unwrap();
        let lines: Vec<&str> = audit_body.lines().collect();
        assert_eq!(lines.len(), 2, "expected start + interrupted: {audit_body}");
        assert!(lines[0].contains("\"event\":\"start\""));
        assert!(
            lines[1].contains("\"event\":\"interrupted\""),
            "expected interrupted event, got: {}",
            lines[1],
        );
        assert!(lines[1].contains("\"signal\":\"SIGTERM\""));
        assert!(lines[1].contains("\"exit_code\":143"));
    }

    #[test]
    #[cfg(unix)]
    fn nul_in_resolved_secret_value_is_rejected_before_spawn() {
        let tmp = tempfile::tempdir().unwrap();
        let script = tmp.path().join("hello");
        std::fs::write(&script, "#!/bin/sh\nexit 0\n").unwrap();
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&script).unwrap().permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&script, perms).unwrap();
        }
        let secret = crate::manifest::SecretSpec {
            env: "TOKEN".into(),
            reference: "ref-bad".into(),
            provider: crate::manifest::SecretProvider::Env,
        };
        let g = group(manifest(false, &[], None, vec![secret]));
        let e = extension(script);
        // Resolver returns a value containing NUL — exec-time would panic.
        let resolver = TestResolver::new().with("ref-bad", "good\0bad");
        let confirm = AlwaysYes;
        let signals = DispatchSignals::new();
        let o = opts(&resolver, &confirm, signals);
        let err = run(&g, &e, std::iter::empty::<&str>(), &o).unwrap_err();
        match err {
            DispatchError::SecretValueInvalid { env, .. } => assert_eq!(env, "TOKEN"),
            other => panic!("expected SecretValueInvalid, got {other:?}"),
        }
    }

    #[test]
    #[cfg(unix)]
    fn child_exit_code_propagates() {
        let tmp = tempfile::tempdir().unwrap();
        let script = tmp.path().join("explode");
        std::fs::write(&script, "#!/bin/sh\nexit 7\n").unwrap();
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&script).unwrap().permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&script, perms).unwrap();
        }
        let g = group(manifest(false, &[], None, Vec::new()));
        let e = extension(script);
        let resolver = TestResolver::new();
        let confirm = AlwaysYes;
        let signals = DispatchSignals::new();
        let o = opts(&resolver, &confirm, signals);
        let code = run(&g, &e, std::iter::empty::<&str>(), &o).unwrap();
        assert_eq!(code, 7);
    }
}
