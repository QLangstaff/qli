//! Embedded extension defaults.
//!
//! The repo's `extensions/` tree is compiled into the binary at build time
//! via [`include_dir!`]. Two write paths consume it:
//!
//! - **Dispatch-time materialization** — on every startup the binary
//!   extracts [`DEFAULTS`] to a version-keyed cache root
//!   (`$XDG_CACHE_HOME/qli/embedded/<version>/`). Discovery then walks
//!   both that cache root and `$XDG_DATA_HOME/qli/extensions/`, with the
//!   user-editable XDG copy shadowing embedded per-group. Net: a freshly
//!   installed binary has working defaults with no user opt-in.
//! - **`qli ext install-defaults`** — explicit user opt-in; copies
//!   [`DEFAULTS`] into `$XDG_DATA_HOME/qli/extensions/` so the user can
//!   edit them. Same [`materialize_to`] entry point, different target
//!   root, optional `--force` overwrite.
//!
//! `include_dir` does not preserve mode bits, so [`materialize_to`]
//! explicitly chmods every non-`_manifest.toml` file to `0o755` on Unix
//! after writing it. Without that, discovery's `is_executable` filter
//! would warn-and-skip every shipped script.
//!
//! ## Crate-publish (resolved in Phase 1.5C via symlink)
//!
//! [`include_dir!`] resolves the path at compile time relative to
//! `$CARGO_MANIFEST_DIR`, and `cargo publish` strips files outside the
//! crate directory — so a path like `../../extensions` would compile
//! locally but produce an empty [`DEFAULTS`] in the published tarball.
//! Resolution: `crates/qli-ext/extensions` is a symlink to the
//! workspace-root `extensions/` directory. `cargo package` dereferences
//! the symlink and bundles the actual files into the crate tarball, so
//! the published `qli-ext` is self-contained while the workspace root
//! stays the canonical edit location.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use include_dir::{include_dir, Dir, DirEntry};
use thiserror::Error;

/// Compile-time snapshot of the repo's `extensions/` tree.
pub static DEFAULTS: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/extensions");

/// Counters from a [`materialize_to`] run. Useful for log lines and
/// `install-defaults` output ("wrote N files, skipped M existing").
#[derive(Debug, Default, Clone, Copy)]
pub struct MaterializeStats {
    pub written: usize,
    pub skipped: usize,
}

/// Errors raised while writing the embedded tree to disk.
#[derive(Debug, Error)]
pub enum MaterializeError {
    #[error("could not create directory {path:?}: {source}")]
    CreateDir {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("could not write {path:?}: {source}")]
    Write {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("could not chmod {path:?}: {source}")]
    Chmod {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
}

/// Write [`DEFAULTS`] into `target_root`, preserving the
/// `<group>/<file>` layout. Files whose name is not `_manifest.toml` are
/// chmod'd to `0o755` on Unix (so discovery treats them as executable).
///
/// Idempotent: existing files are skipped unless `force = true`. Skipping
/// is observed at the file granularity, not the group — a partially
/// installed group remains partially installed unless `force` is passed.
pub fn materialize_to(
    target_root: &Path,
    force: bool,
) -> Result<MaterializeStats, MaterializeError> {
    let mut stats = MaterializeStats::default();
    // Top-level files (e.g. the repo's `extensions/README.md`) are
    // documentation, not extensions — skip them. Only descend into
    // subdirectories: each one is a group.
    for entry in DEFAULTS.entries() {
        if let DirEntry::Dir(sub) = entry {
            let sub_target = target_root.join(sub.path());
            fs::create_dir_all(&sub_target).map_err(|source| MaterializeError::CreateDir {
                path: sub_target.clone(),
                source,
            })?;
            materialize_dir(sub, target_root, force, &mut stats)?;
        }
    }
    Ok(stats)
}

fn materialize_dir(
    dir: &Dir<'_>,
    target_root: &Path,
    force: bool,
    stats: &mut MaterializeStats,
) -> Result<(), MaterializeError> {
    for entry in dir.entries() {
        match entry {
            DirEntry::Dir(sub) => {
                let sub_target = target_root.join(sub.path());
                fs::create_dir_all(&sub_target).map_err(|source| MaterializeError::CreateDir {
                    path: sub_target.clone(),
                    source,
                })?;
                materialize_dir(sub, target_root, force, stats)?;
            }
            DirEntry::File(file) => {
                let dest = target_root.join(file.path());
                if let Some(parent) = dest.parent() {
                    fs::create_dir_all(parent).map_err(|source| MaterializeError::CreateDir {
                        path: parent.to_path_buf(),
                        source,
                    })?;
                }
                if dest.exists() && !force {
                    stats.skipped += 1;
                    continue;
                }
                fs::write(&dest, file.contents()).map_err(|source| MaterializeError::Write {
                    path: dest.clone(),
                    source,
                })?;
                let is_manifest = dest
                    .file_name()
                    .and_then(|s| s.to_str())
                    .is_some_and(|name| name == "_manifest.toml");
                if !is_manifest {
                    set_executable(&dest)?;
                }
                stats.written += 1;
            }
        }
    }
    Ok(())
}

#[cfg(unix)]
fn set_executable(path: &Path) -> Result<(), MaterializeError> {
    use std::os::unix::fs::PermissionsExt;
    let perms = fs::Permissions::from_mode(0o755);
    fs::set_permissions(path, perms).map_err(|source| MaterializeError::Chmod {
        path: path.to_path_buf(),
        source,
    })
}

#[cfg(not(unix))]
fn set_executable(_path: &Path) -> Result<(), MaterializeError> {
    // On non-Unix, executability isn't a permission bit; discovery's
    // `is_executable` returns true for any regular file there. Nothing
    // to do.
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_contains_expected_groups() {
        // If this fails, the include_dir! path probably regressed.
        let dirs: Vec<_> = DEFAULTS
            .entries()
            .iter()
            .filter_map(|e| match e {
                DirEntry::Dir(d) => d.path().file_name().and_then(|s| s.to_str()),
                DirEntry::File(_) => None,
            })
            .collect();
        for expected in &["dev", "prod", "org"] {
            assert!(
                dirs.contains(expected),
                "expected `{expected}` group in DEFAULTS, got {dirs:?}",
            );
        }
    }

    #[test]
    fn materialize_writes_manifests_and_scripts() {
        let tmp = tempfile::tempdir().unwrap();
        let stats = materialize_to(tmp.path(), false).unwrap();
        assert!(
            stats.written >= 6,
            "expected ≥6 files, got {}",
            stats.written
        );
        assert_eq!(stats.skipped, 0);

        for group in &["dev", "prod", "org"] {
            let manifest = tmp.path().join(group).join("_manifest.toml");
            assert!(
                manifest.exists(),
                "manifest missing for {group}: {}",
                manifest.display(),
            );
            let script = tmp.path().join(group).join("hello");
            assert!(
                script.exists(),
                "script missing for {group}: {}",
                script.display(),
            );
        }
    }

    #[test]
    #[cfg(unix)]
    fn materialize_sets_exec_bit_on_scripts_only() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        materialize_to(tmp.path(), false).unwrap();

        let script_mode = fs::metadata(tmp.path().join("dev/hello"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(
            script_mode & 0o777,
            0o755,
            "hello script should be 0o755, got {script_mode:o}",
        );

        // Manifest should NOT have exec bits — discovery's `_*` skip
        // would never see it anyway, but the principle stands: only
        // files we'd dispatch are executable.
        let manifest_mode = fs::metadata(tmp.path().join("dev/_manifest.toml"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(
            manifest_mode & 0o111,
            0,
            "manifest should not be executable, got {manifest_mode:o}",
        );
    }

    #[test]
    fn materialize_is_idempotent_without_force() {
        let tmp = tempfile::tempdir().unwrap();
        let first = materialize_to(tmp.path(), false).unwrap();
        let second = materialize_to(tmp.path(), false).unwrap();
        assert!(first.written >= 6);
        assert_eq!(second.written, 0, "second run should write nothing");
        assert_eq!(second.skipped, first.written);
    }

    #[test]
    fn materialize_skips_top_level_files() {
        // The repo's `extensions/README.md` is documentation, not an
        // extension. It must not be installed into the user's XDG dir.
        let tmp = tempfile::tempdir().unwrap();
        materialize_to(tmp.path(), false).unwrap();
        assert!(
            !tmp.path().join("README.md").exists(),
            "top-level README.md must not be materialized",
        );
    }

    #[test]
    fn materialize_force_overwrites_existing_files() {
        let tmp = tempfile::tempdir().unwrap();
        materialize_to(tmp.path(), false).unwrap();
        let target = tmp.path().join("dev/hello");
        fs::write(&target, "edited by user\n").unwrap();
        let stats = materialize_to(tmp.path(), true).unwrap();
        assert_eq!(stats.skipped, 0);
        assert!(stats.written >= 6);
        let body = fs::read_to_string(&target).unwrap();
        assert!(
            !body.contains("edited by user"),
            "force should overwrite, got: {body}",
        );
    }
}
