//! Asking the mod for one more column, and deciding which one to ask for.
//!
//! Everything that used to decide this lived in the mod, as `Backfill` and
//! `Repair`: which columns the savegame might hold that the map has never drawn,
//! in what order, and how fast to ask the server to load them. Ground that
//! moves arrives on its own, pushed by the mod over the API channel — see
//! [`crate::apiport`]; what this decides is the *edge* of that, the ground
//! nobody has exported yet. This service holds the whole map already and knows
//! where every viewer is looking, so deciding what to ask for next belongs here
//! — the mod becomes the half that only answers.
//!
//! Two queues, not one. `near` is wherever a player is standing right now —
//! offered first, however long `far` has grown, because it is where somebody
//! actually is. `far` is the map's own edge, the slow background
//! fill that draws in a world evenly with no notion of where anybody stands.
//! Both are capped, so a long-explored world cannot queue its whole frontier in
//! one pass on a cold start; a column dropped for the cap is not lost, only
//! un-asked until something beside it is drawn and offers it again.

use std::collections::{HashMap, HashSet, VecDeque};
use std::io::Read as _;
use std::time::SystemTime;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::Deserialize;

use crate::columns::Chunk;
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

/// Where players have stood lately, and how far each of them saw from there.
///
/// This is the fix for a map that fills far past where anyone has walked: the
/// mod's savegame answers "does this chunk exist," which is true of the
/// generator's own margin around spawn as much as of ground somebody explored,
/// so backfilling from that alone draws a disc around spawn no player ever
/// pushed the in-game map that wide. A column only earns a place in the queue
/// when it sits within sight of somewhere a player actually was.
///
/// Within sight of *lately*. A place stood in is kept for [`VISITED_FOR`] and
/// then let go, so the map fills in around the path somebody is walking and
/// stops filling behind them once they are long gone. What was drawn stays
/// drawn; what stops is asking the game for the ground beside it. Whatever the
/// game loaded while they were there has already been offered by then, and a
/// place they come back to is a place they stand in again.
///
/// Each place carries its own reach: how far the game loaded ground around
/// that player, which is theirs to say and not one number for everybody.
pub struct Visited {
    inner: Mutex<VisitedInner>,
    path: PathBuf,
}

/// How long a place somebody stood keeps the ground around it worth asking
/// for. An hour is longer than any backfill takes to catch up with a walk.
const VISITED_FOR: Duration = Duration::from_secs(60 * 60);

#[derive(Default)]
struct VisitedInner {
    /// Each chunk stood in lately: how far was seen from it, and when, in
    /// seconds since the epoch.
    stood: HashMap<(i32, i32), (i32, u64)>,
    /// Every chunk within sight of one of them — the discs, unioned, so that
    /// asking whether a column is in reach is one lookup rather than a pass
    /// over everywhere anybody has been.
    within: HashSet<(i32, i32)>,
}

impl VisitedInner {
    /// Draws `within` again from what is stood in. Done whenever the set of
    /// places changes, which is a player entering a new chunk or an old place
    /// expiring — a few hundred inserts, a few times a minute.
    fn rebuild(&mut self) {
        self.within.clear();
        for (&at, &(radius, _)) in &self.stood {
            self.within.extend(crate::columns::disc_of(at, radius));
        }
    }

    /// Drops what was stood in longer ago than is kept. Says whether anything went.
    fn expire(&mut self, now: u64) -> bool {
        let horizon = now.saturating_sub(VISITED_FOR.as_secs());
        let before = self.stood.len();
        self.stood.retain(|_, &mut (_, when)| when >= horizon);
        self.stood.len() != before
    }
}

fn now_secs() -> u64 {
    SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).map_or(0, |since| since.as_secs())
}

impl Visited {
    /// Reads back what a previous run recorded, or starts empty — an unreadable
    /// or missing file is answered the same way a fresh world is, since a
    /// bounded reach found empty is the honest starting shape and not a fault.
    /// A file an older build wrote, with no reach or time per place, reads as
    /// empty for the same reason.
    #[must_use]
    pub fn load(exports: &Path) -> Self {
        let path = visited_path_in(exports);
        let mut inner = VisitedInner::default();
        if let Some(places) = std::fs::read(&path)
            .ok()
            .and_then(|body| serde_json::from_slice::<Vec<(i32, i32, i32, u64)>>(&body).ok())
        {
            for (cx, cz, radius, when) in places {
                inner.stood.insert((cx, cz), (radius, when));
            }
        }
        inner.expire(now_secs());
        inner.rebuild();
        Self { inner: Mutex::new(inner), path }
    }

    /// Records that players are standing in these chunks right now, each
    /// seeing `radius` chunks around them. Returns whether the set of places
    /// or a reach changed, so a caller only bothers writing the file back when
    /// there is something the next start would want to know.
    pub fn visit(&self, at: impl IntoIterator<Item = ((i32, i32), i32)>) -> bool {
        let Ok(mut inner) = self.inner.lock() else { return false };
        let now = now_secs();
        let mut changed = inner.expire(now);
        for (chunk, radius) in at {
            let was = inner.stood.insert(chunk, (radius, now));
            changed |= was.is_none_or(|(had, _)| had != radius);
        }
        if changed {
            inner.rebuild();
        }
        changed
    }

    /// Whether a candidate column is within sight of somewhere a player has
    /// stood lately — nothing is, of a world nobody has moved in yet.
    #[must_use]
    fn reaches(&self, at: (i32, i32)) -> bool {
        self.inner.lock().is_ok_and(|inner| inner.within.contains(&at))
    }

    /// Writes what is held, when there is something worth keeping.
    pub fn save(&self) {
        let Ok(inner) = self.inner.lock() else { return };
        let places: Vec<(i32, i32, i32, u64)> =
            inner.stood.iter().map(|(&(cx, cz), &(radius, when))| (cx, cz, radius, when)).collect();
        drop(inner);

        let Ok(body) = serde_json::to_vec(&places) else { return };
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
    near: Mutex<VecDeque<(i32, i32)>>,
    far: Mutex<VecDeque<(i32, i32)>>,
    tried: Mutex<HashSet<(i32, i32)>>,
    visited: Visited,
    /// The reach an operator set. Zero means "ask the mod," and the answer is
    /// cached in `radius` below once heard.
    configured_radius: i32,
    /// The reach to fall back on for a player whose own the mod did not say:
    /// `configured_radius` where it is not zero, or the mod's own
    /// `MaxChunkRadius` once `step` has reached it at least once. Zero until
    /// then, which is nothing recorded for such a player — the safe default
    /// while nothing is known yet.
    radius: std::sync::atomic::AtomicI32,
    agent: ureq::Agent,
}

impl Puller {
    #[must_use]
    pub fn new(exports: &Path, configured_radius: i32) -> Self {
        Self {
            exports: exports.to_path_buf(),
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

    /// Records that players are standing in these chunks, each seeing `radius`
    /// chunks around them, and writes it down when that changed anything the
    /// next restart would want to know.
    pub fn visit(&self, at: impl IntoIterator<Item = ((i32, i32), i32)>) {
        if self.visited.visit(at) {
            self.visited.save();
        }
    }

    /// Offers columns beside wherever a player is standing — worth asking about
    /// ahead of anything the background edge has queued. `held` answers whether
    /// the map already has a column, so one it has is never asked for again.
    pub fn seed_near(&self, around: impl IntoIterator<Item = (i32, i32)>, held: &dyn Fn((i32, i32)) -> bool) {
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
    pub fn seed_edge(&self, mapped: impl IntoIterator<Item = (i32, i32)>, held: &dyn Fn((i32, i32)) -> bool) {
        for (cx, cz) in mapped {
            for at in [(cx + 1, cz), (cx - 1, cz), (cx, cz + 1), (cx, cz - 1)] {
                self.offer(&self.far, at, held);
            }
        }
    }

    fn offer(&self, onto: &Mutex<VecDeque<(i32, i32)>>, at: (i32, i32), held: &dyn Fn((i32, i32)) -> bool) {
        if held(at) || !self.visited.reaches(at) {
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

    /// How far a player sees where their own view distance is not known: the
    /// operator's setting, or the game's own chunk radius once the mod has
    /// said it. Zero until either is known.
    #[must_use]
    pub fn reach(&self) -> i32 {
        self.radius.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// How many columns are queued, across every queue — for `witchlight
    /// status`.
    #[must_use]
    pub fn waiting(&self) -> usize {
        let near = self.near.lock().map(|q| q.len()).unwrap_or(0);
        let far = self.far.lock().map(|q| q.len()).unwrap_or(0);
        near + far
    }

    /// Takes the next few columns to ask about: wherever a player is first,
    /// then the map's own edge.
    fn next_batch(&self, most: usize) -> Vec<(i32, i32)> {
        let mut batch = Vec::with_capacity(most);
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

        let mut arrived = Vec::new();
        for (cx, cz) in self.next_batch(PER_STEP) {
            match self.fetch_column(&endpoint, cx, cz) {
                Ok(Some((edge, chunk))) => {
                    // A pull carries no season; the chunk keeps whatever it had,
                    // which for ground never drawn before is the year's start
                    // until the mod's next season pass says otherwise.
                    let season = state
                        .world
                        .read()
                        .ok()
                        .and_then(|world| world.chunks.get(&(cx, cz)).map(Chunk::season))
                        .unwrap_or(0);
                    arrived.push((edge, crate::store::Arrived { cx, cz, season, record: chunk.record() }));
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

        let Some(edge) = arrived.first().map(|(edge, _)| *edge) else { return };
        let arrived: Vec<crate::store::Arrived> = arrived.into_iter().map(|(_, chunk)| chunk).collect();
        let stored = state.take_chunks(edge, &arrived, SystemTime::now());
        state.terrain_changed(&stored);
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
        let bytes = crate::wire::decode(&parsed.record)?;

        let edge = Chunk::edge_of(bytes.len())
            .ok_or_else(|| format!("a column of {} bytes is not a square number of entries", bytes.len()))?;
        let chunk = Chunk::from_record(&bytes, edge, 0).ok_or_else(|| "a record too short to read".to_owned())?;
        Ok(Some((edge, chunk)))
    }

    /// Asks the mod to load a column it does not currently hold, so that the
    /// game's own "chunk loaded" event hands it to the exporter and it arrives
    /// by the push path — but only a column the savegame already has. Loading
    /// one it does not have would generate it, and a map must not be what
    /// makes the world bigger: ground nobody has walked is not ground to draw.
    fn request_load(&self, endpoint: &Endpoint, cx: i32, cz: i32) {
        match self.exists(endpoint, cx, cz) {
            Ok(true) => {}
            Ok(false) => return,
            Err(error) => {
                warn!("could not ask the mod whether ({cx}, {cz}) exists: {error}");
                return;
            }
        }

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

    /// Whether the savegame holds this column at all.
    fn exists(&self, endpoint: &Endpoint, cx: i32, cz: i32) -> Result<bool, String> {
        let url = format!("{}/exists/{cx}/{cz}", endpoint.base);
        let mut response = self
            .agent
            .get(&url)
            .header("Authorization", &format!("Bearer {}", endpoint.token))
            .call()
            .map_err(|error| error.to_string())?;
        let mut body = String::new();
        response.body_mut().as_reader().read_to_string(&mut body).map_err(|error| error.to_string())?;
        let parsed: ExistsResponse = serde_json::from_str(&body).map_err(|error| error.to_string())?;
        Ok(parsed.exists)
    }
}

#[derive(Deserialize)]
struct ExistsResponse {
    #[serde(rename = "Exists")]
    exists: bool,
}

#[derive(Deserialize)]
struct ColumnResponse {
    #[serde(rename = "Record")]
    record: String,
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

    fn nothing_held(_: (i32, i32)) -> bool {
        false
    }

    #[test]
    fn near_is_asked_about_before_far_however_long_far_has_grown() {
        // A generous reach and every candidate visited by hand: this test is
        // about queue ordering, not about the reach gate, so the gate is held
        // wide open rather than exercised.
        let puller = Puller::new(Path::new("/nonexistent"), 0);
        puller.visit([((0, 0), 100), ((10, 10), 100), ((5, 5), 100)]);
        puller.seed_edge([(0, 0)], &nothing_held);
        puller.seed_edge([(10, 10)], &nothing_held);
        puller.seed_near([(5, 5)], &nothing_held);

        let batch = puller.next_batch(1);
        // A neighbour of (5, 5), not of (0, 0) or (10, 10).
        assert!(batch[0].0.abs_diff(5) <= 1 && batch[0].1.abs_diff(5) <= 1);
    }

    #[test]
    fn a_column_already_held_is_never_queued() {
        let puller = Puller::new(Path::new("/nonexistent"), 0);
        puller.visit([((0, 0), 100)]);
        let held = |at: (i32, i32)| [(1, 0), (-1, 0), (0, 1), (0, -1)].contains(&at);
        puller.seed_near([(0, 0)], &held);

        // Every neighbour of (0, 0) is already held; only (0, 0) itself is new.
        assert_eq!(puller.waiting(), 1);
    }

    #[test]
    fn nothing_is_queued_past_the_reach_of_anywhere_stood_in() {
        let puller = Puller::new(Path::new("/nonexistent"), 0);
        puller.visit([((0, 0), 2)]);

        // Six chunks out is past a reach of two from (0, 0).
        puller.seed_edge([(6, 0)], &nothing_held);
        assert_eq!(puller.waiting(), 0, "nothing this far from anywhere visited is queued");

        // One chunk out is within reach and is queued as usual.
        puller.seed_near([(0, 0)], &nothing_held);
        assert!(puller.waiting() > 0);
    }

    #[test]
    fn reach_is_a_disc_and_not_the_square_around_it() {
        let puller = Puller::new(Path::new("/nonexistent"), 0);
        puller.visit([((0, 0), 2)]);

        // (2, 0) is two chunks out along an axis and inside; (2, 2) is two out
        // along both and 2.8 as the crow flies, which is the square's corner
        // and outside the disc.
        puller.seed_edge([(1, 0)], &|at| at != (2, 0));
        assert_eq!(puller.waiting(), 1, "(2, 0) is within reach");
        puller.seed_edge([(2, 1)], &|at| at != (2, 2));
        assert_eq!(puller.waiting(), 1, "(2, 2) is not");
    }

    #[test]
    fn nothing_is_queued_where_nobody_has_stood() {
        let puller = Puller::new(Path::new("/nonexistent"), 12);
        puller.seed_near([(0, 0)], &nothing_held);
        assert_eq!(puller.waiting(), 0);
    }

    #[test]
    fn each_place_carries_its_own_reach() {
        let puller = Puller::new(Path::new("/nonexistent"), 0);
        puller.visit([((0, 0), 1), ((100, 100), 3)]);
        puller.seed_edge([(1, 0)], &|at| at != (2, 0));
        assert_eq!(puller.waiting(), 0, "two out from a place seen one around is out of reach");
        puller.seed_edge([(101, 100)], &|at| at != (102, 100));
        assert_eq!(puller.waiting(), 1, "two out from a place seen three around is in reach");
    }
}
