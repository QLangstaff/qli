//! End-to-end `qli analyze` tests against the real `qli` binary.
//!
//! These cover language resolution, which lives in the binary (it probes the
//! filesystem for project marker files): a `.html` file is an Angular template
//! under `--lang auto` only when an ancestor directory holds `angular.json`.
//!
//! Like the dispatcher tests, every case runs under [`XdgSandbox`] (the
//! analysis cache would otherwise land in the host's cache dir) and is gated
//! `#[serial]` because the sandbox mutates process env.

mod common;

use std::path::PathBuf;

use assert_cmd::Command as AssertCommand;
use common::XdgSandbox;
use predicates::prelude::*;
use serial_test::serial;

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures")
}

fn analyze() -> AssertCommand {
    let mut cmd = AssertCommand::cargo_bin("qli").unwrap();
    cmd.args(["analyze", "--format", "jsonl"]);
    cmd
}

#[test]
#[serial]
fn auto_analyzes_html_inside_an_angular_workspace() {
    let _sandbox = XdgSandbox::new();
    analyze()
        .arg(fixtures().join("angular"))
        .assert()
        .success()
        .stdout(predicate::str::contains("sample.component.html"))
        .stdout(predicate::str::contains(r#""code":"todo""#))
        .stdout(predicate::str::contains(r#""code":"hack""#))
        // `angular.json` itself has no adapter and is skipped.
        .stdout(predicate::str::contains("angular.json").not());
}

#[test]
#[serial]
fn auto_finds_the_marker_above_a_bare_relative_path() {
    let _sandbox = XdgSandbox::new();
    // `sample.component.html` has no parent component of its own; the marker
    // above the working directory must still be found.
    analyze()
        .current_dir(fixtures().join("angular/src/app"))
        .arg("sample.component.html")
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""code":"fixme""#));
}

#[test]
#[serial]
fn auto_skips_html_outside_an_angular_workspace() {
    let _sandbox = XdgSandbox::new();
    // A copy in a tempdir, not the checked-in fixture: the ancestor probe
    // would otherwise walk the directories above this checkout.
    let site = tempfile::tempdir().unwrap();
    std::fs::copy(
        fixtures().join("html/plain.html"),
        site.path().join("plain.html"),
    )
    .unwrap();
    analyze()
        .arg(site.path())
        .assert()
        .success()
        .stdout(predicate::str::is_empty());
}

#[test]
#[serial]
fn auto_judges_a_dotdot_path_by_where_the_file_lives() {
    let _sandbox = XdgSandbox::new();
    // Run from inside a workspace, pointing at a sibling outside it: the
    // `..` passes through the marked directory but the file isn't under it.
    let root = tempfile::tempdir().unwrap();
    let workspace = root.path().join("workspace");
    let site = root.path().join("site");
    std::fs::create_dir_all(&workspace).unwrap();
    std::fs::create_dir_all(&site).unwrap();
    std::fs::write(workspace.join("angular.json"), "{}").unwrap();
    std::fs::copy(fixtures().join("html/plain.html"), site.join("plain.html")).unwrap();
    analyze()
        .current_dir(&workspace)
        .arg("../site/plain.html")
        .assert()
        .success()
        .stdout(predicate::str::is_empty());
}

#[test]
#[serial]
fn explicit_lang_angular_bypasses_the_marker() {
    let _sandbox = XdgSandbox::new();
    analyze()
        .args(["--lang", "angular"])
        .arg(fixtures().join("html/plain.html"))
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""code":"todo""#));
}

#[test]
#[serial]
fn defrefs_reads_expressions_embedded_in_a_template() {
    let _sandbox = XdgSandbox::new();
    analyze()
        .args(["--analyzer", "defrefs"])
        .arg(fixtures().join("angular"))
        .assert()
        .success()
        // `@let format = …` is a definition, and `{{ format(…) }}` resolves to it.
        .stdout(predicate::str::contains(
            r#""kind":"variable","name":"format""#,
        ))
        .stdout(
            predicate::str::contains(r#""name":"format","range""#)
                .and(predicate::str::contains(r#""resolved":"qli-local:"#)),
        )
        // `(click)="retry($event)"` is a call site inside an event binding.
        .stdout(predicate::str::contains(r#""name":"retry""#));
}
