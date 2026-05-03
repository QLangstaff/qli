//! Custom panic hook for the `qli` binary.
//!
//! Replaces Rust's default panic UI ("thread 'main' panicked at <file:line>:
//! <message>" plus a backtrace prompt) with a single user-facing message
//! that names the bug as a bug, points at the issue tracker, and includes
//! the location + message a maintainer needs to triage.
//!
//! Expected failures surface as `anyhow::Error` rendered through `main()`'s
//! `eprintln!("error: ...")` path — no traceback, no panic. Unexpected
//! failures (a `.expect()` that fires, an out-of-bounds index, a poisoned
//! mutex) reach the hook installed here.
//!
//! `set_hook` *replaces* the default hook entirely, so the standard
//! backtrace under `RUST_BACKTRACE=1` would be lost unless we explicitly
//! delegate. [`install`] captures the prior hook via `take_hook` and the
//! installed closure re-invokes it when `RUST_BACKTRACE` is set.

use std::any::Any;
use std::panic;

const ISSUE_TRACKER_URL: &str = "https://github.com/QLangstaff/qli/issues";

/// Install the qli panic hook. Call once from `main` before any code that
/// might panic.
pub fn install() {
    let default = panic::take_hook();
    panic::set_hook(Box::new(move |info| {
        let location = info
            .location()
            .map_or_else(|| "<unknown location>".to_owned(), ToString::to_string);
        let message = panic_message(info.payload());

        eprintln!(
            "error: qli encountered an internal bug. Please report it at {ISSUE_TRACKER_URL}"
        );
        eprintln!("  panic at {location}: {message}");
        if std::env::var_os("RUST_BACKTRACE").is_some() {
            // Delegate to the default hook for the backtrace. It re-prints
            // its own `thread 'main' panicked at ...` line above the stack
            // frames — unavoidable without reimplementing backtrace capture.
            default(info);
        } else {
            eprintln!("  re-run with RUST_BACKTRACE=1 for a backtrace");
        }
    }));
}

/// Decode a `panic!()` payload into a printable string.
///
/// `panic!("literal")` produces a `&'static str`; `panic!("{var}")` produces
/// a `String`. Anything else (a `Box<MyError>`, a non-string panic) collapses
/// to a generic placeholder — a maintainer running with `RUST_BACKTRACE=1`
/// will still see the type via the standard backtrace.
fn panic_message(payload: &(dyn Any + Send)) -> String {
    if let Some(s) = payload.downcast_ref::<&str>() {
        (*s).to_owned()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "<non-string panic payload>".to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn panic_message_decodes_str_payload() {
        // `panic!("literal")` form.
        let payload: Box<dyn Any + Send> = Box::new("hello panic");
        assert_eq!(panic_message(&*payload), "hello panic");
    }

    #[test]
    fn panic_message_decodes_string_payload() {
        // `panic!("{var}")` / `panic!("{}", x)` forms.
        let payload: Box<dyn Any + Send> = Box::new(String::from("formatted panic"));
        assert_eq!(panic_message(&*payload), "formatted panic");
    }

    #[test]
    fn panic_message_handles_unknown_payload() {
        // `panic_any(MyError)` / non-string panic.
        #[derive(Debug)]
        struct Weird;
        let payload: Box<dyn Any + Send> = Box::new(Weird);
        assert_eq!(panic_message(&*payload), "<non-string panic payload>");
    }
}
