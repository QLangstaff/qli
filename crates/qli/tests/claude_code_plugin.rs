//! Plugin contract tests: keep `claude-code-plugin/` aligned with the
//! qli binary's actual behavior.
//!
//! Two layers:
//!   - **Static** (no sandbox): plugin.json validity, version pinning,
//!     frontmatter shape on every command file and the skill.
//!   - **Behavioral** (sandboxed): the output-stream / exit-code claims
//!     SKILL.md makes about qli actually hold when invoking the real
//!     binary.
//!
//! Bumping the qli crate version fails these tests until
//! `claude-code-plugin/.claude-plugin/plugin.json` and SKILL.md's body
//! pin are bumped in lockstep. That's intentional — the plugin
//! documents one specific qli release.

#![cfg(unix)]

mod common;

use std::fs;
use std::path::{Path, PathBuf};

use assert_cmd::Command as AssertCommand;
use common::XdgSandbox;
use predicates::prelude::*;
use serial_test::serial;

fn plugin_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root is two levels up from crates/qli")
        .join("claude-code-plugin")
}

/// Strip the YAML frontmatter (between leading `---` lines) off a file.
fn frontmatter_of(path: &Path) -> String {
    let body = fs::read_to_string(path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let stripped = body.strip_prefix("---\n").unwrap_or_else(|| {
        panic!(
            "{} missing leading `---` frontmatter delimiter",
            path.display()
        )
    });
    let end = stripped.find("\n---\n").unwrap_or_else(|| {
        panic!(
            "{} missing closing `---` frontmatter delimiter",
            path.display()
        )
    });
    stripped[..end].to_string()
}

#[test]
fn plugin_json_has_required_fields() {
    let manifest_path = plugin_dir().join(".claude-plugin/plugin.json");
    let raw = fs::read_to_string(&manifest_path)
        .unwrap_or_else(|e| panic!("read {}: {e}", manifest_path.display()));
    let value: serde_json::Value =
        serde_json::from_str(&raw).unwrap_or_else(|e| panic!("plugin.json parse: {e}"));
    let obj = value.as_object().expect("plugin.json is a JSON object");
    for field in ["name", "version", "description", "license"] {
        assert!(obj.contains_key(field), "plugin.json missing `{field}`");
    }
    assert_eq!(obj["name"], "qli", "plugin.json name must be `qli`");
}

#[test]
fn plugin_version_matches_qli_crate_version() {
    let crate_version = env!("CARGO_PKG_VERSION");

    let manifest_path = plugin_dir().join(".claude-plugin/plugin.json");
    let value: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&manifest_path).unwrap()).unwrap();
    let plugin_version = value["version"]
        .as_str()
        .expect("plugin.json version is string");
    assert_eq!(
        plugin_version, crate_version,
        "plugin.json version drifted from qli crate version; bump in lockstep"
    );

    // SKILL.md body carries its own pinned version reference; catch its
    // drift too — otherwise plugin.json can be bumped while SKILL.md
    // body silently lies about the targeted qli release.
    let skill_path = plugin_dir().join("skills/qli/SKILL.md");
    let skill_body = fs::read_to_string(&skill_path).unwrap();
    let expected = format!("qli v{crate_version}");
    assert!(
        skill_body.contains(&expected),
        "SKILL.md missing `{expected}` — version pin in body has drifted from crate"
    );
}

#[test]
fn every_command_file_has_valid_frontmatter() {
    let commands_dir = plugin_dir().join("commands");
    let entries: Vec<_> = fs::read_dir(&commands_dir)
        .unwrap_or_else(|e| panic!("read_dir {}: {e}", commands_dir.display()))
        .filter_map(Result::ok)
        .filter(|e| e.path().extension().and_then(|s| s.to_str()) == Some("md"))
        .collect();
    assert!(
        !entries.is_empty(),
        "expected at least one `.md` file in {}",
        commands_dir.display()
    );
    for entry in entries {
        let fm = frontmatter_of(&entry.path());
        for required in ["description:", "allowed-tools:"] {
            assert!(
                fm.lines().any(|l| l.trim_start().starts_with(required)),
                "{}: frontmatter missing `{}`",
                entry.path().display(),
                required
            );
        }
    }
}

#[test]
fn skill_file_has_valid_frontmatter() {
    let skill_path = plugin_dir().join("skills/qli/SKILL.md");
    let fm = frontmatter_of(&skill_path);
    assert!(
        fm.lines().any(|l| l.trim() == "name: qli"),
        "SKILL.md frontmatter must declare `name: qli`"
    );
    assert!(
        fm.lines()
            .any(|l| l.trim_start().starts_with("description:")),
        "SKILL.md frontmatter missing `description`"
    );
}

#[test]
#[serial]
fn documented_subcommands_appear_in_qli_help() {
    let _sandbox = XdgSandbox::new();
    let root_help = AssertCommand::cargo_bin("qli")
        .unwrap()
        .arg("--help")
        .output()
        .expect("run qli --help");
    let root_help_text = String::from_utf8(root_help.stdout).unwrap();
    for cmd in ["ext", "completions", "self-update", "dev", "prod", "org"] {
        assert!(
            root_help_text.contains(cmd),
            "`qli --help` missing documented subcommand `{cmd}`:\n{root_help_text}"
        );
    }

    let ext_help = AssertCommand::cargo_bin("qli")
        .unwrap()
        .args(["ext", "--help"])
        .output()
        .expect("run qli ext --help");
    let ext_help_text = String::from_utf8(ext_help.stdout).unwrap();
    for action in ["list", "which", "install-defaults"] {
        assert!(
            ext_help_text.contains(action),
            "`qli ext --help` missing documented action `{action}`:\n{ext_help_text}"
        );
    }
}

#[test]
#[serial]
fn ext_list_writes_to_stdout() {
    let _sandbox = XdgSandbox::new();
    AssertCommand::cargo_bin("qli")
        .unwrap()
        .args(["ext", "list"])
        .assert()
        .success()
        .stdout(predicate::str::is_empty().not());
}

#[test]
#[serial]
fn ext_install_defaults_writes_summary_to_stderr() {
    let _sandbox = XdgSandbox::new();
    AssertCommand::cargo_bin("qli")
        .unwrap()
        .args(["ext", "install-defaults"])
        .assert()
        .success()
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::starts_with("installed defaults to "));
}

#[test]
#[serial]
fn self_update_exits_2_with_stub_message() {
    let _sandbox = XdgSandbox::new();
    AssertCommand::cargo_bin("qli")
        .unwrap()
        .arg("self-update")
        .assert()
        .failure()
        .code(2)
        .stderr(predicate::str::contains("not yet implemented"));
}

#[test]
#[serial]
fn ext_which_unknown_extension_exits_1() {
    let _sandbox = XdgSandbox::new();
    AssertCommand::cargo_bin("qli")
        .unwrap()
        .args(["ext", "which", "dev", "nonexistent-zzz"])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("unknown extension"));
}
