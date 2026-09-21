//! Reports a panic through the service's own log, and names the threads that
//! run forever.
//!
//! A panic writes to stderr in Rust's own format and says nothing about which
//! part of the service was running. The service is started by the mod, which
//! keeps only the last run's output, so a panic that ends a run is the last
//! thing written and the first thing lost when the service is started again.
//!
//! What this adds:
//!
//! - Every panic goes through [`crate::util::log`], so it carries the service
//!   name and sits in order among the lines around it.
//! - A panic on a named thread says which thread it was. A worker that dies
//!   takes its work with it and the service carries on without it, so the name
//!   is the only sign of what stopped.
//! - A panic that ends the process says so outright, and says the exit code the
//!   mod will report.

use std::panic::PanicHookInfo;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::util::log::warn;

/// The exit code a Rust process returns when a panic unwinds out of main.
///
/// The mod reports this number and nothing else, so the log says what it means
/// while the log still exists.
const PANIC_EXIT: i32 = 101;

/// Set once a panic has been reported, so the summary at the end of a run can
/// say whether anything went wrong even when the panic was on a worker.
static PANICKED: AtomicBool = AtomicBool::new(false);

/// Reports whether any thread has panicked during this run.
#[must_use]
pub fn panicked() -> bool {
    PANICKED.load(Ordering::Relaxed)
}

/// Sends every panic through the service's log, naming the thread it happened
/// on.
///
/// Call once, before any thread is started.
pub fn report_panics() {
    // Keep the default hook. It writes the backtrace, which this does not try
    // to reproduce, and it writes it to stderr where the mod already captures
    // it.
    let default = std::panic::take_hook();

    std::panic::set_hook(Box::new(move |info: &PanicHookInfo<'_>| {
        PANICKED.store(true, Ordering::Relaxed);

        let thread = std::thread::current();
        let name = thread.name().unwrap_or("unnamed").to_owned();
        let at = info
            .location()
            .map_or_else(|| "an unknown place".to_owned(), |where_| format!("{where_}"));
        let said = message_of(info);

        // What a panic costs depends on where it happened, and the hook cannot
        // tell on its own. A request is answered inside a catch and costs one
        // response; a named worker loop is not, and its work stops for the rest
        // of the run; the main thread unwinds out of `serve` and ends the run.
        // Each line below says which, so the log names the damage and not just
        // the fault.
        match name.as_str() {
            "main" => warn!(
                "the main thread panicked at {at}: {said}. This ends the run, and the \
                 service exits {PANIC_EXIT}. The mod reports that as stopping on its own."
            ),
            // The request threads are unnamed: `answer` runs on the main thread
            // and on workers started without a name, and both catch what a
            // route panics with. The line naming the request follows this one.
            "unnamed" => warn!(
                "a request thread panicked at {at}: {said}. The request it was answering \
                 is named on the next line. The service is still running."
            ),
            _ => warn!(
                "the {name} thread panicked at {at}: {said}. That thread is gone and the \
                 service carries on without it, so whatever it did stops happening until \
                 the service is started again."
            ),
        }

        default(info);
    }));
}

/// Returns what was passed to `panic!`, as text.
///
/// A panic carries either a `&str` or a `String`, and neither is reachable
/// without asking for both.
fn message_of(info: &PanicHookInfo<'_>) -> String {
    let payload = info.payload();
    if let Some(said) = payload.downcast_ref::<&str>() {
        (*said).to_owned()
    } else if let Some(said) = payload.downcast_ref::<String>() {
        said.clone()
    } else {
        "no message".to_owned()
    }
}

/// Starts a thread that runs for the life of the service, under a name.
///
/// Every long-running thread goes through this. A thread started without a name
/// reports as `unnamed` when it panics, which is the one moment the name is
/// needed. This also says in the log that the thread has ended, which a loop
/// meant to run forever should never do.
pub fn forever(name: &'static str, work: impl FnOnce() + Send + 'static) {
    let started = std::thread::Builder::new().name(name.to_owned()).spawn(move || {
        work();
        // A loop written to run forever reaching its end is worth a line. It
        // means the loop was left some other way than by panicking, and
        // whatever it did has quietly stopped.
        warn!("the {name} thread ended on its own, so whatever it did stops happening");
    });

    if let Err(error) = started {
        warn!("could not start the {name} thread: {error}. Whatever it does will not happen.");
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_panic_payload_reads_back_as_the_text_it_was_given() {
        // Both shapes a panic carries, since only one of the two is reachable
        // by any single downcast.
        let from_str = std::panic::catch_unwind(|| panic!("a literal")).unwrap_err();
        assert_eq!(from_str.downcast_ref::<&str>().copied(), Some("a literal"));

        let built = std::panic::catch_unwind(|| panic!("{}", "a built string".to_owned()))
            .unwrap_err();
        assert_eq!(built.downcast_ref::<String>().map(String::as_str), Some("a built string"));
    }

    #[test]
    fn a_named_thread_carries_its_name_into_the_panic_hook() {
        let name = std::thread::Builder::new()
            .name("the-namer".to_owned())
            .spawn(|| std::thread::current().name().map(str::to_owned))
            .expect("a thread")
            .join()
            .expect("it ran");
        assert_eq!(name.as_deref(), Some("the-namer"), "the name the hook reports");
    }
}
