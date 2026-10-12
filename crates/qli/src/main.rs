mod analysis_cache;
mod analyze;
mod cli;
mod exit;
mod ext;
mod index;
mod logging;
mod lsp;
mod mcp;
mod panic;
mod paths;
mod self_update;
mod signal;

use std::collections::HashMap;
use std::ffi::OsString;
use std::process::ExitCode;
use std::sync::Arc;

use anyhow::Context;
use clap::{ArgMatches, ColorChoice, Command, CommandFactory, Parser};
use qli_ext::{tty_confirm, DispatchOptions, DispatchSignals, ProductionResolver};

fn main() -> ExitCode {
    // Suppress Rust's default panic UI in favour of a "this is a bug, please
    // report" message. Must run before any code that could panic (which is
    // all of it).
    panic::install();

    // `qli lsp` owns its output streams — stdout is LSP frames in stdio mode,
    // stderr must stay silent in tcp mode — so it is routed here, before
    // anything below can print. Extension discovery, embedded-defaults
    // materialization, and the dispatcher's signal handler (which would
    // swallow an editor's SIGTERM) are all skipped: the server needs none of
    // them. The static derive tree is enough to recognise the subcommand; a
    // command line that needs a discovered group, `--help`, or a parse error
    // fails this probe silently and takes the normal path.
    if let Ok(cli::Cli {
        verbose,
        quiet,
        color,
        command: Some(cli::Command::Lsp { tcp, .. }),
        ..
    }) = cli::Cli::try_parse()
    {
        cli::apply_color_choice(color);
        return exit_code(lsp::run(tcp, verbose, quiet));
    }

    let discovery = ext::discover();

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

    exit_code(dispatch(root, &discovery, &matches, assume_yes))
}

/// Map a command's outcome to the process exit code, printing a failure as
/// `error: …` on stderr.
fn exit_code(result: anyhow::Result<u8>) -> ExitCode {
    match result {
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
) -> anyhow::Result<u8> {
    use clap_complete::Shell;

    match matches.subcommand() {
        None => {
            root.print_help().context("could not print help")?;
            Ok(exit::SUCCESS)
        }
        Some(("analyze", sub)) => analyze::run(sub),
        Some(("completions", sub)) => {
            let shell: Shell = *sub.get_one::<Shell>("shell").expect("required by clap");
            let bin_name = root.get_name().to_string();
            clap_complete::generate(shell, &mut root, bin_name, &mut std::io::stdout());
            Ok(exit::SUCCESS)
        }
        Some(("ext", sub)) => dispatch_ext(sub, discovery),
        Some(("index", sub)) => index::run(sub),
        Some(("mcp", _)) => mcp::run(),
        Some(("self-update", sub)) => dispatch_self_update(sub),
        Some((group_name, group_matches)) => {
            // Installed only here: the handler exists to forward Ctrl+C to a
            // running extension. A built-in command has no child to forward
            // to, so it keeps the default disposition and Ctrl+C ends it.
            let signals = signal::install();
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
        "cache" => dispatch_ext_cache(sub),
        other => {
            anyhow::bail!("unknown `qli ext` action: `{other}`");
        }
    }
}

fn dispatch_ext_cache(matches: &ArgMatches) -> anyhow::Result<u8> {
    let (action, sub) = matches
        .subcommand()
        .context("missing `qli ext cache` subcommand; try `qli ext cache clean`")?;
    match action {
        "clean" => analyze::cache_clean(sub),
        other => anyhow::bail!("unknown `qli ext cache` action: `{other}`"),
    }
}

fn dispatch_ext_list(matches: &ArgMatches, discovery: &qli_ext::Discovery) -> anyhow::Result<u8> {
    use std::io::Write;
    let json = matches.get_flag("json");
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    let records = ext::records(discovery);
    if json {
        // Pretty-print so the output is readable when the user runs the
        // command interactively. `jq -c .` re-collapses for piping.
        serde_json::to_writer_pretty(&mut out, &records).context("failed to write JSON output")?;
        writeln!(out).context("failed to write trailing newline")?;
    } else {
        // Tab-separated; preserves spaces in paths (callers can `column -t`
        // for visual alignment).
        for record in records {
            writeln!(
                out,
                "{}\t{}\t{}\t{}",
                record.group, record.extension, record.origin, record.path,
            )
            .context("failed to write list row")?;
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
        let payload = ext::ExtensionRecord::new(group, ext);
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
