//! CLI contract snapshots driven by `trycmd`.
//!
//! Each case file under `tests/cmd/` is a frozen sample of the qli
//! binary's user-visible contract: argv, exit code, stdout, stderr.
//! These guard against accidental regressions to the help text,
//! suggestion behaviour, error messages, and exit codes — the
//! clig.dev compliance bar set in Phase 1.
//!
//! ## Scope (Phase 1L Push B)
//!
//! Six cases, deliberately narrow to keep the snapshots maintainable
//! across clap upgrades:
//!   - `version.toml` — `qli --version`
//!   - `completions-zsh.toml` — first line + redaction of zsh script body
//!   - `unknown-no-tip.toml` — `qli foo` (no close subcommand → no tip)
//!   - `unknown-with-tip.toml` — `qli porod hello` (typo of `prod` → tip)
//!   - `missing-env.toml` — `qli prod hello` without `QLI_ENV` → export hint
//!
//! ## Refresh
//!
//! ```text
//! TRYCMD=overwrite cargo test -p qli --test cli
//! ```
//!
//! After overwriting, eyeball the diff before committing — clap
//! upgrades occasionally reflow help text in ways that look right but
//! drop a contract detail.
//!
//! ## Hermeticity
//!
//! Cases that touch dispatch (e.g. `missing-env`) need `XDG_*` and
//! `HOME` to point at non-host paths so the test never reads or writes
//! the developer's `~/.config/qli`. We set them to a pinned tempdir per
//! test invocation via `TestCases::env`. The embedded extension layer
//! (the `prod` group used by `missing-env`) ships inside the binary via
//! `include_dir!` and does not depend on host XDG state.

use std::sync::Mutex;

// trycmd's runner mutates process env (XDG_*, HOME) inside `.run()`. The
// other integration tests in this crate (assert_cmd-based) gate with
// `#[serial_test::serial]`; this test serializes against them via the
// same `serial_test::serial` macro. The `Mutex` guarding `TRYCMD_LOCK`
// is belt-and-suspenders for any future trycmd parallelism.
static TRYCMD_LOCK: Mutex<()> = Mutex::new(());

#[test]
#[serial_test::serial]
fn cli_contract_snapshots() {
    let _lock = TRYCMD_LOCK.lock().unwrap();
    // `QLI_ENV` set in the test runner's env would change the missing-env
    // case's behaviour. trycmd's `TestCases::env` only sets, not unsets,
    // so we mutate process env here and restore on Drop. The case files
    // themselves never touch `QLI_ENV`.
    let _qli_env_guard = EnvUnset::new("QLI_ENV");
    let tmp = tempfile::tempdir().expect("xdg sandbox tempdir");
    // Explicit `.run()` so the test still executes if trycmd's `Drop`
    // semantics ever change. Without it, a future trycmd that drops the
    // run-on-Drop fallback would turn this into a silent no-op.
    trycmd::TestCases::new()
        .case("tests/cmd/*.toml")
        .env("HOME", tmp.path().display().to_string())
        .env(
            "XDG_CONFIG_HOME",
            tmp.path().join("config").display().to_string(),
        )
        .env(
            "XDG_DATA_HOME",
            tmp.path().join("data").display().to_string(),
        )
        .env(
            "XDG_STATE_HOME",
            tmp.path().join("state").display().to_string(),
        )
        .env(
            "XDG_CACHE_HOME",
            tmp.path().join("cache").display().to_string(),
        )
        .env("NO_COLOR", "1")
        .run();
}

struct EnvUnset {
    key: &'static str,
    prior: Option<std::ffi::OsString>,
}

impl EnvUnset {
    fn new(key: &'static str) -> Self {
        let prior = std::env::var_os(key);
        std::env::remove_var(key);
        Self { key, prior }
    }
}

impl Drop for EnvUnset {
    fn drop(&mut self) {
        if let Some(v) = self.prior.take() {
            std::env::set_var(self.key, v);
        }
    }
}
