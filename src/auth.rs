//! Who is looking at the map.
//!
//! The map is public and stays public. This decides only whose settings and
//! whose markers a page may act on, which is a question nobody has had to ask
//! until now because there was nothing to act on.
//!
//! Identity comes from the game, because that is the only place it exists. The
//! mod asks for a word on the API channel, hands it to one player in chat, and
//! that player following the link is the whole of the proof — the game already
//! decided who they were when it let them in. Nothing here has a password, and
//! nothing here can be reached without the mod having spoken first.
//!
//! The link is single use and short lived; what it buys is a session, which is
//! long lived and lives in a cookie rather than in the address. A session in the
//! path would travel every time somebody shared a link to a view of the map,
//! which is exactly what the address is for.
//!
//! Sessions are held in memory and go when this stops. That costs one click of
//! one link, and it means the map keeps nothing about anybody that it was not
//! asked to keep.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// How long a login link is worth following. Long enough to notice the message
/// and click it, short enough that one left in a chat log is worth nothing.
const LINK_GOOD_FOR: Duration = Duration::from_secs(10 * 60);

/// How long a browser stays known. A map is a thing somebody opens for a minute
/// while playing; asking them to log in again every week would be its own bug.
const SESSION_GOOD_FOR: Duration = Duration::from_secs(30 * 24 * 60 * 60);

/// The cookie a session travels in.
pub const COOKIE: &str = "witchlight_session";

/// A player, as the game knows them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Who {
    /// The uid, which survives a rename and is what anything stored is keyed on.
    pub uid: String,
    /// What to call them on screen. Not an identity — it can change.
    pub name: String,
}

struct Held {
    who: Who,
    until: Instant,
}

/// Every login link waiting to be followed, and every browser that followed one.
#[derive(Default)]
pub struct Sessions {
    /// Kept apart from the sessions on purpose: these are spent on use, and one
    /// table with two lifetimes in it is one table with two meanings.
    links: Mutex<HashMap<String, Held>>,
    browsers: Mutex<HashMap<String, Held>>,
}

impl Sessions {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// A word for the mod to hand one player. Worth one login.
    pub fn mint(&self, who: Who) -> String {
        let word = crate::random::word(16);
        if let Ok(mut links) = self.links.lock() {
            // Anything nobody came back for, while the lock is already held.
            links.retain(|_, held| held.until > Instant::now());
            links.insert(word.clone(), Held { who, until: Instant::now() + LINK_GOOD_FOR });
        }
        word
    }

    /// Spends a login link, and gives back the session it buys.
    ///
    /// Removed whether or not it had expired, so a link is worth one attempt and
    /// a second press of the same address does nothing.
    pub fn redeem(&self, link: &str) -> Option<String> {
        let held = {
            let mut links = self.links.lock().ok()?;
            links.remove(link)?
        };
        if held.until <= Instant::now() {
            return None;
        }

        let word = crate::random::word(24);
        let mut browsers = self.browsers.lock().ok()?;
        browsers.retain(|_, held| held.until > Instant::now());
        browsers
            .insert(word.clone(), Held { who: held.who, until: Instant::now() + SESSION_GOOD_FOR });
        Some(word)
    }

    /// Who a request's cookies say it is, or nobody.
    ///
    /// Seeing somebody puts their session's clock back to the full term, so a map
    /// that is used stays open and one that is forgotten does not.
    pub fn who(&self, cookies: &str) -> Option<Who> {
        let word = cookie(cookies, COOKIE)?;
        let mut browsers = self.browsers.lock().ok()?;
        let held = browsers.get_mut(word)?;
        if held.until <= Instant::now() {
            browsers.remove(word);
            return None;
        }
        held.until = Instant::now() + SESSION_GOOD_FOR;
        Some(held.who.clone())
    }

    /// Forgets one browser. What a logout is.
    pub fn forget(&self, cookies: &str) {
        let Some(word) = cookie(cookies, COOKIE) else {
            return;
        };
        if let Ok(mut browsers) = self.browsers.lock() {
            browsers.remove(word);
        }
    }
}

/// One cookie's value out of a `Cookie` header.
///
/// Apart from the request so that it can be asserted directly, and because the
/// header is a list written by somebody else: names that merely end with the one
/// being looked for must not answer to it.
#[must_use]
pub fn cookie<'a>(header: &'a str, name: &str) -> Option<&'a str> {
    header.split(';').find_map(|pair| {
        let (found, value) = pair.split_once('=')?;
        (found.trim() == name).then(|| value.trim())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ada() -> Who {
        Who { uid: "uid-ada".to_owned(), name: "ada".to_owned() }
    }

    #[test]
    fn a_link_is_worth_one_login() {
        let sessions = Sessions::new();
        let link = sessions.mint(ada());

        let session = sessions.redeem(&link).expect("the first press logs in");
        assert!(sessions.redeem(&link).is_none(), "the second press must not");

        let held = sessions.who(&format!("{COOKIE}={session}")).expect("the session holds");
        assert_eq!(held, ada());
    }

    #[test]
    fn a_word_nobody_minted_is_worth_nothing() {
        let sessions = Sessions::new();
        assert!(sessions.redeem("not-a-link").is_none());
        assert!(sessions.who(&format!("{COOKIE}=not-a-session")).is_none());
        assert!(sessions.who("").is_none());
    }

    #[test]
    fn one_players_session_is_not_anothers() {
        let sessions = Sessions::new();
        let bob = Who { uid: "uid-bob".to_owned(), name: "bob".to_owned() };
        let first = sessions.redeem(&sessions.mint(ada())).expect("ada logs in");
        let second = sessions.redeem(&sessions.mint(bob.clone())).expect("bob logs in");

        assert_eq!(sessions.who(&format!("{COOKIE}={first}")), Some(ada()));
        assert_eq!(sessions.who(&format!("{COOKIE}={second}")), Some(bob));
    }

    #[test]
    fn logging_out_forgets_that_browser_alone() {
        let sessions = Sessions::new();
        let first = sessions.redeem(&sessions.mint(ada())).expect("one browser");
        let second = sessions.redeem(&sessions.mint(ada())).expect("and another");

        sessions.forget(&format!("{COOKIE}={first}"));
        assert!(sessions.who(&format!("{COOKIE}={first}")).is_none());
        assert!(sessions.who(&format!("{COOKIE}={second}")).is_some(), "the phone stays logged in");
    }

    #[test]
    fn a_cookie_is_read_by_its_whole_name() {
        assert_eq!(cookie("a=1; witchlight_session=abc; b=2", COOKIE), Some("abc"));
        assert_eq!(cookie("witchlight_session=abc", COOKIE), Some("abc"));
        assert_eq!(cookie("  witchlight_session = abc  ", COOKIE), Some("abc"));

        // A name this one merely ends with must not answer for it.
        assert_eq!(cookie("not_witchlight_session=abc", COOKIE), None);
        assert_eq!(cookie("witchlight_session_other=abc", COOKIE), None);
        assert_eq!(cookie("", COOKIE), None);
        assert_eq!(cookie("witchlight_session", COOKIE), None);
    }
}
