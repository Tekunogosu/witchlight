//! What the mod says about the world itself.
//!
//! One small file beside the map, written once when the world is up. Read on
//! demand rather than held: it is asked for every few seconds by one page, it is
//! a line long, and holding it would mean noticing when it changed for the sake
//! of a number that changes when a world does.

use std::path::Path;

use serde::Deserialize;

/// What the mod says about the world.
///
/// One file, read as one thing. It was two structs over the same bytes — where
/// the world counts from, and where its oceans sit — so a page asking for one and
/// a tile asking for the other each opened, read and parsed the same file, and a
/// field added to either was a field the other silently did not have.
///
/// Every field defaults, because a mod older than this build wrote none of them.
/// Counting from the origin is what the map did before there was a spawn to ask
/// for, and a sea level of zero puts every column above the sea, which is what the
/// season weight assumed before there was a number for it.
///
/// What the clock says is not here: it changes every second a server is up, and
/// this is a file.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(default, rename_all = "PascalCase")]
pub struct Facts {
    /// Where the game counts from.
    ///
    /// Vintage Story shows coordinates relative to world spawn everywhere a
    /// player sees them, while the world itself is a million blocks across with
    /// spawn somewhere near the middle. A map showing absolute positions is not
    /// wrong, but it does not agree with anything the player can compare it to,
    /// which amounts to the same thing.
    pub spawn_x: i32,
    pub spawn_z: i32,
    /// Where the world's oceans sit, which is what a column's height is measured
    /// against when deciding how much of the season it feels.
    pub sea_level: i32,
}

/// What the mod last said about the world.
#[must_use]
pub fn read(exports: &Path) -> Facts {
    std::fs::read_to_string(path_in(exports))
        .ok()
        .and_then(|body| serde_json::from_str(&body).ok())
        .unwrap_or_default()
}

/// Where `world.json` lives inside the export directory.
#[must_use]
pub fn path_in(exports: &Path) -> std::path::PathBuf {
    exports.join("world.json")
}

/// Whether the mod has written the world's facts at all.
///
/// Apart from the reading, because a file that says nothing and a file that is
/// not there are the same [`Facts`] and are not the same thing to report: one is
/// a world with spawn at the origin, and the other is a mod older than this build.
#[must_use]
pub fn written(exports: &Path) -> bool {
    path_in(exports).exists()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_world_is_read_out_of_what_the_mod_writes() {
        // The mod's own serializer names these, so the spelling is the contract —
        // and it writes fields this does not read, which must not refuse the file.
        let read: Facts = serde_json::from_str(
            r#"{"SpawnX":512035,"SpawnY":110,"SpawnZ":-318,"Name":"Ashlands",
                "Id":"0c4419ae","SeaLevel":110}"#,
        )
        .expect("what the mod writes");
        assert_eq!(read, Facts { spawn_x: 512035, spawn_z: -318, sea_level: 110 });
    }

    #[test]
    fn a_mod_older_than_a_field_still_answers_for_the_rest() {
        // Sea level arrived after spawn did, and a world.json without it is a
        // world whose oceans are unknown rather than a world with no spawn.
        let read: Facts =
            serde_json::from_str(r#"{"SpawnX":10,"SpawnZ":-4}"#).expect("an older file");
        assert_eq!(read, Facts { spawn_x: 10, spawn_z: -4, sea_level: 0 });
    }

    #[test]
    fn a_world_nobody_has_written_facts_for_counts_from_zero() {
        assert_eq!(read(Path::new("/nonexistent")), Facts::default());
        assert!(!written(Path::new("/nonexistent")));
    }
}
