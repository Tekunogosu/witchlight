//! Holds what the game made of each claim somebody asked for, until the browser
//! that asked reads it.
//!
//! This is the return leg of [`crate::protocol::pending`], and the mirror of it.
//! The queue there carries an ask towards the game; this carries the answer back.
//! Both exist because the two halves talk one way: the mod posts to this service
//! and reads what it answers, and nothing here can reach into a game server.
//!
//! An answer is kept against the ticket the service minted when the ask was
//! queued, so the page recognises the answer to its own ask. Before this the page
//! watched the ground it had drawn on and guessed: a claim that appeared had
//! landed, and one that never appeared had been refused for a reason the page
//! invented out of the five it might have been. The real reason was in the game
//! server's log and reached nobody.
//!
//! Answers are held in memory and taken on reading, like the queue. An answer
//! nobody comes back for is dropped once it is old, because the browser that
//! asked has closed. A service that restarts loses them, which costs the page
//! its patience timer instead.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// How long an answer nobody has read is kept.
///
/// Longer than the page's own patience, so an answer is never dropped while the
/// form that asked is still waiting for it. A browser closed mid-ask leaves one
/// behind, which is what this bound is for.
const KEPT_FOR: Duration = Duration::from_secs(120);

/// How many answers one person may have waiting.
///
/// A page asks one thing at a time and reads the answer on its next beat. A
/// number far above that bounds what a page in a loop can pile up.
const MOST_EACH: usize = 32;

/// What became of one ask.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct Answer {
    /// The ticket the service minted when the ask was queued.
    pub ticket: String,
    /// What was asked for, in words the page shows: "claim that land", "change
    /// that claim", "give up that claim". The mod writes it.
    pub doing: String,
    /// Whether the game did it.
    pub done: bool,
    /// Why not, as one sentence, when it did not. Empty when it did.
    ///
    /// The mod's own words. The rules a claim is judged by are the game's, and a
    /// service that worded the refusal itself would be a second opinion on a
    /// question with one right answer.
    pub why: String,
}

/// Holds one answer and when it arrived.
struct Held {
    answer: Answer,
    at: Instant,
}

/// Holds what the game made of what was asked, by whose ask it was.
#[derive(Default)]
pub struct Answers {
    /// Keyed by uid, because an answer goes to the person who asked and to
    /// nobody else. A claim refused for overlapping says whose land it overlaps.
    by_person: Mutex<HashMap<String, Vec<Held>>>,
}

impl Answers {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Keeps one answer for whoever asked.
    ///
    /// Drops the oldest when somebody already holds the most allowed, so a page
    /// that asks and never reads cannot grow without end.
    pub fn keep(&self, uid: &str, answer: Answer) {
        let Ok(mut by_person) = self.by_person.lock() else {
            return;
        };
        let held = by_person.entry(uid.to_owned()).or_default();
        held.push(Held { answer, at: Instant::now() });
        if held.len() > MOST_EACH {
            held.remove(0);
        }
    }

    /// Returns what this person has not read, and forgets it.
    ///
    /// Taken on reading rather than kept, because an answer is news and a page
    /// that has been told does not need telling again. Anything too old to
    /// belong to a page still waiting is dropped on the way past.
    pub fn take(&self, uid: &str) -> Vec<Answer> {
        let Ok(mut by_person) = self.by_person.lock() else {
            return Vec::new();
        };
        self.forget_stale(&mut by_person);
        by_person
            .remove(uid)
            .map(|held| held.into_iter().map(|one| one.answer).collect())
            .unwrap_or_default()
    }

    /// Drops answers nobody came back for.
    fn forget_stale(&self, by_person: &mut HashMap<String, Vec<Held>>) {
        by_person.retain(|_, held| {
            held.retain(|one| one.at.elapsed() < KEPT_FOR);
            !held.is_empty()
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn refusal(ticket: &str) -> Answer {
        Answer {
            ticket: ticket.to_owned(),
            doing: "claim that land".to_owned(),
            done: false,
            why: "it overlaps a claim of Theysa's".to_owned(),
        }
    }

    #[test]
    fn an_answer_reaches_the_person_who_asked_and_nobody_else() {
        let answers = Answers::new();
        answers.keep("ada", refusal("t1"));
        assert!(answers.take("bob").is_empty(), "an answer is not another person's news");
        let mine = answers.take("ada");
        assert_eq!(mine.len(), 1);
        assert_eq!(mine[0].why, "it overlaps a claim of Theysa's");
    }

    #[test]
    fn reading_an_answer_forgets_it() {
        let answers = Answers::new();
        answers.keep("ada", refusal("t1"));
        assert_eq!(answers.take("ada").len(), 1);
        assert!(answers.take("ada").is_empty(), "a page told once is not told again");
    }

    #[test]
    fn a_page_that_asks_and_never_reads_does_not_grow_without_end() {
        let answers = Answers::new();
        for n in 0..(MOST_EACH + 10) {
            answers.keep("ada", refusal(&format!("t{n}")));
        }
        let held = answers.take("ada");
        assert_eq!(held.len(), MOST_EACH, "the oldest are dropped, not the newest");
        assert_eq!(held.last().unwrap().ticket, format!("t{}", MOST_EACH + 9));
    }
}
