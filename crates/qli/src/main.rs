mod cli;
mod exit;
mod ext;
mod logging;
mod paths;
mod signal;

use std::collections::HashMap;
use std::ffi::OsString;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;

use anyhow::Context;
use clap::{ArgMatches, ColorChoice, Command, CommandFactory};
use qli_ext::secrets::{ResolvedSecret, SecretsError, SecretsResolver};
use qli_ext::SecretSpec;
use qli_ext::{tty_confirm, DispatchOptions, DispatchSignals};

fn main() -> ExitCode {
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
    let discovery = qli_ext::discover(&extensions_root);

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
        Some((group_name, group_matches)) => {
            dispatch_group(discovery, group_name, group_matches, assume_yes, signals)
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

    let resolver = StubResolver;
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

/// Phase 1F has the trait surface but ships no production secret providers
/// — those land in Phase 1G. Until then any manifest that declares
/// `[[secrets]]` fails closed with this stub. Dev/org/prod manifests
/// without `[[secrets]]` are unaffected.
struct StubResolver;

impl SecretsResolver for StubResolver {
    fn resolve_all(&self, specs: &[SecretSpec]) -> Result<Vec<ResolvedSecret>, SecretsError> {
        if let Some(spec) = specs.first() {
            return Err(SecretsError::ProviderUnavailable {
                env: spec.env.clone(),
                provider: match spec.provider {
                    qli_ext::SecretProvider::OnePassword => "one_password",
                    qli_ext::SecretProvider::Env => "env",
                },
                message: "secret providers ship in Phase 1G; remove `[[secrets]]` or wait for the next release".into(),
            });
        }
        Ok(Vec::new())
    }
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
