use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// Install a Ctrl+C / SIGTERM handler that flips the returned `AtomicBool`.
///
/// Long-running operations should poll this flag and exit cleanly with the
/// appropriate exit code (130 for SIGINT, 143 for SIGTERM). The dispatcher
/// (Phase 1F) forwards signals to spawned extensions and updates this flag.
pub fn install() -> Arc<AtomicBool> {
    let interrupted = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&interrupted);
    if let Err(err) = ctrlc::set_handler(move || {
        flag.store(true, Ordering::Relaxed);
    }) {
        tracing::warn!("failed to install signal handler: {err}");
    }
    interrupted
}
