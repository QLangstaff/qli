//! Smoke test for the `XdgSandbox` helper in `tests/common/mod.rs`.
//!
//! Push B will exercise the sandbox more heavily via `assert_cmd` /
//! `trycmd` integration tests against the qli binary. This smoke just
//! proves the harness sets + restores env correctly so a typo in the
//! harness fails here, not from a confusing failure deep in a real test.

mod common;

use serial_test::serial;

use common::XdgSandbox;

#[test]
#[serial]
fn sandbox_overrides_xdg_vars_then_restores_them() {
    let prior_home = std::env::var_os("HOME");
    let prior_data = std::env::var_os("XDG_DATA_HOME");

    {
        let sandbox = XdgSandbox::new();
        let home = std::env::var_os("HOME").expect("HOME set inside sandbox");
        let data = std::env::var_os("XDG_DATA_HOME").expect("XDG_DATA_HOME set inside sandbox");
        assert!(
            std::path::Path::new(&home).starts_with(sandbox.path()),
            "HOME points inside sandbox"
        );
        assert!(
            std::path::Path::new(&data).starts_with(sandbox.path()),
            "XDG_DATA_HOME points inside sandbox"
        );
    }

    assert_eq!(std::env::var_os("HOME"), prior_home);
    assert_eq!(std::env::var_os("XDG_DATA_HOME"), prior_data);
}
