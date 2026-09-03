//! The parts of the map that move.
//!
//! Players and markers arrive from the server mod over the API channel and are
//! held here rather than on disk. Positions change every couple of seconds and
//! are worth nothing once they are old; writing them to a file to read them back
//! a moment later was work with no product.
//!
//! Markers are the exception. They change a few times an hour, and they are the
//! one thing worth seeing when the game server is off, so they are written when
//! they arrive and read back at start.
//!
//! Nothing here parses either payload for what it means to a browser: the mod
//! knows what a waypoint is; this knows that it is a JSON array to hand on,
//! which is the whole of the contract between them for markers and claims.
//! Players are the one exception — [`Live::positions`] reads a position out of
//! the same arrays, because [`crate::pull`] needs to know where people are and
//! the game server already knows that regardless of who a browser may show it
//! to. Nothing downstream of that reads a name, a health bar, or anything else
//! a player's own entry carries — only where they are.
//!
//! Markers arrive already sorted into what anyone may see and what only one
//! person may, because deciding that needs to know what a waypoint is and this
//! does not. The arrays inside are never looked into — they are held as they
//! arrived and handed on, and the most that happens to one is being joined to
//! another end to end.
//!
//! Land claims arrive the same way and are sorted differently, because the
//! question is a different one. A marker is private to whoever made it, so who
//! may see one is answered per marker; a claim is public within the server or it
//! is not, and who may see the lot is answered per person, by a privilege. So the
//! claims travel as one list with the names of everyone the mod says may be sent
//! it, rather than as a copy of that list per person — which on a server with
//! fifty players and a hundred claims would be the same hundred claims fifty
//! times over.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde_json::value::RawValue;

use crate::memory::Group;

/// How long a report of who is online stays believable.
///
/// Without this a game server that stops — crashed, killed, shut down — leaves
/// its last positions on the map forever, and a dot that says someone is standing
/// somewhere is worse than no dot at all.
const PLAYERS_GOOD_FOR: Duration = Duration::from_secs(30);

/// Every marker, as the mod sorted them.
///
/// Held as the text that arrived rather than as anything read out of it. The
/// posted body is kept beside the pieces so that an unchanged post is recognised
/// as unchanged without taking it apart again.
struct Markers {
    body: String,
    /// The colours the game offers, so the page's form can offer the same.
    colors: String,
    /// Markers anyone may see.
    open: String,
    /// Markers only their owner may see, by the uid of that owner.
    owned: HashMap<String, String>,
    /// Which markers each person keeps in sight on their own map in game, by
    /// their uid, as the list of marker names that arrived. Sorted by reader for
    /// the reason the private markers are: a pin is one person's answer about one
    /// marker, and nobody else has any business being handed it.
    pinned: HashMap<String, String>,
}

impl Default for Markers {
    /// Empty arrays rather than empty strings. These are spliced into the body a
    /// browser is handed, and a hole where an array should be is not a map with
    /// no markers on it — it is a page that fails to parse what it was sent.
    fn default() -> Self {
        Self {
            body: String::new(),
            colors: "[]".to_owned(),
            open: "[]".to_owned(),
            owned: HashMap::new(),
            pinned: HashMap::new(),
        }
    }
}

/// The envelope the mod posts for markers. Only its shape is read; the arrays
/// inside are carried through untouched.
#[derive(serde::Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Sorted {
    #[serde(default)]
    colors: Option<Box<RawValue>>,
    #[serde(default)]
    public: Option<Box<RawValue>>,
    #[serde(default)]
    private: HashMap<String, Box<RawValue>>,
    #[serde(default)]
    pins: HashMap<String, Box<RawValue>>,
}

/// Who is online, as the mod sorted them.
///
/// The same shape the markers arrive in and for the same reason: whether where
/// somebody is standing may be shown to somebody else depends on a setting and on
/// what groups the game has them in, and the half that knows both is the mod.
/// This holds two lists it hands out to a browser without looking inside them —
/// `positions` is the one place anything here reads what is in them, and it is
/// read once, for every player regardless of `owned`, since a browser's view of
/// who may see whom has no bearing on what the game server already knows.
struct Seen {
    /// How many are on, whoever is asking. A server that hides positions still
    /// says how busy it is — that is a fact about the server, not about anybody.
    online: u32,
    /// Players anyone may see.
    open: String,
    /// Players one particular person may see beyond that, by their uid. Empty
    /// where positions are everybody's, since then everyone is in `open`.
    owned: HashMap<String, String>,
    /// Who shares a group with one particular person, by their uid, as a list of
    /// uids. Not about who may be seen — it is what lets the page offer "my
    /// group" as a way of reading a list it already has.
    grouped: HashMap<String, String>,
    /// Where everyone the mod posted this beat is standing, in blocks — every
    /// player the mod knows about, independent of `open`/`owned`.
    positions: Vec<Whereabouts>,
    /// Every group the server has, by id: what it is called and who is in it,
    /// online or not. What sharing a map with a group is decided against.
    groups: HashMap<i32, Group>,
}

impl Default for Seen {
    /// Empty arrays rather than empty strings, for the reason `Markers` gives.
    fn default() -> Self {
        Self {
            online: 0,
            open: "[]".to_owned(),
            owned: HashMap::new(),
            grouped: HashMap::new(),
            positions: Vec::new(),
            groups: HashMap::new(),
        }
    }
}

/// The envelope the mod posts for players.
#[derive(serde::Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Watching {
    #[serde(default)]
    online: u32,
    #[serde(default)]
    public: Option<Box<RawValue>>,
    #[serde(default)]
    private: HashMap<String, Box<RawValue>>,
    #[serde(default)]
    grouped: HashMap<String, Box<RawValue>>,
    /// Every group, by its id as a string — JSON has no other kind of key.
    #[serde(default)]
    groups: HashMap<String, GroupPosted>,
}

/// One group as the mod posts it.
#[derive(serde::Deserialize)]
#[serde(rename_all = "PascalCase")]
struct GroupPosted {
    #[serde(default)]
    name: String,
    #[serde(default)]
    members: Vec<String>,
}

/// One player, read only far enough to say where they are.
///
/// The mod already decided who may be shown this before it arrived — `open` and
/// `owned` above exist for that — so reading a position out of either array here
/// is not a second look at a decision already made. It is this service seeing
/// what it is already being told, for a reason the mod's own visibility rules
/// were never about: keeping the terrain this service asks for anchored to where
/// people actually are, which the game server already knows regardless of who
/// may see whom on a page.
#[derive(serde::Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Positioned {
    #[serde(default)]
    uid: String,
    x: i32,
    z: i32,
    #[serde(default)]
    view_chunks: i32,
}

/// Where one player is standing, and who: the uid is what their memory of the
/// map is kept under.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Whereabouts {
    pub uid: String,
    pub x: i32,
    pub z: i32,
    /// How far the game loads ground around them, in chunks: their own view
    /// distance as the server granted it. Zero where the mod did not say,
    /// which is a mod older than this build.
    pub reach: i32,
}

/// Every position named in one of the mod's player arrays, as raw text.
fn positions_in(raw: Option<&RawValue>) -> Vec<Whereabouts> {
    let Some(raw) = raw else { return Vec::new() };
    serde_json::from_str::<Vec<Positioned>>(raw.get())
        .map(|players| players.into_iter().map(|p| Whereabouts { uid: p.uid, x: p.x, z: p.z, reach: p.view_chunks }).collect())
        .unwrap_or_default()
}

/// Every land claim, and who the mod says may be shown them.
///
/// One list rather than a list per person. Who may see a claim is not a fact
/// about the claim — the game shares every one of them with every client — it is
/// a fact about the reader, and the mod answers it from a privilege. So the
/// claims are held once and the names of everyone entitled to them are held
/// beside, which is the shape that stays one copy however many people are on.
///
/// Nothing is written down. Markers are filed because they are the one thing
/// worth seeing when the game server is off and nothing else could give them
/// back; a claim comes with a list of who may see it, and a file read back at
/// start would be a list of claims whose permissions this build was told about
/// some other day. The mod reposts within one share interval, and until it does
/// the honest answer is that the map has not been told.
#[derive(Default)]
struct Claims {
    /// The post as it arrived, so an unchanged one is recognised without being
    /// taken apart again.
    body: String,
    /// Whether the claims are everybody's to see, which is what the setting says
    /// on a server that has not narrowed it.
    everyones: bool,
    /// The claims themselves, as the text that arrived.
    list: String,
    /// Who may be shown them beyond that, by uid.
    seen: HashSet<String>,
    /// Who may draw a new one and what each of them is allowed, by uid, as the
    /// text that arrived. Not about who may see: a server can show every boundary
    /// to everybody and still let nobody but its landholders draw one.
    making: HashMap<String, String>,
    /// How tall this world is, so the form knows what the whole height of it
    /// means. A fact about the world, carried with the claims because it is the
    /// one thing a map drawn from above cannot show.
    height: i32,
}

/// The envelope the mod posts for claims. Only its shape is read; the array
/// inside is carried through untouched.
#[derive(serde::Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Held {
    #[serde(default)]
    everyones: bool,
    #[serde(default)]
    claims: Option<Box<RawValue>>,
    #[serde(default)]
    seen: HashSet<String>,
    #[serde(default)]
    making: HashMap<String, Box<RawValue>>,
    #[serde(default)]
    height: i32,
}

pub struct Live {
    players: Mutex<Option<(Seen, Instant)>>,
    /// What the world's clock last said. Held rather than filed, and forgotten
    /// the same way the players are: a clock from a server that has stopped is
    /// not the time, it is the time it stopped.
    world: Mutex<Option<(String, Instant)>>,
    markers: Mutex<Markers>,
    /// Where the land claims are, and who may be shown them. Memory only, for
    /// the reason [`Claims`] gives.
    claims: Mutex<Claims>,
    /// Where markers are kept so they survive both programs stopping.
    path: PathBuf,
}

impl Live {
    /// Reads back whatever markers a previous run was told about.
    ///
    /// A file this build cannot read is no markers rather than a guess at what it
    /// meant. One written before markers carried who may see them says nothing
    /// about that, and showing it to everybody would be deciding on the owner's
    /// behalf; the mod replaces it within one share interval either way.
    #[must_use]
    pub fn load(exports: &Path) -> Self {
        let path = markers_path(exports);
        let markers = std::fs::read_to_string(&path)
            .ok()
            .and_then(|body| sorted(&body))
            .unwrap_or_default();

        Self {
            players: Mutex::new(None),
            world: Mutex::new(None),
            markers: Mutex::new(markers),
            claims: Mutex::new(Claims::default()),
            path,
        }
    }

    /// Takes a report of who is online, sorted by who may see them. In memory
    /// only: a position is stale before a write of it would finish.
    pub fn set_players(&self, body: String) -> bool {
        let Some(taken) = watching(&body) else {
            return false;
        };
        if let Ok(mut players) = self.players.lock() {
            *players = Some((taken, Instant::now()));
        }
        true
    }

    /// Where every player the mod last posted is standing, in blocks, and who
    /// each of them is — stale data answered the same way `body` answers it,
    /// with nothing rather than a position that may no longer be true.
    ///
    /// A player posted twice — to the public and to their group — is here once:
    /// where somebody stands is one fact however many lists carry it.
    #[must_use]
    pub fn whereabouts(&self) -> Vec<Whereabouts> {
        let mut all = self
            .players
            .lock()
            .ok()
            .and_then(|held| {
                held.as_ref()
                    .filter(|(_, at)| at.elapsed() < PLAYERS_GOOD_FOR)
                    .map(|(seen, _)| seen.positions.clone())
            })
            .unwrap_or_default();
        all.sort_by(|a, b| a.uid.cmp(&b.uid));
        all.dedup_by(|a, b| !a.uid.is_empty() && a.uid == b.uid);
        all
    }



    /// Every group the mod last posted, whether or not the post is still fresh:
    /// who is in a group does not go stale the way where they stand does.
    #[must_use]
    pub fn groups(&self) -> HashMap<i32, Group> {
        self.players
            .lock()
            .ok()
            .and_then(|held| held.as_ref().map(|(seen, _)| seen.groups.clone()))
            .unwrap_or_default()
    }

    /// Takes what the world's clock says.
    ///
    /// Only that it is an object is checked. What is in it is the mod's to word
    /// and the page's to read, and a service that understood the words would be a
    /// third place for a date format to disagree with itself.
    pub fn set_world(&self, body: String) -> bool {
        if !body.trim_start().starts_with('{') {
            return false;
        }
        if let Ok(mut world) = self.world.lock() {
            *world = Some((body, Instant::now()));
        }
        true
    }

    /// Takes the markers, and writes them if they are not what is already held.
    pub fn set_markers(&self, body: String) -> bool {
        let Some(taken) = sorted(&body) else {
            return false;
        };

        let Ok(mut markers) = self.markers.lock() else {
            return true;
        };
        if markers.body == body {
            return true;
        }

        crate::files::publish(&self.path, body.as_bytes());

        *markers = taken;
        true
    }

    /// Takes the land claims, sorted by who the mod says may see them.
    ///
    /// Held and not filed, unlike the markers. See [`Claims`] for why a list that
    /// arrives with its own permissions is not a list to read back off a disk on
    /// some later day.
    pub fn set_claims(&self, body: String) -> bool {
        let Some(taken) = held(&body) else {
            return false;
        };

        if let Ok(mut claims) = self.claims.lock()
            && claims.body != body
        {
            *claims = taken;
        }
        true
    }

    /// The colours the game offers for a marker, for the page's own form.
    ///
    /// Empty until the mod has posted once. A page that asks early gets an empty
    /// picker and asks again, which is the same answer a service that has never
    /// heard from a mod should give.
    #[must_use]
    pub fn colors(&self) -> String {
        self.markers.lock().map_or_else(|_| "[]".to_owned(), |held| held.colors.clone())
    }

    /// What the viewer asks for: who is online, and every marker they may see.
    ///
    /// Whose markers those are is decided by who is asking. Everyone gets the ones
    /// their owners share; somebody logged in also gets their own. A marker kept
    /// private never leaves this process for a browser that is not its owner's,
    /// which is the only place that promise can be kept — a page cannot be trusted
    /// to hide what it has been handed.
    ///
    /// Empty is empty. There was once a fallback to the `live.json` an older mod
    /// wrote, and it disguised a mod that posted no markers as a map that merely
    /// had none — the map filled in from a stale file the moment the game server
    /// stopped and its players expired. Nothing here reads what this build does
    /// not write.
    #[must_use]
    pub fn body(&self, uid: Option<&str>) -> String {
        // Who is online, whom of them this person may see, and who of those is in
        // a group with them. A report older than the patience is a game server
        // that has gone, and a dot saying somebody is standing somewhere is worse
        // than no dot at all.
        let (players, online, grouped) = self
            .players
            .lock()
            .ok()
            .and_then(|held| {
                held.as_ref()
                    .filter(|(_, at)| at.elapsed() < PLAYERS_GOOD_FOR)
                    .map(|(seen, _)| {
                        (mine(&seen.open, seen.owned.get(uid.unwrap_or_default())),
                         seen.online,
                         seen.grouped.get(uid.unwrap_or_default()).cloned())
                    })
            })
            .map_or_else(
                || ("[]".to_owned(), 0, "[]".to_owned()),
                |(list, online, grouped)| {
                    (list, online, grouped.unwrap_or_else(|| "[]".to_owned()))
                },
            );

        // The markers this person may see, and which of them they keep in sight in
        // game. The pins go out only to whoever set them: what somebody has
        // chosen to keep on their own map is nobody else's business, and the page
        // has one question — is this one mine to see pinned — rather than a list
        // per player to search.
        let (markers, pins) = self.markers.lock().map_or_else(
            |_| ("[]".to_owned(), "[]".to_owned()),
            |held| {
                (
                    mine(&held.open, uid.and_then(|uid| held.owned.get(uid))),
                    uid.and_then(|uid| held.pinned.get(uid))
                        .cloned()
                        .unwrap_or_else(|| "[]".to_owned()),
                )
            },
        );

        // Every claim, or none. Unlike the markers there is nothing to join: a
        // reader is entitled to the lot or to none of it, because that is the
        // shape of the question the mod answered.
        //
        // What this reader is allowed to claim goes out beside them, and only to
        // them. It is what the form needs to say a rectangle's cost before asking
        // for it — see the mod's `ClaimAllowance` — and it is nobody else's
        // business how much land somebody has left.
        let (claims, claiming, height) = self.claims.lock().map_or_else(
            |_| ("[]".to_owned(), "null".to_owned(), 0),
            |held| {
                let allowed = held.everyones
                    || uid.is_some_and(|uid| held.seen.contains(uid));
                let list = if allowed { held.list.clone() } else { "[]".to_owned() };
                let mine = uid
                    .and_then(|uid| held.making.get(uid))
                    .cloned()
                    .unwrap_or_else(|| "null".to_owned());
                (list, mine, held.height)
            },
        );

        let world = self
            .world
            .lock()
            .ok()
            .and_then(|held| held.clone())
            .filter(|(_, at)| at.elapsed() < PLAYERS_GOOD_FOR)
            .map_or_else(|| "null".to_owned(), |(body, _)| body);

        format!(
            r#"{{"Players":{players},"Online":{online},"Grouped":{grouped},"Waypoints":{markers},"Pins":{pins},"Claims":{claims},"Claiming":{claiming},"Height":{height},"World":{world}}}"#
        )
    }
}

/// What everybody is shown, plus whatever one person is shown on top of it.
///
/// The one sum both the players and the markers are made of: two JSON arrays,
/// joined end to end where there is a second one. Written once because getting it
/// wrong in either place is the same bug — somebody handed what is not theirs, or
/// not handed what is.
fn mine(open: &str, extra: Option<&String>) -> String {
    match extra {
        Some(theirs) => joined(open, theirs),
        None => open.to_owned(),
    }
}

/// The mod's envelope, taken apart no further than it has to be.
///
/// Anything that is not the shape this build posts is nothing rather than a
/// partial reading of it: a body that arrived mangled is a body whose markers are
/// not known, and showing some of them would be worse than showing none.
fn sorted(body: &str) -> Option<Markers> {
    // An object, said out loud. Serde will fill a struct from a JSON array as
    // happily as from an object, so without this the bare array an older mod
    // posted is read as an envelope whose first marker is the colour list — which
    // is a map that comes back empty rather than a post that was refused.
    if !body.trim_start().starts_with('{') {
        return None;
    }

    let read: Sorted = serde_json::from_str(body).ok()?;
    Some(Markers {
        body: body.to_owned(),
        colors: array(read.colors.as_deref()),
        open: array(read.public.as_deref()),
        owned: arrays(read.private),
        pinned: arrays(read.pins),
    })
}

/// The mod's report of who is online, taken apart no further than it has to be.
///
/// An array rather than an object is a mod older than this build, which posted
/// every player to everybody. Refused rather than read that way: the two halves
/// ship as one release and carry one version, so this is a mis-deployment — and
/// quietly showing every position to everybody would be exactly the thing an
/// operator turned the setting off to prevent.
fn watching(body: &str) -> Option<Seen> {
    if !body.trim_start().starts_with('{') {
        return None;
    }

    let read: Watching = serde_json::from_str(body).ok()?;

    // Every position the mod posted this beat, whichever list it sorted a
    // player into — `positions_in` reads the same text `open`/`owned` below
    // hold as strings, before either of those takes ownership of it.
    let mut positions = positions_in(read.public.as_deref());
    for list in read.private.values() {
        positions.extend(positions_in(Some(list)));
    }

    let groups = read
        .groups
        .into_iter()
        .filter_map(|(id, group)| {
            Some((id.parse::<i32>().ok()?, Group { name: group.name, members: group.members.into_iter().collect() }))
        })
        .collect();

    Some(Seen {
        online: read.online,
        open: array(read.public.as_deref()),
        owned: arrays(read.private),
        grouped: arrays(read.grouped),
        positions,
        groups,
    })
}

/// The mod's claims, taken apart no further than they have to be.
///
/// Anything that is not the shape this build posts is nothing rather than a
/// partial reading of it, for the reason the markers give: a body that arrived
/// mangled is a body whose permissions are not known, and a claim shown to
/// somebody the mod did not name is the one mistake this file exists to prevent.
fn held(body: &str) -> Option<Claims> {
    if !body.trim_start().starts_with('{') {
        return None;
    }

    let read: Held = serde_json::from_str(body).ok()?;
    Some(Claims {
        body: body.to_owned(),
        everyones: read.everyones,
        list: array(read.claims.as_deref()),
        seen: read.seen,
        making: read
            .making
            .into_iter()
            .map(|(uid, allowance)| (uid, allowance.get().trim().to_owned()))
            .collect(),
        height: read.height,
    })
}

/// One of the mod's by-uid maps, each of its arrays as the text that arrived.
///
/// The three the mod posts — whose markers, whose positions, whose group — are
/// the same shape read the same way, and were three copies of the same four
/// lines. What is in an array is never looked at here; what this does is make
/// sure each one is an array at all.
fn arrays(held: HashMap<String, Box<RawValue>>) -> HashMap<String, String> {
    held.into_iter().map(|(uid, list)| (uid, array(Some(&list)))).collect()
}

/// One of the mod's arrays as text, or an empty one where it sent nothing.
fn array(raw: Option<&RawValue>) -> String {
    raw.map(|held| held.get().trim().to_owned())
        .filter(|held| held.starts_with('['))
        .unwrap_or_else(|| "[]".to_owned())
}

/// Two JSON arrays as one.
///
/// Textual, because the point of holding the mod's arrays as they arrived is not
/// having looked inside them, and putting two lists end to end does not need to.
fn joined(first: &str, second: &str) -> String {
    match (within(first), within(second)) {
        ("", "") => "[]".to_owned(),
        (only, "") | ("", only) => format!("[{only}]"),
        (first, second) => format!("[{first},{second}]"),
    }
}

/// What is between an array's brackets.
fn within(array: &str) -> &str {
    array
        .trim()
        .strip_prefix('[')
        .and_then(|rest| rest.strip_suffix(']'))
        .unwrap_or("")
        .trim()
}

#[must_use]
pub fn markers_path(exports: &Path) -> PathBuf {
    exports.join("markers.json")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One public marker, one of Ada's and one of Bob's — and Ada keeping the
    /// public one in sight, which is a marker of neither of theirs.
    const POSTED: &str = r##"{
        "Colors":["#f9d0dc","#ed272a"],
        "Public":[{"Title":"trader","Key":"a"}],
        "Private":{
            "uid-ada":[{"Title":"ada's hoard","Key":"b"}],
            "uid-bob":[{"Title":"bob's hoard","Key":"c"}]
        },
        "Pins":{"uid-ada":["a"]}
    }"##;

    fn told() -> Live {
        let live = Live::load(Path::new("/nonexistent"));
        assert!(live.set_markers(POSTED.to_owned()), "the envelope this build posts is taken");
        live
    }

    #[test]
    fn a_pin_reaches_whoever_set_it_and_nobody_else() {
        let live = told();

        assert!(live.body(Some("uid-ada")).contains(r#""Pins":["a"]"#), "Ada is sent her own");
        // Bob and a stranger are shown the marker Ada pinned and told nothing
        // about her keeping it: what somebody keeps on their own map is theirs.
        assert!(live.body(Some("uid-bob")).contains(r#""Pins":[]"#));
        assert!(live.body(None).contains(r#""Pins":[]"#));
    }

    #[test]
    fn a_post_from_a_mod_that_knows_nothing_of_pins_has_none() {
        let live = Live::load(Path::new("/nonexistent"));
        assert!(live.set_markers(
            r##"{"Colors":[],"Public":[{"Title":"trader","Key":"a"}],"Private":{}}"##.to_owned()));
        // An empty array rather than a hole, for the reason every other list
        // here is one: a page cannot parse what it was not sent.
        assert!(live.body(Some("uid-ada")).contains(r#""Pins":[]"#));
    }

    #[test]
    fn a_private_marker_reaches_its_owner_and_nobody_else() {
        let live = told();

        let ada = live.body(Some("uid-ada"));
        assert!(ada.contains("ada's hoard"), "Ada is sent her own");
        assert!(!ada.contains("bob's hoard"), "and never Bob's");

        let bob = live.body(Some("uid-bob"));
        assert!(bob.contains("bob's hoard"));
        assert!(!bob.contains("ada's hoard"));

        // The map is public and stays public, so a stranger is still shown the
        // markers whose owners share them — and only those.
        let stranger = live.body(None);
        assert!(stranger.contains("trader"));
        assert!(!stranger.contains("hoard"), "nobody's private markers reach a stranger");
    }

    #[test]
    fn everybody_is_shown_what_is_shared() {
        let live = told();
        for who in [None, Some("uid-ada"), Some("uid-bob"), Some("uid-nobody")] {
            assert!(live.body(who).contains("trader"), "{who:?} is shown the shared marker");
        }
    }

    #[test]
    fn an_owner_is_sent_one_list_a_browser_can_read() {
        let live = told();
        let body: serde_json::Value =
            serde_json::from_str(&live.body(Some("uid-ada"))).expect("valid JSON");

        let markers = body["Waypoints"].as_array().expect("an array of markers");
        assert_eq!(markers.len(), 2, "the shared one and her own, joined end to end");
        assert_eq!(markers[0]["Title"], "trader");
        assert_eq!(markers[1]["Title"], "ada's hoard");
    }

    #[test]
    fn a_post_this_build_cannot_read_is_refused() {
        let live = Live::load(Path::new("/nonexistent"));

        // A bare array is what an older mod posted. It says nothing about who may
        // see what, and reading it as all-public would decide on owners' behalf.
        assert!(!live.set_markers(r#"[{"Title":"trader"}]"#.to_owned()));
        assert!(!live.set_markers("not json".to_owned()));
        assert!(!live.body(Some("uid-ada")).contains("trader"));
    }

    #[test]
    fn a_map_nobody_has_posted_to_still_answers() {
        let live = Live::load(Path::new("/nonexistent"));
        let body: serde_json::Value =
            serde_json::from_str(&live.body(None)).expect("valid JSON");
        assert_eq!(body["Players"].as_array().expect("an array").len(), 0);
        assert_eq!(body["Waypoints"].as_array().expect("an array").len(), 0);
        assert_eq!(live.colors(), "[]");
    }

    #[test]
    fn the_colours_the_game_offers_come_back_as_the_mod_sent_them() {
        let colors: Vec<String> = serde_json::from_str(&told().colors()).expect("an array");
        assert_eq!(colors, vec!["#f9d0dc", "#ed272a"]);
    }

    #[test]
    fn two_lists_become_one() {
        assert_eq!(joined("[]", "[]"), "[]");
        assert_eq!(joined("[1]", "[]"), "[1]");
        assert_eq!(joined("[]", "[2]"), "[2]");
        assert_eq!(joined("[1]", "[2]"), "[1,2]");
        assert_eq!(joined("[1,2]", "[3]"), "[1,2,3]");
        assert_eq!(joined(" [ 1 ] ", " [ 2 ] "), "[1,2]");
    }

    /// Ada is on and everybody may see her; Bob is on and only his own group may.
    /// Cass is in that group and is not on, which is what the map is asked about.
    fn watched() -> Live {
        let live = Live::load(Path::new("/nonexistent"));
        assert!(live.set_players(
            r#"{"Online":2,
                "Public":[{"Name":"ada","Uid":"a"}],
                "Private":{"b":[{"Name":"bob","Uid":"b"}],"c":[{"Name":"bob","Uid":"b"}]},
                "Grouped":{"b":["b","c"],"c":["b","c"]}}"#
                .to_owned()
        ));
        live
    }

    #[test]
    fn a_report_of_who_is_online_goes_stale() {
        let live = watched();
        assert!(live.body(None).contains("ada"));

        // A game server that stopped must not leave a dot standing on the map.
        // Reaching in is the only way to age it without waiting out the clock.
        if let Ok(mut players) = live.players.lock()
            && let Some((_, at)) = players.as_mut()
        {
            *at = Instant::now() - PLAYERS_GOOD_FOR - Duration::from_secs(1);
        }
        let gone = live.body(None);
        assert!(!gone.contains("ada"), "the players go");
        assert!(gone.contains(r#""Online":0"#), "and so does the count of them");
    }

    #[test]
    fn a_position_only_a_group_may_see_reaches_that_group_and_nobody_else() {
        let live = watched();

        let stranger = live.body(None);
        assert!(stranger.contains("ada"), "what is public is everybody's");
        assert!(!stranger.contains("bob"), "and what is not, is not");

        // Bob's own browser, and Cass, who is in his group and is not even on.
        for uid in ["b", "c"] {
            let theirs = live.body(Some(uid));
            assert!(theirs.contains("bob"), "{uid} shares a group with bob");
            assert!(theirs.contains("ada"), "and still sees what is public");
        }
    }

    #[test]
    fn the_groups_the_mod_posts_are_read_whole() {
        let live = Live::load(Path::new("/nonexistent"));
        assert!(live.set_players(
            r#"{"Online":0,"Public":[],"Private":{},"Grouped":{},
                "Groups":{"7":{"Name":"the guild","Members":["a","b","c"]},"x":{"Name":"nonsense"}}}"#
                .to_owned()
        ));
        let groups = live.groups();
        assert_eq!(groups.len(), 1, "a group whose id is not a number is not a group");
        assert_eq!(groups[&7].name, "the guild");
        assert_eq!(groups[&7].members.len(), 3, "offline members included");
    }

    #[test]
    fn how_many_are_on_is_said_to_everybody() {
        // A server that hides where people are standing still says how busy it
        // is: that is a fact about the server rather than about anybody on it.
        let live = watched();
        for uid in [None, Some("a"), Some("b")] {
            assert!(live.body(uid).contains(r#""Online":2"#), "{uid:?} is told the count");
        }
    }

    #[test]
    fn who_shares_a_group_is_said_only_to_them() {
        let live = watched();
        assert!(live.body(Some("b")).contains(r#""Grouped":["b","c"]"#));
        assert!(live.body(None).contains(r#""Grouped":[]"#), "a stranger is in no group");
        assert!(live.body(Some("a")).contains(r#""Grouped":[]"#), "nor is somebody in none");
    }

    /// Two claims, seen by Ada alone, and only Ada may draw one.
    const CLAIMED: &str = r##"{
        "Everyones": false,
        "Height": 256,
        "Claims": [{"Key":"a","Owner":"Ada","OwnerUid":"uid-ada","Areas":[
            {"X1":10,"Z1":10,"X2":20,"Z2":20,"Y1":0,"Y2":256}]}],
        "Seen": ["uid-ada"],
        "Making": {"uid-ada":{"Allowance":262144,"Used":0,"MaxAreas":3,"Areas":0}}
    }"##;

    fn claimed() -> Live {
        let live = Live::load(Path::new("/nonexistent"));
        assert!(live.set_claims(CLAIMED.to_owned()), "the envelope this build posts is taken");
        live
    }

    #[test]
    fn a_claim_reaches_whoever_the_mod_named_and_nobody_else() {
        let live = claimed();

        let ada = live.body(Some("uid-ada"));
        assert!(ada.contains(r#""Key":"a""#), "Ada was named, so Ada is sent them");

        // The whole point of the gate. A reader the mod did not name is sent an
        // empty list rather than a list to be filtered in a browser, because a
        // browser cannot be asked to hide what it has already been handed.
        for who in [None, Some("uid-bob")] {
            assert!(
                live.body(who).contains(r#""Claims":[]"#),
                "{who:?} was not named and is sent no claims"
            );
        }
    }

    #[test]
    fn claims_open_to_everybody_reach_a_stranger() {
        // What a server that has not narrowed `[claims] view` looks like: the
        // game already sends every claim to every client, so the map saying less
        // would be telling players less than the game does.
        let live = Live::load(Path::new("/nonexistent"));
        assert!(live.set_claims(
            r#"{"Everyones":true,"Claims":[{"Key":"a"}],"Seen":[],"Making":{}}"#.to_owned()
        ));
        for who in [None, Some("uid-bob")] {
            assert!(live.body(who).contains(r#""Key":"a""#), "{who:?} is shown an open claim");
        }
    }

    #[test]
    fn what_somebody_may_claim_is_said_to_them_alone() {
        let live = claimed();

        let ada = live.body(Some("uid-ada"));
        assert!(ada.contains(r#""Allowance":262144"#), "Ada is told her own allowance");
        assert!(ada.contains(r#""Height":256"#), "and how tall the world is");

        // How much land somebody has left is nobody else's business, and a
        // reader who may not claim at all is told nothing rather than zero.
        for who in [None, Some("uid-bob")] {
            assert!(
                live.body(who).contains(r#""Claiming":null"#),
                "{who:?} may not claim, so there is nothing to tell them"
            );
        }
    }

    #[test]
    fn a_claim_post_this_build_cannot_read_is_refused() {
        let live = Live::load(Path::new("/nonexistent"));
        assert!(!live.set_claims(r#"[{"Key":"a"}]"#.to_owned()), "a bare array is not the envelope");
        assert!(!live.set_claims("not json".to_owned()));
        assert!(live.body(Some("uid-ada")).contains(r#""Claims":[]"#));
    }

    #[test]
fn a_report_in_the_shape_an_older_mod_posted_is_refused() {
        // A bare array was every player, to everybody. Read that way it would be
        // exactly what an operator turned the setting off to prevent.
        let live = Live::load(Path::new("/nonexistent"));
        assert!(!live.set_players(r#"[{"Name":"ada"}]"#.to_owned()));
        assert!(!live.body(None).contains("ada"));
    }
}
