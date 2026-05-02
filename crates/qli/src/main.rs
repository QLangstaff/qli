mod cli;
mod exit;
mod logging;
mod paths;
mod signal;

use std::process::ExitCode;

use clap::{CommandFactory, Parser};

fn main() -> ExitCode {
    let cli = cli::Cli::parse();

    cli::apply_color_choice(cli.color);
    logging::init(cli.verbose, cli.quiet);
    // `--version` / `--help` short-circuit in `Cli::parse()` above, so this
    // line only runs for real subcommands. Verify XDG-dir creation with one
    // (e.g. `qli completions zsh`).
    paths::ensure_all();
    let _interrupted = signal::install();

    match run(cli.command.as_ref()) {
        Ok(()) => ExitCode::from(exit::SUCCESS),
        Err(err) => {
            eprintln!("error: {err:#}");
            ExitCode::from(exit::ERROR)
        }
    }
}

fn run(command: Option<&cli::Command>) -> anyhow::Result<()> {
    match command {
        Some(cli::Command::Completions { shell }) => {
            let mut cmd = cli::Cli::command();
            let bin_name = cmd.get_name().to_string();
            clap_complete::generate(*shell, &mut cmd, bin_name, &mut std::io::stdout());
            Ok(())
        }
        None => {
            cli::Cli::command().print_help()?;
            Ok(())
        }
    }
}
