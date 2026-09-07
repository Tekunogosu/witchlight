//! Queues what the web asked for until the game does it.
//!
//! The queue holds markers and the land claims somebody drew a rectangle for.
//! Both kinds share one queue because one thing empties it: the mod collects on
//! the tick that already posts positions. A second queue would mean a second
//! round trip every two seconds to learn it was empty.
//!
//! The two halves talk one way. The mod posts to this service and reads what it
//! answers. Nothing here can reach into a game server, which may not even be on
//! the same machine. A marker somebody types into the map's form is therefore
//! held here until the mod collects it.
//!
//! The queued data is small and short lived, so it stays in memory. A service
//! that restarts loses whatever had not been collected, which costs one form.
//! Writing every marker to disk would cost a write per marker to save a case
//! that already ends in the person seeing their marker did not appear.
//!
//! Markers are named here rather than by the game. The browser that asked must
//! recognise its own marker among everybody else's when it arrives, and a name
//! minted at the moment of asking is the only thing both ends can agree on
//! before the marker exists. The mod makes the waypoint under that same name.

use std::sync::Mutex;

/// How many requests may wait to be collected.
///
/// The mod empties the queue every couple of seconds, so a depth near this bound
/// means the game server is not running rather than that the map is busy. The
/// bound exists because anyone with a session fills the queue and something that
/// may never come back drains it.
const MOST_WAITING: usize = 64;

/// The longest a marker's name may be.
const LONGEST_TITLE: usize = 128;

/// Holds one marker as it travels to the mod.
///
/// The same fields carry a marker being made and one being changed. The form
/// holds all of them either way, and sending only what differs would make the
/// far end work out what differs against a marker somebody else may have moved
/// since.
///
/// The wire format is PascalCase, because a C# mod reads it.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct Marker {
    /// The guid the waypoint is or will be made under.
    pub key: String,
    /// The owner, taken from their session and never from the page.
    pub uid: String,
    pub title: String,
    pub icon: String,
    pub color: String,
    pub x: i32,
    pub y: i32,
    pub z: i32,
    /// Keeps the marker visible to its owner alone.
    pub private: bool,
    /// Names which block this marker is about, when the page says so. It is the
    /// pattern of the preset the marker is being made to look like. Empty is the
    /// ordinary case, and means the mod decides by reading the world under the
    /// marker. A page is never trusted to name a block.
    pub block: String,
}

/// Holds what a browser sent. Every field is unvalidated input.
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
    #[serde(default)]
    block: String,
}

impl Marker {
    /// Validates a new marker a page asked for and gives it a name.
    ///
    /// The owner comes from the session rather than the body. A page that could
    /// say whose marker it is making could make a marker for anyone.
    ///
    /// The name is minted here rather than by the game. The browser that asked
    /// must recognise its own marker among everybody else's when it arrives, and
    /// a name agreed before the marker exists is the only thing both ends can
    /// match on.
    pub fn wanted(uid: &str, body: &str) -> Result<Self, &'static str> {
        Self::asked(guid(), uid, body)
    }

    /// Validates a change to a marker that already exists.
    ///
    /// The key arrives from a page and reaches the mod as the identity of a
    /// waypoint, so its shape is checked. A page must not be able to name a
    /// waypoint this map never named.
    pub fn changed(uid: &str, key: &str, body: &str) -> Result<Self, &'static str> {
        if !named(key) {
            return Err("that is not a marker this map made");
        }
        Self::asked(key.to_owned(), uid, body)
    }

    /// Runs the validation both of them share.
    ///
    /// The error text is shown to the person, so each one names the field that
    /// was wrong. A form that says only "bad request" is one somebody retypes at
    /// random.
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

        let Some(color) = crate::util::text::hex_colour(&asked.color) else {
            return Err("a colour is six hex digits behind a hash");
        };

        let Some(block) = block_pattern(asked.block.trim()) else {
            return Err("that is not a block code");
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
            block,
        })
    }
}

/// Holds one marker somebody asked to be taken away.
///
/// It carries a key and who asked, and nothing else. A removal names a waypoint
/// rather than describing one, and the mod reads the waypoint itself before
/// removing anything.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct Gone {
    /// The guid of the waypoint to remove.
    pub key: String,
    /// Who asked, taken from their session. The mod decides whether they may.
    pub uid: String,
}

impl Gone {
    /// Validates a removal a page asked for, the way a change is validated.
    ///
    /// The key reaches the mod as the identity of a waypoint, so its shape is
    /// checked. A page must not be able to name a waypoint this map never named.
    pub fn asked(uid: &str, key: &str) -> Result<Self, &'static str> {
        if !named(key) {
            return Err("that is not a marker this map made");
        }
        Ok(Self { key: key.to_owned(), uid: uid.to_owned() })
    }
}

/// Holds one marker somebody asked to keep in sight on their own map, or to
/// stop keeping.
///
/// It carries a key, who asked, and which way. It says nothing about the marker
/// itself, because a pin changes only what one person's own in-game map shows.
/// That is why it is separate from a change, which is its owner's business and
/// which everybody sees.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct Pin {
    /// The guid of the waypoint to keep in sight.
    pub key: String,
    /// Whose map this is for, taken from their session. The mod decides whether
    /// they may see the marker at all, which is the whole permission.
    pub uid: String,
    /// Keeps the marker in sight, or stops keeping it.
    pub on: bool,
}

impl Pin {
    /// Validates a pin a page asked for, the way a removal is validated.
    ///
    /// The key reaches the mod as the identity of a waypoint, so its shape is
    /// checked. A page must not be able to name a waypoint this map never named.
    pub fn asked(uid: &str, key: &str, on: bool) -> Result<Self, &'static str> {
        if !named(key) {
            return Err("that is not a marker this map made");
        }
        Ok(Self { key: key.to_owned(), uid: uid.to_owned(), on })
    }
}

/// The longest a claim's description may be. It shows in game when somebody
/// walks into the claim, so it is a line rather than a paragraph.
const LONGEST_DESCRIPTION: usize = 128;

/// Holds one land claim somebody drew on the map.
///
/// It carries a rectangle, a depth range and a description. The depth is asked
/// for rather than assumed, even though the map is drawn from above. A role's
/// allowance is measured in cubic metres and depth is most of a claim's volume,
/// so making every claim the full height of the world would give a player a
/// square thirty-two blocks across and no way to ask for a wider, shallower one.
///
/// Nothing here grants permission. The mod decides whether this person may take
/// land, against the game's own privilege, allowance, size and overlap rules.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct Claim {
    /// Who asked, taken from their session and never from the page.
    pub uid: String,
    pub description: String,
    /// The corners, west and north first. They are normalised here so the mod
    /// is never handed an inverted rectangle.
    pub x1: i32,
    pub z1: i32,
    pub x2: i32,
    pub z2: i32,
    /// How far down the claim starts and how far up it reaches, lower first.
    /// The mod clamps it to the world, because only the mod knows how tall the
    /// world is.
    pub y1: i32,
    pub y2: i32,
}

/// Holds what a browser sent for a claim. Every field is unvalidated input.
#[derive(serde::Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Drawn {
    #[serde(default)]
    description: String,
    x1: i32,
    z1: i32,
    x2: i32,
    z2: i32,
    y1: i32,
    y2: i32,
}

impl Claim {
    /// Validates a claim a page asked for and normalises its rectangle.
    ///
    /// The owner comes from the session rather than the body, for the same
    /// reason a marker's does. A page that could say whose claim it is making
    /// could take land in somebody else's name.
    ///
    /// The error text is shown to the person, so each one names the part that
    /// was wrong.
    pub fn drawn(uid: &str, body: &str) -> Result<Self, &'static str> {
        let Ok(asked) = serde_json::from_str::<Drawn>(body) else {
            return Err("expected a claim: two corners and a description");
        };

        let description = asked.description.trim();
        if description.chars().count() > LONGEST_DESCRIPTION {
            return Err("that description is too long");
        }

        // Normalise here rather than at the far end. A rectangle dragged
        // north-west arrives with its corners reversed, and every reader that
        // had to remember which way it was drawn could forget.
        let (x1, x2) = (asked.x1.min(asked.x2), asked.x1.max(asked.x2));
        let (z1, z2) = (asked.z1.min(asked.z2), asked.z1.max(asked.z2));
        if x1 == x2 || z1 == z2 {
            return Err("a claim needs some ground in it");
        }

        Ok(Self {
            uid: uid.to_owned(),
            description: description.to_owned(),
            x1,
            z1,
            x2,
            z2,
            y1: asked.y1.min(asked.y2),
            y2: asked.y1.max(asked.y2),
        })
    }
}

/// The most people one claim may name. It is far past a working set, and stops
/// a page in a loop from growing a claim without end.
const MOST_PERMITTED: usize = 64;

/// Names who a claim lets in beyond its owner.
///
/// People are named rather than identified by uid, because a browser sends a
/// list of player names typed into a box and only the mod can turn one into the
/// uid a claim stores. The mod sends the names back too, so the form shows names
/// rather than a column of base64.
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct Allowed {
    /// The people named, by the name the game knows them by.
    #[serde(default)]
    pub names: Vec<String>,
    /// Lets anybody use what is on this land, such as doors, chests and levers.
    /// This is the game's own `AllowUseEveryone`.
    #[serde(default)]
    pub everyone_uses: bool,
    /// Lets anybody walk across the claim. This is the game's own
    /// `AllowTraverseEveryone`.
    #[serde(default)]
    pub everyone_walks: bool,
}

impl Allowed {
    /// Returns the same list brought inside the bounds this will carry.
    fn sane(mut self) -> Self {
        for name in &mut self.names {
            let trimmed = name.trim();
            if trimmed.chars().count() > LONGEST_NAME {
                *name = trimmed.chars().take(LONGEST_NAME).collect();
            } else if trimmed.len() != name.len() {
                *name = trimmed.to_owned();
            }
        }
        self.names.retain(|name| !name.is_empty());
        self.names.dedup();
        self.names.truncate(MOST_PERMITTED);
        self
    }
}

/// The longest a player name may be. The game's own limit is well under this.
const LONGEST_NAME: usize = 64;

/// Holds a change to a claim that already exists.
///
/// It carries the claim's name and who it lets in, but not the ground it covers.
/// Moving a boundary must be judged against everybody else's claims and against
/// an allowance, and the map cannot show somebody what they would give up.
/// Redrawing a claim makes a new one, which the form already does.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct ClaimEdit {
    /// The claim to change, by the name the mod gave it.
    pub key: String,
    /// Who asked, taken from their session. The mod decides whether they may.
    pub uid: String,
    pub description: String,
    pub allowed: Allowed,
}

/// Holds one claim somebody asked to give up.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct ClaimGone {
    pub key: String,
    /// Who asked, taken from their session. The mod decides whether they may.
    pub uid: String,
}

/// Holds what a browser sent for a change to a claim.
#[derive(serde::Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Edited {
    #[serde(default)]
    description: String,
    #[serde(default)]
    allowed: Allowed,
}

impl ClaimEdit {
    /// Validates a change a page asked for, the way a new claim is validated.
    pub fn asked(uid: &str, key: &str, body: &str) -> Result<Self, &'static str> {
        if !claim_named(key) {
            return Err("that is not a claim this map knows");
        }
        let Ok(edit) = serde_json::from_str::<Edited>(body) else {
            return Err("expected a description and who is allowed");
        };

        let description = edit.description.trim();
        if description.chars().count() > LONGEST_DESCRIPTION {
            return Err("that description is too long");
        }

        Ok(Self {
            key: key.to_owned(),
            uid: uid.to_owned(),
            description: description.to_owned(),
            allowed: edit.allowed.sane(),
        })
    }
}

impl ClaimGone {
    pub fn asked(uid: &str, key: &str) -> Result<Self, &'static str> {
        if !claim_named(key) {
            return Err("that is not a claim this map knows");
        }
        Ok(Self { key: key.to_owned(), uid: uid.to_owned() })
    }
}

/// Reports whether this is a name the mod hands out for a claim.
///
/// The mod derives the name from the claim, because the game gives a claim
/// nothing to be known by. The shape is checked because a key arrives from a page
/// and reaches the mod as the identity of a piece of land.
fn claim_named(key: &str) -> bool {
    !key.is_empty()
        && key.len() <= 32
        && key.bytes().all(|byte| byte.is_ascii_hexdigit())
}

/// Holds everything about claims that is waiting, in the shape the mod collects.
#[derive(Debug, Default, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct ClaimAsks {
    pub make: Vec<Claim>,
    pub change: Vec<ClaimEdit>,
    pub remove: Vec<ClaimGone>,
}

impl ClaimAsks {
    fn held(&self) -> usize {
        self.make.len() + self.change.len() + self.remove.len()
    }
}

/// Holds everything about markers that is waiting.
#[derive(Debug, Default, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct MarkerAsks {
    pub make: Vec<Marker>,
    pub change: Vec<Marker>,
    pub remove: Vec<Gone>,
    pub pin: Vec<Pin>,
}

impl MarkerAsks {
    fn held(&self) -> usize {
        self.make.len() + self.change.len() + self.remove.len() + self.pin.len()
    }
}

/// Holds everything waiting, in the shape the mod collects it in.
#[derive(Debug, Default, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct Collected {
    /// Groups the made, changed and removed requests per kind of thing, because
    /// markers and claims answer the same three verbs.
    pub markers: MarkerAsks,
    pub claims: ClaimAsks,
}

/// Holds every request asked for and not yet collected.
#[derive(Default)]
pub struct Pending {
    waiting: Mutex<Collected>,
}

impl Pending {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Queues one new marker for the mod to collect. Returns false when there
    /// is no room, which means the game server stopped collecting.
    pub fn want(&self, wanted: Marker) -> bool {
        self.hold(|waiting| waiting.markers.make.push(wanted))
    }

    /// Queues one change. It shares the bound with the rest, because one thing
    /// empties them all.
    pub fn change(&self, edit: Marker) -> bool {
        self.hold(|waiting| waiting.markers.change.push(edit))
    }

    /// Queues one removal.
    pub fn remove(&self, gone: Gone) -> bool {
        self.hold(|waiting| waiting.markers.remove.push(gone))
    }

    /// Queues one marker somebody asked to keep in sight, or to stop keeping.
    pub fn pin(&self, pin: Pin) -> bool {
        self.hold(|waiting| waiting.markers.pin.push(pin))
    }

    /// Queues one land claim, under the same bound as the rest.
    pub fn claim(&self, drawn: Claim) -> bool {
        self.hold(|waiting| waiting.claims.make.push(drawn))
    }

    /// Queues one change to a claim.
    pub fn claim_edit(&self, edit: ClaimEdit) -> bool {
        self.hold(|waiting| waiting.claims.change.push(edit))
    }

    /// Queues one claim somebody asked to give up.
    pub fn claim_gone(&self, gone: ClaimGone) -> bool {
        self.hold(|waiting| waiting.claims.remove.push(gone))
    }

    /// Adds one request if there is room. The bound covers every list together,
    /// because one page fills them and one collection empties them.
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

    /// Returns everything waiting and empties the queue.
    ///
    /// The queue empties on collection rather than on confirmation. A reply lost
    /// on the way back loses what was in it, which costs one form to fill in
    /// again. Holding each request until the mod confirmed would put the same
    /// marker on the map twice every time an answer went missing.
    pub fn take(&self) -> Collected {
        self.waiting.lock().map(|mut waiting| std::mem::take(&mut *waiting)).unwrap_or_default()
    }

    /// Returns how many requests are waiting.
    ///
    /// The page is told this, so a form whose marker has not appeared can say
    /// whether the game server stopped collecting or is merely slow. Those look
    /// the same from a browser and are different problems.
    #[must_use]
    pub fn waiting(&self) -> usize {
        self.waiting.lock().map_or(0, |waiting| waiting.held())
    }
}

impl Collected {
    /// Returns how many requests this holds, of every kind.
    fn held(&self) -> usize {
        self.markers.held() + self.claims.held()
    }
}

/// Mints a fresh marker name, shaped the way the game shapes its own.
///
/// The game writes a waypoint's guid with `Guid.NewGuid().ToString()`, and this
/// name is used as one verbatim, so it is spelled the same way. Nothing parses it
/// today, but a name that would not parse would break whatever does.
fn guid() -> String {
    let word = crate::util::random::word(16);
    format!(
        "{}-{}-{}-{}-{}",
        &word[0..8],
        &word[8..12],
        &word[12..16],
        &word[16..20],
        &word[20..32]
    )
}

/// Reports whether this is a name this map hands out, meaning a guid as
/// [`guid`] mints them.
fn named(key: &str) -> bool {
    key.len() == 36
        && key
            .split('-')
            .map(str::len)
            .eq([8, 4, 4, 4, 12])
        && key.chars().all(|c| c.is_ascii_hexdigit() || c == '-')
}

/// The longest a block code may be. A code is a domain, a name and its
/// variants.
const LONGEST_CODE: usize = 128;

/// Validates which block a marker is about, as a browser sent it.
///
/// The value is a block code such as `game:rock-granite`, or a preset's pattern,
/// which is a code with `*` standing for a run of characters. Nothing here
/// reaches a path or a query, so the check is about shape rather than safety. A
/// value that could not be a block code is one nothing will ever match.
///
/// Empty is accepted. It is the ordinary case, where the page says nothing about
/// the block and the mod reads the world under the marker.
fn block_pattern(said: &str) -> Option<String> {
    if said.is_empty() {
        return Some(String::new());
    }
    if said.chars().count() > LONGEST_CODE {
        return None;
    }
    said.bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || b"-_:*./".contains(&byte))
        .then(|| said.to_ascii_lowercase())
}

/// Validates an icon name against the rule for files this service serves.
///
/// It defers to the one place that decides that rule rather than restating it. A
/// name arriving from a page becomes a path, and two copies of a path rule can
/// disagree. A marker made with `Gravestone` would be stored and then drawn as
/// nothing, because the address that serves the picture refuses that name.
///
/// Empty is accepted and means the game's own default. A form nobody chose a
/// picture on is a plain marker.
fn stored_name(said: &str) -> Option<String> {
    if said.is_empty() {
        return Some("circle".to_owned());
    }
    crate::util::urls::is_stored_name(said).then(|| said.to_owned())
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

        // Lowercase on the way in, so what the mod stores and what the page
        // reads back are the same six digits.
        assert_eq!(wanted.color, "#c8772e");
    }

    #[test]
    fn the_owner_is_the_session_and_never_the_page() {
        // A page that could say whose marker it is making could make one for
        // anyone.
        let claiming = r##"{"Uid":"uid-bob","OwnerUid":"uid-bob","Color":"#ffffff","X":0,"Y":0,"Z":0}"##;
        let wanted = Marker::wanted("uid-ada", claiming).expect("a marker");
        assert_eq!(wanted.uid, "uid-ada");
    }

    /// Checks that the icon field accepts exactly the names the icon route
    /// serves.
    ///
    /// An icon accepted here is one the map is about to be asked for at
    /// `/icons/{name}.svg`. A name this accepts and that address refuses would
    /// draw the marker as a hole.
    #[test]
    fn every_picture_this_takes_is_one_the_map_can_serve() {
        for name in ["circle", "gravestone", "star1", "my-mod_icon2", "Gravestone", "a/b", ""] {
            let body = format!(r##"{{"Icon":"{name}","Color":"#ffffff","X":0,"Y":0,"Z":0}}"##);
            let Ok(taken) = Marker::wanted("uid-ada", &body) else {
                continue;
            };
            assert_eq!(
                crate::util::urls::icon_name(&format!("/icons/{}.svg", taken.icon)),
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
    fn a_pin_names_a_marker_this_map_made_and_says_which_way() {
        let kept = Pin::asked("uid-ada", KEY, true).expect("a pin");
        assert_eq!(kept.uid, "uid-ada");
        assert_eq!(kept.key, KEY);
        assert!(kept.on);

        assert!(!Pin::asked("uid-ada", KEY, false).expect("a pin").on);
        // The key reaches the mod as the identity of a waypoint, so a page must
        // not be able to name one this map never named.
        assert!(Pin::asked("uid-ada", "../secret", true).is_err());
        assert!(Pin::asked("uid-ada", "", true).is_err());
    }

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
        assert_eq!(taken.markers.make, vec![wanted]);
        assert_eq!(taken.markers.change, vec![edit]);
        assert_eq!(taken.markers.remove, vec![gone]);
        assert_eq!(pending.waiting(), 0, "collecting empties it");
        let again = pending.take();
        assert_eq!(again, Collected::default(), "and a second collection finds nothing");
    }

    #[test]
    fn a_removal_names_a_marker_and_who_asked() {
        let gone = Gone::asked("uid-ada", KEY).expect("a removal");
        assert_eq!(gone.key, KEY);
        assert_eq!(gone.uid, "uid-ada", "the owner is the session and never the page");
    }

    #[test]
    fn only_a_name_this_map_hands_out_can_be_removed() {
        // This follows the same rule as a change, because what arrives here
        // reaches the mod as the identity of a waypoint.
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

        // The same refusals apply, because the same form is behind it.
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

    /// Builds what a browser sends for a claim, with the corners it was drawn
    /// between.
    fn drawn(x1: i32, z1: i32, x2: i32, z2: i32) -> String {
        format!(
            r#"{{"Description":"the north field","X1":{x1},"Z1":{z1},"X2":{x2},"Z2":{z2},"Y1":90,"Y2":60}}"#
        )
    }

    #[test]
    fn a_claim_is_squared_up_however_it_was_drawn() {
        // Dragging north-west reverses every corner. Normalising once here
        // spares every reader from remembering which way it was drawn.
        let claim = Claim::drawn("uid-ada", &drawn(60, 80, 20, 10)).expect("a claim");
        assert_eq!((claim.x1, claim.z1, claim.x2, claim.z2), (20, 10, 60, 80));
        assert_eq!((claim.y1, claim.y2), (60, 90), "and the depth with them");

        // Dragging the other way must give the same answer.
        let same = Claim::drawn("uid-ada", &drawn(20, 10, 60, 80)).expect("a claim");
        assert_eq!((same.x1, same.z1, same.x2, same.z2), (20, 10, 60, 80));
    }

    #[test]
    fn a_claims_owner_is_the_session_and_never_the_page() {
        // This follows the same rule as a marker, and is checked separately
        // because the stake is higher. A page that could say whose claim it is
        // making could take land in somebody else's name.
        let body = r#"{"Uid":"uid-bob","Description":"","X1":0,"Z1":0,"X2":10,"Z2":10,"Y1":0,"Y2":9}"#;
        let claim = Claim::drawn("uid-ada", body).expect("a claim");
        assert_eq!(claim.uid, "uid-ada", "what the page said about the owner is ignored");
    }

    #[test]
    fn a_rectangle_with_no_ground_in_it_is_refused() {
        for (x1, z1, x2, z2) in [(5, 5, 5, 40), (5, 5, 40, 5), (5, 5, 5, 5)] {
            assert!(
                Claim::drawn("uid-ada", &drawn(x1, z1, x2, z2)).is_err(),
                "{x1},{z1} to {x2},{z2} is a line or a point, not a claim"
            );
        }
    }

    #[test]
    fn a_description_longer_than_a_line_is_refused() {
        let long = "x".repeat(LONGEST_DESCRIPTION + 1);
        let body = format!(r#"{{"Description":"{long}","X1":0,"Z1":0,"X2":9,"Z2":9,"Y1":0,"Y2":9}}"#);
        assert!(Claim::drawn("uid-ada", &body).is_err());
    }

    #[test]
    fn a_claim_waits_with_the_markers_and_is_collected_with_them() {
        // One bound covers every list, because one page fills them and one
        // collection on one tick empties them.
        let waiting = Pending::new();
        assert!(waiting.claim(Claim::drawn("uid-ada", &drawn(0, 0, 9, 9)).expect("a claim")));
        assert_eq!(waiting.waiting(), 1, "a claim counts against the same bound");

        let taken = waiting.take();
        assert_eq!(taken.claims.make.len(), 1);
        assert_eq!(taken.claims.make[0].description, "the north field");
        assert_eq!(waiting.waiting(), 0, "and collecting empties it");
    }
}
