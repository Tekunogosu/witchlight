//! The parts of the map that move.
//!
//! Players and markers arrive from the server mod over the API socket and are
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

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// How long a report of who is online stays believable.
///
/// Without this a game server that stops — crashed, killed, shut down — leaves
/// its last positions on the map forever, and a dot that says someone is standing
/// somewhere is worse than no dot at all.
const PLAYERS_GOOD_FOR: Duration = Duration::from_secs(30);

pub struct Live {
    players: Mutex<Option<(String, Instant)>>,
    markers: Mutex<String>,
    /// Where markers are kept so they survive both programs stopping.
    path: PathBuf,
}

impl Live {
    /// Reads back whatever markers a previous run was told about.
    #[must_use]
    pub fn load(exports: &Path) -> Self {
        let path = markers_path(exports);
        let markers = std::fs::read_to_string(&path)
            .ok()
            .filter(|body| is_json_array(body))
            .unwrap_or_else(|| "[]".to_owned());

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
        if !is_json_array(&body) {
            return false;
        }

        let Ok(mut markers) = self.markers.lock() else {
            return true;
        };
        if *markers == body {
            return true;
        }

        // Beside itself and then into place, so a reader never sees half of it.
        let temporary = self.path.with_extension("part");
        if std::fs::write(&temporary, &body)
            .and_then(|()| std::fs::rename(&temporary, &self.path))
            .is_err()
        {
            eprintln!("mapstique: could not write {}", self.path.display());
        }

        *markers = body;
        true
    }

    /// What the viewer asks for: who is online, and every marker.
    ///
    /// Empty is empty. There was once a fallback to the `live.json` an older mod
    /// wrote, and it disguised a mod that posted no markers as a map that merely
    /// had none — the map filled in from a stale file the moment the game server
    /// stopped and its players expired. Nothing here reads what this build does
    /// not write.
    #[must_use]
    pub fn body(&self) -> String {
        let players = self
            .players
            .lock()
            .ok()
            .and_then(|held| held.clone())
            .filter(|(_, at)| at.elapsed() < PLAYERS_GOOD_FOR)
            .map(|(body, _)| body);

        let markers = self.markers.lock().map_or_else(|_| "[]".to_owned(), |held| held.clone());

        let players = players.unwrap_or_else(|| "[]".to_owned());
        format!(r#"{{"Players":{players},"Waypoints":{markers}}}"#)
    }
}

#[must_use]
pub fn markers_path(exports: &Path) -> PathBuf {
    exports.join("markers.json")
}

/// Where the mod posts, unless told otherwise.
///
/// In `/tmp`, because a socket is how two programs talk and not something either
/// of them keeps: it belongs with the running system rather than beside the map.
/// That also keeps it far inside the hundred-odd bytes a socket address holds,
/// which a data directory several levels deep does not.
///
/// The name carries a hash of the export directory so that two game servers on
/// one machine do not land on the same socket. Both sides derive it the same way
/// from the path they already agree on, so neither needs configuring — and both
/// print what they resolved, because a mismatch is otherwise silent.
#[must_use]
pub fn default_api_socket(exports: &Path) -> PathBuf {
    let full = std::path::absolute(exports).unwrap_or_else(|_| exports.to_path_buf());
    let key = full.to_string_lossy();
    PathBuf::from(format!("/tmp/mapstique-{:08x}.sock", tag(key.trim_end_matches('/'))))
}

/// FNV-1a, 32 bits. Short, and simple enough that the mod computes the same
/// number from the same path without either side sharing code with the other.
fn tag(text: &str) -> u32 {
    let mut hash: u32 = 0x811c_9dc5;
    for byte in text.as_bytes() {
        hash ^= u32::from(*byte);
        hash = hash.wrapping_mul(0x0100_0193);
    }
    hash
}

/// Enough of a check to keep a mangled body from reaching a browser as the map's
/// own data. What is inside is the mod's business.
fn is_json_array(body: &str) -> bool {
    let body = body.trim();
    body.starts_with('[') && body.ends_with(']')
}
