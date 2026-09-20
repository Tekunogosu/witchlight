//! Stores what each person has set for themselves.
//!
//! Settings that belong to the person live here. Presets, and whether a new
//! marker is private or becomes a preset, follow a uid, should be the same on a
//! phone and a desktop, and would be lost work if a cleared cache took them.
//! Settings that belong to the screen, such as how large the page draws its
//! panels, stay in the browser instead, because they differ per machine.
//!
//! These are held in memory and written to the map's database. The data is
//! small, a handful of presets per person who has used the form, and nothing
//! else could give it back. A run that changes nothing writes nothing.

use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex};

use crate::util::log::warn;
use crate::mapdata::store::Store;

pub use crate::render::pyramid::TileFormat;

/// The most markers one person may hide. This is more than a server has
/// markers.
const MOST_HIDDEN: usize = 5000;

/// The most presets one person may keep. It is far past a working set, and it
/// stops a page in a loop from growing this data without end.
const MOST_PRESETS: usize = 200;

/// The most hotkeys one person may rebind. The page offers under a dozen
/// actions, and the remaining room covers actions a later build adds.
const MOST_HOTKEYS: usize = 64;

/// The longest a stored string may be. Names, patterns and icon names all
/// arrive here from a browser, and none of them is a paragraph.
const LONGEST_WORD: usize = 128;

/// Describes how to fill a marker form in when somebody marks a particular
/// thing.
///
/// The page matches the pattern against a block code such as
/// `game:ore-bountiful-nativecopper-basalt`. The page is the only side holding
/// both the code under the pointer and the presets to try against it. A `*`
/// stands for any run of characters, so a preset saved against copper ore in one
/// rock can be widened by hand to every rock it appears in.
///
/// An empty pattern names no block. A preset with one never matches anything and
/// is reached only by picking it from the list by hand.
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct Preset {
    pub pattern: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub icon: String,
    #[serde(default)]
    pub color: String,
    /// Makes markers from this preset their owner's alone. Absent means the
    /// person's own default decides, which suits a preset about what a thing is
    /// called rather than who may see it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub private: Option<bool>,
}

/// Holds one person's settings.
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct Person {
    #[serde(default)]
    pub presets: Vec<Preset>,

    /// Makes a new marker private when this person has decided so. Absent means
    /// the operator's setting decides, which is where everybody starts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub private_by_default: Option<bool>,

    /// Starts the form's "remember this" box ticked.
    #[serde(default)]
    pub presets_by_default: bool,

    /// Makes the map follow this person's own player as soon as it knows who
    /// they are.
    ///
    /// This belongs to the person rather than the screen. Somebody who wants the
    /// map to open on where they are standing wants that wherever they open it.
    #[serde(default)]
    pub follow_self: bool,

    /// Names which groups this person shares their map with, by the id the game
    /// gives each group. Every group starts off, because what somebody has
    /// explored is theirs and the game put them in the group.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub share_map_with: Vec<i32>,
    /// Sets how this person's tiles are encoded, exact or compact. See
    /// [`TileFormat`].
    #[serde(default)]
    pub tile_format: TileFormat,

    /// The colour this person is drawn in on the map, as `#rrggbb`. It applies
    /// to their own mark and every claim of theirs. Empty means the default
    /// colour.
    ///
    /// Every person's colour is sent to every browser, so the same land is the
    /// same colour on every screen.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub color: String,

    /// Names markers this person has chosen not to see on their own map, by
    /// key. It affects only them, and nothing anybody else is sent changes with
    /// it. Empty means every marker is shown.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hidden_markers: Vec<String>,

    /// Maps each rebound action's name to the key the browser reports for the
    /// press, such as `c`, `P` or `F2`. An empty key means the action is unbound
    /// on purpose. An action absent here keeps the page's default. These follow
    /// the person to every machine, which is why they live here rather than in
    /// the browser with the panel sizes.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub hotkeys: BTreeMap<String, String>,
}

impl Person {
    /// Returns the same settings with everything a browser sent brought inside
    /// the stored bounds.
    ///
    /// Values are trimmed rather than refused. What arrives is somebody's own
    /// settings, and discarding all of it because one pattern was too long would
    /// lose their presets to a typo.
    fn sane(mut self) -> Self {
        self.presets.truncate(MOST_PRESETS);
        for preset in &mut self.presets {
            clip(&mut preset.pattern);
            clip(&mut preset.title);
            clip(&mut preset.icon);
            clip(&mut preset.color);
        }
        self.color = crate::util::text::hex_colour(&self.color).unwrap_or_default();
        // A key is a guid, so anything longer is not one. A list longer than
        // the map could hold means the browser sent something other than
        // settings.
        self.hidden_markers.truncate(MOST_HIDDEN);
        self.hidden_markers.retain(|key| !key.is_empty() && key.len() <= LONGEST_WORD);
        self.hidden_markers.sort();
        self.hidden_markers.dedup();
        // An action's name is a short word the page chose, and a key is what
        // one press reports. Neither is a paragraph, and a map larger than the
        // page's action list means the browser sent something other than
        // settings.
        self.hotkeys.retain(|action, _| !action.is_empty() && action.len() <= LONGEST_WORD);
        for key in self.hotkeys.values_mut() {
            clip(key);
        }
        while self.hotkeys.len() > MOST_HOTKEYS {
            self.hotkeys.pop_last();
        }
        self
    }
}

fn clip(word: &mut String) {
    let trimmed = word.trim();
    if trimmed.chars().count() > LONGEST_WORD {
        *word = trimmed.chars().take(LONGEST_WORD).collect();
    } else if trimmed.len() != word.len() {
        *word = trimmed.to_owned();
    }
}

/// Holds everyone's settings, keyed by uid.
pub struct Preferences {
    held: Mutex<HashMap<String, Person>>,
    store: Arc<Store>,
}

impl Preferences {
    /// Loads whatever a previous run stored.
    ///
    /// A row this build cannot read is treated as that person having set
    /// nothing, which is where everybody starts. The form still works on the
    /// operator's defaults.
    #[must_use]
    pub fn load(store: Arc<Store>) -> Self {
        let held = store
            .preferences()
            .unwrap_or_else(|error| {
                warn!("{error}");
                Vec::new()
            })
            .into_iter()
            .filter_map(|(uid, body)| serde_json::from_str::<Person>(&body).ok().map(|person| (uid, person)))
            .collect();
        Self { held: Mutex::new(held), store }
    }

    /// Returns everybody who has set anything, so a start knows who shares with
    /// whom before the first request arrives.
    #[must_use]
    pub fn all(&self) -> Vec<(String, Person)> {
        self.held
            .lock()
            .map(|held| held.iter().map(|(uid, person)| (uid.clone(), person.clone())).collect())
            .unwrap_or_default()
    }

    /// Returns what one person has set. Everybody has an answer, whether or not
    /// they have ever set anything.
    #[must_use]
    pub fn of(&self, uid: &str) -> Person {
        self.held.lock().ok().and_then(|held| held.get(uid).cloned()).unwrap_or_default()
    }

    /// Returns everybody who chose a colour, keyed by uid, as one JSON object
    /// for the live feed to carry.
    #[must_use]
    pub fn colors(&self) -> String {
        let chosen: std::collections::BTreeMap<String, String> = self
            .held
            .lock()
            .map(|held| {
                held.iter()
                    .filter(|(_, person)| !person.color.is_empty())
                    .map(|(uid, person)| (uid.clone(), person.color.clone()))
                    .collect()
            })
            .unwrap_or_default();
        serde_json::to_string(&chosen).unwrap_or_else(|_| "{}".to_owned())
    }

    /// Returns how one person's tiles are encoded. This is asked on every tile,
    /// so it reads in place rather than copying everything they have set.
    #[must_use]
    pub fn tile_format_of(&self, uid: &str) -> TileFormat {
        self.held.lock().ok().and_then(|held| held.get(uid).map(|person| person.tile_format)).unwrap_or_default()
    }

    /// Stores one person's settings, writing only when they differ from what is
    /// already held. Returns whether they were accepted.
    pub fn set(&self, uid: &str, person: Person) -> bool {
        if uid.is_empty() {
            return false;
        }
        let person = person.sane();

        let body = {
            let Ok(mut held) = self.held.lock() else {
                return false;
            };
            if held.get(uid) == Some(&person) {
                return true;
            }
            let body = serde_json::to_string(&person).unwrap_or_default();
            held.insert(uid.to_owned(), person);
            body
        };

        if body.is_empty() {
            return false;
        }

        // Write one person's row, and only when theirs changed. The comparison
        // above keeps a settings window saved twice from costing two writes.
        if let Err(error) = self.store.put_preferences(uid, &body) {
            warn!("{error}");
        }
        true
    }

    /// Stores one preset for somebody and returns everything they have set.
    ///
    /// A browser sends and receives the whole document, because it holds all of
    /// it. A game client holds none of it and knows only the preset just made in
    /// front of the player, so it uses this instead. A read-modify-write across
    /// a network channel and a game tick would write the document back as it
    /// looked when the window opened.
    ///
    /// Presets are keyed on the pattern, so making a preset for a block that
    /// already has one replaces it rather than adding a second that can never be
    /// reached. The map's own form follows the same rule.
    pub fn keep_one(&self, uid: &str, preset: Preset) -> Person {
        let mut person = self.of(uid);
        person.presets.retain(|held| held.pattern != preset.pattern);
        person.presets.insert(0, preset);
        self.set(uid, person);
        self.of(uid)
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    fn ferns() -> Person {
        Person {
            presets: vec![Preset {
                pattern: "game:*fern*".to_owned(),
                title: "Fern".to_owned(),
                icon: "circle".to_owned(),
                color: "#47b749".to_owned(),
                private: Some(true),
            }],
            private_by_default: Some(true),
            presets_by_default: true,
            follow_self: true,
            share_map_with: Vec::new(),
            tile_format: TileFormat::Png,
            color: String::new(),
            hidden_markers: Vec::new(),
            hotkeys: BTreeMap::new(),
        }
    }

    fn store() -> Preferences {
        Preferences::load(Arc::new(Store::in_memory()))
    }

    #[test]
    fn a_colour_is_kept_only_as_a_hex_triplet() {
        for (said, kept) in [("#A1B2C3", "#a1b2c3"), (" #a1b2c3 ", "#a1b2c3"), ("red", ""), ("#abc", ""), ("#a1b2c3d4", ""), ("", "")] {
            assert_eq!(crate::util::text::hex_colour(said).unwrap_or_default(), kept, "{said:?}");
        }
    }

    #[test]
    fn everybody_who_chose_a_colour_is_listed_by_uid() {
        let prefs = store();
        assert!(prefs.set("ada", Person { color: "#A1B2C3".to_owned(), ..Person::default() }));
        assert!(prefs.set("bob", Person::default()));
        assert_eq!(prefs.colors(), r##"{"ada":"#a1b2c3"}"##, "bob chose nothing and is not listed");
    }

    #[test]
    fn somebody_who_has_set_nothing_still_has_an_answer() {
        let held = store().of("uid-ada");
        assert!(held.presets.is_empty());
        assert_eq!(held.private_by_default, None, "the operator's setting decides");
        assert!(!held.presets_by_default);
    }

    #[test]
    fn what_one_person_sets_is_theirs_alone() {
        let preferences = store();
        assert!(preferences.set("uid-ada", ferns()));

        assert_eq!(preferences.of("uid-ada"), ferns());
        assert_eq!(preferences.of("uid-bob"), Person::default(), "and nobody else's");
    }

    #[test]
    fn nobody_is_not_a_person() {
        assert!(!store().set("", ferns()), "a session with no uid sets nothing");
    }

    #[test]
    fn one_preset_kept_from_a_game_client_replaces_the_one_it_names() {
        let preferences = store();
        preferences.set("uid-ada", ferns());

        let moss = Preset {
            pattern: "game:*moss*".to_owned(),
            title: "Moss".to_owned(),
            ..Preset::default()
        };
        let held = preferences.keep_one("uid-ada", moss.clone());
        assert_eq!(held.presets, vec![moss.clone(), ferns().presets[0].clone()],
            "a new one goes to the front and leaves the rest alone");

        let widened = Preset { title: "Mossy".to_owned(), ..moss };
        let held = preferences.keep_one("uid-ada", widened.clone());
        assert_eq!(held.presets, vec![widened, ferns().presets[0].clone()],
            "and one for a block that already has a preset replaces it");

        assert_eq!(held.private_by_default, Some(true), "nothing else they set moves");
    }

    #[test]
    fn a_preset_naming_no_block_is_kept_with_an_empty_pattern() {
        let preferences = store();
        let mut asked = ferns();
        asked.presets.push(Preset { pattern: "   ".to_owned(), ..Preset::default() });
        preferences.set("uid-ada", asked);

        let held = preferences.of("uid-ada").presets;
        assert_eq!(held.len(), 2, "a preset that names no block is still a preset");
        assert_eq!(held[1].pattern, "", "and its pattern is stored empty rather than as spaces");
    }

    #[test]
    fn a_rebound_key_follows_the_person_and_an_unbound_one_stays_unbound() {
        let preferences = store();
        let mut asked = ferns();
        asked.hotkeys.insert("marker".to_owned(), "x".to_owned());
        asked.hotkeys.insert("inspect".to_owned(), String::new());
        asked.hotkeys.insert(String::new(), "q".to_owned());
        preferences.set("uid-ada", asked);

        let held = preferences.of("uid-ada").hotkeys;
        assert_eq!(held.get("marker").map(String::as_str), Some("x"));
        assert_eq!(held.get("inspect").map(String::as_str), Some(""), "unbound is a choice, not an absence");
        assert_eq!(held.len(), 2, "an action with no name is nothing to rebind");
    }

    #[test]
    fn a_page_cannot_grow_this_without_end() {
        let preferences = store();
        let many = Person {
            presets: (0..MOST_PRESETS * 3)
                .map(|n| Preset { pattern: format!("game:rock-{n}"), ..Preset::default() })
                .collect(),
            ..Person::default()
        };
        preferences.set("uid-ada", many);
        assert_eq!(preferences.of("uid-ada").presets.len(), MOST_PRESETS);

        let long = Person {
            presets: vec![Preset { pattern: "p".repeat(LONGEST_WORD * 4), ..Preset::default() }],
            ..Person::default()
        };
        preferences.set("uid-bob", long);
        assert_eq!(preferences.of("uid-bob").presets[0].pattern.len(), LONGEST_WORD);
    }

    #[test]
    fn hidden_markers_are_kept_and_tidied() {
        let preferences = Preferences::load(Arc::new(Store::in_memory()));
        let mut person = Person::default();
        person.hidden_markers = vec!["b".to_owned(), "a".to_owned(), "".to_owned(), "a".to_owned()];
        assert!(preferences.set("uid-ada", person));
        assert_eq!(preferences.of("uid-ada").hidden_markers, vec!["a", "b"], "sorted, deduplicated, nothing empty");
        assert!(!serde_json::to_string(&Person::default()).unwrap().contains("HiddenMarkers"),
            "nothing hidden is nothing written");
    }

    #[test]
    fn what_is_written_is_read_back() {
        let store = Arc::new(Store::in_memory());

        let first = Preferences::load(Arc::clone(&store));
        assert!(first.set("uid-ada", ferns()));
        assert!(first.set("uid-bob", Person::default()));

        let again = Preferences::load(Arc::clone(&store));
        assert_eq!(again.of("uid-ada"), ferns(), "a restart keeps what somebody set");
        assert_eq!(again.all().len(), 2, "one row per person");
    }
}
