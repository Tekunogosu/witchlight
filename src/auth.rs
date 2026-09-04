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
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

use crate::config::Rules;
use crate::log::{said, warned};
use crate::store::{Session, Store};

/// How long a login link is worth following. Long enough to notice the message
/// and click it, short enough that one left in a chat log is worth nothing.
const LINK_GOOD_FOR: Duration = Duration::from_secs(10 * 60);

/// How often a browser seen again has that written down. A session is read on
/// every request and a write on every request would be the map's busiest
/// table; one an hour per browser is what "last seen" is worth.
const SEEN_WRITTEN_EVERY: Duration = Duration::from_secs(60 * 60);

/// The cookie a session travels in.
pub const COOKIE: &str = "witchlight_session";

/// How the operator wants browsers kept, out of the settings file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Keeping {
    /// How long a browser stays known after it was last seen. `None` is for
    /// ever: until it logs out or the operator forgets everybody.
    pub good_for: Option<Duration>,
    /// Whether a start forgets every browser that was logged in.
    pub reset_on_restart: bool,
}

impl Keeping {
    /// For ever, and kept across a restart. What a settings file that says
    /// nothing means.
    pub const FOREVER: Self = Self { good_for: None, reset_on_restart: false };

    #[must_use]
    pub fn from_rules(rules: &Rules) -> Self {
        Self {
            good_for: (rules.session_hours > 0)
                .then(|| Duration::from_secs(rules.session_hours.saturating_mul(60 * 60))),
            reset_on_restart: rules.sessions_reset_on_restart,
        }
    }
}

/// The `Set-Cookie` that takes a seat away again. What a logout leaves behind.
#[must_use]
pub fn unseat() -> String {
    format!("{COOKIE}=; Path=/; Max-Age=0; SameSite=Lax")
}

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

/// One browser that followed a link.
struct Browser {
    who: Who,
    /// When it was last seen, which is what its lifetime is counted from.
    seen: SystemTime,
    /// The last-seen time the database holds, so that a browser seen every
    /// second is written once an hour rather than once a second.
    written: SystemTime,
}

/// Every login link waiting to be followed, and every browser that followed one.
///
/// The browsers are kept in the map's database as well as here, so that a
/// restart of the service — which every restart of the game server is — does
/// not log everybody out. Here is what answers a request; the database is what
/// the next start reads. Links are here alone: they are worth ten minutes.
pub struct Sessions {
    /// Kept apart from the sessions on purpose: these are spent on use, and one
    /// table with two lifetimes in it is one table with two meanings.
    links: Mutex<HashMap<String, Held>>,
    browsers: Mutex<HashMap<String, Browser>>,
    keeping: Keeping,
    /// Where logins outlive the process. None in a test that wants nothing kept.
    store: Option<Arc<Store>>,
}

impl Default for Sessions {
    fn default() -> Self {
        Self::new()
    }
}

impl Sessions {
    /// Kept for ever and in memory alone.
    #[must_use]
    pub fn new() -> Self {
        Self::with(Keeping::FOREVER, None)
    }

    fn with(keeping: Keeping, store: Option<Arc<Store>>) -> Self {
        Self {
            links: Mutex::new(HashMap::new()),
            browsers: Mutex::new(HashMap::new()),
            keeping,
            store,
        }
    }

    /// The browsers the database remembers, or none of them where the operator
    /// asked for a restart to forget everybody.
    ///
    /// A login that has lapsed since the last start is dropped here, and the
    /// row with it: that is the one write a start makes, and only where there
    /// is something to forget.
    pub fn load(store: Arc<Store>, keeping: Keeping) -> crate::error::Result<Self> {
        let sessions = Self::with(keeping, Some(Arc::clone(&store)));

        if keeping.reset_on_restart {
            let forgotten = store.clear_sessions()?;
            if forgotten > 0 {
                said(format_args!("{forgotten} login(s) forgotten, as the settings ask on a start"));
            }
            return Ok(sessions);
        }

        let now = SystemTime::now();
        let mut kept = 0;
        let mut lapsed = 0;
        if let Ok(mut browsers) = sessions.browsers.lock() {
            for session in store.sessions()? {
                if sessions.lapsed(session.seen, now) {
                    store.delete_session(&session.word)?;
                    lapsed += 1;
                    continue;
                }
                browsers.insert(
                    session.word,
                    Browser {
                        who: Who { uid: session.uid, name: session.name },
                        seen: session.seen,
                        written: session.seen,
                    },
                );
                kept += 1;
            }
        }
        if kept > 0 || lapsed > 0 {
            said(format_args!("{kept} login(s) kept from before, {lapsed} lapsed"));
        }
        Ok(sessions)
    }

    /// Whether a browser last seen then is no longer known now.
    fn lapsed(&self, seen: SystemTime, now: SystemTime) -> bool {
        match self.keeping.good_for {
            Some(good_for) => now.duration_since(seen).is_ok_and(|since| since >= good_for),
            None => false,
        }
    }

    /// The `Set-Cookie` that seats a browser, for as long as the session behind
    /// it: a cookie that outlived its session would log somebody out without
    /// saying so, and one that died first would throw away a session still kept.
    /// A session kept for ever is a cookie with no age at all, which a browser
    /// keeps until told otherwise.
    ///
    /// `HttpOnly` because no script on the page has any use for the word, and
    /// `SameSite=Lax` because the only thing that should arrive carrying it is
    /// somebody following a link to this map themselves.
    ///
    /// Not `Secure`: this is served over plain HTTP on a LAN as often as not, and a
    /// cookie a browser refuses to send is a login that silently never works. An
    /// operator putting the map on the internet puts TLS in front of it, and that is
    /// the same place the flag belongs.
    #[must_use]
    pub fn seat(&self, session: &str) -> String {
        let age = match self.keeping.good_for {
            Some(good_for) => format!("Max-Age={}; ", good_for.as_secs()),
            None => String::new(),
        };
        format!("{COOKIE}={session}; Path=/; {age}HttpOnly; SameSite=Lax")
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
        let now = SystemTime::now();
        let mut browsers = self.browsers.lock().ok()?;
        browsers.retain(|_, browser| !self.lapsed(browser.seen, now));
        self.write(|store| {
            store.put_session(&Session {
                word: word.clone(),
                uid: held.who.uid.clone(),
                name: held.who.name.clone(),
                seen: now,
            })
        });
        browsers.insert(word.clone(), Browser { who: held.who, seen: now, written: now });
        Some(word)
    }

    /// Who a request's cookies say it is, or nobody.
    ///
    /// Seeing somebody puts their session's clock back to the full term, so a map
    /// that is used stays open and one that is forgotten does not.
    pub fn who(&self, cookies: &str) -> Option<Who> {
        let word = cookie(cookies, COOKIE)?;
        let now = SystemTime::now();
        let mut browsers = self.browsers.lock().ok()?;
        let browser = browsers.get_mut(word)?;
        if self.lapsed(browser.seen, now) {
            browsers.remove(word);
            self.write(|store| store.delete_session(word));
            return None;
        }
        browser.seen = now;
        if now.duration_since(browser.written).is_ok_and(|since| since >= SEEN_WRITTEN_EVERY) {
            browser.written = now;
            self.write(|store| store.touch_session(word, now));
        }
        Some(browser.who.clone())
    }

    /// Forgets one browser. What a logout is.
    pub fn forget(&self, cookies: &str) {
        let Some(word) = cookie(cookies, COOKIE) else {
            return;
        };
        if self.browsers.lock().is_ok_and(|mut browsers| browsers.remove(word).is_some()) {
            self.write(|store| store.delete_session(word));
        }
    }

    /// One write to the database, where there is one. A write that fails is
    /// said and survived: the login still works until the next restart, which
    /// is what it always did.
    fn write(&self, writing: impl FnOnce(&Store) -> crate::error::Result<()>) {
        if let Some(Err(error)) = self.store.as_deref().map(writing) {
            warned(format_args!("a login could not be written down: {error}"));
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

    fn hours(hours: u64) -> Keeping {
        Keeping { good_for: Some(Duration::from_secs(hours * 60 * 60)), reset_on_restart: false }
    }

    fn cookie_of(session: &str) -> String {
        format!("{COOKIE}={session}")
    }

    #[test]
    fn a_seat_lasts_as_long_as_the_session_behind_it() {
        // A cookie that outlives its session logs somebody out without saying so;
        // one that dies first throws away a session still being kept.
        let seated = Sessions::with(hours(12), None).seat("abc");
        assert!(seated.contains(&format!("Max-Age={}", 12 * 60 * 60)));
        assert_eq!(cookie(&seated, COOKIE), Some("abc"));
        assert!(seated.contains("HttpOnly"), "no script has any use for the word");

        // For ever is a cookie with no age at all.
        let forever = Sessions::new().seat("abc");
        assert!(!forever.contains("Max-Age"), "{forever}");
        assert_eq!(cookie(&forever, COOKIE), Some("abc"));

        assert!(unseat().contains("Max-Age=0"));
        assert_eq!(cookie(&unseat(), COOKIE), Some(""));
    }

    #[test]
    fn the_settings_say_how_long_and_whether_a_restart_forgets() {
        let rules = crate::state::testing::rules(false);
        assert_eq!(Keeping::from_rules(&rules), Keeping::FOREVER, "0 hours is for ever");

        let rules = Rules { session_hours: 36, sessions_reset_on_restart: true, ..rules };
        assert_eq!(
            Keeping::from_rules(&rules),
            Keeping { good_for: Some(Duration::from_secs(36 * 60 * 60)), reset_on_restart: true }
        );
    }

    #[test]
    fn a_login_outlives_a_restart() {
        let store = Arc::new(Store::in_memory());
        let session = {
            let sessions = Sessions::load(Arc::clone(&store), Keeping::FOREVER).expect("a first start");
            sessions.redeem(&sessions.mint(ada())).expect("ada logs in")
        };

        let again = Sessions::load(Arc::clone(&store), Keeping::FOREVER).expect("a restart");
        assert_eq!(again.who(&cookie_of(&session)), Some(ada()), "the cookie still works");

        again.forget(&cookie_of(&session));
        let third = Sessions::load(store, Keeping::FOREVER).expect("another restart");
        assert!(third.who(&cookie_of(&session)).is_none(), "a logout is kept as well");
    }

    #[test]
    fn a_restart_forgets_everybody_when_the_settings_ask() {
        let store = Arc::new(Store::in_memory());
        let session = {
            let sessions = Sessions::load(Arc::clone(&store), Keeping::FOREVER).expect("a first start");
            sessions.redeem(&sessions.mint(ada())).expect("ada logs in")
        };

        let reset = Keeping { good_for: None, reset_on_restart: true };
        let again = Sessions::load(Arc::clone(&store), reset).expect("a restart");
        assert!(again.who(&cookie_of(&session)).is_none());
        assert!(store.sessions().expect("the table").is_empty(), "and the rows are gone");
    }

    #[test]
    fn a_login_lapses_after_its_hours_and_never_on_zero() {
        let store = Arc::new(Store::in_memory());
        let long_ago = SystemTime::now() - Duration::from_secs(48 * 60 * 60);
        let who = ada();
        store
            .put_session(&Session { word: "old".to_owned(), uid: who.uid.clone(), name: who.name.clone(), seen: long_ago })
            .expect("a login from two days ago");

        let forever = Sessions::load(Arc::clone(&store), Keeping::FOREVER).expect("a start");
        assert_eq!(forever.who(&cookie_of("old")), Some(who.clone()), "0 hours never lapses");

        // Being seen just now was written down, an hour and more after the
        // last time; the stale row is put back for the setting that minds.
        store
            .put_session(&Session { word: "old".to_owned(), uid: who.uid.clone(), name: who.name.clone(), seen: long_ago })
            .expect("the same login, two days old again");
        let day = Sessions::load(Arc::clone(&store), hours(24)).expect("a start");
        assert!(day.who(&cookie_of("old")).is_none(), "two days is past one");
        assert!(store.sessions().expect("the table").is_empty(), "and the row went with it");

        let week = Sessions::load(store, hours(24 * 7)).expect("a start");
        assert!(week.who(&cookie_of("old")).is_none(), "gone is gone, whatever the next setting says");
    }

    #[test]
    fn being_seen_is_written_down_no_more_than_hourly() {
        let store = Arc::new(Store::in_memory());
        let sessions = Sessions::load(Arc::clone(&store), Keeping::FOREVER).expect("a start");
        let session = sessions.redeem(&sessions.mint(ada())).expect("ada logs in");
        let written = store.sessions().expect("the table")[0].seen;

        for _ in 0..3 {
            assert!(sessions.who(&cookie_of(&session)).is_some());
        }
        assert_eq!(store.sessions().expect("the table")[0].seen, written, "seen again within the hour is not a write");

        // An hour on, one request is one write.
        if let Ok(mut browsers) = sessions.browsers.lock() {
            let browser = browsers.get_mut(&session).expect("held");
            browser.written = written - SEEN_WRITTEN_EVERY;
        }
        assert!(sessions.who(&cookie_of(&session)).is_some());
        assert!(store.sessions().expect("the table")[0].seen >= written, "and now it is");
        assert!(store.sessions().expect("the table")[0].seen > written - Duration::from_secs(1));
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
