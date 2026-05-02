//! Permanent regression test: no resolved secret value ever appears in the
//! audit log, the dispatcher's stdout, or the dispatcher's stderr.
//!
//! Plan reference: Phase 1F task `Regression test: no resolved secret value
//! appears in audit log, stdout, or stderr.` Drives every guard path with a
//! distinct sentinel string and asserts none of them leak.

use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::process::Command;
use std::sync::Arc;

use qli_ext::dispatch::run;
use qli_ext::guard::{ConfirmPrompt, GuardError};
use qli_ext::{
    DispatchOptions, DispatchSignals, Extension, ExtensionOrigin, Group, Manifest, SecretProvider,
    SecretSpec, TestResolver,
};

const SENTINEL_HAPPY: &str = "SECRET_SENTINEL_HAPPY_001";
const SENTINEL_ENV_FAIL: &str = "SECRET_SENTINEL_ENV_FAIL_002";
const SENTINEL_CONFIRM: &str = "SECRET_SENTINEL_CONFIRM_003";
const SENTINEL_CHILD_FAIL: &str = "SECRET_SENTINEL_CHILD_FAIL_004";

struct AlwaysYes;
impl ConfirmPrompt for AlwaysYes {
    fn ask(&self, _message: &str) -> Result<bool, GuardError> {
        Ok(true)
    }
}

struct NeverYes;
impl ConfirmPrompt for NeverYes {
    fn ask(&self, _message: &str) -> Result<bool, GuardError> {
        Ok(false)
    }
}

fn make_group(
    name: &str,
    confirm: bool,
    env: &[(&str, &str)],
    audit: Option<&str>,
    secrets: Vec<SecretSpec>,
) -> Group {
    Group {
        name: name.into(),
        manifest_path: PathBuf::from("/dev/null/_manifest.toml"),
        manifest: Manifest {
            schema_version: 1,
            description: "test".into(),
            banner: None,
            requires_env: env
                .iter()
                .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
                .collect(),
            confirm,
            audit_log: audit.map(str::to_owned),
            secrets,
        },
        extensions: BTreeMap::new(),
    }
}

fn make_opts<'a>(
    resolver: &'a TestResolver,
    confirm: &'a dyn ConfirmPrompt,
    signals: &Arc<DispatchSignals>,
) -> DispatchOptions<'a> {
    DispatchOptions {
        assume_yes: false,
        resolver,
        confirm,
        signals: Arc::clone(signals),
        audit_path_defaults: HashMap::new(),
    }
}

#[cfg(unix)]
fn make_extension(tmp: &std::path::Path, body: &str) -> Extension {
    use std::os::unix::fs::PermissionsExt;
    let path = tmp.join("script");
    std::fs::write(&path, body).unwrap();
    let mut perms = std::fs::metadata(&path).unwrap().permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&path, perms).unwrap();
    Extension {
        name: "hello".into(),
        group: "dev".into(),
        path,
        origin: ExtensionOrigin::Xdg,
    }
}

/// Run the dispatcher in a helper subprocess so stdout/stderr are captured
/// cleanly. The helper is the same `secrets_never_leak` test binary, invoked
/// with `QLI_SECRETS_NEVER_LEAK_HELPER=1`; that env var routes it to the
/// helper `main` below before any test runs.
fn spawn_helper(scenario: &str, fixtures_dir: &std::path::Path) -> std::process::Output {
    let exe = std::env::current_exe().expect("test exe path");
    Command::new(exe)
        .env("QLI_SECRETS_NEVER_LEAK_HELPER", "1")
        .env("QLI_SECRETS_NEVER_LEAK_SCENARIO", scenario)
        .env("QLI_SECRETS_NEVER_LEAK_DIR", fixtures_dir)
        .output()
        .expect("spawn helper")
}

fn assert_no_sentinel(stream: &str, sentinels: &[&str], name: &str) {
    for s in sentinels {
        assert!(
            !stream.contains(s),
            "sentinel `{s}` leaked into {name}: {stream}",
        );
    }
}

#[test]
#[cfg(unix)]
fn secrets_never_appear_in_any_observable_stream() {
    if std::env::var_os("QLI_SECRETS_NEVER_LEAK_HELPER").is_some() {
        run_helper();
        return;
    }
    let tmp = tempfile::tempdir().unwrap();

    for scenario in &["happy", "env_fail", "confirm_decline", "child_fail"] {
        let dir = tmp.path().join(scenario);
        std::fs::create_dir_all(&dir).unwrap();
        let out = spawn_helper(scenario, &dir);
        let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
        let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
        let audit_path = dir.join("audit.log");
        let audit = std::fs::read_to_string(&audit_path).unwrap_or_default();

        let sentinels = [
            SENTINEL_HAPPY,
            SENTINEL_ENV_FAIL,
            SENTINEL_CONFIRM,
            SENTINEL_CHILD_FAIL,
        ];
        assert_no_sentinel(&stdout, &sentinels, &format!("{scenario} stdout"));
        assert_no_sentinel(&stderr, &sentinels, &format!("{scenario} stderr"));
        assert_no_sentinel(&audit, &sentinels, &format!("{scenario} audit log"));

        match *scenario {
            "happy" => {
                assert!(
                    out.status.success(),
                    "happy scenario should succeed: {stderr}"
                );
                // The happy path should still record the env var name.
                assert!(
                    audit.contains("\"SECRET\""),
                    "audit must list env name: {audit}"
                );
            }
            "env_fail" | "confirm_decline" => {
                assert!(!out.status.success(), "{scenario} should fail");
                // Pre-spawn failures: no audit-start line should exist (we
                // resolve secrets and write audit AFTER confirm/env checks).
                assert!(
                    audit.is_empty(),
                    "audit must be empty for {scenario}: {audit}"
                );
            }
            "child_fail" => {
                assert!(
                    !out.status.success(),
                    "child_fail should propagate child exit"
                );
                assert!(audit.contains("\"event\":\"finish\""), "audit: {audit}");
            }
            _ => unreachable!(),
        }
    }
}

#[allow(clippy::too_many_lines)] // Inline scenarios are clearer here than indirection.
fn run_helper() {
    let scenario = std::env::var("QLI_SECRETS_NEVER_LEAK_SCENARIO").unwrap();
    let dir = PathBuf::from(std::env::var_os("QLI_SECRETS_NEVER_LEAK_DIR").unwrap());

    let signals = DispatchSignals::new();
    let audit = dir.join("audit.log");
    let audit_str = audit.to_str().unwrap();

    let result = match scenario.as_str() {
        "happy" => {
            let body = "#!/bin/sh\nexit 0\n";
            let group = make_group(
                "dev",
                false,
                &[],
                Some(audit_str),
                vec![SecretSpec {
                    env: "SECRET".into(),
                    reference: "ref-happy".into(),
                    provider: SecretProvider::Env,
                }],
            );
            let ext = make_extension(&dir, body);
            let resolver = TestResolver::new().with("ref-happy", SENTINEL_HAPPY);
            let confirm = AlwaysYes;
            let opts = make_opts(&resolver, &confirm, &signals);
            run(&group, &ext, std::iter::empty::<&str>(), &opts)
        }
        "env_fail" => {
            std::env::remove_var("QLI_NEVER_LEAK_REQ");
            let group = make_group(
                "prod",
                false,
                &[("QLI_NEVER_LEAK_REQ", "yes")],
                Some(audit_str),
                vec![SecretSpec {
                    env: "SECRET".into(),
                    reference: "ref-env-fail".into(),
                    provider: SecretProvider::OnePassword,
                }],
            );
            let ext = make_extension(&dir, "#!/bin/sh\nexit 0\n");
            let resolver = TestResolver::new().with("ref-env-fail", SENTINEL_ENV_FAIL);
            let confirm = AlwaysYes;
            let opts = make_opts(&resolver, &confirm, &signals);
            run(&group, &ext, std::iter::empty::<&str>(), &opts)
        }
        "confirm_decline" => {
            let group = make_group(
                "prod",
                true,
                &[],
                Some(audit_str),
                vec![SecretSpec {
                    env: "SECRET".into(),
                    reference: "ref-confirm".into(),
                    provider: SecretProvider::Env,
                }],
            );
            let ext = make_extension(&dir, "#!/bin/sh\nexit 0\n");
            let resolver = TestResolver::new().with("ref-confirm", SENTINEL_CONFIRM);
            let confirm = NeverYes;
            let opts = make_opts(&resolver, &confirm, &signals);
            run(&group, &ext, std::iter::empty::<&str>(), &opts)
        }
        "child_fail" => {
            let group = make_group(
                "dev",
                false,
                &[],
                Some(audit_str),
                vec![SecretSpec {
                    env: "SECRET".into(),
                    reference: "ref-child".into(),
                    provider: SecretProvider::Env,
                }],
            );
            let ext = make_extension(&dir, "#!/bin/sh\nexit 9\n");
            let resolver = TestResolver::new().with("ref-child", SENTINEL_CHILD_FAIL);
            let confirm = AlwaysYes;
            let opts = make_opts(&resolver, &confirm, &signals);
            run(&group, &ext, std::iter::empty::<&str>(), &opts)
        }
        other => panic!("unknown helper scenario: {other}"),
    };

    match result {
        Ok(0) => std::process::exit(0),
        Ok(code) => std::process::exit(u8::try_from(code).unwrap_or(1).into()),
        Err(err) => {
            // We must surface the error so the parent test can see *that* it
            // failed, but the formatted error MUST NOT include a secret
            // value. Print only the variant; in this scaffold, no variant
            // includes a resolved secret value (env names yes, values no).
            eprintln!("dispatch error: {err}");
            std::process::exit(1);
        }
    }
}
