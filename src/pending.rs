//! Markers asked for on the web, waiting for the game to make them.
//!
//! The two halves talk one way. The mod posts to this service and reads what it
//! answers; nothing here can reach into a game server, which may not even be on
//! the same machine. So a marker somebody types into the map's form is not sent
//! to the game — it is held here until the mod collects it, which it does on the
//! tick that already posts positions.
//!
//! What is held is small and short lived, so it is held in memory. A service that
//! restarts loses whatever had not been collected, which costs one form; writing
//! every marker to disk on the way past would cost a write per marker to save a
//! case that already ends in the person seeing their marker did not appear.
//!
//! A marker is named here rather than by the game. The browser that asked has to
//! recognise its own marker among everybody else's when it arrives, and a name
//! minted at the moment of asking is the only thing both ends can agree on before
//! the marker exists. The mod makes the waypoint under that same name.

use std::sync::Mutex;

/// How many may wait to be collected.
///
/// The mod empties this every couple of seconds, so anything approaching this is
/// a game server that is not running rather than a busy map. Bounded because the
/// queue is filled by anyone with a session and drained by something that may
/// never come back.
const MOST_WAITING: usize = 64;

/// The most a marker's name may be. Longer is a paragraph, not a name.
const LONGEST_TITLE: usize = 128;

/// One marker, as it travels to the mod.
///
/// The same nine fields whether it is a marker being made or one being changed:
/// the form holds all of it either way, and a patch of only what differs would
/// need the far end to work out what "differs" meant against a marker somebody
/// else may have moved since. Two structs said this twice and drifted apart the
/// first time a field was added to one of them.
///
/// PascalCase on the wire, because the only thing that reads it is a C# mod and
/// this is the same wire as the rest of what the two halves say to each other.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct Marker {
    /// The guid the waypoint is, or will be, made under.
    pub key: String,
    /// Whose marker it is, taken from their session and never from the page.
    pub uid: String,
    pub title: String,
    pub icon: String,
    pub color: String,
    pub x: i32,
    pub y: i32,
    pub z: i32,
    /// Whether its owner asked to keep it to themselves.
    pub private: bool,
}

/// What a browser sent. Every field of it is a claim, and none is taken as read.
#[derive(serde::Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Asked {
    #[serde(default)]
    title: String,
    #[serde(default)]
    icon: String,
    #[serde(default)]
    color: String,
    x: i32,
    y: i32,
    z: i32,
    #[serde(default)]
    private: bool,
}

impl Marker {
    /// A new marker a page asked for, checked, and named.
    ///
    /// The owner comes from the session rather than the body: a page that could
    /// say whose marker it is making is a page that can make a marker for anyone.
    ///
    /// The name is minted here rather than by the game. The browser that asked
    /// has to recognise its own marker among everybody else's when it arrives,
    /// and a name agreed before the marker exists is the only thing both ends can
    /// match on.
    pub fn wanted(uid: &str, body: &str) -> Result<Self, &'static str> {
        Self::asked(guid(), uid, body)
    }

    /// A change to a marker that already exists, checked the same way.
    ///
    /// The key arrives from a page and reaches the mod as the identity of a
    /// waypoint, so it is checked for shape rather than taken as a string: a page
    /// must not be able to name a waypoint this map never named.
    pub fn changed(uid: &str, key: &str, body: &str) -> Result<Self, &'static str> {
        if !named(key) {
            return Err("that is not a marker this map made");
        }
        Self::asked(key.to_owned(), uid, body)
    }

    /// The checking both of them share.
    ///
    /// The error is what to tell the person, so each says which field was wrong.
    /// A form that says only "bad request" is a form somebody retypes at random.
    fn asked(key: String, uid: &str, body: &str) -> Result<Self, &'static str> {
        let Ok(asked) = serde_json::from_str::<Asked>(body) else {
            return Err("expected a marker: a title, an icon, a colour and a place");
        };

        let title = asked.title.trim();
        if title.chars().count() > LONGEST_TITLE {
            return Err("that name is too long");
        }

        let Some(icon) = stored_name(asked.icon.trim()) else {
            return Err("that is not one of the marker pictures");
        };

        let Some(color) = css_colour(asked.color.trim()) else {
            return Err("a colour is six hex digits behind a hash");
        };

        Ok(Self {
            key,
            uid: uid.to_owned(),
            title: title.to_owned(),
            icon,
            color,
            x: asked.x,
            y: asked.y,
            z: asked.z,
            private: asked.private,
        })
    }
}

/// One marker somebody asked to be taken away.
///
/// A key and whose ask it was, and nothing else. A removal names a waypoint
/// rather than describing one, so the nine fields a marker carries would be nine
/// fields kept in step for a reader that never looks at them — and the mod reads
/// the waypoint itself before it removes anything anyway.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct Gone {
    /// The guid of the waypoint to take away.
    pub key: String,
    /// Who asked, taken from their session. The mod decides whether they may.
    pub uid: String,
}

impl Gone {
    /// A removal a page asked for, checked the way a change is.
    ///
    /// The key reaches the mod as the identity of a waypoint, so it is checked
    /// for shape rather than taken as a string: a page must not be able to name
    /// a waypoint this map never named.
    pub fn asked(uid: &str, key: &str) -> Result<Self, &'static str> {
        if !named(key) {
            return Err("that is not a marker this map made");
        }
        Ok(Self { key: key.to_owned(), uid: uid.to_owned() })
    }
}

/// Everything waiting, in the shape the mod collects it in.
#[derive(Debug, Default, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct Collected {
    pub make: Vec<Marker>,
    pub change: Vec<Marker>,
    pub remove: Vec<Gone>,
}

/// Every marker asked for and not yet collected.
#[derive(Default)]
pub struct Pending {
    waiting: Mutex<Collected>,
}

impl Pending {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Holds one new marker for the mod to collect. False where there is no room,
    /// which is a game server that has stopped collecting.
    pub fn want(&self, wanted: Marker) -> bool {
        self.hold(|waiting| waiting.make.push(wanted))
    }

    /// Holds one change. Bounded with the rest: the three share a queue because
    /// they share the one thing that empties it.
    pub fn change(&self, edit: Marker) -> bool {
        self.hold(|waiting| waiting.change.push(edit))
    }

    /// Holds one removal.
    pub fn remove(&self, gone: Gone) -> bool {
        self.hold(|waiting| waiting.remove.push(gone))
    }

    /// Room for one more, and the putting of it. The bound is over all three
    /// lists, because what fills them is one page and what empties them is one
    /// ask.
    fn hold(&self, put: impl FnOnce(&mut Collected)) -> bool {
        let Ok(mut waiting) = self.waiting.lock() else {
            return false;
        };
        if waiting.held() >= MOST_WAITING {
            return false;
        }
        put(&mut waiting);
        true
    }

    /// Gives the mod everything waiting, and keeps none of it.
    ///
    /// Emptied on collection rather than on confirmation. A reply lost on the way
    /// back loses what was in it, which is one form to fill in again; holding
    /// each until the mod said it was done would put the same marker on the map
    /// twice every time an answer went missing.
    pub fn take(&self) -> Collected {
        self.waiting.lock().map(|mut waiting| std::mem::take(&mut *waiting)).unwrap_or_default()
    }

    /// How many are waiting.
    ///
    /// Told to the page, so a form whose marker has not appeared can say whether
    /// the game server has stopped collecting or is merely slow. Those look the
    /// same from a browser and are not the same problem.
    #[must_use]
    pub fn waiting(&self) -> usize {
        self.waiting.lock().map_or(0, |waiting| waiting.held())
    }
}

impl Collected {
    /// How many asks this is, whatever kind each of them is.
    fn held(&self) -> usize {
        self.make.len() + self.change.len() + self.remove.len()
    }
}

/// A fresh name for a marker, shaped the way the game shapes its own.
///
/// The game writes a waypoint's guid with `Guid.NewGuid().ToString()` and this
/// name is used as one verbatim, so it is spelled the same way. Nothing parses it
/// today; a name that would not parse is a trap left for whatever does.
fn guid() -> String {
    let word = crate::random::word(16);
    format!(
        "{}-{}-{}-{}-{}",
        &word[0..8],
        &word[8..12],
        &word[12..16],
        &word[16..20],
        &word[20..32]
    )
}

/// Whether this is a name this map hands out: a guid, as [`guid`] mints them.
fn named(key: &str) -> bool {
    key.len() == 36
        && key
            .split('-')
            .map(str::len)
            .eq([8, 4, 4, 4, 12])
        && key.chars().all(|c| c.is_ascii_hexdigit() || c == '-')
}

/// A colour a browser sent, as the mod will store it. Lowercased, so the same
/// colour typed two ways is one colour.
fn css_colour(said: &str) -> Option<String> {
    let digits = said.strip_prefix('#')?;
    if digits.len() != 6 || !digits.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    Some(format!("#{}", digits.to_ascii_lowercase()))
}

/// An icon name that could be a file this service serves.
///
/// Asked of the one place that decides it, rather than spelled out again: a name
/// arriving from a page reaches a path, and a rule about paths with two copies is
/// a rule with two answers. It had two, and they disagreed about capitals — a
/// marker made with `Gravestone` was stored happily and then drawn as nothing,
/// because the address that would serve the picture refuses that name.
///
/// Empty is the game's own default rather than a refusal: a form nobody chose a
/// picture on is a plain marker, not an error.
fn stored_name(said: &str) -> Option<String> {
    if said.is_empty() {
        return Some("circle".to_owned());
    }
    crate::urls::is_stored_name(said).then(|| said.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    const BODY: &str = r##"
        {"Title":"home","Icon":"home","Color":"#C8772E","X":10,"Y":110,"Z":-4,"Private":true}
    "##;

    #[test]
    fn a_marker_is_taken_as_the_form_filled_it_in() {
        let wanted = Marker::wanted("uid-ada", BODY).expect("a whole marker");
        assert_eq!(wanted.uid, "uid-ada");
        assert_eq!(wanted.title, "home");
        assert_eq!(wanted.icon, "home");
        assert_eq!((wanted.x, wanted.y, wanted.z), (10, 110, -4));
        assert!(wanted.private);

        // Lowercased on the way in, so what the mod stores and what the page next
        // reads back are the same six digits.
        assert_eq!(wanted.color, "#c8772e");
    }

    #[test]
    fn the_owner_is_the_session_and_never_the_page() {
        // A page saying whose marker it is making is a page making one for anyone.
        let claiming = r##"{"Uid":"uid-bob","OwnerUid":"uid-bob","Color":"#ffffff","X":0,"Y":0,"Z":0}"##;
        let wanted = Marker::wanted("uid-ada", claiming).expect("a marker");
        assert_eq!(wanted.uid, "uid-ada");
    }

    /// The claim the icon field rests on.
    ///
    /// A picture taken here is a picture the map is about to be asked for at
    /// `/icons/{name}.svg`, so a name this accepts and that address refuses is a
    /// marker drawn as a hole. The two rules were written out separately and
    /// disagreed about capitals, which is exactly that.
    #[test]
    fn every_picture_this_takes_is_one_the_map_can_serve() {
        for name in ["circle", "gravestone", "star1", "my-mod_icon2", "Gravestone", "a/b", ""] {
            let body = format!(r##"{{"Icon":"{name}","Color":"#ffffff","X":0,"Y":0,"Z":0}}"##);
            let Ok(taken) = Marker::wanted("uid-ada", &body) else {
                continue;
            };
            assert_eq!(
                crate::urls::icon_name(&format!("/icons/{}.svg", taken.icon)),
                Some(taken.icon.as_str()),
                "{name:?} was taken as {:?}, which the icon route will not serve",
                taken.icon
            );
        }
    }

    #[test]
    fn a_marker_with_no_picture_gets_the_game_s_own() {
        let bare = r##"{"Color":"#ffffff","X":0,"Y":0,"Z":0}"##;
        assert_eq!(Marker::wanted("uid-ada", bare).expect("a marker").icon, "circle");
    }

    #[test]
    fn nothing_that_would_not_draw_is_taken() {
        for (body, why) in [
            (r##"{"Color":"#ffffff"}"##, "no place"),
            (r##"{"Color":"ffffff","X":0,"Y":0,"Z":0}"##, "a colour with no hash"),
            (r##"{"Color":"#fff","X":0,"Y":0,"Z":0}"##, "a short colour"),
            (r##"{"Color":"#gggggg","X":0,"Y":0,"Z":0}"##, "a colour that is not hex"),
            (r##"{"Icon":"../secret","Color":"#ffffff","X":0,"Y":0,"Z":0}"##, "a path"),
            (r##"{"Icon":"a/b","Color":"#ffffff","X":0,"Y":0,"Z":0}"##, "a slash"),
            (r##"{"Icon":"Gravestone","Color":"#ffffff","X":0,"Y":0,"Z":0}"##, "a capital"),
            ("not json at all", "not json"),
        ] {
            assert!(Marker::wanted("uid-ada", body).is_err(), "{why} must not be taken");
        }
    }

    #[test]
    fn a_name_longer_than_a_name_is_refused() {
        let long = "n".repeat(LONGEST_TITLE + 1);
        let body = format!(r##"{{"Title":"{long}","Color":"#ffffff","X":0,"Y":0,"Z":0}}"##);
        assert!(Marker::wanted("uid-ada", &body).is_err());

        let just = "n".repeat(LONGEST_TITLE);
        let body = format!(r##"{{"Title":"{just}","Color":"#ffffff","X":0,"Y":0,"Z":0}}"##);
        assert!(Marker::wanted("uid-ada", &body).is_ok());
    }

    const KEY: &str = "9e5738f0-303a-673d-a328-f19e0d08e7d1";

    #[test]
    fn what_is_held_is_given_up_once() {
        let pending = Pending::new();
        let wanted = Marker::wanted("uid-ada", BODY).expect("a marker");
        let edit = Marker::changed("uid-ada", KEY, BODY).expect("a change");
        assert!(pending.want(wanted.clone()));
        assert!(pending.change(edit.clone()));
        assert_eq!(pending.waiting(), 2);

        let gone = Gone::asked("uid-ada", KEY).expect("a removal");
        assert!(pending.remove(gone.clone()));
        assert_eq!(pending.waiting(), 3);

        let taken = pending.take();
        assert_eq!(taken.make, vec![wanted]);
        assert_eq!(taken.change, vec![edit]);
        assert_eq!(taken.remove, vec![gone]);
        assert_eq!(pending.waiting(), 0, "collecting empties it");
        let again = pending.take();
        assert!(
            again.make.is_empty() && again.change.is_empty() && again.remove.is_empty(),
            "and a second collection finds nothing"
        );
    }

    #[test]
    fn a_removal_names_a_marker_and_who_asked() {
        let gone = Gone::asked("uid-ada", KEY).expect("a removal");
        assert_eq!(gone.key, KEY);
        assert_eq!(gone.uid, "uid-ada", "the owner is the session and never the page");
    }

    #[test]
    fn only_a_name_this_map_hands_out_can_be_removed() {
        // The same rule a change is held to, and for the same reason: what
        // arrives here reaches the mod as the identity of a waypoint.
        for key in ["", "not-a-guid", "../../etc/passwd", "9e5738f0303a673da328f19e0d08e7d1"] {
            assert!(Gone::asked("uid-ada", key).is_err(), "{key:?} must not be taken");
        }
    }

    #[test]
    fn a_change_is_checked_the_way_a_new_marker_is() {
        let edit = Marker::changed("uid-ada", KEY, BODY).expect("a change");
        assert_eq!(edit.key, KEY);
        assert_eq!(edit.uid, "uid-ada");
        assert_eq!(edit.color, "#c8772e");

        // The same refusals, because it is the same form behind it.
        let bad = r##"{"Color":"nonsense","X":0,"Y":0,"Z":0}"##;
        assert!(Marker::changed("uid-ada", KEY, bad).is_err());
    }

    #[test]
    fn only_a_name_this_map_hands_out_can_be_changed() {
        // What arrives here reaches the mod as the identity of a waypoint, so a
        // page must not be able to name one this map never named.
        for key in [
            "",
            "not-a-guid",
            "../../etc/passwd",
            "9e5738f0303a673da328f19e0d08e7d1",
            "9e5738f0-303a-673d-a328-f19e0d08e7d",
            "9e5738g0-303a-673d-a328-f19e0d08e7d1",
        ] {
            assert!(Marker::changed("uid-ada", key, BODY).is_err(), "{key:?} must not be taken");
        }
    }

    #[test]
    fn a_queue_nobody_is_collecting_does_not_grow_without_end() {
        let pending = Pending::new();
        for _ in 0..MOST_WAITING {
            assert!(pending.want(Marker::wanted("uid-ada", BODY).expect("a marker")));
        }
        assert!(
            !pending.want(Marker::wanted("uid-ada", BODY).expect("a marker")),
            "a game server that stopped collecting must not fill this process"
        );
        assert!(
            !pending.change(Marker::changed("uid-ada", KEY, BODY).expect("a change")),
            "and the bound is over all three, since one page fills them and one ask empties them"
        );
        assert!(!pending.remove(Gone::asked("uid-ada", KEY).expect("a removal")));
    }

    #[test]
    fn every_marker_is_named_differently_and_shaped_like_a_guid() {
        let mut seen = std::collections::HashSet::new();
        for _ in 0..100 {
            let key = Marker::wanted("uid-ada", BODY).expect("a marker").key;
            assert_eq!(key.len(), 36, "{key} is not a guid\'s length");
            assert_eq!(
                key.split('-').map(str::len).collect::<Vec<_>>(),
                vec![8, 4, 4, 4, 12],
                "{key} is not a guid\'s shape"
            );
            assert!(seen.insert(key), "a marker\'s name came round twice");
        }
    }
}
