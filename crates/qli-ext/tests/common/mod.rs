//! Hermetic test harness for qli-ext integration tests.
//!
//! Plan reference: Phase 1L "Hermetic test harness". The pattern: every test
//! that calls into XDG-aware code (`paths::*`, `etcetera::Xdg`) wraps
//! itself in [`XdgSandbox`] so it never touches the host's `~/.config/qli`,
//! `~/.local/share/qli`, etc. Push B mirrors this file into
//! `crates/qli/tests/common/mod.rs` for `assert_cmd` integration tests.
//!
//! ## Usage
//!
//! ```ignore
//! mod common;
//! use common::XdgSandbox;
//! use serial_test::serial;
//!
//! #[test]
//! #[serial] // process env is global; serialize all sandbox tests
//! fn my_hermetic_test() {
//!     let sandbox = XdgSandbox::new();
//!     // Code under test resolves XDG paths to subdirs of `sandbox.path()`.
//!     // The sandbox restores prior env on Drop.
//! }
//! ```
//!
//! ## Why `#[serial]`
//!
//! `XdgSandbox` mutates process-wide env vars; two sandboxed tests running
//! concurrently in the same process would race. Gate every test that uses
//! the sandbox (or any other env mutation) with `#[serial_test::serial]`.
//! Unit tests in `src/*.rs` follow the same convention — see e.g.
//! `audit::tests::expand_uses_process_env_first`.

#![allow(dead_code)] // Helpers used by Push B integration tests; quiet 1L Push A.

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

/// RAII guard that points every XDG env var (plus `HOME`) at a fresh
/// `TempDir`, then restores the prior values on `Drop`.
pub struct XdgSandbox {
    tmp: TempDir,
    saved: Vec<(&'static str, Option<OsString>)>,
}

impl XdgSandbox {
    pub fn new() -> Self {
        // Build `Self` *before* mutating any env. Once it's stack-resident,
        // a panic in the loop unwinds through `Drop`, which restores any
        // vars we've already pushed into `saved`. If we built `saved` as a
        // bare Vec, a panic mid-loop would leave the process env mutated.
        let tmp = tempfile::tempdir().expect("create xdg sandbox tempdir");
        let mut sandbox = Self {
            tmp,
            saved: Vec::with_capacity(XDG_VARS.len()),
        };
        for &var in XDG_VARS {
            sandbox.saved.push((var, std::env::var_os(var)));
            let dir = sandbox.tmp.path().join(subdir_for(var));
            std::fs::create_dir_all(&dir).expect("create xdg subdir");
            std::env::set_var(var, &dir);
        }
        sandbox
    }

    pub fn path(&self) -> &Path {
        self.tmp.path()
    }

    pub fn data_dir(&self) -> PathBuf {
        self.tmp.path().join("data").join("qli")
    }

    pub fn extensions_dir(&self) -> PathBuf {
        self.data_dir().join("extensions")
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
