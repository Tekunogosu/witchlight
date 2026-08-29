//! Noticing that the mod has written something.
//!
//! One thread, on one clock. This used to run on every request, which was a
//! filesystem check per tile and — with more than one thread taking requests —
//! two of them racing to reload the same regions and bumping the generation
//! twice for one export, so every viewer repainted twice.
//!
//! Every file here is watched the same way: its own timestamp is the cheap gate,
//! and only a timestamp that moved earns a read. The common tick is three stat
//! calls and nothing else.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use crate::columns::Region;
use crate::files;
use crate::palette::Palette;
use crate::state::{State, region_times};

/// How often to look for a newer export. The mod writes at most every thirty
/// seconds, so this is far more attentive than it needs to be and still costs
/// three stat calls when nothing has changed.
const WATCH_EVERY: Duration = Duration::from_secs(1);

/// How often the levels above zero are rebuilt from what has changed.
///
/// Slower than the watcher on purpose. A region that changes twice in this window
/// costs one rebuild of everything above it rather than two, and the levels are
/// what someone zoomed out is looking at — a second late is not noticeable, and
/// rebuilding eleven levels per change would be.
const BUILD_EVERY: Duration = Duration::from_secs(2);

/// Starts the two background clocks: one that reads what changed, one that
/// redraws what it changed.
pub fn start(state: &Arc<State>) {
    every(WATCH_EVERY, Arc::clone(state), State::refresh);
    every(BUILD_EVERY, Arc::clone(state), State::build_levels);
}

fn every(period: Duration, state: Arc<State>, work: fn(&State)) {
    std::thread::spawn(move || {
        loop {
            work(&state);
            std::thread::sleep(period);
        }
    });
}

/// Where the mod writes what the game calls each block.
#[must_use]
pub fn names_path(data: &Path) -> std::path::PathBuf {
    data.join("blocknames.json")
}

/// What the game calls each block, as the mod last exported it.
///
/// No file is no names, which is the state every server is in before the mod has
/// exported once: everything that asks falls back to the block's code, which is
/// what the page showed before there were names at all.
///
/// A file that will not parse is a different answer — `None` — and the difference
/// matters. Reading one as "no names" replaces a good table with an empty one and
/// records the file as seen, so the names would stay gone until the mod set
/// changed, which on a settled server is never.
pub fn block_names(data: &Path) -> Option<HashMap<String, String>> {
    match std::fs::read_to_string(names_path(data)) {
        Ok(body) => serde_json::from_str(&body).ok(),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Some(HashMap::new()),
        Err(_) => None,
    }
}

impl State {
    /// Picks up whatever has changed on disk.
    pub fn refresh(&self) {
        self.refresh_palette();
        self.refresh_names();
        self.refresh_world();
        // The mod writes the world's facts once it has a world, which on a cold
        // start is after this has already been read for the first time. Taken
        // again on the beat rather than at start-up only, so a service that came
        // up first still learns where the sea is.
        self.resettle_sea_level();
    }

    /// Rereads the block names when the mod has written them again.
    fn refresh_names(&self) {
        if !moved(&self.named, files::modified(&names_path(&self.data))) {
            return;
        }

        let Some(names) = block_names(&self.data) else {
            // Caught mid-write, or written by something that is not the mod.
            // Unseen again, so the next tick tries rather than leaving the names
            // as they were and never looking at the file again.
            forget(&self.named);
            return;
        };

        let count = names.len();
        if let Ok(mut held) = self.names.write() {
            *held = names;
        }
        println!("witchlight: block names reloaded from disk — {count} named");
    }

    /// Takes a new palette when one appears. Colours change for every tile, so
    /// this drops the cache exactly as a world reload does.
    fn refresh_palette(&self) {
        if !moved(&self.painted, files::modified(&crate::palette::path_in(&self.data))) {
            return;
        }

        // A palette being written as it is read is not worth complaining about —
        // but it is worth looking at again. Left recorded as seen, a palette that
        // failed to parse once would never be read again on a server whose mod
        // set has settled, and the map would go on drawing bare ground.
        let Ok(palette) = Palette::load(&self.data) else {
            forget(&self.painted);
            return;
        };

        // A file written again with the same colours in it is not a new palette.
        // Reloading one costs every tile in the cache and a redraw of every stored
        // level, which is seconds of blank map — so the timestamp moving is what
        // prompts a look, and the colours themselves are what decides.
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

        // Every stored level is drawn from level 0, so redrawing them against a
        // palette with no colours replaces a map that works with a blank one —
        // and the old pictures are the only thing left to look at until a real
        // palette arrives. The pyramid is left exactly as it is.
        if blank {
            let generation = self.bump(None);
            eprintln!(
                "witchlight: the palette that just arrived has no colours at all \
                 (source {source}). The stored zoom levels are being kept as they are \
                 and the finest level will not draw until a usable palette arrives — \
                 an admin joining the game supplies one."
            );
            println!("witchlight: generation {generation}, tiles dropped");
            return;
        }

        if let Ok(world) = self.world.read() {
            self.mark_stale(world.regions());
        }

        let generation = self.bump(None);
        println!(
            "witchlight: palette reloaded from disk — {named} blocks, source {source} \
             (generation {generation}, tiles dropped)"
        );
        self.report_coverage();
    }

    /// Reloads the regions the mod has rewritten, and forgets the ones it removed.
    fn refresh_world(&self) {
        let current = files::modified(&self.columns);
        let Ok(mut seen) = self.seen.lock() else {
            return;
        };
        if current == *seen {
            return;
        }

        let (touched, incomplete) = self.reload_regions();

        // Leaving the directory unseen sends the next look back for whatever was
        // half-written this time. The lock is held across the reload, so this is
        // the only thread that can be doing any of it.
        *seen = if incomplete { None } else { current };
        drop(seen);

        if touched.is_empty() {
            return;
        }

        // Slope shading reads the column to the west and the one to the north, so
        // a region also changes the western edge of the tile east of it and the
        // northern edge of the tile below. A region is a level 0 tile, so these
        // are tile coordinates already.
        let mut repaint: Vec<(i32, i32)> = touched
            .iter()
            .flat_map(|&(rx, rz)| [(rx, rz), (rx + 1, rz), (rx, rz + 1)])
            .collect();
        repaint.sort_unstable();
        repaint.dedup();

        self.drop_tiles(&repaint.iter().map(|&(x, z)| (0, x, z)).collect::<Vec<_>>());

        // Handed to the builder, which announces the change once it has rebuilt
        // the levels above as well. Announcing it here too would announce it
        // twice: the builder follows two seconds later and bumps the generation
        // again, and since the generation versions every tile URL, that is the
        // same pixels fetched under two different names. On a dense world a tile
        // is a third of a megabyte, so the second fetch is not free and the swap
        // is visible.
        self.mark_stale(repaint);

        // Coverage is a pass over every column in the world, which is worth it
        // when the palette changes because that changes every tile. A region
        // arriving changes one square, so it is reported by count alone.
        println!(
            "witchlight: {} regions reloaded — {} chunks",
            touched.len(),
            self.chunks()
        );
    }

    /// Takes the regions whose files have moved, and drops the ones that have
    /// gone. Says which squares changed, and whether anything was caught being
    /// written and must be tried again.
    fn reload_regions(&self) -> (Vec<(i32, i32)>, bool) {
        let now = region_times(&self.columns);
        let Ok(mut held) = self.regions.lock() else {
            return (Vec::new(), true);
        };

        let mut touched = Vec::new();
        let mut incomplete = false;

        for (at, time) in &now {
            if held.get(at) == Some(time) {
                continue;
            }

            // A region being written as it is read is not worth complaining
            // about, but it must be tried again rather than remembered as done.
            match Region::read(&self.columns.join(format!("r.{}.{}.msqr", at.0, at.1))) {
                Ok(region) => {
                    if let Ok(mut world) = self.world.write() {
                        world.apply(region);
                    }
                    held.insert(*at, *time);
                    touched.push(*at);
                }
                Err(_) => incomplete = true,
            }
        }

        for at in held.keys().copied().collect::<Vec<_>>() {
            if !now.contains_key(&at) {
                if let Ok(mut world) = self.world.write() {
                    world.forget(at);
                }
                held.remove(&at);
                touched.push(at);
            }
        }

        (touched, incomplete)
    }
}

/// A file this watcher is following, by the timestamp it was last read at.
type Watched = std::sync::Mutex<Option<SystemTime>>;

/// Whether a watched file's timestamp has moved, taking the new one when it has.
///
/// The gate every reload here sits behind, so "has this changed" is one question
/// with one answer rather than the same six lines written three times.
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

/// Puts a watched file back to never having been read, so the next tick reads it
/// again. What a reload that could not finish leaves behind.
fn forget(held: &Watched) {
    if let Ok(mut held) = held.lock() {
        *held = None;
    }
}
