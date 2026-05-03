//! Extension discovery: walk one or more on-disk extension roots and
//! `PATH`, building the group/extension table the dispatcher uses to route
//! subcommands.
//!
//! Discovery is pure: it returns a [`Discovery`] value plus a list of
//! human-readable warnings. The CLI binary is responsible for printing the
//! warnings to stderr. This keeps the library testable and lets callers
//! decide their own logging policy.
//!
//! Phase 1H ranks two sources: the user's `$XDG_DATA_HOME/qli/extensions/`
//! and the embedded-defaults cache materialized by [`crate::defaults`].
//! [`discover`] takes the sources in priority order; the **first** source
//! to claim a group name keeps it wholesale — its manifest *and* its
//! extensions. Later sources' copies of that group are silently shadowed.
//! Per-group (not per-extension) shadowing means a user who deletes
//! `dev/hello` from XDG does not see it re-appear from embedded.
//!
//! `PATH` binaries are merged in last and only attach to groups that
//! already exist in some on-disk source. They never define a new group.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use crate::manifest::Manifest;

/// Group names we never let an extension shadow. Includes today's static
/// `qli` subcommand (`completions`) plus the names already promised by later
/// phases of the foundation plan (`ext`, `analyze`, `lsp`, `index`,
/// `self-update`, `mcp`) and clap's own `help`. A user group with one of
/// these names is skipped with a warning rather than left to panic clap at
/// `--help` time.
const RESERVED_GROUP_NAMES: &[&str] = &[
    "analyze",
    "completions",
    "ext",
    "help",
    "index",
    "lsp",
    "mcp",
    "self-update",
];

/// Result of walking the extensions root and PATH.
#[derive(Debug)]
pub struct Discovery {
    pub groups: BTreeMap<String, Group>,
    pub warnings: Vec<String>,
}

/// A discovered extension group: one `_manifest.toml` plus the executables
/// rooted under it.
#[derive(Debug)]
pub struct Group {
    pub name: String,
    pub manifest: Manifest,
    pub manifest_path: PathBuf,
    pub extensions: BTreeMap<String, Extension>,
}

/// A single dispatchable extension within a group.
#[derive(Debug)]
pub struct Extension {
    pub name: String,
    pub group: String,
    pub path: PathBuf,
    pub origin: ExtensionOrigin,
}

/// Where an extension's executable was found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExtensionOrigin {
    /// `<xdg_root>/<group>/<name>` — user-installed, editable.
    Xdg,
    /// Materialized from the binary's embedded defaults to a cache root
    /// (`$XDG_CACHE_HOME/qli/embedded/<version>/<group>/<name>`).
    Embedded,
    /// `qli-<group>-<name>` discovered on `PATH`.
    Path,
}

impl ExtensionOrigin {
    /// Canonical lowercase label. Used by both the human-facing `--help`
    /// blurb (`xdg: /path/to/ext`) and the machine-readable `qli ext list` /
    /// `qli ext which` output, so the two stay in sync.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            ExtensionOrigin::Xdg => "xdg",
            ExtensionOrigin::Embedded => "embedded",
            ExtensionOrigin::Path => "path",
        }
    }
}

/// Walk every `(root, origin)` source in priority order, then merge
/// matching `qli-<group>-<name>` binaries from `PATH`, returning every
/// group + extension we can dispatch.
///
/// Sources earlier in the slice win. A group claimed by source N is
/// silently shadowed in sources N+1, N+2, ... — including its
/// extensions, not just its manifest. This keeps "user owns it now"
/// semantics: if XDG defines `dev` (even if `dev/hello` is missing), the
/// embedded `dev/hello` does not bleed through.
///
/// Missing roots are not an error — they're skipped. Bad manifests,
/// non-executable files, reserved group names, malformed PATH binary
/// names, and unknown-group PATH binaries each produce a warning and are
/// skipped.
pub fn discover(sources: &[(&Path, ExtensionOrigin)]) -> Discovery {
    let mut warnings = Vec::new();
    let mut groups: BTreeMap<String, Group> = BTreeMap::new();
    for (root, origin) in sources {
        scan_root(root, *origin, &mut groups, &mut warnings);
    }
    merge_path_binaries(&mut groups, &mut warnings);
    Discovery { groups, warnings }
}

/// Walk one source and add its groups to `groups`. Existing entries are
/// not overwritten — earlier sources win.
fn scan_root(
    root: &Path,
    origin: ExtensionOrigin,
    groups: &mut BTreeMap<String, Group>,
    warnings: &mut Vec<String>,
) {
    let entries = match fs::read_dir(root) {
        Ok(it) => it,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return,
        Err(err) => {
            warnings.push(format!(
                "could not read extensions root {}: {err}",
                root.display(),
            ));
            return;
        }
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let Some(name) = path.file_name().and_then(|s| s.to_str()).map(str::to_owned) else {
            warnings.push(format!(
                "skipping group with non-UTF-8 directory name at {}",
                path.display(),
            ));
            continue;
        };
        if RESERVED_GROUP_NAMES.contains(&name.as_str()) {
            warnings.push(format!(
                "group `{name}` at {} shadows a built-in qli subcommand; skipping",
                path.display(),
            ));
            continue;
        }
        if groups.contains_key(&name) {
            // Earlier source already claimed this group; later sources
            // do not contribute, even per-extension.
            continue;
        }
        let manifest_path = path.join("_manifest.toml");
        let Some(manifest) = load_manifest(&manifest_path, warnings) else {
            continue;
        };
        let extensions = scan_group_executables(&path, &name, origin, warnings);
        groups.insert(
            name.clone(),
            Group {
                name,
                manifest,
                manifest_path,
                extensions,
            },
        );
    }
}

fn load_manifest(manifest_path: &Path, warnings: &mut Vec<String>) -> Option<Manifest> {
    let bytes = match fs::read_to_string(manifest_path) {
        Ok(b) => b,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return None,
        Err(err) => {
            warnings.push(format!(
                "could not read manifest {}: {err}",
                manifest_path.display(),
            ));
            return None;
        }
    };
    match Manifest::from_str(&bytes) {
        Ok(m) => Some(m),
        Err(err) => {
            warnings.push(format!(
                "skipping group at {}: {err}",
                manifest_path.display(),
            ));
            None
        }
    }
}

fn scan_group_executables(
    group_dir: &Path,
    group_name: &str,
    origin: ExtensionOrigin,
    warnings: &mut Vec<String>,
) -> BTreeMap<String, Extension> {
    let mut extensions = BTreeMap::new();
    let entries = match fs::read_dir(group_dir) {
        Ok(it) => it,
        Err(err) => {
            warnings.push(format!(
                "could not list group {} at {}: {err}",
                group_name,
                group_dir.display(),
            ));
            return extensions;
        }
    };

    for entry in entries.flatten() {
        let path = entry.path();
        let Some(file_name) = path.file_name().and_then(|s| s.to_str()) else {
            warnings.push(format!(
                "skipping non-UTF-8 file under {}",
                group_dir.display(),
            ));
            continue;
        };
        if file_name.starts_with('_') {
            continue;
        }
        let Ok(meta) = fs::metadata(&path) else {
            continue;
        };
        if !meta.is_file() {
            continue;
        }
        if !is_executable(&meta) {
            warnings.push(format!(
                "skipping non-executable file {}; chmod +x to enable",
                path.display(),
            ));
            continue;
        }
        let name = file_name.to_owned();
        extensions.insert(
            name.clone(),
            Extension {
                name,
                group: group_name.to_owned(),
                path,
                origin,
            },
        );
    }
    extensions
}

fn merge_path_binaries(groups: &mut BTreeMap<String, Group>, warnings: &mut Vec<String>) {
    for (group_name, ext_name, path) in scan_path_for_qli_binaries(warnings) {
        if RESERVED_GROUP_NAMES.contains(&group_name.as_str()) {
            warnings.push(format!(
                "PATH binary `qli-{group_name}-{ext_name}` ({}) uses reserved group name `{group_name}`; skipping",
                path.display(),
            ));
            continue;
        }
        let Some(group) = groups.get_mut(&group_name) else {
            warnings.push(format!(
                "PATH binary `qli-{group_name}-{ext_name}` references unknown group `{group_name}`; create extensions/{group_name}/_manifest.toml to enable it",
            ));
            continue;
        };
        if let Some(existing) = group.extensions.get(&ext_name) {
            warnings.push(format!(
                "extension `{group_name} {ext_name}` exists in both XDG ({}) and PATH ({}); using XDG. Use `qli ext which` to inspect.",
                existing.path.display(),
                path.display(),
            ));
            continue;
        }
        group.extensions.insert(
            ext_name.clone(),
            Extension {
                name: ext_name,
                group: group_name,
                path,
                origin: ExtensionOrigin::Path,
            },
        );
    }
}

/// Walk every directory in `PATH`, return `(group, extension, path)` tuples
/// for every regular, executable file whose basename matches
/// `qli-<group>-<extension>`. Both halves must be non-empty; extra dashes in
/// the extension name are kept verbatim (`qli-dev-hello-world` → group
/// `dev`, ext `hello-world`).
fn scan_path_for_qli_binaries(warnings: &mut Vec<String>) -> Vec<(String, String, PathBuf)> {
    let Some(path_var) = std::env::var_os("PATH") else {
        return Vec::new();
    };
    let mut found = Vec::new();
    let mut seen: BTreeSet<(String, String)> = BTreeSet::new();
    for dir in std::env::split_paths(&path_var) {
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Some(file_name) = path.file_name().and_then(|s| s.to_str()) else {
                continue;
            };
            let Some(rest) = file_name.strip_prefix("qli-") else {
                continue;
            };
            let Some((group, ext)) = rest.split_once('-') else {
                warnings.push(format!(
                    "PATH binary `{file_name}` ({}) is missing a group/extension separator; expected `qli-<group>-<name>`",
                    path.display(),
                ));
                continue;
            };
            if group.is_empty() || ext.is_empty() {
                warnings.push(format!(
                    "PATH binary `{file_name}` ({}) has an empty group or extension name; expected `qli-<group>-<name>`",
                    path.display(),
                ));
                continue;
            }
            let Ok(meta) = fs::metadata(&path) else {
                continue;
            };
            if !meta.is_file() || !is_executable(&meta) {
                continue;
            }
            // First occurrence on PATH wins (matches shell behaviour).
            if seen.insert((group.to_owned(), ext.to_owned())) {
                found.push((group.to_owned(), ext.to_owned(), path));
            }
        }
    }
    found
}

#[cfg(unix)]
fn is_executable(meta: &fs::Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt;
    meta.permissions().mode() & 0o111 != 0
}

#[cfg(not(unix))]
fn is_executable(_meta: &fs::Metadata) -> bool {
    // Non-Unix executable detection (PATHEXT, etc.) is deferred until a
    // Windows port is in scope. Treat any regular file as executable so
    // discovery doesn't silently swallow scripts on those platforms.
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::{self, File};
    use std::io::Write;

    #[cfg(unix)]
    fn chmod_exec(path: &Path) {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(path).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(path, perms).unwrap();
    }

    fn write(path: &Path, body: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        let mut f = File::create(path).unwrap();
        f.write_all(body.as_bytes()).unwrap();
    }

    fn write_manifest(group_dir: &Path, description: &str) {
        write(
            &group_dir.join("_manifest.toml"),
            &format!("schema_version = 1\ndescription = \"{description}\"\n"),
        );
    }

    fn xdg(root: &Path) -> [(&Path, ExtensionOrigin); 1] {
        [(root, ExtensionOrigin::Xdg)]
    }

    #[test]
    fn missing_root_is_empty() {
        let tmp = tempfile::tempdir().unwrap();
        let missing = tmp.path().join("does-not-exist");
        let d = discover(&xdg(&missing));
        assert!(d.groups.is_empty());
        assert!(d.warnings.is_empty());
    }

    #[test]
    #[cfg(unix)]
    fn discovers_group_and_executable() {
        let tmp = tempfile::tempdir().unwrap();
        let group_dir = tmp.path().join("dev");
        write_manifest(&group_dir, "Dev tools");
        let script = group_dir.join("hello");
        write(&script, "#!/bin/sh\necho hi\n");
        chmod_exec(&script);

        let d = discover(&xdg(tmp.path()));
        let group = d.groups.get("dev").expect("dev group present");
        assert_eq!(group.manifest.description, "Dev tools");
        let ext = group.extensions.get("hello").expect("hello extension");
        assert_eq!(ext.path, script);
        assert_eq!(ext.origin, ExtensionOrigin::Xdg);
        assert!(d.warnings.is_empty(), "warnings: {:?}", d.warnings);
    }

    #[test]
    #[cfg(unix)]
    fn skips_files_starting_with_underscore() {
        let tmp = tempfile::tempdir().unwrap();
        let group_dir = tmp.path().join("dev");
        write_manifest(&group_dir, "Dev tools");
        let script = group_dir.join("_helper");
        write(&script, "#!/bin/sh\n");
        chmod_exec(&script);

        let d = discover(&xdg(tmp.path()));
        let group = d.groups.get("dev").unwrap();
        assert!(group.extensions.is_empty());
        assert!(d.warnings.is_empty());
    }

    #[test]
    #[cfg(unix)]
    fn warns_on_non_executable() {
        let tmp = tempfile::tempdir().unwrap();
        let group_dir = tmp.path().join("dev");
        write_manifest(&group_dir, "Dev tools");
        write(&group_dir.join("hello"), "#!/bin/sh\n");

        let d = discover(&xdg(tmp.path()));
        let group = d.groups.get("dev").unwrap();
        assert!(group.extensions.is_empty());
        assert_eq!(d.warnings.len(), 1, "warnings: {:?}", d.warnings);
        assert!(d.warnings[0].contains("non-executable"));
        assert!(d.warnings[0].contains("hello"));
    }

    #[test]
    fn warns_and_skips_malformed_manifest() {
        let tmp = tempfile::tempdir().unwrap();
        let group_dir = tmp.path().join("dev");
        write(&group_dir.join("_manifest.toml"), "schema_version = 99\n");

        let d = discover(&xdg(tmp.path()));
        assert!(d.groups.is_empty());
        assert_eq!(d.warnings.len(), 1, "warnings: {:?}", d.warnings);
        assert!(d.warnings[0].contains("schema_version"));
    }

    #[test]
    fn skips_subdir_without_manifest() {
        let tmp = tempfile::tempdir().unwrap();
        fs::create_dir_all(tmp.path().join("dev")).unwrap();
        let d = discover(&xdg(tmp.path()));
        assert!(d.groups.is_empty());
        assert!(d.warnings.is_empty());
    }

    #[test]
    fn warns_on_reserved_group_name() {
        let tmp = tempfile::tempdir().unwrap();
        let group_dir = tmp.path().join("completions");
        write_manifest(&group_dir, "Should be skipped");

        let d = discover(&xdg(tmp.path()));
        assert!(d.groups.is_empty());
        assert_eq!(d.warnings.len(), 1, "warnings: {:?}", d.warnings);
        assert!(d.warnings[0].contains("completions"));
        assert!(d.warnings[0].contains("built-in"));
    }

    #[test]
    #[cfg(unix)]
    fn embedded_visible_when_xdg_missing_group() {
        // No XDG root has `dev`; embedded does. Embedded fills in.
        let tmp = tempfile::tempdir().unwrap();
        let xdg_root = tmp.path().join("xdg");
        let embedded_root = tmp.path().join("embedded");
        let group_dir = embedded_root.join("dev");
        write_manifest(&group_dir, "Embedded dev");
        let script = group_dir.join("hello");
        write(&script, "#!/bin/sh\necho embedded\n");
        chmod_exec(&script);

        let sources: &[(&Path, ExtensionOrigin)] = &[
            (xdg_root.as_path(), ExtensionOrigin::Xdg),
            (embedded_root.as_path(), ExtensionOrigin::Embedded),
        ];
        let d = discover(sources);
        let group = d.groups.get("dev").expect("dev group should be visible");
        let ext = group.extensions.get("hello").unwrap();
        assert_eq!(ext.origin, ExtensionOrigin::Embedded);
        assert_eq!(ext.path, script);
    }

    #[test]
    #[cfg(unix)]
    fn xdg_shadows_embedded_per_group() {
        // Both sources define `dev`; XDG must win wholesale, including
        // its (potentially different) extensions list.
        let tmp = tempfile::tempdir().unwrap();
        let xdg_root = tmp.path().join("xdg");
        let embedded_root = tmp.path().join("embedded");

        let xdg_dev = xdg_root.join("dev");
        write_manifest(&xdg_dev, "User-edited dev");
        let xdg_script = xdg_dev.join("hello");
        write(&xdg_script, "#!/bin/sh\necho user\n");
        chmod_exec(&xdg_script);

        let embedded_dev = embedded_root.join("dev");
        write_manifest(&embedded_dev, "Embedded dev");
        let embedded_script = embedded_dev.join("hello");
        write(&embedded_script, "#!/bin/sh\necho embedded\n");
        chmod_exec(&embedded_script);
        // Add a second extension in embedded that XDG doesn't have. The
        // shadowing rule means it must NOT bleed through.
        let embedded_extra = embedded_dev.join("only-embedded");
        write(&embedded_extra, "#!/bin/sh\necho only-embedded\n");
        chmod_exec(&embedded_extra);

        let sources: &[(&Path, ExtensionOrigin)] = &[
            (xdg_root.as_path(), ExtensionOrigin::Xdg),
            (embedded_root.as_path(), ExtensionOrigin::Embedded),
        ];
        let d = discover(sources);
        let group = d.groups.get("dev").unwrap();
        assert_eq!(group.manifest.description, "User-edited dev");
        let ext = group.extensions.get("hello").unwrap();
        assert_eq!(ext.origin, ExtensionOrigin::Xdg);
        assert_eq!(ext.path, xdg_script);
        assert!(
            !group.extensions.contains_key("only-embedded"),
            "embedded extras must not bleed into a group XDG owns",
        );
    }

    #[test]
    #[cfg(unix)]
    fn distinct_groups_layer_across_sources() {
        // XDG defines `dev`; embedded defines `prod`. Both should appear.
        let tmp = tempfile::tempdir().unwrap();
        let xdg_root = tmp.path().join("xdg");
        let embedded_root = tmp.path().join("embedded");

        let xdg_dev = xdg_root.join("dev");
        write_manifest(&xdg_dev, "Dev");
        let xdg_script = xdg_dev.join("hello");
        write(&xdg_script, "#!/bin/sh\n");
        chmod_exec(&xdg_script);

        let emb_prod = embedded_root.join("prod");
        write_manifest(&emb_prod, "Prod");
        let emb_script = emb_prod.join("hello");
        write(&emb_script, "#!/bin/sh\n");
        chmod_exec(&emb_script);

        let sources: &[(&Path, ExtensionOrigin)] = &[
            (xdg_root.as_path(), ExtensionOrigin::Xdg),
            (embedded_root.as_path(), ExtensionOrigin::Embedded),
        ];
        let d = discover(sources);
        assert_eq!(
            d.groups.get("dev").unwrap().extensions["hello"].origin,
            ExtensionOrigin::Xdg
        );
        assert_eq!(
            d.groups.get("prod").unwrap().extensions["hello"].origin,
            ExtensionOrigin::Embedded
        );
    }
}
