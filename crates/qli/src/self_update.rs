//! Install-method detection for `qli self-update`.
//!
//! `qli` ships via three paths today (cargo / Homebrew / curl installer). The
//! Claude Code plugin install path runs `qli` from `PATH` after one of the
//! three; it doesn't install the binary itself and can't be detected from the
//! binary's canonical path, so it's surfaced as an always-appended note rather
//! than a fourth branch.
//!
//! Detection is path-based with one twist: cargo-dist's curl installer drops
//! the binary into `~/.cargo/bin/` — the same directory as `cargo install`. The
//! tie-breaker is `~/.cargo/.crates.toml`, the on-disk registry of crates
//! installed via cargo. Cargo writes that file; the curl installer doesn't.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{anyhow, Context, Result};

pub const INSTALLER_URL: &str =
    "https://github.com/QLangstaff/qli/releases/latest/download/qli-installer.sh";

pub const PLUGIN_NOTE: &str = "Claude Code plugin? Refresh it with \
    `/plugin marketplace update qli-plugins` from inside Claude Code.";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InstallMethod {
    Cargo,
    Homebrew,
    Curl,
    Unknown,
}

impl InstallMethod {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Cargo => "cargo",
            Self::Homebrew => "homebrew",
            Self::Curl => "curl",
            Self::Unknown => "unknown",
        }
    }

    pub fn upgrade_command(self) -> Option<&'static str> {
        match self {
            Self::Cargo => Some("cargo install qli --force"),
            Self::Homebrew => Some("brew upgrade qli"),
            Self::Curl => Some(
                "curl -LsSf \
                https://github.com/QLangstaff/qli/releases/latest/download/qli-installer.sh | sh",
            ),
            Self::Unknown => None,
        }
    }
}

pub struct Detection {
    pub method: InstallMethod,
    pub binary_path: PathBuf,
}

pub fn detect() -> Detection {
    let binary_path = match std::env::current_exe() {
        Ok(p) => std::fs::canonicalize(&p).unwrap_or(p),
        Err(_) => {
            return Detection {
                method: InstallMethod::Unknown,
                binary_path: PathBuf::new(),
            }
        }
    };
    // Canonicalize HOME to match `binary_path` — on macOS, `/tmp` resolves to
    // `/private/tmp` (and other symlinks exist in the user-directory tree on
    // some setups), so leaving HOME un-canonicalized makes `starts_with`
    // miss when the binary's canonical form lives below a symlinked HOME.
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .map(|h| std::fs::canonicalize(&h).unwrap_or(h));
    let method = detect_method(&binary_path, home.as_deref());
    Detection {
        method,
        binary_path,
    }
}

fn detect_method(canonical: &Path, home: Option<&Path>) -> InstallMethod {
    // Homebrew resolves through `bin/qli -> ../Cellar/qli/<ver>/bin/qli`, so
    // the canonical path contains `/Cellar/` on both Intel (`/usr/local/Cellar`)
    // and Apple Silicon (`/opt/homebrew/Cellar`).
    if path_contains_segment(canonical, "Cellar") {
        return InstallMethod::Homebrew;
    }
    if let Some(home) = home {
        let cargo_bin = home.join(".cargo").join("bin");
        if canonical.starts_with(&cargo_bin) {
            return if cargo_registry_contains_qli(home) {
                InstallMethod::Cargo
            } else {
                InstallMethod::Curl
            };
        }
        let local_bin = home.join(".local").join("bin");
        if canonical.starts_with(&local_bin) {
            return InstallMethod::Curl;
        }
    }
    InstallMethod::Unknown
}

fn path_contains_segment(p: &Path, segment: &str) -> bool {
    p.components()
        .any(|c| c.as_os_str().to_string_lossy() == segment)
}

/// `~/.cargo/.crates.toml` is cargo's on-disk registry of `cargo install`'d
/// binaries. Entries look like:
///
/// ```toml
/// [v1]
/// "qli 0.1.1 (registry+https://github.com/rust-lang/crates.io-index)" = ["qli"]
/// ```
///
/// We deliberately don't parse the TOML — the key is `"qli "` (crate name +
/// space) on its own line, and a substring match is robust to future cargo
/// format tweaks. If the file is absent or unreadable, treat it as "no entry".
fn cargo_registry_contains_qli(home: &Path) -> bool {
    let path = home.join(".cargo").join(".crates.toml");
    let Ok(content) = std::fs::read_to_string(&path) else {
        return false;
    };
    content
        .lines()
        .any(|line| line.trim_start().starts_with("\"qli "))
}

/// Re-execute the same `qli-installer.sh` that ships from cargo-dist's release
/// pipeline. The installer detects the running platform, downloads the right
/// tarball, replaces the binary in place. Re-execing it (rather than a parallel
/// `self_update`-crate implementation) means there's only one asset-layout
/// contract to keep in sync with cargo-dist.
pub fn run_curl_installer() -> Result<()> {
    if cfg!(target_os = "windows") {
        // Windows ships a `.ps1` installer, not the shell one. Don't try to
        // shell out to `sh` here; the caller will fall back to printing the
        // upgrade command.
        return Err(anyhow!(
            "automated upgrade is not supported on Windows; \
             re-run the .ps1 installer manually"
        ));
    }
    let cmd = format!("curl -LsSf {INSTALLER_URL} | sh");
    let status = Command::new("sh")
        .arg("-c")
        .arg(&cmd)
        .status()
        .with_context(|| format!("failed to spawn `sh -c '{cmd}'`"))?;
    if !status.success() {
        let code = status
            .code()
            .map_or_else(|| "signal".into(), |c| c.to_string());
        return Err(anyhow!("installer exited with status {code}"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn write_crates_toml(home: &Path, body: &str) {
        let dir = home.join(".cargo");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join(".crates.toml"), body).unwrap();
    }

    #[test]
    fn homebrew_apple_silicon_path() {
        let p = PathBuf::from("/opt/homebrew/Cellar/qli/0.1.1/bin/qli");
        assert_eq!(detect_method(&p, None), InstallMethod::Homebrew);
    }

    #[test]
    fn homebrew_intel_path() {
        let p = PathBuf::from("/usr/local/Cellar/qli/0.1.1/bin/qli");
        assert_eq!(detect_method(&p, None), InstallMethod::Homebrew);
    }

    #[test]
    fn cargo_install_path_with_registry_entry() {
        let home = TempDir::new().unwrap();
        write_crates_toml(
            home.path(),
            "[v1]\n\"qli 0.1.1 (registry+https://github.com/rust-lang/crates.io-index)\" = [\"qli\"]\n",
        );
        let p = home.path().join(".cargo/bin/qli");
        assert_eq!(detect_method(&p, Some(home.path())), InstallMethod::Cargo);
    }

    #[test]
    fn cargo_bin_without_registry_entry_is_curl() {
        let home = TempDir::new().unwrap();
        // No .crates.toml on disk — the curl installer doesn't write it.
        let p = home.path().join(".cargo/bin/qli");
        assert_eq!(detect_method(&p, Some(home.path())), InstallMethod::Curl);
    }

    #[test]
    fn cargo_bin_with_registry_missing_qli_entry_is_curl() {
        let home = TempDir::new().unwrap();
        write_crates_toml(
            home.path(),
            "[v1]\n\"ripgrep 14.0.0 (registry+https://github.com/rust-lang/crates.io-index)\" = [\"rg\"]\n",
        );
        let p = home.path().join(".cargo/bin/qli");
        assert_eq!(detect_method(&p, Some(home.path())), InstallMethod::Curl);
    }

    #[test]
    fn local_bin_is_curl() {
        let home = TempDir::new().unwrap();
        let p = home.path().join(".local/bin/qli");
        assert_eq!(detect_method(&p, Some(home.path())), InstallMethod::Curl);
    }

    #[test]
    fn unknown_path_yields_unknown() {
        let home = TempDir::new().unwrap();
        let p = PathBuf::from("/opt/random/qli");
        assert_eq!(detect_method(&p, Some(home.path())), InstallMethod::Unknown);
    }

    #[test]
    fn registry_check_handles_qli_prefix_not_qli_substring() {
        // "qli-ext" should NOT match — we look for the binary crate name `qli`
        // followed by a space (the version).
        let home = TempDir::new().unwrap();
        write_crates_toml(
            home.path(),
            "[v1]\n\"qli-ext 0.1.1 (path+file:///somewhere)\" = [\"qli-ext\"]\n",
        );
        assert!(!cargo_registry_contains_qli(home.path()));
    }

    #[test]
    fn upgrade_commands_present_for_known_methods() {
        assert!(InstallMethod::Cargo.upgrade_command().is_some());
        assert!(InstallMethod::Homebrew.upgrade_command().is_some());
        assert!(InstallMethod::Curl.upgrade_command().is_some());
        assert!(InstallMethod::Unknown.upgrade_command().is_none());
    }
}
