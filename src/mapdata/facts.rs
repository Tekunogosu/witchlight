//! Reads the world facts the mod writes beside the map.
//!
//! The mod writes one small file when the world comes up. This module reads it
//! on demand instead of caching it. The file is one line long and only one page
//! asks for it, so caching would cost an invalidation path for no gain.

use std::path::Path;

use serde::Deserialize;

/// Holds the world facts the mod exported.
///
/// Every field defaults, because a mod older than this build writes none of
/// them. A spawn at the origin and a sea level of zero reproduce the behaviour
/// the map had before those fields existed.
///
/// The in-game clock is not stored here. It changes every second, and this
/// struct comes from a file written once.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(default, rename_all = "PascalCase")]
pub struct Facts {
    /// The absolute position of world spawn.
    ///
    /// Vintage Story shows players coordinates relative to spawn, so the map
    /// subtracts this to display numbers a player can compare against the game.
    pub spawn_x: i32,
    pub spawn_z: i32,
    /// The height of the world's oceans. A column's height is measured against
    /// this when deciding how much of the season it feels.
    pub sea_level: i32,
}

/// Reads the world facts, returning defaults if the file is missing or invalid.
#[must_use]
pub fn read(exports: &Path) -> Facts {
    std::fs::read_to_string(path_in(exports))
        .ok()
        .and_then(|body| serde_json::from_str(&body).ok())
        .unwrap_or_default()
}

/// Returns the path of `world.json` inside the export directory.
#[must_use]
pub fn path_in(exports: &Path) -> std::path::PathBuf {
    exports.join("world.json")
}

/// Reports whether the mod has written the world facts file.
///
/// A missing file and an empty one both read as the default [`Facts`], so
/// callers that must tell them apart check this separately.
#[must_use]
pub fn written(exports: &Path) -> bool {
    path_in(exports).exists()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_world_is_read_out_of_what_the_mod_writes() {
        // The mod's serializer picks these names, so the spelling is the
        // contract. It also writes fields this struct does not read, and those
        // must not make the file fail to parse.
        let read: Facts = serde_json::from_str(
            r#"{"SpawnX":512035,"SpawnY":110,"SpawnZ":-318,"Name":"Ashlands",
                "Id":"0c4419ae","SeaLevel":110}"#,
        )
        .expect("what the mod writes");
        assert_eq!(read, Facts { spawn_x: 512035, spawn_z: -318, sea_level: 110 });
    }

    #[test]
    fn a_mod_older_than_a_field_still_answers_for_the_rest() {
        // Sea level was added after spawn. A world.json without it must still
        // yield the spawn it does carry.
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
