//! What each person has set for themselves.
//!
//! Two kinds of thing were candidates for living here and only one of them does.
//! Presets, and whether a new marker is private or becomes a preset, are about
//! the person: they follow a uid, they should be the same on a phone and a
//! desktop, and losing them to a cleared cache would be losing work. Those are
//! kept here. How large the page draws its panels is about the screen in front
//! of somebody, which is a different answer on each of their machines, so that
//! stays in the browser with the rest of the view settings.
//!
//! Held in memory and written to one file. It is small — a handful of presets
//! per person who has ever used the form — and it is the only thing this service
//! owns that nothing else could give it back, which is what earns the write. A
//! run that changes nothing writes nothing.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// The most presets one person may keep. Far past a working set, and there so
/// that a page in a loop cannot grow this file without end.
const MOST_PRESETS: usize = 200;

/// The most a stored word may be. Names, patterns and icon names all pass
/// through here from a browser, and none of them is a paragraph.
const LONGEST_WORD: usize = 128;

/// What to fill a marker form in with, when somebody marks a particular thing.
///
/// The pattern is matched against a block code —
/// `game:ore-bountiful-nativecopper-basalt` — by the page, which is the only
/// side holding both the code under the pointer and the presets to try against
/// it. `*` stands for any run of characters, so a preset saved against copper
/// ore in one rock can be widened by hand to every rock it appears in.
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
    /// Whether markers made from this preset are their owner's alone. Absent
    /// means the person's own default decides, which is the useful answer for a
    /// preset that is about what a thing is called rather than who may see it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub private: Option<bool>,
}

/// One person's choices.
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct Person {
    #[serde(default)]
    pub presets: Vec<Preset>,

    /// Whether a new marker is private, where this person has decided. Absent
    /// means the operator's setting decides, which is where everybody starts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub private_by_default: Option<bool>,

    /// Whether the form's "remember this" box starts ticked.
    #[serde(default)]
    pub presets_by_default: bool,

    /// Whether the map takes up following this person's own player as soon as it
    /// knows who they are.
    ///
    /// About the person rather than the screen, which is what puts it here: it is
    /// the same answer on a phone and a desktop, and somebody who wants the map
    /// to open on where they are standing wants it wherever they open it.
    #[serde(default)]
    pub follow_self: bool,
}

impl Person {
    /// The same choices, with everything a browser sent brought inside the
    /// bounds this file is willing to hold.
    ///
    /// Trimmed rather than refused. What arrives is somebody's own settings, and
    /// throwing the lot away because one pattern was too long would lose their
    /// presets to a typo.
    fn sane(mut self) -> Self {
        self.presets.truncate(MOST_PRESETS);
        for preset in &mut self.presets {
            clip(&mut preset.pattern);
            clip(&mut preset.title);
            clip(&mut preset.icon);
            clip(&mut preset.color);
        }
        // A preset that matches nothing can never be reached and would sit in
        // the window forever being scrolled past.
        self.presets.retain(|preset| !preset.pattern.is_empty());
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

/// Everyone's choices, by uid.
pub struct Preferences {
    held: Mutex<HashMap<String, Person>>,
    path: PathBuf,
}

impl Preferences {
    /// Reads back whatever a previous run stored.
    ///
    /// An unreadable file is nobody having set anything, which is where every
    /// person starts anyway: the form still works, on the operator's defaults.
    #[must_use]
    pub fn load(exports: &Path) -> Self {
        let path = path_in(exports);
        let held = std::fs::read_to_string(&path)
            .ok()
            .and_then(|body| serde_json::from_str::<HashMap<String, Person>>(&body).ok())
            .unwrap_or_default();

        Self { held: Mutex::new(held), path }
    }

    /// What one person has set. Everybody has an answer, whether or not they
    /// have ever set anything.
    #[must_use]
    pub fn of(&self, uid: &str) -> Person {
        self.held.lock().ok().and_then(|held| held.get(uid).cloned()).unwrap_or_default()
    }

    /// Takes one person's choices, and writes them where they are not what is
    /// already held. Answers whether they were taken at all.
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
            held.insert(uid.to_owned(), person);
            serde_json::to_string(&*held).unwrap_or_default()
        };

        if body.is_empty() {
            return false;
        }

        crate::files::publish(&self.path, body.as_bytes());
        true
    }

    /// Keeps one preset for somebody, and gives back everything they have set.
    ///
    /// The whole document goes to and from a browser, which holds the lot and
    /// puts the lot back. A game client holds none of it: it knows one preset,
    /// the one somebody has just made in front of them, and a read-modify-write
    /// spanning a network channel and a game tick would be a document written
    /// back from whatever it looked like when the window opened.
    ///
    /// Keyed on the pattern, so making a preset for a block that already has one
    /// replaces it rather than laying a second one that can never be reached —
    /// which is the rule the map's own form follows.
    pub fn keep_one(&self, uid: &str, preset: Preset) -> Person {
        let mut person = self.of(uid);
        person.presets.retain(|held| held.pattern != preset.pattern);
        person.presets.insert(0, preset);
        self.set(uid, person);
        self.of(uid)
    }
}

#[must_use]
pub fn path_in(exports: &Path) -> PathBuf {
    exports.join("preferences.json")
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
        }
    }

    fn store() -> Preferences {
        Preferences::load(Path::new("/nonexistent"))
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
    fn a_pattern_that_matches_nothing_is_dropped() {
        let preferences = store();
        let mut asked = ferns();
        asked.presets.push(Preset { pattern: "   ".to_owned(), ..Preset::default() });
        preferences.set("uid-ada", asked);

        assert_eq!(preferences.of("uid-ada").presets.len(), 1);
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
    fn what_is_written_is_read_back() {
        let exports = std::env::temp_dir().join(format!("witchlight-{}", crate::random::word(8)));
        std::fs::create_dir_all(&exports).expect("a directory to write into");

        let first = Preferences::load(&exports);
        assert!(first.set("uid-ada", ferns()));

        let again = Preferences::load(&exports);
        assert_eq!(again.of("uid-ada"), ferns(), "a restart keeps what somebody set");

        std::fs::remove_dir_all(&exports).ok();
    }
}
