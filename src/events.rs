//! Telling a browser the moment something changes, rather than making it ask
//! on a clock.
//!
//! A long poll: the page asks `/events?since=G&live=L`, and the answer is held
//! back until the map has moved past generation `G` or the live feed past its
//! sequence `L` — then it says what moved, and the page asks again at once. A
//! change reaches the page within milliseconds of arriving here, and a quiet
//! server costs one request every half minute, when the wait gives up so that
//! nothing between here and the browser times the connection out.
//!
//! Not server-sent events, though that was the first design. The server library
//! writes a response through a chunked encoder that holds eight kilobytes
//! before it writes any of them, with no way to flush from outside, so a
//! stream of small events would have arrived in bursts of eight kilobytes; the
//! only other road to the socket is the upgrade path, which stamps the answer
//! with headers meant for a protocol switch. A poll that waits has neither
//! problem: it is an ordinary response that ends, and ends the moment there is
//! something to say.
//!
//! One thread per waiting browser, which is what the library's shape asks for
//! — a response is written by the thread that calls `respond` — and a thread
//! that spends its life blocked on a condition variable costs a few
//! kilobytes. A cap keeps a public server from spending one per crawler; past
//! it the page is refused and falls back to its own clock.
//!
//! What is answered is exactly what the two polls would have answered — the
//! `info.json` for that reader since their last, and their `live.json` — so
//! the page handles both the same way whichever road they arrived by.

use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Condvar, Mutex};
use std::time::{Duration, Instant};

/// How many browsers may wait at once. Past this the page is refused and
/// polls on its own clock instead.
pub const MOST_WAITING: usize = 256;

/// How long a wait may last before it answers with nothing, so that a proxy
/// or a browser does not give up on a connection that is only quiet.
pub const LONGEST_WAIT: Duration = Duration::from_secs(25);

#[derive(Default)]
pub struct Events {
    /// Woken whenever anything moves. The map's own generation says whether
    /// the map did; `live` below says whether the feed did.
    moved: (Mutex<()>, Condvar),
    /// Rises every time the mod posts a feed, so a page can ask "since when".
    live: AtomicU64,
    waiting: AtomicUsize,
}

impl Events {
    /// The live feed's own clock.
    #[must_use]
    pub fn live_seq(&self) -> u64 {
        self.live.load(Ordering::Relaxed)
    }

    /// How many browsers are waiting, for the log and for `witchlight status`.
    #[must_use]
    pub fn waiting(&self) -> usize {
        self.waiting.load(Ordering::Relaxed)
    }

    /// Wakes everybody waiting, for the map having moved. Called from inside
    /// [`crate::state::State::bump`], under whatever lock the caller holds:
    /// waking is a notify and nothing else, and the woken threads take their
    /// own locks on their own time.
    pub fn map_changed(&self) {
        self.moved.1.notify_all();
    }

    /// Wakes everybody waiting, for the feed having moved.
    pub fn live_changed(&self) {
        self.live.fetch_add(1, Ordering::Relaxed);
        self.moved.1.notify_all();
    }

    /// Waits until `has_moved` says so, or the longest wait is up. Answers
    /// whether anything moved, or nothing where there is no room to wait —
    /// which the caller answers with a refusal the page reads as "poll".
    pub fn wait(&self, has_moved: impl Fn() -> bool) -> Option<bool> {
        if self.waiting.fetch_add(1, Ordering::Relaxed) >= MOST_WAITING {
            self.waiting.fetch_sub(1, Ordering::Relaxed);
            return None;
        }

        let started = Instant::now();
        let moved = (|| {
            let Ok(mut guard) = self.moved.0.lock() else { return false };
            loop {
                if has_moved() {
                    return true;
                }
                let left = LONGEST_WAIT.saturating_sub(started.elapsed());
                if left.is_zero() {
                    return false;
                }
                // A spurious wake-up costs one more look at `has_moved`, which
                // is the loop.
                let Ok((next, _)) = self.moved.1.wait_timeout(guard, left) else { return false };
                guard = next;
            }
        })();

        self.waiting.fetch_sub(1, Ordering::Relaxed);
        Some(moved)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn a_wait_ends_when_told_something_moved() {
        let events = Arc::new(Events::default());
        let live_before = events.live_seq();
        let waiter = {
            let events = Arc::clone(&events);
            std::thread::spawn(move || events.wait(|| events.live_seq() > live_before))
        };
        std::thread::sleep(Duration::from_millis(50));
        events.live_changed();
        assert_eq!(waiter.join().unwrap(), Some(true));
        assert_eq!(events.waiting(), 0);
    }

    #[test]
    fn a_wait_that_is_already_satisfied_does_not_wait() {
        let events = Events::default();
        let started = Instant::now();
        assert_eq!(events.wait(|| true), Some(true));
        assert!(started.elapsed() < Duration::from_millis(100));
    }
}
