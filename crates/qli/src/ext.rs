//! Glue between [`qli_ext`] discovery and the dynamic clap command tree.
//!
//! Each discovered group becomes a clap subcommand whose subcommands are the
//! group's extensions. Extensions disable clap's `--help` / `--version` so
//! those flags reach the script unchanged, and use a `trailing_var_arg`
//! positional to forward every remaining argument verbatim (`OsString`,
//! since Unix args may not be UTF-8).

use std::ffi::OsString;
use std::path::PathBuf;

use clap::{Arg, Command};
use qli_ext::{Discovery, Extension, ExtensionOrigin, Group};
use rmcp::schemars;
use serde::Serialize;

use crate::paths;

/// Discover every extension: the user's XDG-installed ones, plus the binary's
/// embedded defaults.
///
/// Never fails — what could not be resolved or read is disabled, with a
/// `warning:` on stderr for a directory that cannot be resolved and an entry in
/// [`Discovery::warnings`] for an extension that cannot be used.
pub fn discover() -> Discovery {
    // If we can't resolve the XDG data dir, dispatch is dead in the water —
    // tell the user loudly and proceed with an empty discovery so the
    // built-in subcommands (`--version`, `completions`) still work.
    let extensions_root = match paths::data_dir() {
        Ok(d) => d.join("extensions"),
        Err(err) => {
            eprintln!(
                "warning: could not resolve XDG data dir ({err:#}); extensions are disabled. \
                 Set $XDG_DATA_HOME or $HOME and retry."
            );
            PathBuf::new()
        }
    };

    // Embedded defaults: materialize to a version-keyed cache so they're
    // dispatchable even before the user runs `qli ext install-defaults`.
    // Best-effort — a failure here disables the embedded layer but does
    // not prevent XDG-installed extensions from running.
    let embedded_root = materialize_embedded_layer();

    let mut sources: Vec<(&std::path::Path, ExtensionOrigin)> = Vec::with_capacity(2);
    sources.push((extensions_root.as_path(), ExtensionOrigin::Xdg));
    if let Some(root) = embedded_root.as_deref() {
        sources.push((root, ExtensionOrigin::Embedded));
    }
    qli_ext::discover(&sources)
}

/// Materialize the binary's embedded extension defaults to a version-keyed
/// cache root and return that root.
///
/// Returns `None` if the cache dir can't be resolved (e.g. no `HOME`) or
/// if extraction fails. Both conditions print a warning and disable the
/// embedded layer for this run; XDG-installed extensions and built-in
/// subcommands keep working.
fn materialize_embedded_layer() -> Option<PathBuf> {
    let cache = match paths::cache_dir() {
        Ok(c) => c,
        Err(err) => {
            eprintln!(
                "warning: could not resolve XDG cache dir ({err:#}); embedded \
                 defaults are disabled. XDG extensions still work."
            );
            return None;
        }
    };
    let root = cache.join("embedded").join(env!("CARGO_PKG_VERSION"));
    if let Err(err) = std::fs::create_dir_all(&root) {
        eprintln!(
            "warning: could not create embedded cache at {} ({err}); \
             embedded defaults are disabled.",
            root.display(),
        );
        return None;
    }
    if let Err(err) = qli_ext::materialize_to(&root, false) {
        eprintln!(
            "warning: could not extract embedded defaults to {} ({err}); \
             embedded defaults are disabled.",
            root.display(),
        );
        return None;
    }
    Some(root)
}

/// One discovered extension, as every frontend reports it (`qli ext list`,
/// `qli ext which`, the MCP `qli_ext_list` tool).
//
// Fields are declared in alphabetical order: that is the key order of the JSON
// this serializes to, which `--json` consumers already depend on.
#[derive(Clone, Debug, Serialize, schemars::JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub struct ExtensionRecord {
    /// Extension name within its group; `qli <group> <extension>` runs it.
    pub extension: String,
    /// Group the extension belongs to (for example `dev` or `prod`).
    pub group: String,
    /// Where it was discovered: `xdg` (installed by the user), `embedded`
    /// (shipped in the binary), or `path` (a `qli-<group>-<name>` binary).
    pub origin: String,
    /// Resolved path of the file that runs.
    pub path: String,
}

impl ExtensionRecord {
    pub fn new(group: &Group, extension: &Extension) -> Self {
        Self {
            extension: extension.name.clone(),
            group: group.name.clone(),
            origin: extension.origin.as_str().to_owned(),
            path: extension.path.display().to_string(),
        }
    }
}

/// Every extension in `discovery`, in listing order: by group, then by name.
pub fn records(discovery: &Discovery) -> Vec<ExtensionRecord> {
    discovery
        .groups
        .values()
        .flat_map(|group| {
            group
                .extensions
                .values()
                .map(|extension| ExtensionRecord::new(group, extension))
        })
        .collect()
}

pub fn build_group_command(group: &Group) -> Command {
    let mut cmd = Command::new(leak_str(&group.name))
        .about(group.manifest.description.clone())
        .arg_required_else_help(true);
    for ext in group.extensions.values() {
        cmd = cmd.subcommand(build_extension_command(ext));
    }
    cmd
}

fn build_extension_command(ext: &Extension) -> Command {
    Command::new(leak_str(&ext.name))
        .about(describe(ext))
        .disable_help_flag(true)
        .disable_version_flag(true)
        .arg(
            Arg::new("args")
                .num_args(0..)
                .trailing_var_arg(true)
                .allow_hyphen_values(true)
                .value_parser(clap::value_parser!(OsString))
                .help("Arguments forwarded to the extension"),
        )
}

// clap's `Command::new` / `Arg::new` only accept `&'static str` for names.
// Discovery produces names at runtime, so we leak each one once at startup.
// The leak is bounded by the number of groups + extensions and lives until
// the process exits anyway.
fn leak_str(s: &str) -> &'static str {
    Box::leak(s.to_owned().into_boxed_str())
}

fn describe(ext: &Extension) -> String {
    format!("{}: {}", ext.origin.as_str(), ext.path.display())
}
