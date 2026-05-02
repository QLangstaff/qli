//! Process-wide signal handling for the dispatcher.
//!
//! On SIGINT or SIGTERM we forward the signal to the running extension (if
//! any) and mark the run as interrupted so the dispatcher writes an
//! `interrupted` audit entry instead of `finish`. The shared
//! [`qli_ext::DispatchSignals`] handle is what lets the handler thread and
//! the dispatcher communicate without a global.

use std::sync::Arc;

use qli_ext::DispatchSignals;

/// Install a Ctrl+C / SIGTERM handler that forwards to any running child
/// via `signals` and flags the run as interrupted. The same `Arc` is passed
/// into [`qli_ext::DispatchOptions`] so the dispatcher can read the flag
/// after `wait`.
pub fn install() -> Arc<DispatchSignals> {
    let signals = DispatchSignals::new();
    let handler = Arc::clone(&signals);
    if let Err(err) = ctrlc::set_handler(move || {
        handler.on_signal();
    }) {
        // Tier-3 must-see warning: a missing handler means Ctrl+C will not
        // forward to running extensions. Don't route through `tracing` —
        // `-q` would silence behaviour the user needs to know about.
        eprintln!("warning: failed to install signal handler: {err}; Ctrl+C will not forward to running extensions");
    }
    signals
}
