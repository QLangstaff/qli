use tracing::level_filters::LevelFilter;
use tracing_subscriber::EnvFilter;

/// Initialize tracing-subscriber to log to stderr.
///
/// Precedence: `RUST_LOG` env var (if set) > `-v` / `-q` flags > default (warn).
/// Target-specific `RUST_LOG` directives (e.g. `myapp=debug`) refine on top of
/// the flag-derived default level.
pub fn init(verbose: u8, quiet: bool) {
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

    let env_filter = EnvFilter::builder()
        .with_default_directive(default_level.into())
        .from_env_lossy();

    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(env_filter)
        .init();
}
