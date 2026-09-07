//! Holds the parts of the map that move.
//!
//! Players and markers arrive from the server mod over the API channel and are
//! held in memory rather than on disk. Positions change every couple of seconds
//! and are worthless once old, so writing them to a file to read back a moment
//! later would be work with no product.
//!
//! Markers are the exception. They change a few times an hour, and they are the
//! one thing worth seeing when the game server is off, so they are written when
//! they arrive and read back at start.
//!
//! This module does not parse either payload for what it means to a browser. The
//! mod knows what a waypoint is. This knows only that it is a JSON array to hand
//! on, which is the whole contract for markers and claims.
//!
//! Players are the one exception. [`Live::positions`] reads a position out of
//! the same arrays, because [`crate::protocol::pull`] needs to know where people
//! are, and the game server knows that regardless of who a browser may show it
//! to. Nothing downstream reads a name, a health bar, or anything else a player's
//! entry carries.
//!
//! Markers arrive already sorted into what anyone may see and what only one
//! person may, because sorting them requires knowing what a waypoint is. The
//! arrays inside are held as they arrived and handed on, and the most that
//! happens to one is being joined to another end to end.
//!
//! Land claims arrive the same way but are sorted differently. A marker is
//! private to whoever made it, so who may see one is answered per marker. A claim
//! is public within the server or it is not, and who may see the lot is answered
//! per person by a privilege. The claims therefore travel as one list with the
//! names of everyone the mod says may be sent it. A copy per person would be the
//! same hundred claims fifty times over on a server with fifty players.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde_json::value::RawValue;

use crate::util::log::warn;
use crate::mapdata::memory::Group;
use crate::mapdata::store::Store;

/// How long a report of who is online stays believable.
///
/// Without this, a game server that crashes or shuts down would leave its last
/// positions on the map indefinitely, and a dot saying somebody is standing
/// somewhere is worse than no dot at all.
const PLAYERS_GOOD_FOR: Duration = Duration::from_secs(30);

/// Holds every marker, as the mod sorted them.
///
/// The markers are stored as the text that arrived rather than as anything
/// parsed out of it. The posted body is kept beside the pieces, so an unchanged
/// post is recognised without taking it apart again.
struct Markers {
    body: String,
    /// The colours the game offers, so the page's form can offer the same ones.
    colors: String,
    /// The markers anyone may see.
    open: String,
    /// The markers only their owner may see, keyed by that owner's uid.
    owned: HashMap<String, String>,
    /// Names which markers each person keeps in sight on their own in-game map,
    /// keyed by uid, as the list of marker names that arrived. It is sorted by
    /// reader for the same reason private markers are: a pin is one person's
    /// choice and nobody else should be handed it.
    pinned: HashMap<String, String>,
}

impl Default for Markers {
    /// Uses empty arrays rather than empty strings. These are spliced into the
    /// body a browser is handed, and a hole where an array should be would make
    /// the page fail to parse what it was sent.
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

/// Holds the envelope the mod posts for markers. Only its shape is read. The
/// arrays inside are carried through untouched.
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

/// Holds who is online, as the mod sorted them.
///
/// This has the same shape markers arrive in, and for the same reason. Whether
/// somebody's position may be shown to somebody else depends on a setting and on
/// the groups the game has them in, and only the mod knows both.
///
/// The two lists are handed to a browser without being looked into. `positions`
/// is the one place anything here reads what is in them, and it reads every
/// player regardless of `owned`, because a browser's view of who may see whom has
/// no bearing on what the game server knows.
struct Seen {
    /// How many players are online, whoever is asking. A server that hides
    /// positions still reports how busy it is, which is a fact about the server
    /// rather than about any player.
    online: u32,
    /// The players anyone may see.
    open: String,
    /// The players one particular person may see beyond that, keyed by uid. It
    /// is empty when positions are everybody's, because then everyone is in
    /// `open`.
    owned: HashMap<String, String>,
    /// Where everyone the mod posted this tick is standing, in blocks. It covers
    /// every player the mod knows about, independent of `open` and `owned`.
    positions: Vec<Whereabouts>,
    /// Every group the server has, keyed by id, with its name and members,
    /// online or not. Sharing a map with a group is decided against this.
    groups: HashMap<i32, Group>,
}

impl Default for Seen {
    /// Uses empty arrays rather than empty strings, for the reason `Markers`
    /// gives.
    fn default() -> Self {
        Self {
            online: 0,
            open: "[]".to_owned(),
            owned: HashMap::new(),
            positions: Vec::new(),
            groups: HashMap::new(),
        }
    }
}

/// Holds the envelope the mod posts for players.
#[derive(serde::Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Watching {
    #[serde(default)]
    online: u32,
    #[serde(default)]
    public: Option<Box<RawValue>>,
    #[serde(default)]
    private: HashMap<String, Box<RawValue>>,
    /// Every group, keyed by its id as a string, because JSON has no other kind
    /// of key.
    #[serde(default)]
    groups: HashMap<String, GroupPosted>,
}

/// Holds one group as the mod posts it.
#[derive(serde::Deserialize)]
#[serde(rename_all = "PascalCase")]
struct GroupPosted {
    #[serde(default)]
    name: String,
    #[serde(default)]
    members: Vec<String>,
}

/// Holds one player, read only far enough to say where they are.
///
/// The mod decided who may be shown this before it arrived, which is what `open`
/// and `owned` above record. Reading a position out of either array is not a
/// second look at that decision. It is the service reading what it is already
/// told, so the terrain it asks for stays anchored to where people actually are.
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

/// Holds where one player is standing and who they are. Their memory of the map
/// is keyed on the uid.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Whereabouts {
    pub uid: String,
    pub x: i32,
    pub z: i32,
    /// How far the game loads ground around them, in chunks. This is their own
    /// view distance as the server granted it. It is zero when the mod did not
    /// report one, which means a mod older than this build.
    pub reach: i32,
}

/// Reads every position out of one of the mod's player arrays, given as raw
/// text.
fn positions_in(raw: Option<&RawValue>) -> Vec<Whereabouts> {
    let Some(raw) = raw else { return Vec::new() };
    serde_json::from_str::<Vec<Positioned>>(raw.get())
        .map(|players| players.into_iter().map(|p| Whereabouts { uid: p.uid, x: p.x, z: p.z, reach: p.view_chunks }).collect())
        .unwrap_or_default()
}

/// Holds every land claim and who the mod says may be shown them.
///
/// The claims are one list rather than a list per person. Who may see a claim is
/// a fact about the reader rather than about the claim, because the game shares
/// every claim with every client and the mod answers from a privilege. Holding
/// the claims once with the entitled names beside them stays one copy however
/// many people are online.
///
/// Nothing here is written to disk. Markers are stored because they are worth
/// seeing when the game server is off and nothing else could give them back. A
/// claim arrives with a list of who may see it, and a file read back at start
/// would carry permissions this build was told about on some other day. The mod
/// reposts within one share interval, and until it does the map has not been
/// told.
#[derive(Default)]
struct Claims {
    /// The post as it arrived, so an unchanged one is recognised without being
    /// parsed again.
    body: String,
    /// Makes the claims visible to everybody, which is what the setting says on
    /// a server that has not narrowed it.
    everyones: bool,
    /// The claims themselves, as the text that arrived.
    list: String,
    /// The people who may be shown the claims beyond that, keyed by uid.
    seen: HashSet<String>,
    /// Names who may draw a new claim and what each is allowed, keyed by uid, as
    /// the text that arrived. This is separate from who may see: a server can
    /// show every boundary to everybody and still let only its landholders draw
    /// one.
    making: HashMap<String, String>,
    /// How tall this world is, so the form knows what its full height means.
    /// This is a fact about the world, carried with the claims because a map
    /// drawn from above cannot show it.
    height: i32,
}

/// Holds the envelope the mod posts for claims. Only its shape is read. The
/// array inside is carried through untouched.
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
    /// The world's clock as the mod last reported it. It is held in memory and
    /// expires the way players do, because a clock from a stopped server reports
    /// the time it stopped rather than the time.
    world: Mutex<Option<(String, Instant)>>,
    markers: Mutex<Markers>,
    /// Holds the land claims and who may be shown them. It is memory only, for
    /// the reason [`Claims`] gives.
    claims: Mutex<Claims>,
    /// The store that keeps markers across both programs stopping.
    store: Arc<Store>,
    /// The group names the operator has hidden, lowercased once here so a post
    /// is compared against them without lowercasing again. See
    /// `Config::hidden_groups`.
    hidden: Vec<String>,
}

impl Live {
    /// Loads whatever markers a previous run was told about.
    ///
    /// A post this build cannot read yields no markers rather than a guess at
    /// what it meant. The mod replaces it within one share interval either way.
    #[must_use]
    pub fn load(store: Arc<Store>, hidden_groups: &[String]) -> Self {
        let markers = store
            .markers()
            .unwrap_or_else(|error| {
                warn!("{error}");
                None
            })
            .and_then(|body| sorted(&body))
            .unwrap_or_default();

        Self {
            players: Mutex::new(None),
            world: Mutex::new(None),
            markers: Mutex::new(markers),
            claims: Mutex::new(Claims::default()),
            store,
            hidden: hidden_groups.iter().map(|name| name.trim().to_lowercase()).collect(),
        }
    }

    /// Stores a report of who is online, sorted by who may see them. It is held
    /// in memory only, because a position goes stale before a write would finish.
    pub fn set_players(&self, body: String) -> bool {
        let Some(taken) = watching(&body, &self.hidden) else {
            return false;
        };
        if let Ok(mut players) = self.players.lock() {
            *players = Some((taken, Instant::now()));
        }
        true
    }

    /// Returns where every player the mod last posted is standing, in blocks,
    /// and who each of them is. Stale data returns nothing, as `body` does,
    /// rather than a position that may no longer be true.
    ///
    /// A player posted twice, once publicly and once to their group, appears
    /// here once.
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



    /// Returns every group the mod last posted, fresh or not. Group membership
    /// does not go stale the way a position does.
    #[must_use]
    pub fn groups(&self) -> HashMap<i32, Group> {
        self.players
            .lock()
            .ok()
            .and_then(|held| held.as_ref().map(|(seen, _)| seen.groups.clone()))
            .unwrap_or_default()
    }

    /// Stores what the world's clock reports.
    ///
    /// Only that the payload is an object is checked. The mod chooses the wording
    /// and the page reads it, and a service that interpreted the words would be a
    /// third place for a date format to disagree.
    pub fn set_world(&self, body: String) -> bool {
        if !body.trim_start().starts_with('{') {
            return false;
        }
        if let Ok(mut world) = self.world.lock() {
            *world = Some((body, Instant::now()));
        }
        true
    }

    /// Stores the markers, writing only when they differ from what is already
    /// held. A post that says nothing new costs no write.
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

        if let Err(error) = self.store.put_markers(&body) {
            warn!("{error}");
        }

        *markers = taken;
        true
    }

    /// Stores the land claims, sorted by who the mod says may see them.
    ///
    /// Unlike the markers these are held in memory only. See [`Claims`] for why a
    /// list that arrives with its own permissions must not be read back off disk
    /// on a later day.
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

    /// Returns the colours the game offers for a marker, for the page's form.
    ///
    /// It is empty until the mod has posted once. A page that asks early gets an
    /// empty picker and asks again.
    #[must_use]
    pub fn colors(&self) -> String {
        self.markers.lock().map_or_else(|_| "[]".to_owned(), |held| held.colors.clone())
    }

    /// Returns what the viewer asks for: who is online, and every marker they
    /// may see.
    ///
    /// Who is asking decides which markers those are. Everyone gets the ones
    /// their owners share, and somebody logged in also gets their own. A private
    /// marker never leaves this process for a browser that is not its owner's,
    /// because a page cannot be trusted to hide what it has been handed.
    ///
    /// An empty result stays empty. There is no fallback to any file this build
    /// does not write.
    ///
    /// `colors` is everybody's chosen colour by uid, as JSON. It belongs to the
    /// preferences and is carried here because this is the one body every browser
    /// polls.
    #[must_use]
    pub fn body(&self, uid: Option<&str>, colors: &str) -> String {
        // Build who is online, which of them this person may see, and who
        // shares a group with them. A report older than the timeout means the
        // game server is gone, and a dot saying somebody is standing somewhere
        // is worse than no dot at all.
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
                         grouped_with(&seen.groups, uid))
                    })
            })
            .unwrap_or_else(|| ("[]".to_owned(), 0, "[]".to_owned()));

        // Build the markers this person may see, and which of them they keep in
        // sight in game. Pins go only to whoever set them, so the page asks
        // whether this marker is pinned for them rather than searching a list per
        // player.
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

        // Send every claim or none. Unlike the markers there is nothing to join,
        // because a reader is entitled to the whole list or to none of it.
        //
        // This reader's own claim allowance goes out beside the claims and only
        // to them. The form needs it to state a rectangle's cost before asking
        // for it. See the mod's `ClaimAllowance`.
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
            r#"{{"Players":{players},"Online":{online},"Grouped":{grouped},"Colors":{colors},"Waypoints":{markers},"Pins":{pins},"Claims":{claims},"Claiming":{claiming},"Height":{height},"World":{world}}}"#
        )
    }
}

/// Joins what everybody is shown with whatever one person is shown on top.
///
/// Both the players and the markers are built this way, as two JSON arrays joined
/// end to end when there is a second one. It is written once because getting it
/// wrong in either place is the same bug.
fn mine(open: &str, extra: Option<&String>) -> String {
    match extra {
        Some(theirs) => joined(open, theirs),
        None => open.to_owned(),
    }
}

/// Parses the mod's marker envelope no further than it has to be.
///
/// Anything that is not the shape this build posts yields nothing rather than a
/// partial reading. A body that arrived mangled is one whose markers are unknown,
/// and showing some of them would be worse than showing none.
fn sorted(body: &str) -> Option<Markers> {
    // Require an object explicitly. Serde fills a struct from a JSON array as
    // readily as from an object, so without this the bare array an older mod
    // posts would be read as an envelope whose first marker is the colour list.
    // That yields an empty map rather than a refused post.
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

/// Returns everybody who shares a group with one person, as a JSON array of
/// uids.
///
/// This is computed here rather than taken from the mod, so a group the operator
/// hid counts for nothing. See `Config::hidden_groups`. The groups were filtered
/// on the way in, and this reads only what is left. A person is in their own
/// group and a stranger is in nobody's. Offline members are included, because a
/// group is who is in it rather than who is online.
fn grouped_with(groups: &HashMap<i32, Group>, uid: Option<&str>) -> String {
    let Some(uid) = uid.filter(|uid| !uid.is_empty()) else {
        return "[]".to_owned();
    };
    let mut together: Vec<&str> = groups
        .values()
        .filter(|group| group.members.contains(uid))
        .flat_map(|group| group.members.iter().map(String::as_str))
        .chain(std::iter::once(uid))
        .collect();
    together.sort_unstable();
    together.dedup();
    serde_json::to_string(&together).unwrap_or_else(|_| "[]".to_owned())
}

/// Parses the mod's report of who is online no further than it has to be.
///
/// A group whose name the operator hid is dropped here. Every group enters
/// through this one function, so nothing downstream needs to know the hidden list
/// exists.
///
/// An array rather than an object means a mod older than this build, which posted
/// every player to everybody. That is refused rather than read, because the two
/// halves ship as one release under one version, and showing every position to
/// everybody is exactly what an operator turned the setting off to prevent.
fn watching(body: &str, hidden: &[String]) -> Option<Seen> {
    if !body.trim_start().starts_with('{') {
        return None;
    }

    let read: Watching = serde_json::from_str(body).ok()?;

    // Read every position the mod posted this tick, whichever list it sorted a
    // player into. `positions_in` reads the same text that `open` and `owned`
    // below hold as strings, before either takes ownership of it.
    let mut positions = positions_in(read.public.as_deref());
    for list in read.private.values() {
        positions.extend(positions_in(Some(list)));
    }

    let groups = read
        .groups
        .into_iter()
        .filter(|(_, group)| !hidden.contains(&group.name.trim().to_lowercase()))
        .filter_map(|(id, group)| {
            Some((id.parse::<i32>().ok()?, Group { name: group.name, members: group.members.into_iter().collect() }))
        })
        .collect();

    Some(Seen {
        online: read.online,
        open: array(read.public.as_deref()),
        owned: arrays(read.private),
        positions,
        groups,
    })
}

/// Parses the mod's claims no further than they have to be.
///
/// Anything that is not the shape this build posts yields nothing rather than a
/// partial reading, for the reason the markers give. A body that arrived mangled
/// is one whose permissions are unknown, and a claim shown to somebody the mod
/// did not name is the mistake this module exists to prevent.
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

/// Reads one of the mod's by-uid maps, keeping each array as the text that
/// arrived.
///
/// The three maps the mod posts, for whose markers, whose positions and whose
/// group, all have this shape. What is inside an array is never read here. This
/// only checks that each value is an array.
fn arrays(held: HashMap<String, Box<RawValue>>) -> HashMap<String, String> {
    held.into_iter().map(|(uid, list)| (uid, array(Some(&list)))).collect()
}

/// Returns one of the mod's arrays as text, or an empty array when it sent
/// nothing.
fn array(raw: Option<&RawValue>) -> String {
    raw.map(|held| held.get().trim().to_owned())
        .filter(|held| held.starts_with('['))
        .unwrap_or_else(|| "[]".to_owned())
}

/// Joins two JSON arrays into one.
///
/// It works on text, because holding the mod's arrays as they arrived means never
/// looking inside them, and joining two lists end to end does not require it.
fn joined(first: &str, second: &str) -> String {
    match (within(first), within(second)) {
        ("", "") => "[]".to_owned(),
        (only, "") | ("", only) => format!("[{only}]"),
        (first, second) => format!("[{first},{second}]"),
    }
}

/// Returns what is between a JSON array's brackets.
fn within(array: &str) -> &str {
    array
        .trim()
        .strip_prefix('[')
        .and_then(|rest| rest.strip_suffix(']'))
        .unwrap_or("")
        .trim()
}


#[cfg(test)]
mod tests {
    use super::*;

    /// Builds one public marker, one of Ada's and one of Bob's, with Ada
    /// keeping the public one in sight.
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
        let live = Live::load(Arc::new(Store::in_memory()), &[]);
        assert!(live.set_markers(POSTED.to_owned()), "the envelope this build posts is taken");
        live
    }

    #[test]
    fn markers_survive_a_restart_through_the_database() {
        let store = Arc::new(Store::in_memory());
        let first = Live::load(Arc::clone(&store), &[]);
        assert!(first.set_markers(POSTED.to_owned()));
        assert!(first.set_markers(POSTED.to_owned()), "the same post again is taken and costs no write");

        let again = Live::load(Arc::clone(&store), &[]);
        assert!(again.body(Some("uid-ada"), "{}").contains("ada's hoard"), "read back from the database");
    }

    #[test]
    fn a_pin_reaches_whoever_set_it_and_nobody_else() {
        let live = told();

        assert!(live.body(Some("uid-ada"), "{}").contains(r#""Pins":["a"]"#), "Ada is sent her own");
        // Bob and a stranger are shown the marker Ada pinned but told nothing
        // about her keeping it.
        assert!(live.body(Some("uid-bob"), "{}").contains(r#""Pins":[]"#));
        assert!(live.body(None, "{}").contains(r#""Pins":[]"#));
    }

    #[test]
    fn a_post_from_a_mod_that_knows_nothing_of_pins_has_none() {
        let live = Live::load(Arc::new(Store::in_memory()), &[]);
        assert!(live.set_markers(
            r##"{"Colors":[],"Public":[{"Title":"trader","Key":"a"}],"Private":{}}"##.to_owned()));
        // An empty array rather than a hole, because a page cannot parse what
        // it was not sent.
        assert!(live.body(Some("uid-ada"), "{}").contains(r#""Pins":[]"#));
    }

    #[test]
    fn a_private_marker_reaches_its_owner_and_nobody_else() {
        let live = told();

        let ada = live.body(Some("uid-ada"), "{}");
        assert!(ada.contains("ada's hoard"), "Ada is sent her own");
        assert!(!ada.contains("bob's hoard"), "and never Bob's");

        let bob = live.body(Some("uid-bob"), "{}");
        assert!(bob.contains("bob's hoard"));
        assert!(!bob.contains("ada's hoard"));

        // The map stays public, so a stranger is still shown the markers whose
        // owners share them, and only those.
        let stranger = live.body(None, "{}");
        assert!(stranger.contains("trader"));
        assert!(!stranger.contains("hoard"), "nobody's private markers reach a stranger");
    }

    #[test]
    fn everybody_is_shown_what_is_shared() {
        let live = told();
        for who in [None, Some("uid-ada"), Some("uid-bob"), Some("uid-nobody")] {
            assert!(live.body(who, "{}").contains("trader"), "{who:?} is shown the shared marker");
        }
    }

    #[test]
    fn an_owner_is_sent_one_list_a_browser_can_read() {
        let live = told();
        let body: serde_json::Value =
            serde_json::from_str(&live.body(Some("uid-ada"), "{}")).expect("valid JSON");

        let markers = body["Waypoints"].as_array().expect("an array of markers");
        assert_eq!(markers.len(), 2, "the shared one and her own, joined end to end");
        assert_eq!(markers[0]["Title"], "trader");
        assert_eq!(markers[1]["Title"], "ada's hoard");
    }

    #[test]
    fn a_post_this_build_cannot_read_is_refused() {
        let live = Live::load(Arc::new(Store::in_memory()), &[]);

        // A bare array is what an older mod posted. It says nothing about who
        // may see what, and reading it as all-public would decide on the owners'
        // behalf.
        assert!(!live.set_markers(r#"[{"Title":"trader"}]"#.to_owned()));
        assert!(!live.set_markers("not json".to_owned()));
        assert!(!live.body(Some("uid-ada"), "{}").contains("trader"));
    }

    #[test]
    fn a_map_nobody_has_posted_to_still_answers() {
        let live = Live::load(Arc::new(Store::in_memory()), &[]);
        let body: serde_json::Value =
            serde_json::from_str(&live.body(None, "{}")).expect("valid JSON");
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

    /// Checks group-scoped positions. Ada is online and visible to everybody.
    /// Bob is online and visible only to his own group. Cass is in that group and
    /// is offline. Everybody is also in a group some mod made for everybody,
    /// which the operator has hidden.
    fn watched() -> Live {
        let live = Live::load(Arc::new(Store::in_memory()), &["XLib".to_owned()]);
        assert!(live.set_players(
            r#"{"Online":2,
                "Public":[{"Name":"ada","Uid":"a"}],
                "Private":{"b":[{"Name":"bob","Uid":"b"}],"c":[{"Name":"bob","Uid":"b"}]},
                "Groups":{"1":{"Name":"the guild","Members":["b","c"]},
                          "2":{"Name":"xlib","Members":["a","b","c"]}}}"#
                .to_owned()
        ));
        live
    }

    #[test]
    fn a_report_of_who_is_online_goes_stale() {
        let live = watched();
        assert!(live.body(None, "{}").contains("ada"));

        // A stopped game server must not leave a dot standing on the map.
        // Reaching in is the only way to age the report without waiting.
        if let Ok(mut players) = live.players.lock()
            && let Some((_, at)) = players.as_mut()
        {
            *at = Instant::now() - PLAYERS_GOOD_FOR - Duration::from_secs(1);
        }
        let gone = live.body(None, "{}");
        assert!(!gone.contains("ada"), "the players go");
        assert!(gone.contains(r#""Online":0"#), "and so does the count of them");
    }

    #[test]
    fn a_position_only_a_group_may_see_reaches_that_group_and_nobody_else() {
        let live = watched();

        let stranger = live.body(None, "{}");
        assert!(stranger.contains("ada"), "what is public is everybody's");
        assert!(!stranger.contains("bob"), "and what is not, is not");

        // Bob's own browser, and Cass, who is in his group and is offline.
        for uid in ["b", "c"] {
            let theirs = live.body(Some(uid), "{}");
            assert!(theirs.contains("bob"), "{uid} shares a group with bob");
            assert!(theirs.contains("ada"), "and still sees what is public");
        }
    }

    #[test]
    fn the_groups_the_mod_posts_are_read_whole() {
        let live = Live::load(Arc::new(Store::in_memory()), &[]);
        assert!(live.set_players(
            r#"{"Online":0,"Public":[],"Private":{},
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
        // A server that hides where people are standing still reports how busy
        // it is, which is a fact about the server rather than about any player.
        let live = watched();
        for uid in [None, Some("a"), Some("b")] {
            assert!(live.body(uid, "{}").contains(r#""Online":2"#), "{uid:?} is told the count");
        }
    }

    #[test]
    fn who_shares_a_group_is_said_only_to_them() {
        let live = watched();
        assert!(live.body(Some("b"), "{}").contains(r#""Grouped":["b","c"]"#));
        assert!(live.body(None, "{}").contains(r#""Grouped":[]"#), "a stranger is in no group");
        assert!(live.body(Some("a"), "{}").contains(r#""Grouped":["a"]"#), "somebody in none is with themselves");
    }

    #[test]
    fn a_hidden_group_is_no_group_whatever_its_case() {
        // `XLib` was hidden and `xlib` was posted. The group everybody is in
        // must not put ada in with bob, and must not be offered to anybody.
        let live = watched();
        assert!(!live.body(Some("a"), "{}").contains("\"b\""), "ada is not grouped with bob");
        let groups = live.groups();
        assert_eq!(groups.len(), 1, "only the guild is kept");
        assert_eq!(groups[&1].name, "the guild");
    }

    /// Checks that claims reach only the readers the mod named, and that only
    /// Ada may draw one.
    const CLAIMED: &str = r##"{
        "Everyones": false,
        "Height": 256,
        "Claims": [{"Key":"a","Owner":"Ada","OwnerUid":"uid-ada","Areas":[
            {"X1":10,"Z1":10,"X2":20,"Z2":20,"Y1":0,"Y2":256}]}],
        "Seen": ["uid-ada"],
        "Making": {"uid-ada":{"Allowance":262144,"Used":0,"MaxAreas":3,"Areas":0}}
    }"##;

    fn claimed() -> Live {
        let live = Live::load(Arc::new(Store::in_memory()), &[]);
        assert!(live.set_claims(CLAIMED.to_owned()), "the envelope this build posts is taken");
        live
    }

    #[test]
    fn a_claim_reaches_whoever_the_mod_named_and_nobody_else() {
        let live = claimed();

        let ada = live.body(Some("uid-ada"), "{}");
        assert!(ada.contains(r#""Key":"a""#), "Ada was named, so Ada is sent them");

        // A reader the mod did not name is sent an empty list rather than a
        // list to filter in the browser, because a browser cannot be asked to
        // hide what it has already been handed.
        for who in [None, Some("uid-bob")] {
            assert!(
                live.body(who, "{}").contains(r#""Claims":[]"#),
                "{who:?} was not named and is sent no claims"
            );
        }
    }

    #[test]
    fn claims_open_to_everybody_reach_a_stranger() {
        // This is a server that has not narrowed `[claims] view`. The game
        // already sends every claim to every client, so the map showing less
        // would tell players less than the game does.
        let live = Live::load(Arc::new(Store::in_memory()), &[]);
        assert!(live.set_claims(
            r#"{"Everyones":true,"Claims":[{"Key":"a"}],"Seen":[],"Making":{}}"#.to_owned()
        ));
        for who in [None, Some("uid-bob")] {
            assert!(live.body(who, "{}").contains(r#""Key":"a""#), "{who:?} is shown an open claim");
        }
    }

    #[test]
    fn what_somebody_may_claim_is_said_to_them_alone() {
        let live = claimed();

        let ada = live.body(Some("uid-ada"), "{}");
        assert!(ada.contains(r#""Allowance":262144"#), "Ada is told her own allowance");
        assert!(ada.contains(r#""Height":256"#), "and how tall the world is");

        // How much land somebody has left goes only to them, and a reader who
        // may not claim at all is told nothing rather than zero.
        for who in [None, Some("uid-bob")] {
            assert!(
                live.body(who, "{}").contains(r#""Claiming":null"#),
                "{who:?} may not claim, so there is nothing to tell them"
            );
        }
    }

    #[test]
    fn a_claim_post_this_build_cannot_read_is_refused() {
        let live = Live::load(Arc::new(Store::in_memory()), &[]);
        assert!(!live.set_claims(r#"[{"Key":"a"}]"#.to_owned()), "a bare array is not the envelope");
        assert!(!live.set_claims("not json".to_owned()));
        assert!(live.body(Some("uid-ada"), "{}").contains(r#""Claims":[]"#));
    }

    #[test]
fn a_report_in_the_shape_an_older_mod_posted_is_refused() {
        // A bare array meant every player to everybody. Reading it that way
        // would be exactly what an operator turned the setting off to prevent.
        let live = Live::load(Arc::new(Store::in_memory()), &[]);
        assert!(!live.set_players(r#"[{"Name":"ada"}]"#.to_owned()));
        assert!(!live.body(None, "{}").contains("ada"));
    }
}
