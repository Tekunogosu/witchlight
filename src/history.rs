//! What changed at each generation, kept long enough for a viewer to catch up.
//!
//! A browser asks "what moved since generation G" and is answered from here:
//! every entry newer than G, or nothing at all where an entry it would have
//! needed has already been let go, which the caller turns into "repaint
//! everything". The window is a length of time rather than a count of
//! entries, because entries arrive as fast as the server is busy: a count that
//! was minutes of slack with one player exploring is seconds with forty, and a
//! tab away for a minute came back to a full repaint. A cap stays underneath,
//! so a server busier than anyone planned for still cannot grow this without
//! end.
//!
//! Generic over what an entry says, since the map's own history names tiles
//! and each person's names regions.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

/// How long an entry is kept. Longer than any tab is left in the background
/// before somebody comes back to it expecting the map to pick up where it was.
pub const KEPT_FOR: Duration = Duration::from_secs(10 * 60);

/// The most entries kept whatever their age. At a thousand a minute, which is
/// far past what any server has produced, this is a megabyte or so.
const MOST: usize = 50_000;

pub struct History<T> {
    entries: VecDeque<(u64, Instant, T)>,
    /// The newest generation ever let go. A reader from before it has missed
    /// something this can no longer name.
    dropped_up_to: u64,
}

impl<T> Default for History<T> {
    fn default() -> Self {
        Self { entries: VecDeque::new(), dropped_up_to: 0 }
    }
}

impl<T> History<T> {
    /// Files what changed at `generation`, and lets go of whatever has aged out.
    pub fn record(&mut self, generation: u64, what: T, now: Instant) {
        self.entries.push_back((generation, now, what));
        while self.entries.len() > MOST
            || self.entries.front().is_some_and(|(_, at, _)| now.duration_since(*at) >= KEPT_FOR)
        {
            if let Some((gone, _, _)) = self.entries.pop_front() {
                self.dropped_up_to = self.dropped_up_to.max(gone);
            }
        }
    }

    /// Everything newer than `since`, oldest first — or nothing where part of
    /// that has already been let go and the honest answer is "everything".
    pub fn since(&self, since: u64) -> Option<impl Iterator<Item = &T> + '_> {
        if self.dropped_up_to > since {
            return None;
        }
        Some(self.entries.iter().filter(move |(at, _, _)| *at > since).map(|(_, _, what)| what))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn named(history: &History<&'static str>, since: u64) -> Option<Vec<&'static str>> {
        history.since(since).map(|found| found.copied().collect())
    }

    #[test]
    fn a_reader_is_told_what_moved_since_they_looked() {
        let start = Instant::now();
        let mut history = History::default();
        history.record(1, "a", start);
        history.record(2, "b", start);
        history.record(3, "c", start);
        assert_eq!(named(&history, 0), Some(vec!["a", "b", "c"]));
        assert_eq!(named(&history, 2), Some(vec!["c"]));
        assert_eq!(named(&history, 3), Some(vec![]));
    }

    #[test]
    fn a_burst_inside_the_window_is_kept_whole() {
        let start = Instant::now();
        let mut history = History::default();
        for generation in 1..=1000 {
            history.record(generation, "x", start + Duration::from_millis(generation));
        }
        assert_eq!(named(&history, 0).map(|found| found.len()), Some(1000), "a count would have cut this");
    }

    #[test]
    fn what_aged_out_makes_an_older_reader_repaint_everything() {
        let start = Instant::now();
        let mut history = History::default();
        history.record(1, "a", start);
        history.record(2, "b", start + KEPT_FOR);
        assert_eq!(named(&history, 1), Some(vec!["b"]), "nothing they needed was dropped");
        assert_eq!(named(&history, 0), None, "generation 1 is gone");
    }
}
