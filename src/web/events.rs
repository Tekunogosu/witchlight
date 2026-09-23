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
//! The answer is the `info` for that reader since their last, and whichever
//! parts of the live feed have moved since the sequences they sent. The page
//! handles both the same way whichever route they arrived by.
//!
//! The live feed is counted per part rather than as a whole. The clock ticks
//! every second and the markers change a few times an hour, so one counter for
//! both sends every marker to every browser every second. Each part carries its
//! own sequence, and a browser is sent only the parts that moved past the
//! sequences it reported.

use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Condvar, Mutex};
use std::time::{Duration, Instant};

/// How many browsers may wait at once. Past this a page is refused and polls on
/// its own clock instead.
pub const MOST_WAITING: usize = 256;

/// How long a wait may last before it answers with nothing, so a proxy or a
/// browser does not give up on a connection that is only quiet.
pub const LONGEST_WAIT: Duration = Duration::from_secs(25);

/// Which part of the live feed a post carried.
///
/// Each part is counted on its own because they move at unrelated rates. The
/// world clock changes every second, player positions as often as somebody
/// walks, and the markers and claims a few times an hour.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Feed {
    Players,
    Markers,
    Claims,
    World,
    /// Rows a plugin's own collector posted. They are live data with no place in
    /// the live body, and a page showing them is woken the same way.
    Plugins,
}

/// The sequence of every part of the live feed.
///
/// A browser reports what it holds and is sent what has moved past it. A part
/// the browser did not name is treated as never seen, so a page that has just
/// loaded is sent everything once.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Sequences {
    pub players: u64,
    pub markers: u64,
    pub claims: u64,
    pub world: u64,
    pub plugins: u64,
}

#[derive(Default)]
pub struct Events {
    /// Wakes whenever anything moves. The map's generation says whether the map
    /// moved, and the counters below say which parts of the feed did.
    moved: (Mutex<()>, Condvar),
    /// Increments every time the mod posts a part of the feed that differs from
    /// what is already held, so a page can ask what changed since a given
    /// sequence.
    players: AtomicU64,
    markers: AtomicU64,
    claims: AtomicU64,
    world: AtomicU64,
    plugins: AtomicU64,
    waiting: AtomicUsize,
}

impl Events {
    /// Returns the current sequence of every part of the live feed.
    #[must_use]
    pub fn sequences(&self) -> Sequences {
        Sequences {
            players: self.players.load(Ordering::Relaxed),
            markers: self.markers.load(Ordering::Relaxed),
            claims: self.claims.load(Ordering::Relaxed),
            world: self.world.load(Ordering::Relaxed),
            plugins: self.plugins.load(Ordering::Relaxed),
        }
    }

    // No caller outside the tests reads one part on its own. `wait_for_events`
    // compares every part at once, through `sequences`.
    #[allow(dead_code)]
    /// Returns one part's current sequence.
    #[must_use]
    pub fn seq_of(&self, feed: Feed) -> u64 {
        self.counter(feed).load(Ordering::Relaxed)
    }

    fn counter(&self, feed: Feed) -> &AtomicU64 {
        match feed {
            Feed::Players => &self.players,
            Feed::Markers => &self.markers,
            Feed::Claims => &self.claims,
            Feed::World => &self.world,
            Feed::Plugins => &self.plugins,
        }
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

    /// Wakes everybody waiting because one part of the feed moved.
    ///
    /// Called only where the post differed from what was already held. A post
    /// that says nothing new wakes nobody, which is what keeps an unchanged
    /// marker list from being sent again every time the clock ticks.
    pub fn live_changed(&self, feed: Feed) {
        self.counter(feed).fetch_add(1, Ordering::Relaxed);
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
        let before = events.seq_of(Feed::Markers);
        let waiter = {
            let events = Arc::clone(&events);
            std::thread::spawn(move || events.wait(|| events.seq_of(Feed::Markers) > before))
        };
        std::thread::sleep(Duration::from_millis(50));
        events.live_changed(Feed::Markers);
        assert_eq!(waiter.join().unwrap(), Some(true));
        assert_eq!(events.waiting(), 0);
    }

    #[test]
    fn a_clock_tick_does_not_wake_a_page_waiting_on_the_markers() {
        // The whole reason the parts are counted separately. The clock moves
        // every second, and a page holding the current markers must sleep
        // through it rather than be handed them again.
        let events = Events::default();
        events.live_changed(Feed::World);
        let markers = events.seq_of(Feed::Markers);
        assert_eq!(markers, 0, "the markers did not move because the clock did");
        assert_eq!(events.seq_of(Feed::World), 1, "the clock moved on its own");
    }

    #[test]
    fn a_wait_that_is_already_satisfied_does_not_wait() {
        let events = Events::default();
        let started = Instant::now();
        assert_eq!(events.wait(|| true), Some(true));
        assert!(started.elapsed() < Duration::from_millis(100));
    }
}
