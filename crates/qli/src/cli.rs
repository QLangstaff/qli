use std::path::PathBuf;

use clap::{ColorChoice, Parser, Subcommand, ValueEnum};

#[derive(Parser, Debug)]
#[command(
    name = "qli",
    version,
    about = "Polyglot code-analysis CLI and any-language extension framework.",
    after_help = ROOT_AFTER_HELP,
)]
pub struct Cli {
    /// Increase logging verbosity. -v info, -vv debug, -vvv trace.
    /// `RUST_LOG=<level>` overrides; `RUST_LOG=<target>=<level>` refines.
    #[arg(short = 'v', long = "verbose", action = clap::ArgAction::Count, global = true)]
    pub verbose: u8,

    /// Decrease logging (suppress info; errors still print).
    #[arg(short = 'q', long = "quiet", global = true, conflicts_with = "verbose")]
    pub quiet: bool,

    /// When to use colored output.
    #[arg(long = "color", value_enum, default_value_t = ColorChoice::Auto, global = true)]
    pub color: ColorChoice,

    /// Assume yes for any guard prompt. Required for non-interactive runs
    /// of extensions whose group manifest sets `confirm = true`.
    #[arg(short = 'y', long = "yes", global = true)]
    pub yes: bool,

    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Analyze source files for diagnostics or definitions/references.
    #[command(after_help = ANALYZE_AFTER_HELP)]
    Analyze {
        /// Files or directories to analyze. Directories are walked
        /// (respecting `.gitignore`); unsupported files are skipped.
        #[arg(value_name = "PATH", required = true)]
        paths: Vec<PathBuf>,

        /// Language to parse as, or `auto` to detect from the file extension.
        #[arg(long, default_value = "auto")]
        lang: String,

        /// Which analyzer to run.
        #[arg(long, default_value = "todofixme")]
        analyzer: String,

        /// Output format. `auto` picks human on a TTY, jsonl when piped.
        #[arg(long, value_enum, default_value_t = FormatArg::Auto)]
        format: FormatArg,

        /// Skip the analysis cache for this run (no read, no write).
        #[arg(long)]
        no_cache: bool,
    },
    /// Generate shell completions.
    #[command(after_help = COMPLETIONS_AFTER_HELP)]
    Completions {
        /// Target shell.
        shell: clap_complete::Shell,
    },
    /// Manage extensions.
    Ext {
        #[command(subcommand)]
        action: ExtAction,
    },
    /// Update qli to the latest release (curl-installed) or print the upgrade command for your install method.
    SelfUpdate {
        /// Emit machine-readable JSON instead of the human message.
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand, Debug)]
pub enum ExtAction {
    /// List every discovered extension with its group, origin, and resolved path.
    List {
        /// Emit one JSON object per extension instead of the tab-separated form.
        #[arg(long)]
        json: bool,
    },
    /// Print the resolved path to a single extension.
    Which {
        /// Group the extension belongs to (e.g. `dev`, `prod`).
        group: String,
        /// Extension name within the group (e.g. `hello`).
        name: String,
        /// Emit a JSON object with group/name/origin/path instead of just the path.
        #[arg(long)]
        json: bool,
    },
    /// Copy the binary's embedded default extensions into
    /// `$XDG_DATA_HOME/qli/extensions/` so they're editable.
    InstallDefaults {
        /// Overwrite files that already exist.
        #[arg(long)]
        force: bool,
    },
    /// Manage the on-disk analysis cache.
    Cache {
        #[command(subcommand)]
        action: CacheAction,
    },
}

#[derive(Subcommand, Debug)]
pub enum CacheAction {
    /// Evict cached analysis results.
    Clean {
        /// Remove every cached entry.
        #[arg(long, conflicts_with = "older_than")]
        all: bool,
        /// Remove entries older than this many days.
        #[arg(long, value_name = "DAYS")]
        older_than: Option<u64>,
    },
}

/// CLI-level output format, adding `auto` over [`qli_outputs::Format`].
#[derive(Clone, Copy, PartialEq, Eq, Debug, ValueEnum)]
pub enum FormatArg {
    /// Human on a TTY, jsonl when piped.
    Auto,
    /// Colored `file:line:col` lines.
    Human,
    /// One JSON object per finding, one per line.
    Jsonl,
}

const ROOT_AFTER_HELP: &str = "\
EXAMPLES:
    qli --version                       Print qli's version.
    qli completions zsh > ~/.zsh/_qli   Generate zsh completion script.
    NO_COLOR=1 qli --help               Disable colored output.
";

const ANALYZE_AFTER_HELP: &str = "\
EXAMPLES:
    qli analyze src/                     Analyze a directory (auto-detect languages).
    qli analyze foo.py bar.ts            Analyze specific files.
    qli analyze --analyzer defrefs src/  Extract definitions and references.
    qli analyze foo.py | jq .            Machine-readable jsonl (auto when piped).
";

const COMPLETIONS_AFTER_HELP: &str = "\
EXAMPLES:
    qli completions bash > /usr/local/etc/bash_completion.d/qli
    qli completions zsh  > \"${fpath[1]}/_qli\"
    qli completions fish > ~/.config/fish/completions/qli.fish
";

/// Apply --color choice by setting the standard env vars that both clap and
/// `anstream` consult. `Auto` leaves the environment untouched so the existing
/// `NO_COLOR` / `CLICOLOR_FORCE` rules apply.
pub fn apply_color_choice(choice: ColorChoice) {
    match choice {
        ColorChoice::Always => {
            std::env::remove_var("NO_COLOR");
            std::env::set_var("CLICOLOR_FORCE", "1");
        }
        ColorChoice::Never => {
            std::env::set_var("NO_COLOR", "1");
        }
        ColorChoice::Auto => {}
    }
}
