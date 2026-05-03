//! End-to-end dispatcher tests against the real `qli` binary.
//!
//! These spawn the production binary via `assert_cmd::Command::cargo_bin`
//! under [`XdgSandbox`] so they never touch the host's `~/.config/qli`,
//! `~/.local/share/qli`, etc. Every test is gated `#[serial]` because the
//! sandbox mutates process env.
//!
//! Coverage maps to the Phase 1F guard sequence:
//!   - happy path (no guards)
//!   - `requires_env` failure
//!   - `confirm` failure (non-TTY without `--yes`)
//!   - `secrets` failure (env-provider, reference unset → audit empty)
//!   - `audit_log` success path (start + finish JSONL records written)
//!   - SIGINT integration (binary level — additive over
//!     `dispatch::tests::signal_forwarding...` which exercises the same
//!     code path at the unit level)
//!
//! Plan reference: Phase 1L "Dispatcher unit + integration tests".

#![cfg(unix)]

mod common;

use std::time::{Duration, Instant};

use assert_cmd::Command as AssertCommand;
use common::{stage_extension, XdgSandbox};
use predicates::prelude::*;
use serial_test::serial;

#[test]
#[serial]
fn dev_hello_runs_clean() {
    let _sandbox = XdgSandbox::new();
    AssertCommand::cargo_bin("qli")
        .unwrap()
        .args(["dev", "hello"])
        .assert()
        .success()
        .stdout(predicate::str::contains("hello from dev"));
}

#[test]
#[serial]
fn prod_without_env_fails_with_export_hint() {
    let _sandbox = XdgSandbox::new();
    AssertCommand::cargo_bin("qli")
        .unwrap()
        .env_remove("QLI_ENV")
        .args(["prod", "hello"])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("export QLI_ENV=prod"));
}

#[test]
#[serial]
fn prod_non_tty_without_yes_fails_confirm() {
    let _sandbox = XdgSandbox::new();
    // assert_cmd inherits piped stdin → not a TTY → confirm refuses
    // without --yes. This is exactly the production behaviour we want
    // to lock in: a script piping into qli prod must explicitly opt in.
    AssertCommand::cargo_bin("qli")
        .unwrap()
        .env("QLI_ENV", "prod")
        .args(["prod", "hello"])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("--yes"));
}

#[test]
#[serial]
fn env_secret_failure_leaves_audit_empty() {
    let sandbox = XdgSandbox::new();
    let audit_path = sandbox.state_dir().join("audit.log");
    let manifest = format!(
        r#"
schema_version = 1
description = "test env-secret failure"
audit_log = "{audit}"

[[secrets]]
env = "INJECTED"
ref = "QLI_NEVER_SET_TEST_SECRET"
provider = "env"
"#,
        audit = audit_path.display(),
    );
    stage_extension(
        &sandbox,
        "secret-fail",
        "hello",
        &manifest,
        "#!/bin/sh\necho should-not-run\n",
    );

    AssertCommand::cargo_bin("qli")
        .unwrap()
        .env_remove("QLI_NEVER_SET_TEST_SECRET")
        .args(["secret-fail", "hello"])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("QLI_NEVER_SET_TEST_SECRET"));

    assert!(
        !audit_path.exists() || std::fs::read_to_string(&audit_path).unwrap().is_empty(),
        "audit log must stay empty when secret resolution fails before audit-start"
    );
}

#[test]
#[serial]
fn successful_run_writes_audit_start_and_finish() {
    let sandbox = XdgSandbox::new();
    let audit_path = sandbox.state_dir().join("audit.log");
    let manifest = format!(
        r#"
schema_version = 1
description = "test audit log shape"
audit_log = "{audit}"
"#,
        audit = audit_path.display(),
    );
    stage_extension(
        &sandbox,
        "auditcheck",
        "hello",
        &manifest,
        "#!/bin/sh\necho ran\n",
    );

    AssertCommand::cargo_bin("qli")
        .unwrap()
        .args(["auditcheck", "hello"])
        .assert()
        .success()
        .stdout(predicate::str::contains("ran"));

    let body = std::fs::read_to_string(&audit_path).expect("audit log written");
    let lines: Vec<&str> = body.lines().collect();
    assert_eq!(lines.len(), 2, "expected start+finish, got: {body}");
    assert!(
        lines[0].contains(r#""event":"start""#),
        "first line not start: {}",
        lines[0]
    );
    assert!(
        lines[1].contains(r#""event":"finish""#) && lines[1].contains(r#""exit_code":0"#),
        "second line not finish/0: {}",
        lines[1]
    );
}

#[test]
#[serial]
fn sigint_during_slow_extension_writes_interrupted_audit() {
    use std::os::unix::process::CommandExt as _;

    use nix::sys::signal::{killpg, Signal};
    use nix::unistd::Pid;

    let sandbox = XdgSandbox::new();
    let audit_path = sandbox.state_dir().join("audit.log");
    let manifest = format!(
        r#"
schema_version = 1
description = "slow extension for SIGINT integration test"
audit_log = "{audit}"
"#,
        audit = audit_path.display(),
    );
    // 30s sleep; the test signals well before that.
    stage_extension(
        &sandbox,
        "slow",
        "wait",
        &manifest,
        "#!/bin/sh\nexec sleep 30\n",
    );

    // Put qli (and its children) in their own process group so `killpg`
    // can reach them without also signalling the test runner. `process_group`
    // is a *safe* stdlib API (stable since Rust 1.64) — equivalent to
    // `setpgid(0, 0)` in the child after fork, no `unsafe` needed. With
    // qli as its own pgrp leader, `killpg(qli.pid, SIGINT)` simulates
    // the terminal Ctrl+C broadcast: both qli AND its child receive
    // SIGINT directly, the child dies of SIGINT (signal 2 → exit 130),
    // and qli propagates that exit code.
    let bin = assert_cmd::cargo::cargo_bin("qli");
    let mut child = std::process::Command::new(bin)
        .args(["slow", "wait"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .process_group(0)
        .spawn()
        .expect("spawn qli");

    // Wait for audit-start to land — proxy for "child is now running".
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if audit_path.exists() {
            let body = std::fs::read_to_string(&audit_path).unwrap_or_default();
            if body.contains(r#""event":"start""#) {
                break;
            }
        }
        assert!(
            Instant::now() < deadline,
            "audit-start never appeared; qli may have failed before dispatch"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
    // Tiny grace period so the child is in `sleep`, not still in fork+exec.
    std::thread::sleep(Duration::from_millis(100));

    // Broadcast SIGINT to the qli process group. The child (`sleep 30`,
    // no SIGINT trap) dies with SIGINT (exit 130). qli's ctrlc handler
    // fires too and forwards SIGTERM to the now-dead child (no-op), then
    // qli's `child.wait()` returns with signal=SIGINT, dispatcher writes
    // `Interrupted`, qli propagates `128 + SIGINT (2) = 130`.
    //
    // Note on the audit signal label: per the Phase 1F simplification,
    // `on_signal()` always labels its forwarded signal as SIGTERM
    // because `ctrlc::set_handler` doesn't expose the originating signal.
    // The audit record's `signal` field reads "SIGTERM" even though the
    // child actually died from SIGINT. The exit-code assertion below is
    // what the plan task pinned; the audit-presence assertion below
    // checks the `interrupted` event without asserting the label.
    let pgid = Pid::from_raw(child.id().try_into().unwrap());
    killpg(pgid, Signal::SIGINT).expect("killpg SIGINT");

    let status = child.wait().expect("wait qli");
    assert_eq!(
        status.code(),
        Some(130),
        "expected qli to exit 130 (128 + SIGINT); got {status:?}"
    );

    let body = std::fs::read_to_string(&audit_path).expect("audit log written");
    assert!(
        body.contains(r#""event":"interrupted""#),
        "expected interrupted audit record; got:\n{body}"
    );
}
