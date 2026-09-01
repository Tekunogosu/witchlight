//! Everything this service says, on one lock and under one name.
//!
//! Two clocks, a listener for the mod and one thread per request all say things
//! while the map is being served. `println!` takes stdout for the length of one
//! call, so a line is never torn in half — but a message written as two calls is
//! two claims on that lock with a gap between them, and on a running server
//! another thread's news lands in the gap. A fault and the sentence saying what
//! to do about it then arrive with something unrelated between them.
//!
//! One gate over both streams settles it: a message is one call, written whole,
//! and no second message begins on either stream until the first has finished.
//! That is a total order over the log rather than one order per stream, so a
//! transcript taken with `2>&1` reads the way the run happened.
//!
//! The name this service answers to lives here as well, rather than at the
//! thirty-odd call sites that each spelled it out.

use std::fmt::Arguments;
use std::io::Write;
use std::sync::{Mutex, MutexGuard, PoisonError};

/// What this service calls itself on the log.
const NAME: &str = "witchlight";

/// Held for the length of one message, whichever stream it goes to.
static SPEAKING: Mutex<()> = Mutex::new(());

/// Says one line on stdout: what happened, for somebody reading along.
pub fn said(what: Arguments) {
    let _speaking = gate();
    let _ = writeln!(std::io::stdout().lock(), "{NAME}: {what}");
}

/// Says one line on stderr: something an operator may have to go and fix.
pub fn warned(what: Arguments) {
    let _speaking = gate();
    let _ = writeln!(std::io::stderr().lock(), "{NAME}: {what}");
}

/// The gate, taken even where a message panicked while holding it. A log that
/// goes quiet after one panic hides every fault after the first as well.
fn gate() -> MutexGuard<'static, ()> {
    SPEAKING.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Says what happened. One whole line on stdout, under the gate.
///
/// One call is one message on purpose. Anything worth saying in two sentences is
/// worth saying in one call, because two calls are two messages and a running
/// server is entitled to put something between them.
#[macro_export]
macro_rules! say {
    ($($what:tt)*) => { $crate::log::said(std::format_args!($($what)*)) };
}

/// Says what went wrong. One whole line on stderr, under the same gate, so it
/// keeps its place among the lines around it.
#[macro_export]
macro_rules! warn {
    ($($what:tt)*) => { $crate::log::warned(std::format_args!($($what)*)) };
}

/// Both macros are `#[macro_export]`, which puts them at the crate root.
/// Re-exported here so a call site names the module it reached for, which is
/// what a reader looks for and what `tests/layers.rs` reads.
pub use crate::{say, warn};
