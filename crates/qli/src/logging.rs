use std::path::Path;
use std::sync::Mutex;

use tracing::level_filters::LevelFilter;
use tracing_subscriber::EnvFilter;

/// Initialize tracing-subscriber to log to stderr.
///
/// The writer is an `anstream` auto-stream, like every other colored output in
/// the binary: level colors show on a terminal and are stripped when stderr is
/// piped (an editor's language-server output pane, `2> file`), honoring
/// `NO_COLOR` / `CLICOLOR_FORCE`.
///
/// Precedence: `RUST_LOG` env var (if set) > `-v` / `-q` flags > default (warn).
/// Target-specific `RUST_LOG` directives (e.g. `myapp=debug`) refine on top of
/// the flag-derived default level.
pub fn init(verbose: u8, quiet: bool) {
    tracing_subscriber::fmt()
        .with_writer(anstream::stderr)
        .with_env_filter(env_filter(verbose, quiet))
        .init();
}

/// Initialize tracing-subscriber to append to the file at `path` and write
/// nothing to stderr — for a mode whose stderr must stay silent
/// (`qli lsp --tcp`). Same level precedence as [`init`].
pub fn init_file(verbose: u8, quiet: bool, path: &Path) -> std::io::Result<()> {
    let file = std::fs::File::options()
        .create(true)
        .append(true)
        .open(path)?;
    tracing_subscriber::fmt()
        .with_writer(Mutex::new(file))
        .with_ansi(false)
        .with_env_filter(env_filter(verbose, quiet))
        .init();
    Ok(())
}

fn env_filter(verbose: u8, quiet: bool) -> EnvFilter {
    let default_level = if quiet {
        LevelFilter::ERROR
    } else {
        match verbose {
            0 => LevelFilter::WARN,
            1 => LevelFilter::INFO,
            2 => LevelFilter::DEBUG,
            _ => LevelFilter::TRACE,
        }
    };

    EnvFilter::builder()
        .with_default_directive(default_level.into())
        .from_env_lossy()
}
