//! Logging for the service, on one lock and under one name.
//!
//! Two clocks, a listener for the mod and one thread per request all log while
//! the map is being served. A single gate covers both stdout and stderr, so a
//! message is one call written whole and no second message starts on either
//! stream until the first finishes. That gives a total order over the log
//! rather than one order per stream, so a transcript taken with `2>&1` reads in
//! the order the run happened.
//!
//! The service name used on every line is defined here.

use std::fmt::Arguments;
use std::io::Write;
use std::sync::{Mutex, MutexGuard, PoisonError};

/// The name this service prints at the start of every log line.
const NAME: &str = "witchlight";

/// Held for the length of one message, whichever stream it goes to.
static SPEAKING: Mutex<()> = Mutex::new(());

/// Writes one line to stdout.
pub fn said(what: Arguments) {
    let _speaking = gate();
    let _ = writeln!(std::io::stdout().lock(), "{NAME}: {what}");
}

/// Writes one line to stderr, for something an operator may have to fix.
pub fn warned(what: Arguments) {
    let _speaking = gate();
    let _ = writeln!(std::io::stderr().lock(), "{NAME}: {what}");
}

/// Takes the gate, including when a previous holder panicked. A log that goes
/// quiet after one panic hides every fault after the first.
fn gate() -> MutexGuard<'static, ()> {
    SPEAKING.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Writes one whole line to stdout, under the gate.
///
/// One call is one message. Two calls are two messages, and another thread can
/// log between them, so put everything for one message in one call.
#[macro_export]
macro_rules! say {
    ($($what:tt)*) => { $crate::util::log::said(std::format_args!($($what)*)) };
}

/// Writes one whole line to stderr, under the same gate, so it keeps its place
/// among the lines around it.
#[macro_export]
macro_rules! warn {
    ($($what:tt)*) => { $crate::util::log::warned(std::format_args!($($what)*)) };
}

/// `#[macro_export]` puts both macros at the crate root. They are re-exported
/// here so a call site names this module, which is what `tests/layers.rs`
/// checks.
pub use crate::{say, warn};
