//! Glue between [`qli_ext`] discovery and the dynamic clap command tree.
//!
//! Each discovered group becomes a clap subcommand whose subcommands are the
//! group's extensions. Extensions disable clap's `--help` / `--version` so
//! those flags reach the script unchanged, and use a `trailing_var_arg`
//! positional to forward every remaining argument verbatim (`OsString`,
//! since Unix args may not be UTF-8).

use std::ffi::OsString;

use clap::{Arg, Command};
use qli_ext::{Extension, ExtensionOrigin, Group};

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
    let origin = match ext.origin {
        ExtensionOrigin::Xdg => "XDG",
        ExtensionOrigin::Path => "PATH",
    };
    format!("{origin}: {}", ext.path.display())
}
