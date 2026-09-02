//! Asking the mod for one more column, and deciding which one to ask for.
//!
//! Everything that used to decide this lived in the mod, as `Backfill` and
//! `Repair`: which columns the savegame might hold that the map has never drawn,
//! in what order, and how fast to ask the server to load them. The mod's own
//! files — `columns/*.msqr` — are still the truth for what has been exported and
//! are still watched by [`crate::watch`]; what changes is the *edge* of that,
//! the ground nobody has exported yet. This service holds the whole map already
//! and knows where every viewer is looking, so deciding what to ask for next
//! belongs here — the mod becomes the half that only answers.
//!
//! Two queues, not one. `near` is wherever a player is standing right now, or
//! has ever stood — offered first, however long `far` has grown, because it is
//! where somebody actually is. `far` is the map's own edge, the slow background
//! fill that draws in a world evenly with no notion of where anybody stands.
//! Both are capped, so a long-explored world cannot queue its whole frontier in
//! one pass on a cold start; a column dropped for the cap is not lost, only
//! un-asked until something beside it is drawn and offers it again.

use std::collections::{HashSet, VecDeque};
use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::Deserialize;

use crate::columns::{Chunk, Column};
use crate::log::warn;
use crate::state::State;

/// How many columns may sit in either queue before new offers are dropped —
/// not lost, only left for the next thing beside them to offer again.
const MAX_QUEUED: usize = 4096;

/// How many columns to ask about, and how many to ask the server to load, in
/// one step. The same number `Repair::PerStep` settled on in the mod: a chunk
/// load is real work for the game's own chunk thread, and this is a fraction of
/// what it generates on its own in a tick.
const PER_STEP: usize = 4;

/// How often to take a step. On a clock of its own rather than tied to how
/// often tiles are watched — getting a column and drawing it are different
/// jobs at different speeds.
const STEP_EVERY: Duration = Duration::from_millis(250);

/// Where the mod's own listener is, what to say to it, and how far the game
/// itself already loads chunks — the fallback reach when nothing overrides it.
#[derive(Clone)]
struct Endpoint {
    base: String,
    token: String,
    max_chunk_radius: i32,
}

#[derive(Deserialize)]
struct ModApiFile {
    #[serde(rename = "Port")]
    port: u16,
    #[serde(rename = "Token")]
    token: String,
    #[serde(rename = "MaxChunkRadius", default)]
    max_chunk_radius: i32,
}

/// Reads where the mod's listener is, or `None` where it has not published one
/// yet — a mod older than this, or one still starting.
fn discover(exports: &Path) -> Option<Endpoint> {
    let path = path_in(exports);
    let body = std::fs::read_to_string(path).ok()?;
    let read: ModApiFile = serde_json::from_str(&body).ok()?;
    if read.port == 0 || read.token.is_empty() {
        return None;
    }
    Some(Endpoint {
        base: format!("http://127.0.0.1:{}", read.port),
        token: read.token,
        max_chunk_radius: read.max_chunk_radius,
    })
}

/// Where the mod publishes its listener's address, mirroring
/// [`crate::api::connection_path`] for the direction this asks in.
#[must_use]
fn path_in(exports: &Path) -> PathBuf {
    exports.join("mod-api.json")
}

/// Every chunk a player has ever stood in, and the reach a candidate column is
/// allowed from the nearest of them.
///
/// This is the fix for a map that fills far past where anyone has walked: the
/// mod's savegame answers "does this chunk exist," which is true of the
/// generator's own margin around spawn as much as of ground somebody explored,
/// so backfilling from that alone draws a disc around spawn no player ever
/// pushed the in-game map that wide. A column only earns a place in the queue
/// when it sits within `radius` chunks of somewhere a player actually was.
pub struct Visited {
    seen: Mutex<HashSet<(i32, i32)>>,
    path: PathBuf,
}

impl Visited {
    /// Reads back what a previous run recorded, or starts empty — an unreadable
    /// or missing file is answered the same way a fresh world is, since a
    /// bounded reach found empty is the honest starting shape and not a fault.
    #[must_use]
    pub fn load(exports: &Path) -> Self {
        let path = visited_path_in(exports);
        let seen = std::fs::read(&path)
            .ok()
            .and_then(|body| serde_json::from_slice::<Vec<(i32, i32)>>(&body).ok())
            .map(|pairs| pairs.into_iter().collect())
            .unwrap_or_default();
        Self { seen: Mutex::new(seen), path }
    }

    /// Records that a player is standing in these chunks right now. Returns
    /// whether anything was new, so a caller only bothers writing the file back
    /// when there was something to write.
    pub fn visit(&self, at: impl IntoIterator<Item = (i32, i32)>) -> bool {
        let Ok(mut seen) = self.seen.lock() else { return false };
        let mut changed = false;
        for chunk in at {
            changed |= seen.insert(chunk);
        }
        changed
    }

    /// Whether a candidate column is within `radius` chunks of somewhere a
    /// player has stood — everything is far from an empty set, which is a world
    /// nobody has moved in yet. `radius` is resolved by the caller rather than
    /// fixed here, since it depends on the mod's own `MaxChunkRadius`, which is
    /// only known once the mod's listener has been reached at least once.
    #[must_use]
    fn reaches(&self, at: (i32, i32), radius: i32) -> bool {
        let Ok(seen) = self.seen.lock() else { return false };
        seen.iter().any(|&(vx, vz)| (vx - at.0).abs() <= radius && (vz - at.1).abs() <= radius)
    }

    /// Writes what is held, when there is something worth keeping. Chebyshev
    /// squares rather than a circle, matching `reaches` — cheaper to check and
    /// close enough that nobody would tell the two apart on a map.
    pub fn save(&self) {
        let Ok(seen) = self.seen.lock() else { return };
        let pairs: Vec<(i32, i32)> = seen.iter().copied().collect();
        drop(seen);

        let Ok(body) = serde_json::to_vec(&pairs) else { return };
        if let Err(error) = crate::files::replace(&self.path, &body) {
            warn!("could not write {}: {error}", self.path.display());
        }
    }
}

/// Where visited chunks are kept, beside the map like everything else this
/// service writes for its own use.
#[must_use]
fn visited_path_in(exports: &Path) -> PathBuf {
    exports.join("visited-chunks.json")
}

/// The frontier, and the pacing over it.
pub struct Puller {
    exports: PathBuf,
    /// Columns the mod has said changed, live — drained before anything else,
    /// and never gated by `tried`: a column already drawn is exactly the case
    /// this exists for, since `tried`'s whole point elsewhere is refusing to ask
    /// about something already settled, and a live change is the opposite of
    /// settled.
    changed: Mutex<VecDeque<(i32, i32)>>,
    near: Mutex<VecDeque<(i32, i32)>>,
    far: Mutex<VecDeque<(i32, i32)>>,
    tried: Mutex<HashSet<(i32, i32)>>,
    visited: Visited,
    /// The reach an operator set. Zero means "ask the mod," and the answer is
    /// cached in `radius` below once heard, rather than asked on every offer.
    configured_radius: i32,
    /// The reach actually in force: `configured_radius` where it is not zero,
    /// or the mod's own `MaxChunkRadius` once `step` has reached it at least
    /// once. Zero until then, which is every candidate refused — the safe
    /// default while nothing is known yet, since a reach nobody has confirmed
    /// is not one worth trusting with an unbounded one instead.
    radius: std::sync::atomic::AtomicI32,
    agent: ureq::Agent,
}

impl Puller {
    #[must_use]
    pub fn new(exports: &Path, configured_radius: i32) -> Self {
        Self {
            exports: exports.to_path_buf(),
            changed: Mutex::new(VecDeque::new()),
            near: Mutex::new(VecDeque::new()),
            far: Mutex::new(VecDeque::new()),
            tried: Mutex::new(HashSet::new()),
            visited: Visited::load(exports),
            configured_radius,
            radius: std::sync::atomic::AtomicI32::new(configured_radius),
            agent: ureq::Agent::config_builder()
                .timeout_global(Some(Duration::from_secs(5)))
                .build()
                .into(),
        }
    }

    /// Records that a player is standing in these chunks, and writes it down
    /// when that changed anything the next restart would want to know.
    pub fn visit(&self, at: impl IntoIterator<Item = (i32, i32)>) {
        let at: Vec<(i32, i32)> = at.into_iter().collect();
        if self.visited.visit(at.iter().copied()) {
            self.visited.save();
        }
    }

    /// Notes columns the mod says changed just now, unconditionally: queued
    /// ahead of everything else and never refused for already being drawn,
    /// since a live change is exactly a column that was drawn and no longer
    /// answers for what it drew. This is the fast path a walking player's own
    /// footsteps ride — see `LiveDirty` on the mod's side.
    pub fn notify_changed(&self, at: impl IntoIterator<Item = (i32, i32)>) {
        let Ok(mut queue) = self.changed.lock() else { return };
        for column in at {
            if queue.len() >= MAX_QUEUED {
                break;
            }
            queue.push_back(column);
        }
    }

    /// Offers columns beside wherever a player is standing, or has stood before —
    /// worth asking about ahead of anything the background edge has queued.
    pub fn seed_near(&self, around: impl IntoIterator<Item = (i32, i32)>, held: &HashSet<(i32, i32)>) {
        for (cx, cz) in around {
            for at in [(cx, cz), (cx + 1, cz), (cx - 1, cz), (cx, cz + 1), (cx, cz - 1)] {
                self.offer(&self.near, at, held);
            }
        }
    }

    /// Offers the columns beside something the map already has — the slow,
    /// background source that fills a world in evenly, bounded to the same
    /// reach as everything else: a region drawn does not widen how far the map
    /// may grow from it, only how far it has already grown.
    pub fn seed_edge(&self, mapped: impl IntoIterator<Item = (i32, i32)>, held: &HashSet<(i32, i32)>) {
        for (cx, cz) in mapped {
            for at in [(cx + 1, cz), (cx - 1, cz), (cx, cz + 1), (cx, cz - 1)] {
                self.offer(&self.far, at, held);
            }
        }
    }

    fn offer(&self, onto: &Mutex<VecDeque<(i32, i32)>>, at: (i32, i32), held: &HashSet<(i32, i32)>) {
        let radius = self.radius.load(std::sync::atomic::Ordering::Relaxed);
        if held.contains(&at) || radius <= 0 || !self.visited.reaches(at, radius) {
            return;
        }
        let Ok(mut queue) = onto.lock() else { return };
        if queue.len() >= MAX_QUEUED {
            return;
        }
        let Ok(mut tried) = self.tried.lock() else { return };
        if !tried.insert(at) {
            return;
        }
        queue.push_back(at);
    }

    /// How many columns are queued, across every queue — for `witchlight
    /// status`.
    #[must_use]
    pub fn waiting(&self) -> usize {
        let changed = self.changed.lock().map(|q| q.len()).unwrap_or(0);
        let near = self.near.lock().map(|q| q.len()).unwrap_or(0);
        let far = self.far.lock().map(|q| q.len()).unwrap_or(0);
        changed + near + far
    }

    /// Takes the next few columns to ask about: a live change first, then
    /// wherever a player is, then the map's own edge.
    fn next_batch(&self, most: usize) -> Vec<(i32, i32)> {
        let mut batch = Vec::with_capacity(most);
        if let Ok(mut changed) = self.changed.lock() {
            while batch.len() < most {
                let Some(at) = changed.pop_front() else { break };
                batch.push(at);
            }
        }
        if let Ok(mut near) = self.near.lock() {
            while batch.len() < most {
                let Some(at) = near.pop_front() else { break };
                batch.push(at);
            }
        }
        if let Ok(mut far) = self.far.lock() {
            while batch.len() < most {
                let Some(at) = far.pop_front() else { break };
                batch.push(at);
            }
        }
        batch
    }

    /// One step: asks the mod whether it can answer for a few queued columns,
    /// and applies whatever it can. Columns the mod cannot answer for right now
    /// (not loaded, not saved) are dropped rather than retried immediately —
    /// `tried` keeps them from being asked again until something beside them is
    /// drawn and offers them afresh, which is the same backoff the mod's own
    /// `Backfill` used.
    pub fn step(&self, state: &State) {
        let Some(endpoint) = discover(&self.exports) else { return };

        // Zero means an operator has not overridden it, so the mod's own answer
        // is what governs — and it is only known once the mod has actually been
        // reached, which is exactly what just happened.
        if self.configured_radius == 0 && endpoint.max_chunk_radius > 0 {
            self.radius.store(endpoint.max_chunk_radius, std::sync::atomic::Ordering::Relaxed);
        }

        let mut regions_touched: HashSet<(i32, i32)> = HashSet::new();

        for (cx, cz) in self.next_batch(PER_STEP) {
            match self.fetch_column(&endpoint, cx, cz) {
                Ok(Some((edge, chunk))) => {
                    if let Ok(mut world) = state.world.write() {
                        world.apply_one(cx, cz, edge, chunk);
                    }
                    regions_touched.insert((
                        cx.div_euclid(crate::columns::REGION_CHUNKS),
                        cz.div_euclid(crate::columns::REGION_CHUNKS),
                    ));
                }
                Ok(None) => {
                    // The mod does not have this column loaded right now. Ask it
                    // to load it, for a later step to try again once it has.
                    self.request_load(&endpoint, cx, cz);
                }
                Err(error) => {
                    warn!("terrain pull for ({cx}, {cz}) failed: {error}");
                }
            }
        }

        if regions_touched.is_empty() {
            return;
        }

        // A region is a level 0 tile, and slope shading reads the column to the
        // west and north of each pixel — so a region drawn also changes the
        // western edge of the tile east of it and the northern edge of the tile
        // below, the same widening `watch::refresh_world` does for a region that
        // changed on disk. Without this, a column this pulls in updates memory
        // but never tells a browser to ask again for the tile it landed in —
        // `state.generation` is what `/info.json` reports and what a browser's
        // poll compares against, and nothing else here would ever move it.
        let mut repaint: Vec<(i32, i32)> = regions_touched
            .iter()
            .flat_map(|&(rx, rz)| [(rx, rz), (rx + 1, rz), (rx, rz + 1)])
            .collect();
        repaint.sort_unstable();
        repaint.dedup();

        state.drop_tiles(&repaint.iter().map(|&(x, z)| (0, x, z)).collect::<Vec<_>>());
        state.mark_stale(repaint);
    }

    /// One column, or `None` where the mod has nothing loaded to answer with.
    fn fetch_column(
        &self, endpoint: &Endpoint, cx: i32, cz: i32,
    ) -> Result<Option<(usize, Chunk)>, String> {
        let url = format!("{}/column/{cx}/{cz}", endpoint.base);
        let mut response = match self
            .agent
            .get(&url)
            .header("Authorization", &format!("Bearer {}", endpoint.token))
            .call()
        {
            Ok(response) => response,
            Err(ureq::Error::StatusCode(404)) => return Ok(None),
            Err(error) => return Err(error.to_string()),
        };

        let mut body = String::new();
        response
            .body_mut()
            .as_reader()
            .read_to_string(&mut body)
            .map_err(|error| error.to_string())?;

        let parsed: ColumnResponse = serde_json::from_str(&body).map_err(|error| error.to_string())?;
        let bytes = base64_decode(&parsed.record)?;

        let edge = (bytes.len() / ENTRY_BYTES) as f64;
        let edge = edge.sqrt().round() as usize;
        if edge == 0 || edge * edge * ENTRY_BYTES != bytes.len() {
            return Err(format!("a column of {} bytes is not a square number of entries", bytes.len()));
        }

        let mut columns = Vec::with_capacity(edge * edge);
        for i in 0..edge * edge {
            let at = i * ENTRY_BYTES;
            columns.push(Column {
                block: u16::from_le_bytes([bytes[at], bytes[at + 1]]),
                height: i16::from_le_bytes([bytes[at + 2], bytes[at + 3]]),
                temperature: bytes[at + 4],
                rainfall: bytes[at + 5],
                season: 0,
            });
        }

        Ok(Some((edge, Chunk { columns })))
    }

    /// Asks the mod to load a column it does not currently hold, so a later
    /// step's `fetch_column` has something to answer with.
    fn request_load(&self, endpoint: &Endpoint, cx: i32, cz: i32) {
        let url = format!("{}/load/{cx}/{cz}", endpoint.base);
        if let Err(error) = self
            .agent
            .post(&url)
            .header("Authorization", &format!("Bearer {}", endpoint.token))
            .send_empty()
        {
            warn!("could not ask the mod to load ({cx}, {cz}): {error}");
        }
    }
}

const ENTRY_BYTES: usize = 6;

#[derive(Deserialize)]
struct ColumnResponse {
    #[serde(rename = "Record")]
    record: String,
}

/// Plain base64, the alphabet .NET's `Convert.ToBase64String` writes.
fn base64_decode(text: &str) -> Result<Vec<u8>, String> {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut table = [255u8; 256];
    for (i, &b) in ALPHABET.iter().enumerate() {
        table[b as usize] = i as u8;
    }

    let stripped: Vec<u8> = text.bytes().filter(|&b| b != b'=').collect();
    let mut out = Vec::with_capacity(stripped.len() * 3 / 4);
    for chunk in stripped.chunks(4) {
        let mut buf = [0u8; 4];
        for (i, &b) in chunk.iter().enumerate() {
            let value = table[b as usize];
            if value == 255 {
                return Err("invalid base64".to_owned());
            }
            buf[i] = value;
        }
        let n = chunk.len();
        out.push((buf[0] << 2) | (buf[1] >> 4));
        if n > 2 {
            out.push((buf[1] << 4) | (buf[2] >> 2));
        }
        if n > 3 {
            out.push((buf[2] << 6) | buf[3]);
        }
    }
    Ok(out)
}

/// Starts the clock that steps the puller. Its own thread, on its own beat —
/// the mod's endpoint may not exist yet on an older mod, and every step already
/// answers that by doing nothing.
pub fn start(puller: Arc<Puller>, state: Arc<State>) {
    std::thread::spawn(move || {
        loop {
            std::thread::sleep(STEP_EVERY);
            puller.step(&state);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_column_encoded_the_way_dotnet_would_decodes_back() {
        // "AQIDBAUG" is base64 for bytes 1..=6, the shape .NET's own encoder
        // would produce for one six-byte entry with no padding needed.
        let decoded = base64_decode("AQIDBAUG").expect("valid base64");
        assert_eq!(decoded, vec![1, 2, 3, 4, 5, 6]);
    }

    #[test]
    fn base64_with_padding_decodes_to_the_right_length() {
        // "AQI=" is bytes [1, 2], padded to a multiple of four characters.
        let decoded = base64_decode("AQI=").expect("valid base64");
        assert_eq!(decoded, vec![1, 2]);
    }

    #[test]
    fn near_is_asked_about_before_far_however_long_far_has_grown() {
        // A generous radius and every candidate visited by hand: this test is
        // about queue ordering, not about the radius gate, so the gate is held
        // wide open rather than exercised.
        let puller = Puller::new(Path::new("/nonexistent"), 100);
        puller.visit([(0, 0), (10, 10), (5, 5)]);
        let held = HashSet::new();
        puller.seed_edge([(0, 0)], &held);
        puller.seed_edge([(10, 10)], &held);
        puller.seed_near([(5, 5)], &held);

        let batch = puller.next_batch(1);
        // A neighbour of (5, 5), not of (0, 0) or (10, 10).
        assert!(batch[0].0.abs_diff(5) <= 1 && batch[0].1.abs_diff(5) <= 1);
    }

    #[test]
    fn a_column_already_held_is_never_queued() {
        let puller = Puller::new(Path::new("/nonexistent"), 100);
        puller.visit([(0, 0)]);
        let mut held = HashSet::new();
        held.insert((1, 0));
        held.insert((-1, 0));
        held.insert((0, 1));
        held.insert((0, -1));
        puller.seed_near([(0, 0)], &held);

        // Every neighbour of (0, 0) is already held; only (0, 0) itself is new.
        assert_eq!(puller.waiting(), 1);
    }

    #[test]
    fn nothing_is_queued_past_the_configured_reach_from_any_visited_chunk() {
        let puller = Puller::new(Path::new("/nonexistent"), 2);
        puller.visit([(0, 0)]);
        let held = HashSet::new();

        // Six chunks out is past a reach of two from (0, 0).
        puller.seed_edge([(6, 0)], &held);
        assert_eq!(puller.waiting(), 0, "nothing this far from anywhere visited is queued");

        // One chunk out is within reach and is queued as usual.
        puller.seed_near([(0, 0)], &held);
        assert!(puller.waiting() > 0);
    }

    #[test]
    fn nothing_is_queued_before_a_reach_is_known_at_all() {
        // Zero means "ask the mod," and nothing has asked it yet in this test —
        // the safe default is refusing every candidate rather than trusting an
        // unbounded one until a real answer arrives.
        let puller = Puller::new(Path::new("/nonexistent"), 0);
        puller.visit([(0, 0)]);
        puller.seed_near([(0, 0)], &HashSet::new());
        assert_eq!(puller.waiting(), 0);
    }
}
