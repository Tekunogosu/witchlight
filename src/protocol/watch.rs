//! Notices when the mod has written new palette, block name or world fact data.
//!
//! These three arrive as files. They are small, change rarely, and are worth
//! having when the mod is off. Terrain arrives over the API channel and goes
//! into the database instead. See [`crate::protocol::apiport`] and
//! [`crate::mapdata::store`].
//!
//! One thread polls on one interval. Every file is watched the same way: its
//! timestamp is the cheap gate, and only a timestamp that moved earns a read.
//! The common tick is three stat calls and nothing else.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use crate::util::files;
use crate::render::palette::Palette;
use crate::state::State;
use crate::util::log::{say, warn};

/// The interval between checks for a newer palette or set of names. A tick with
/// nothing changed costs three stat calls.
const WATCH_EVERY: Duration = Duration::from_secs(1);

/// The interval between rebuilds of the levels above zero.
///
/// This is deliberately slower than the watcher. A region that changes twice in
/// one window costs one rebuild of everything above it rather than two. A
/// zoomed-out viewer does not notice a second's delay, but would notice eleven
/// levels rebuilt per change.
const BUILD_EVERY: Duration = Duration::from_secs(2);

/// The interval between telling browsers which level 0 tiles changed.
///
/// Ground arrives a few chunks at a time, several times a second while the map
/// fills in. Announcing each arrival separately would make a browser looking at
/// that ground refetch the same half-megabyte tile for each one. One
/// announcement per interval means one fetch per interval.
const ANNOUNCE_EVERY: Duration = Duration::from_secs(1);

/// The interval between offering the level tiles waiting in memory to disk.
/// `levels.rs` decides which of them are written.
const FLUSH_EVERY: Duration = Duration::from_secs(5);

/// The interval between logging how many tiles went out and how many were
/// drawn.
const REPORT_EVERY: Duration = Duration::from_secs(60);

/// Starts the background threads. One reads what changed, one tells browsers,
/// one redraws the levels above, and one writes them to disk.
pub fn start(state: &Arc<State>) {
    every(WATCH_EVERY, Arc::clone(state), State::refresh);
    every(ANNOUNCE_EVERY, Arc::clone(state), State::announce);
    every(BUILD_EVERY, Arc::clone(state), State::build_levels);
    every(FLUSH_EVERY, Arc::clone(state), State::flush_levels);
    every(REPORT_EVERY, Arc::clone(state), State::report_serving);
}

fn every(period: Duration, state: Arc<State>, work: fn(&State)) {
    std::thread::spawn(move || {
        loop {
            work(&state);
            std::thread::sleep(period);
        }
    });
}

/// Returns the path the mod writes block names to.
#[must_use]
pub fn names_path(data: &Path) -> std::path::PathBuf {
    data.join("blocknames.json")
}

/// Reads the block names the mod last exported.
///
/// A missing file yields an empty table, which is the state of every server
/// before the mod has exported once. Callers then fall back to the block's code.
///
/// A file that will not parse returns `None` instead. The difference matters:
/// treating it as an empty table would replace a good table with nothing and
/// record the file as seen, so the names would stay gone until the mod set
/// changed, which on a settled server is never.
pub fn block_names(data: &Path) -> Option<HashMap<String, String>> {
    match std::fs::read_to_string(names_path(data)) {
        Ok(body) => serde_json::from_str(&body).ok(),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Some(HashMap::new()),
        Err(_) => None,
    }
}

impl State {
    /// Reloads whatever has changed on disk.
    pub fn refresh(&self) {
        self.refresh_palette();
        self.refresh_names();
        // The mod writes the world's facts once it has a world, which on a cold
        // start is after this service first read them. Re-read them on every
        // tick, so a service that came up first still learns the sea level.
        self.resettle_sea_level();
    }

    /// Re-reads the block names when the mod has rewritten them.
    fn refresh_names(&self) {
        if !moved(&self.named, files::modified(&names_path(&self.data))) {
            return;
        }

        let Some(names) = block_names(&self.data) else {
            // The file was caught mid-write, or written by something other
            // than the mod. Mark it unseen so the next tick retries rather than
            // leaving the names as they are and never reading the file again.
            forget(&self.named);
            return;
        };

        let count = names.len();
        if let Ok(mut held) = self.names.write() {
            *held = names;
        }
        say!("block names reloaded from disk — {count} named");
    }

    /// Loads a new palette when one appears. New colours change every tile, so
    /// this drops the cache as a world reload does.
    fn refresh_palette(&self) {
        if !moved(&self.painted, files::modified(&crate::render::palette::path_in(&self.data))) {
            return;
        }

        // A palette read while it was being written is not worth a warning, but
        // it is worth reading again. Recorded as seen, a palette that failed to
        // parse once would never be read again on a server whose mod set has
        // settled, and the map would keep drawing bare ground.
        let Ok(palette) = Palette::load(&self.data) else {
            forget(&self.painted);
            return;
        };

        // A file rewritten with the same colours is not a new palette. Reloading
        // one costs every tile in the cache and a redraw of every stored level,
        // which leaves the map blank for seconds. The timestamp moving prompts a
        // read, and the colours themselves decide whether to reload.
        if self.palette.read().is_ok_and(|held| held.same_as(&palette)) {
            return;
        }

        let (named, source, blank) =
            (palette.named, palette.source.clone(), palette.paints_nothing());
        if let Ok(mut held) = self.palette.write() {
            *held = palette;
        }
        if let Ok(mut cache) = self.cache.lock() {
            cache.clear();
        }
        self.forget_patches();

        // Every stored level is drawn from level 0, so redrawing them against a
        // palette with no colours would replace a working map with a blank one.
        // The old pictures are the only thing left to look at until a real
        // palette arrives, so leave the pyramid alone.
        if blank {
            let generation = self.bump(None);
            // Log one message rather than two. This runs on the watcher's
            // thread beside others that log, and a fault split across two calls
            // can arrive with another thread's output between the halves.
            warn!(
                "the palette that just arrived has no colours at all (source {source}). \
                 The stored zoom levels are being kept as they are and the finest level \
                 will not draw until a usable palette arrives — an admin joining the \
                 game supplies one. Generation {generation}, tiles dropped."
            );
            return;
        }

        if let Ok(world) = self.world.read() {
            self.mark_stale(world.regions());
        }

        let generation = self.bump(None);
        say!(
            "palette reloaded from disk — {named} blocks, source {source} \
             (generation {generation}, tiles dropped)"
        );
        self.report_coverage();
    }
}

/// Tracks one watched file by the timestamp it was last read at.
type Watched = std::sync::Mutex<Option<SystemTime>>;

/// Reports whether a watched file's timestamp moved, and records the new one
/// when it has. Every reload in this module sits behind this gate.
fn moved(held: &Watched, current: Option<SystemTime>) -> bool {
    let Ok(mut held) = held.lock() else {
        return false;
    };
    if current == *held {
        return false;
    }
    *held = current;
    true
}

/// Marks a watched file as never read, so the next tick reads it again. A
/// reload that could not finish calls this.
fn forget(held: &Watched) {
    if let Ok(mut held) = held.lock() {
        *held = None;
    }
}
