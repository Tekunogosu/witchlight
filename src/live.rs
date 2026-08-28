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
//! Nothing here parses either payload. The mod knows what a waypoint is; this
//! knows that it is a JSON array to hand to a browser, which is the whole of the
//! contract between them.
//!
//! Markers arrive already sorted into what anyone may see and what only one
//! person may, because deciding that needs to know what a waypoint is and this
//! does not. The arrays inside are never looked into — they are held as they
//! arrived and handed on, and the most that happens to one is being joined to
//! another end to end.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde_json::value::RawValue;

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
        }
    }
}

/// The envelope the mod posts. Only its shape is read; the arrays inside are
/// carried through untouched.
#[derive(serde::Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Sorted {
    #[serde(default)]
    colors: Option<Box<RawValue>>,
    #[serde(default)]
    public: Option<Box<RawValue>>,
    #[serde(default)]
    private: HashMap<String, Box<RawValue>>,
}

pub struct Live {
    players: Mutex<Option<(String, Instant)>>,
    markers: Mutex<Markers>,
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

        Self { players: Mutex::new(None), markers: Mutex::new(markers), path }
    }

    /// Takes a report of who is online. Held in memory only.
    pub fn set_players(&self, body: String) -> bool {
        if !is_json_array(&body) {
            return false;
        }
        if let Ok(mut players) = self.players.lock() {
            *players = Some((body, Instant::now()));
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

        // Beside itself and then into place, so a reader never sees half of it.
        let temporary = self.path.with_extension("part");
        if std::fs::write(&temporary, &body)
            .and_then(|()| std::fs::rename(&temporary, &self.path))
            .is_err()
        {
            eprintln!("witchlight: could not write {}", self.path.display());
        }

        *markers = taken;
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
        let players = self
            .players
            .lock()
            .ok()
            .and_then(|held| held.clone())
            .filter(|(_, at)| at.elapsed() < PLAYERS_GOOD_FOR)
            .map(|(body, _)| body);

        let markers = self.markers.lock().map_or_else(
            |_| "[]".to_owned(),
            |held| match uid.and_then(|uid| held.owned.get(uid)) {
                Some(mine) => joined(&held.open, mine),
                None => held.open.clone(),
            },
        );

        let players = players.unwrap_or_else(|| "[]".to_owned());
        format!(r#"{{"Players":{players},"Waypoints":{markers}}}"#)
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
        owned: read
            .private
            .into_iter()
            .map(|(uid, markers)| (uid, array(Some(&markers))))
            .collect(),
    })
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

/// Enough of a check to keep a mangled body from reaching a browser as the map's
/// own data. What is inside is the mod's business.
fn is_json_array(body: &str) -> bool {
    let body = body.trim();
    body.starts_with('[') && body.ends_with(']')
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One public marker, one of Ada's and one of Bob's.
    const POSTED: &str = r##"{
        "Colors":["#f9d0dc","#ed272a"],
        "Public":[{"Title":"trader","Key":"a"}],
        "Private":{
            "uid-ada":[{"Title":"ada's hoard","Key":"b"}],
            "uid-bob":[{"Title":"bob's hoard","Key":"c"}]
        }
    }"##;

    fn told() -> Live {
        let live = Live::load(Path::new("/nonexistent"));
        assert!(live.set_markers(POSTED.to_owned()), "the envelope this build posts is taken");
        live
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

    #[test]
    fn a_report_of_who_is_online_goes_stale() {
        let live = Live::load(Path::new("/nonexistent"));
        assert!(live.set_players(r#"[{"Name":"ada"}]"#.to_owned()));
        assert!(live.body(None).contains("ada"));

        // A game server that stopped must not leave a dot standing on the map.
        // Reaching in is the only way to age it without waiting out the clock.
        if let Ok(mut players) = live.players.lock()
            && let Some((_, at)) = players.as_mut()
        {
            *at = Instant::now() - PLAYERS_GOOD_FOR - Duration::from_secs(1);
        }
        assert!(!live.body(None).contains("ada"));
    }
}
