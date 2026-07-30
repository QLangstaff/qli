mod cli;
mod exit;
mod ext;
mod logging;
mod panic;
mod paths;
mod self_update;
mod signal;

use std::collections::HashMap;
use std::ffi::OsString;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;

use anyhow::Context;
use clap::{ArgMatches, ColorChoice, Command, CommandFactory};
use qli_ext::{tty_confirm, DispatchOptions, DispatchSignals, ExtensionOrigin, ProductionResolver};

fn main() -> ExitCode {
    // Suppress Rust's default panic UI in favour of a "this is a bug, please
    // report" message. Must run before any code that could panic (which is
    // all of it).
    panic::install();

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
    let discovery = qli_ext::discover(&sources);

    // Print warnings before `get_matches` so they fire even when clap exits
    // early (`--help`, `--version`, parse errors). The plan requires
    // discovery warnings on stderr at startup, not gated on the subcommand.
    for warning in &discovery.warnings {
        eprintln!("warning: {warning}");
    }

    let mut root = cli::Cli::command();
    for group in discovery.groups.values() {
        root = root.subcommand(ext::build_group_command(group));
    }

    // Clone so we still have a usable `root` for `print_help` / completions
    // generation after `get_matches` consumes it.
    let matches = root.clone().get_matches();

    let verbose: u8 = matches.get_count("verbose");
    let quiet: bool = matches.get_flag("quiet");
    let color: ColorChoice = matches
        .get_one::<ColorChoice>("color")
        .copied()
        .unwrap_or(ColorChoice::Auto);
    let assume_yes: bool = matches.get_flag("yes");

    cli::apply_color_choice(color);
    logging::init(verbose, quiet);

    paths::ensure_all();
    let signals = signal::install();

    match dispatch(root, &discovery, &matches, assume_yes, signals) {
        Ok(code) => ExitCode::from(code),
        Err(err) => {
            eprintln!("error: {err:#}");
            ExitCode::from(exit::ERROR)
        }
    }
}

fn dispatch(
    mut root: Command,
    discovery: &qli_ext::Discovery,
    matches: &ArgMatches,
    assume_yes: bool,
    signals: Arc<DispatchSignals>,
) -> anyhow::Result<u8> {
    use clap_complete::Shell;

    match matches.subcommand() {
        None => {
            root.print_help().context("could not print help")?;
            Ok(exit::SUCCESS)
        }
        Some(("completions", sub)) => {
            let shell: Shell = *sub.get_one::<Shell>("shell").expect("required by clap");
            let bin_name = root.get_name().to_string();
            clap_complete::generate(shell, &mut root, bin_name, &mut std::io::stdout());
            Ok(exit::SUCCESS)
        }
        Some(("ext", sub)) => dispatch_ext(sub, discovery),
        Some(("self-update", sub)) => dispatch_self_update(sub),
        Some((group_name, group_matches)) => {
            dispatch_group(discovery, group_name, group_matches, assume_yes, signals)
        }
    }
}

fn dispatch_ext(matches: &ArgMatches, discovery: &qli_ext::Discovery) -> anyhow::Result<u8> {
    let (action, sub) = matches
        .subcommand()
        .context("missing `qli ext` subcommand; try `qli ext list`, `qli ext which`, or `qli ext install-defaults`")?;
    match action {
        "list" => dispatch_ext_list(sub, discovery),
        "which" => dispatch_ext_which(sub, discovery),
        "install-defaults" => dispatch_ext_install_defaults(sub),
        other => {
            anyhow::bail!("unknown `qli ext` action: `{other}`");
        }
    }
}

fn dispatch_ext_list(matches: &ArgMatches, discovery: &qli_ext::Discovery) -> anyhow::Result<u8> {
    use std::io::Write;
    let json = matches.get_flag("json");
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    if json {
        let items: Vec<_> = discovery
            .groups
            .values()
            .flat_map(|group| {
                group.extensions.values().map(|ext| {
                    serde_json::json!({
                        "group": group.name,
                        "extension": ext.name,
                        "origin": ext.origin.as_str(),
                        "path": ext.path.display().to_string(),
                    })
                })
            })
            .collect();
        // Pretty-print so the output is readable when the user runs the
        // command interactively. `jq -c .` re-collapses for piping.
        serde_json::to_writer_pretty(&mut out, &items).context("failed to write JSON output")?;
        writeln!(out).context("failed to write trailing newline")?;
    } else {
        // Tab-separated; preserves spaces in paths (callers can `column -t`
        // for visual alignment).
        for group in discovery.groups.values() {
            for ext in group.extensions.values() {
                writeln!(
                    out,
                    "{}\t{}\t{}\t{}",
                    group.name,
                    ext.name,
                    ext.origin.as_str(),
                    ext.path.display(),
                )
                .context("failed to write list row")?;
            }
        }
    }
    Ok(exit::SUCCESS)
}

fn dispatch_ext_which(matches: &ArgMatches, discovery: &qli_ext::Discovery) -> anyhow::Result<u8> {
    use std::io::Write;
    let group_name: &String = matches
        .get_one::<String>("group")
        .context("internal: clap should require `group`")?;
    let ext_name: &String = matches
        .get_one::<String>("name")
        .context("internal: clap should require `name`")?;
    let json = matches.get_flag("json");
    let group = discovery.groups.get(group_name).with_context(|| {
        format!("unknown group `{group_name}`; run `qli ext list` to see what's available")
    })?;
    let ext = group.extensions.get(ext_name).with_context(|| {
        format!(
            "unknown extension `{group_name} {ext_name}`; \
             run `qli ext list` to see what's available"
        )
    })?;
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    if json {
        let payload = serde_json::json!({
            "group": group.name,
            "extension": ext.name,
            "origin": ext.origin.as_str(),
            "path": ext.path.display().to_string(),
        });
        serde_json::to_writer_pretty(&mut out, &payload).context("failed to write JSON output")?;
        writeln!(out).context("failed to write trailing newline")?;
    } else {
        // Plain output is just the path — Unix `which` semantics, easy to
        // pipe into `cat`, `bat`, an editor, etc.
        writeln!(out, "{}", ext.path.display()).context("failed to write path")?;
    }
    Ok(exit::SUCCESS)
}

fn dispatch_ext_install_defaults(matches: &ArgMatches) -> anyhow::Result<u8> {
    let force = matches.get_flag("force");
    let target = paths::data_dir()
        .context("could not resolve XDG data dir for install-defaults")?
        .join("extensions");
    std::fs::create_dir_all(&target)
        .with_context(|| format!("could not create extensions dir at {}", target.display()))?;
    let stats = qli_ext::materialize_to(&target, force)
        .with_context(|| format!("failed to install defaults to {}", target.display()))?;
    eprintln!(
        "installed defaults to {}: wrote {}, skipped {} (use --force to overwrite)",
        target.display(),
        stats.written,
        stats.skipped,
    );
    Ok(exit::SUCCESS)
}

fn dispatch_self_update(matches: &ArgMatches) -> anyhow::Result<u8> {
    use self_update::{Detection, InstallMethod};

    let json = matches.get_flag("json");
    let detection = self_update::detect();
    let Detection {
        method,
        binary_path,
    } = &detection;

    // Active update for the curl-installed path; passive guidance for the
    // others. The plan separates "perform" from "hint" deliberately — package
    // managers (cargo, brew) own their own upgrade ceremony; tearing those
    // down from inside qli would surprise users on a tool that's meant to
    // respect the host package manager.
    let status = match method {
        InstallMethod::Curl if !cfg!(target_os = "windows") => {
            match self_update::run_curl_installer() {
                Ok(()) => SelfUpdateStatus::Performed,
                Err(err) => {
                    eprintln!("error: automated upgrade failed: {err:#}");
                    SelfUpdateStatus::Failed
                }
            }
        }
        InstallMethod::Unknown => SelfUpdateStatus::Failed,
        _ => SelfUpdateStatus::Hinted,
    };

    let upgrade_command = method.upgrade_command();

    if json {
        use std::io::Write;
        let payload = serde_json::json!({
            "install_method": method.as_str(),
            "binary_path": binary_path.to_string_lossy(),
            "upgrade_command": upgrade_command,
            "status": status.as_str(),
            "plugin_note": self_update::PLUGIN_NOTE,
        });
        let stderr = std::io::stderr();
        let mut out = stderr.lock();
        serde_json::to_writer_pretty(&mut out, &payload).context("failed to write JSON output")?;
        writeln!(out).context("failed to write trailing newline")?;
    } else {
        match (method, status) {
            (InstallMethod::Curl, SelfUpdateStatus::Performed) => {
                eprintln!("qli upgraded via {}.", self_update::INSTALLER_URL);
            }
            (InstallMethod::Unknown, _) => {
                eprintln!(
                    "could not detect how qli was installed (binary: {}).",
                    binary_path.display()
                );
                eprintln!("If you used:");
                eprintln!("  cargo install qli  →  cargo install qli --force");
                eprintln!("  brew install QLangstaff/qli/qli  →  brew upgrade qli");
                eprintln!(
                    "  the curl installer  →  curl -LsSf {} | sh",
                    self_update::INSTALLER_URL
                );
            }
            (_, _) => {
                let command = upgrade_command
                    .expect("non-Unknown InstallMethod always has an upgrade command");
                eprintln!("Upgrade qli with: {command}");
            }
        }
        eprintln!();
        eprintln!("{}", self_update::PLUGIN_NOTE);
    }

    Ok(match status {
        SelfUpdateStatus::Performed | SelfUpdateStatus::Hinted => exit::SUCCESS,
        SelfUpdateStatus::Failed => exit::ERROR,
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SelfUpdateStatus {
    /// Active upgrade (curl path) completed.
    Performed,
    /// Detection succeeded; printed the upgrade command for a passive path.
    Hinted,
    /// Detection succeeded but the active upgrade attempt errored, or the
    /// install method could not be detected.
    Failed,
}

impl SelfUpdateStatus {
    fn as_str(self) -> &'static str {
        match self {
            Self::Performed => "performed",
            Self::Hinted => "hinted",
            Self::Failed => "failed",
        }
    }
}

fn dispatch_group(
    discovery: &qli_ext::Discovery,
    group_name: &str,
    group_matches: &ArgMatches,
    assume_yes: bool,
    signals: Arc<DispatchSignals>,
) -> anyhow::Result<u8> {
    let group = discovery
        .groups
        .get(group_name)
        .with_context(|| format!("unknown subcommand `{group_name}`"))?;
    let (ext_name, ext_matches) = group_matches
        .subcommand()
        .with_context(|| format!("no extension specified for group `{group_name}`"))?;
    let extension = group
        .extensions
        .get(ext_name)
        .with_context(|| format!("unknown extension `{group_name} {ext_name}`"))?;
    let args: Vec<&OsString> = ext_matches
        .get_many::<OsString>("args")
        .map(Iterator::collect)
        .unwrap_or_default();

    let resolver = ProductionResolver::new();
    let confirm = tty_confirm();
    let opts = DispatchOptions {
        assume_yes,
        resolver: &resolver,
        confirm: &confirm,
        signals,
        audit_path_defaults: audit_path_defaults(),
    };

    let code = qli_ext::dispatch::run(group, extension, args, &opts)
        .with_context(|| format!("failed to run `{group_name} {ext_name}`"))?;
    // Map any exit code that fits in u8 (0..=255) to ExitCode; otherwise
    // collapse to our generic ERROR. Unix exit codes are normalized to
    // 0..=255 by the kernel, so the fallback is rare.
    Ok(u8::try_from(code).unwrap_or(exit::ERROR))
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

/// Resolved fallbacks for the env vars `audit_log` literals are most likely
/// to reference. Lets a manifest written `$XDG_STATE_HOME/qli/...` work even
/// when the user hasn't exported `XDG_STATE_HOME`.
fn audit_path_defaults() -> HashMap<String, String> {
    let mut defaults = HashMap::new();
    let pairs = [
        ("XDG_STATE_HOME", paths::state_dir().ok()),
        ("XDG_DATA_HOME", paths::data_dir().ok()),
        ("XDG_CACHE_HOME", paths::cache_dir().ok()),
        ("XDG_CONFIG_HOME", paths::config_dir().ok()),
    ];
    for (key, value) in pairs {
        if let Some(path) = value {
            // The XDG values stored under each key should be the BASE dir
            // (e.g. `~/.local/state`), not the qli-scoped subdir. Strip the
            // trailing `qli` segment our `paths` helpers add.
            let base = path
                .parent()
                .map(std::path::Path::to_path_buf)
                .unwrap_or(path);
            if let Some(s) = base.to_str() {
                defaults.insert(key.to_owned(), s.to_owned());
            }
        }
    }
    defaults
}
