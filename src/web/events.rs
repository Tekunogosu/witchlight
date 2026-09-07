//! Tells a browser the moment something changes, rather than making it poll on
//! a clock.
//!
//! This is a long poll. The page asks `/events?since=G&live=L`, and the answer
//! is held back until the map moves past generation `G` or the live feed past
//! sequence `L`. The response then says what moved and the page asks again at
//! once. A change reaches the page within milliseconds. A quiet server costs one
//! request every half minute, when the wait gives up so nothing between here and
//! the browser times the connection out.
//!
//! This is not server-sent events. The server library writes a response through
//! a chunked encoder that buffers eight kilobytes with no way to flush from
//! outside, so a stream of small events would arrive in eight-kilobyte bursts.
//! The only other route to the socket is the upgrade path, which stamps the
//! answer with headers meant for a protocol switch. A poll that waits has
//! neither problem. It is an ordinary response that ends the moment there is
//! something to say.
//!
//! Each waiting browser holds one thread, because the library writes a response
//! from the thread that calls `respond`. A thread blocked on a condition
//! variable costs a few kilobytes. A cap stops a public server from spending one
//! thread per crawler. Past the cap the page is refused and falls back to its own
//! clock.
//!
//! The answer is exactly what the two polls would have returned: the `info` for
//! that reader since their last, and their `live`. The page handles both the same
//! way whichever route they arrived by.

use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Condvar, Mutex};
use std::time::{Duration, Instant};

/// How many browsers may wait at once. Past this a page is refused and polls on
/// its own clock instead.
pub const MOST_WAITING: usize = 256;

/// How long a wait may last before it answers with nothing, so a proxy or a
/// browser does not give up on a connection that is only quiet.
pub const LONGEST_WAIT: Duration = Duration::from_secs(25);

#[derive(Default)]
pub struct Events {
    /// Wakes whenever anything moves. The map's generation says whether the map
    /// moved, and `live` below says whether the feed did.
    moved: (Mutex<()>, Condvar),
    /// Increments every time the mod posts a feed, so a page can ask what
    /// changed since a given sequence.
    live: AtomicU64,
    waiting: AtomicUsize,
}

impl Events {
    /// Returns the live feed's current sequence.
    #[must_use]
    pub fn live_seq(&self) -> u64 {
        self.live.load(Ordering::Relaxed)
    }

    // No caller reads this yet. `/witchlight status` answers from the mod's
    // own state and never asks the service. It is kept because it is the read
    // side of a store that has a write side, and the tests exercise it.
    #[allow(dead_code)]
    /// Returns how many browsers are waiting, for the log and for `witchlight
    /// status`.
    #[must_use]
    pub fn waiting(&self) -> usize {
        self.waiting.load(Ordering::Relaxed)
    }

    /// Wakes everybody waiting because the map moved.
    ///
    /// [`crate::state::State::bump`] calls this under whatever lock the caller
    /// holds. Waking is only a notify, and the woken threads take their own locks
    /// afterwards.
    pub fn map_changed(&self) {
        self.moved.1.notify_all();
    }

    /// Wakes everybody waiting because the feed moved.
    pub fn live_changed(&self) {
        self.live.fetch_add(1, Ordering::Relaxed);
        self.moved.1.notify_all();
    }

    /// Waits until `has_moved` returns true or the maximum wait elapses.
    ///
    /// Returns whether anything moved, or `None` when there is no room to wait.
    /// The caller turns that into a refusal the page reads as an instruction to
    /// poll.
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
                // A spurious wake-up costs one more call to `has_moved`, which
                // the loop handles.
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
