//! What the mod says about the world itself.
//!
//! One small file beside the map, written once when the world is up. Read on
//! demand rather than held: it is asked for every few seconds by one page, it is
//! a line long, and holding it would mean noticing when it changed for the sake
//! of a number that changes when a world does.

use std::path::Path;

use serde::Deserialize;

/// Where the game counts from.
///
/// Vintage Story shows coordinates relative to world spawn everywhere a player
/// sees them, while the world itself is a million blocks across with spawn
/// somewhere near the middle. A map showing absolute positions is not wrong, but
/// it does not agree with anything the player can compare it to, which amounts
/// to the same thing.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct Spawn {
    #[serde(rename = "SpawnX")]
    pub x: i32,
    #[serde(rename = "SpawnZ")]
    pub z: i32,
}

/// What the world is like, beyond where it counts from.
///
/// The one field defaults, because a mod older than this build wrote none. A sea
/// level of zero puts every column above the sea, which is what the season weight
/// assumed before there was a number to ask for. What the clock says is not here:
/// it changes every second a server is up, and this is a file.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct World {
    #[serde(default)]
    pub sea_level: i32,
}

/// What the mod last said about the world.
#[must_use]
pub fn world(exports: &Path) -> World {
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

/// Where the world counts from, as the mod last wrote it.
///
/// The origin where there is no file to read: a mod older than this build wrote
/// none, and the alternative to counting from zero is refusing to draw a map.
/// [`self::written`] is what says which of the two happened.
#[must_use]
pub fn spawn(exports: &Path) -> Spawn {
    std::fs::read_to_string(path_in(exports))
        .ok()
        .and_then(|body| serde_json::from_str(&body).ok())
        .unwrap_or_default()
}

/// Whether the mod has written the world's facts at all.
#[must_use]
pub fn written(exports: &Path) -> bool {
    path_in(exports).exists()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spawn_is_read_out_of_what_the_mod_writes() {
        // The mod's own serializer names these, so the spelling is the contract.
        let read: Spawn = serde_json::from_str(
            r#"{"SpawnX":512035,"SpawnY":110,"SpawnZ":-318,"Name":"Ashlands"}"#,
        )
        .expect("what the mod writes");
        assert_eq!(read, Spawn { x: 512035, z: -318 });
    }

    #[test]
    fn a_world_nobody_has_written_facts_for_counts_from_zero() {
        assert_eq!(spawn(Path::new("/nonexistent")), Spawn::default());
        assert!(!written(Path::new("/nonexistent")));
    }
}
