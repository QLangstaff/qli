//! Hermetic test harness for the `qli` binary's integration tests.
//!
//! Mirrors `crates/qli-ext/tests/common/mod.rs::XdgSandbox`. Push B added
//! this file alongside the `assert_cmd`-based dispatcher integration
//! tests; keeping the helper file shape identical between crates makes
//! the cross-crate test pattern easy to recognise.
//!
//! Every test that spawns the qli binary wraps itself in
//! `XdgSandbox::new()` and gates with `#[serial_test::serial]` — the
//! sandbox mutates process env, so concurrent sandboxed tests would race.

#![allow(dead_code)] // Helpers picked up à la carte by integration tests.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use tempfile::TempDir;

const XDG_VARS: &[&str] = &[
    "HOME",
    "XDG_CONFIG_HOME",
    "XDG_DATA_HOME",
    "XDG_STATE_HOME",
    "XDG_CACHE_HOME",
];

// Runtime vars whose presence in the test process leaks into children
// spawned via `assert_cmd` (which inherits parent env by default) and
// `std::process::Command` (likewise). Scrubbed to `unset` for the
// lifetime of the sandbox so a developer with e.g. `QLI_ENV=prod`
// exported in their shell sees the same behaviour as CI. Restored on
// Drop via the same saved-prev mechanism as XDG_VARS.
const SCRUBBED_VARS: &[&str] = &["QLI_ENV"];

/// RAII guard: points `HOME` and the four `XDG_*` vars at fresh subdirs
/// under a `TempDir`, scrubs runtime vars listed in `SCRUBBED_VARS`, and
/// restores all prior values on `Drop`.
pub struct XdgSandbox {
    tmp: TempDir,
    saved: Vec<(&'static str, Option<OsString>)>,
}

impl XdgSandbox {
    pub fn new() -> Self {
        // Build `Self` *before* mutating any env. Once it's stack-resident,
        // a panic in the loop unwinds through `Drop`, which restores any
        // vars we've already pushed into `saved`.
        let tmp = tempfile::tempdir().expect("create xdg sandbox tempdir");
        let mut sandbox = Self {
            tmp,
            saved: Vec::with_capacity(XDG_VARS.len() + SCRUBBED_VARS.len()),
        };
        for &var in XDG_VARS {
            sandbox.saved.push((var, std::env::var_os(var)));
            let dir = sandbox.tmp.path().join(subdir_for(var));
            std::fs::create_dir_all(&dir).expect("create xdg subdir");
            std::env::set_var(var, &dir);
        }
        for &var in SCRUBBED_VARS {
            sandbox.saved.push((var, std::env::var_os(var)));
            std::env::remove_var(var);
        }
        sandbox
    }

    pub fn path(&self) -> &Path {
        self.tmp.path()
    }

    pub fn extensions_dir(&self) -> PathBuf {
        self.tmp.path().join("data").join("qli").join("extensions")
    }

    pub fn state_dir(&self) -> PathBuf {
        self.tmp.path().join("state").join("qli")
    }
}

impl Drop for XdgSandbox {
    fn drop(&mut self) {
        for (var, prev) in self.saved.drain(..) {
            match prev {
                Some(v) => std::env::set_var(var, v),
                None => std::env::remove_var(var),
            }
        }
    }
}

fn subdir_for(var: &str) -> &'static str {
    match var {
        "HOME" => "home",
        "XDG_CONFIG_HOME" => "config",
        "XDG_DATA_HOME" => "data",
        "XDG_STATE_HOME" => "state",
        "XDG_CACHE_HOME" => "cache",
        _ => unreachable!("unknown xdg var: {var}"),
    }
}

/// Stage an extension into the sandbox's XDG extensions dir.
///
/// Writes `<sandbox>/data/qli/extensions/<group>/_manifest.toml` with
/// `manifest_toml`, plus `<sandbox>/data/qli/extensions/<group>/<name>`
/// with `script_body` and mode 0o755.
#[cfg(unix)]
pub fn stage_extension(
    sandbox: &XdgSandbox,
    group: &str,
    name: &str,
    manifest_toml: &str,
    script_body: &str,
) {
    use std::os::unix::fs::PermissionsExt;
    let group_dir = sandbox.extensions_dir().join(group);
    std::fs::create_dir_all(&group_dir).expect("create group dir");
    std::fs::write(group_dir.join("_manifest.toml"), manifest_toml).expect("write manifest");
    let script = group_dir.join(name);
    std::fs::write(&script, script_body).expect("write script");
    let mut perms = std::fs::metadata(&script).unwrap().permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&script, perms).expect("chmod script");
}
