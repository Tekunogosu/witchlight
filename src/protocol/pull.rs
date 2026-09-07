//! Decides which column to ask the mod for next, and asks for it.
//!
//! Ground that changes arrives on its own, pushed by the mod over the API
//! channel. See [`crate::protocol::apiport`]. This module decides the edge of
//! that: the ground nobody has exported yet. The service holds the whole map and
//! knows where every viewer is looking, so it decides what to ask for and the
//! mod only answers.
//!
//! There are two queues. `near` holds columns beside where a player is standing
//! right now, and is offered first however long `far` has grown. `far` holds the
//! map's own edge, the slow background fill that draws a world in evenly with no
//! notion of where anybody stands.
//!
//! Both queues are capped, so a long-explored world cannot queue its whole
//! frontier in one pass on a cold start. A column dropped for the cap is not
//! lost, only unasked until something beside it is drawn and offers it again.

use std::collections::{HashMap, HashSet, VecDeque};
use std::io::Read as _;
use std::time::SystemTime;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::Deserialize;

use crate::render::columns::Chunk;
use crate::util::log::warn;
use crate::mapdata::store::Store;
use crate::state::State;

/// How many columns may sit in either queue before new offers are dropped. A
/// dropped column is offered again by the next thing drawn beside it.
const MAX_QUEUED: usize = 4096;

/// How many columns to ask about, and how many to ask the server to load, in
/// one step. A chunk load is real work for the game's own chunk thread, and this
/// is a fraction of what the game generates on its own in a tick.
const PER_STEP: usize = 4;

/// The interval between steps. Fetching a column and drawing one are different
/// jobs at different speeds, so this runs on its own interval rather than the
/// tile watcher's.
const STEP_EVERY: Duration = Duration::from_millis(250);

/// Holds where the mod's listener is, what token to present, and how far the
/// game itself loads chunks. That last is the fallback reach when nothing
/// overrides it.
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

/// Reads where the mod's listener is, or `None` when it has not published one
/// yet. That happens with a mod older than this build, or one still starting.
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

/// Returns the path the mod publishes its listener's address to. It mirrors
/// [`crate::protocol::api::connection_path`] for the opposite direction.
#[must_use]
fn path_in(exports: &Path) -> PathBuf {
    exports.join("mod-api.json")
}

/// Records where players have stood lately and how far each saw from there.
///
/// This bounds how far the map fills. The mod's savegame answers only whether a
/// chunk exists, which is true of the generator's margin around spawn as much as
/// of ground somebody explored. Backfilling from that alone would draw a disc
/// around spawn wider than any player pushed the in-game map. A column earns a
/// place in the queue only when it sits within sight of somewhere a player
/// actually was.
///
/// A place stood in is kept for [`VISITED_FOR`] and then released, so the map
/// fills in around the path somebody is walking and stops filling behind them
/// once they are gone. What was drawn stays drawn. What stops is asking the game
/// for the ground beside it. Whatever the game loaded while they were there has
/// already been offered by then, and a place they return to is one they stand in
/// again.
///
/// Each place carries its own reach, which is how far the game loaded ground
/// around that player rather than one number for everybody.
pub struct Visited {
    inner: Mutex<VisitedInner>,
    store: Arc<Store>,
}

/// How long a place somebody stood keeps the ground around it worth asking for.
/// An hour is longer than any backfill takes to catch up with a walk.
const VISITED_FOR: Duration = Duration::from_secs(60 * 60);

#[derive(Default)]
struct VisitedInner {
    /// Holds each chunk stood in lately, with how far was seen from it and
    /// when, in seconds since the epoch.
    stood: HashMap<(i32, i32), (i32, u64)>,
    /// Holds every chunk within sight of one of those places, as the union of
    /// their discs, so asking whether a column is in reach is one lookup rather
    /// than a pass over everywhere anybody has been.
    within: HashSet<(i32, i32)>,
}

impl VisitedInner {
    /// Rebuilds `within` from the places stood in. This runs whenever the set
    /// of places changes, which is a player entering a new chunk or an old place
    /// expiring, and costs a few hundred inserts a few times a minute.
    fn rebuild(&mut self) {
        self.within.clear();
        for (&at, &(radius, _)) in &self.stood {
            self.within.extend(crate::render::columns::disc_of(at, radius));
        }
    }

    /// Drops places stood in longer ago than [`VISITED_FOR`], and returns which
    /// went.
    fn expire(&mut self, now: u64) -> Vec<(i32, i32)> {
        let horizon = now.saturating_sub(VISITED_FOR.as_secs());
        let gone: Vec<(i32, i32)> = self
            .stood
            .iter()
            .filter(|(_, (_, when))| *when < horizon)
            .map(|(&at, _)| at)
            .collect();
        for at in &gone {
            self.stood.remove(at);
        }
        gone
    }
}

fn now_secs() -> u64 {
    SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).map_or(0, |since| since.as_secs())
}

impl Visited {
    /// Loads what a previous run recorded, or starts empty. A table that cannot
    /// be read is treated the same as a fresh world, because an empty bounded
    /// reach is a correct starting state rather than a fault.
    #[must_use]
    pub fn load(store: Arc<Store>) -> Self {
        let mut inner = VisitedInner::default();
        match store.visited() {
            Ok(places) => {
                for (cx, cz, radius, when) in places {
                    inner.stood.insert((cx, cz), (radius, when));
                }
            }
            Err(error) => warn!("{error}"),
        }
        let gone = inner.expire(now_secs());
        inner.rebuild();
        let visited = Self { inner: Mutex::new(inner), store };
        visited.save(&[], &gone);
        visited
    }

    /// Records that players are standing in these chunks, each seeing `radius`
    /// chunks around them, and writes exactly what changed: the places entered
    /// or seen further from, and the places released. A tick that moved nothing
    /// writes nothing.
    pub fn visit(&self, at: impl IntoIterator<Item = ((i32, i32), i32)>) {
        let Ok(mut inner) = self.inner.lock() else { return };
        let now = now_secs();
        let gone = inner.expire(now);
        let mut moved = Vec::new();
        for (chunk, radius) in at {
            let was = inner.stood.insert(chunk, (radius, now));
            if was.is_none_or(|(had, _)| had != radius) {
                moved.push((chunk.0, chunk.1, radius, now));
            }
        }
        if moved.is_empty() && gone.is_empty() {
            return;
        }
        inner.rebuild();
        drop(inner);
        self.save(&moved, &gone);
    }

    /// Reports whether a candidate column is within sight of somewhere a player
    /// stood lately. Nothing is, in a world nobody has moved in yet.
    #[must_use]
    fn reaches(&self, at: (i32, i32)) -> bool {
        self.inner.lock().is_ok_and(|inner| inner.within.contains(&at))
    }

    /// Writes the rows that moved and the ones that went, and nothing else.
    fn save(&self, moved: &[(i32, i32, i32, u64)], gone: &[(i32, i32)]) {
        if moved.is_empty() && gone.is_empty() {
            return;
        }
        if let Err(error) = self.store.put_visited(moved, gone) {
            warn!("{error}");
        }
    }
}

/// Holds the frontier queues and paces the requests over them.
pub struct Puller {
    exports: PathBuf,
    near: Mutex<VecDeque<(i32, i32)>>,
    far: Mutex<VecDeque<(i32, i32)>>,
    tried: Mutex<HashSet<(i32, i32)>>,
    visited: Visited,
    /// The reach an operator set. Zero means to ask the mod, and the answer is
    /// cached in `radius` below once heard.
    configured_radius: i32,
    /// The reach to fall back on for a player whose own the mod did not report.
    /// It is `configured_radius` when that is not zero, or the mod's
    /// `MaxChunkRadius` once `step` has reached the mod at least once. It is
    /// zero until then, which records nothing for such a player.
    radius: std::sync::atomic::AtomicI32,
    agent: ureq::Agent,
}

impl Puller {
    #[must_use]
    pub fn new(store: Arc<Store>, exports: &Path, configured_radius: i32) -> Self {
        Self {
            exports: exports.to_path_buf(),
            near: Mutex::new(VecDeque::new()),
            far: Mutex::new(VecDeque::new()),
            tried: Mutex::new(HashSet::new()),
            visited: Visited::load(store),
            configured_radius,
            radius: std::sync::atomic::AtomicI32::new(configured_radius),
            agent: ureq::Agent::config_builder()
                .timeout_global(Some(Duration::from_secs(5)))
                .build()
                .into(),
        }
    }

    /// Records that players are standing in these chunks, each seeing `radius`
    /// chunks around them, and writes only what changed.
    pub fn visit(&self, at: impl IntoIterator<Item = ((i32, i32), i32)>) {
        self.visited.visit(at);
    }

    /// Queues columns beside where a player is standing, ahead of anything the
    /// background edge has queued. `held` reports whether the map already has a
    /// column, so one it has is never asked for again.
    pub fn seed_near(&self, around: impl IntoIterator<Item = (i32, i32)>, held: &dyn Fn((i32, i32)) -> bool) {
        for (cx, cz) in around {
            for at in [(cx, cz), (cx + 1, cz), (cx - 1, cz), (cx, cz + 1), (cx, cz - 1)] {
                self.offer(&self.near, at, held);
            }
        }
    }

    /// Queues the columns beside something the map already has. This is the
    /// slow background source that fills a world in evenly. It is bounded to the
    /// same reach as everything else, so a region drawn does not widen how far
    /// the map may grow.
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

    /// Returns how far a player sees when their own view distance is unknown.
    /// That is the operator's setting, or the game's own chunk radius once the
    /// mod has reported it. Returns zero until either is known.
    #[must_use]
    pub fn reach(&self) -> i32 {
        self.radius.load(std::sync::atomic::Ordering::Relaxed)
    }

    // No caller reads this yet. `/witchlight status` answers from the mod's
    // own state and never asks the service. It is kept because it is the read
    // side of a store that has a write side, and the tests exercise it.
    #[allow(dead_code)]
    /// Returns how many columns are queued across every queue, for `witchlight
    /// status`.
    #[must_use]
    pub fn waiting(&self) -> usize {
        let near = self.near.lock().map(|q| q.len()).unwrap_or(0);
        let far = self.far.lock().map(|q| q.len()).unwrap_or(0);
        near + far
    }

    /// Takes the next few columns to ask about, drawing from the near queue
    /// first and then the map's own edge.
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

    /// Takes one step: asks the mod whether it can answer for a few queued
    /// columns, and applies whatever it can.
    ///
    /// Columns the mod cannot answer for right now, because they are not loaded
    /// or not saved, are dropped rather than retried immediately. `tried` keeps
    /// them from being asked again until something beside them is drawn and
    /// offers them afresh.
    pub fn step(&self, state: &State) {
        let Some(endpoint) = discover(&self.exports) else { return };

        // Zero means an operator has not overridden it, so the mod's answer
        // governs. That answer is known only once the mod has been reached,
        // which just happened.
        if self.configured_radius == 0 && endpoint.max_chunk_radius > 0 {
            self.radius.store(endpoint.max_chunk_radius, std::sync::atomic::Ordering::Relaxed);
        }

        let mut arrived = Vec::new();
        for (cx, cz) in self.next_batch(PER_STEP) {
            match self.fetch_column(&endpoint, cx, cz) {
                Ok(Some((edge, chunk))) => {
                    // A pull carries no season, so the chunk keeps whatever it
                    // had. For ground never drawn before that is the year's
                    // start, until the mod's next season pass says otherwise.
                    let season = state
                        .world
                        .read()
                        .ok()
                        .and_then(|world| world.chunks.get(&(cx, cz)).map(Chunk::season))
                        .unwrap_or(0);
                    arrived.push((edge, crate::mapdata::store::Arrived { cx, cz, season, record: chunk.record() }));
                }
                Ok(None) => {
                    // The mod does not have this column loaded. Ask it to load
                    // one, so a later step can try again once it has.
                    self.request_load(&endpoint, cx, cz);
                }
                Err(error) => {
                    warn!("terrain pull for ({cx}, {cz}) failed: {error}");
                }
            }
        }

        let Some(edge) = arrived.first().map(|(edge, _)| *edge) else { return };
        let arrived: Vec<crate::mapdata::store::Arrived> = arrived.into_iter().map(|(_, chunk)| chunk).collect();
        let stored = state.take_chunks(edge, &arrived, SystemTime::now());
        state.terrain_changed(&stored);
    }

    /// Fetches one column, or returns `None` when the mod has nothing loaded to
    /// answer with.
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
        let bytes = crate::util::wire::decode(&parsed.record)?;

        let edge = Chunk::edge_of(bytes.len())
            .ok_or_else(|| format!("a column of {} bytes is not a square number of entries", bytes.len()))?;
        let chunk = Chunk::from_record(&bytes, edge, 0).ok_or_else(|| "a record too short to read".to_owned())?;
        Ok(Some((edge, chunk)))
    }

    /// Asks the mod to load a column it does not currently hold, so the game's
    /// own chunk-loaded event hands it to the exporter and it arrives by the
    /// push path.
    ///
    /// This only asks for a column the savegame already has. Loading one it does
    /// not have would generate it, and the map must never make the world bigger.
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

    /// Reports whether the savegame holds this column at all.
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

/// Starts the thread that steps the puller on its own interval. An older mod
/// may not have the endpoint, and a step against one does nothing.
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
    fn where_players_stood_survives_a_restart_and_a_still_tick_writes_nothing() {
        let store = Arc::new(Store::in_memory());
        let first = Visited::load(Arc::clone(&store));
        first.visit([((5, 5), 4)]);
        first.visit([((5, 5), 4)]);
        assert_eq!(store.visited().unwrap().len(), 1, "one place, one row");

        let again = Visited::load(Arc::clone(&store));
        assert!(again.reaches((6, 6)), "a place read back is still in reach");
        assert!(!again.reaches((50, 50)));
    }

    fn nothing_held(_: (i32, i32)) -> bool {
        false
    }

    #[test]
    fn near_is_asked_about_before_far_however_long_far_has_grown() {
        // Use a generous reach and visit every candidate by hand. This test
        // covers queue ordering rather than the reach gate, so the gate is held
        // open.
        let puller = Puller::new(Arc::new(Store::in_memory()), Path::new("/nonexistent"), 0);
        puller.visit([((0, 0), 100), ((10, 10), 100), ((5, 5), 100)]);
        puller.seed_edge([(0, 0)], &nothing_held);
        puller.seed_edge([(10, 10)], &nothing_held);
        puller.seed_near([(5, 5)], &nothing_held);

        let batch = puller.next_batch(1);
        // This is a neighbour of (5, 5), not of (0, 0) or (10, 10).
        assert!(batch[0].0.abs_diff(5) <= 1 && batch[0].1.abs_diff(5) <= 1);
    }

    #[test]
    fn a_column_already_held_is_never_queued() {
        let puller = Puller::new(Arc::new(Store::in_memory()), Path::new("/nonexistent"), 0);
        puller.visit([((0, 0), 100)]);
        let held = |at: (i32, i32)| [(1, 0), (-1, 0), (0, 1), (0, -1)].contains(&at);
        puller.seed_near([(0, 0)], &held);

        // Every neighbour of (0, 0) is already held, so only (0, 0) itself is
        // new.
        assert_eq!(puller.waiting(), 1);
    }

    #[test]
    fn nothing_is_queued_past_the_reach_of_anywhere_stood_in() {
        let puller = Puller::new(Arc::new(Store::in_memory()), Path::new("/nonexistent"), 0);
        puller.visit([((0, 0), 2)]);

        // Six chunks out is beyond a reach of two from (0, 0).
        puller.seed_edge([(6, 0)], &nothing_held);
        assert_eq!(puller.waiting(), 0, "nothing this far from anywhere visited is queued");

        // One chunk out is within reach and must be queued.
        puller.seed_near([(0, 0)], &nothing_held);
        assert!(puller.waiting() > 0);
    }

    #[test]
    fn reach_is_a_disc_and_not_the_square_around_it() {
        let puller = Puller::new(Arc::new(Store::in_memory()), Path::new("/nonexistent"), 0);
        puller.visit([((0, 0), 2)]);

        // (2, 0) is two chunks out along one axis and inside. (2, 2) is two out
        // along both, which is 2.8 in a straight line, so it sits at the square's
        // corner and outside the disc.
        puller.seed_edge([(1, 0)], &|at| at != (2, 0));
        assert_eq!(puller.waiting(), 1, "(2, 0) is within reach");
        puller.seed_edge([(2, 1)], &|at| at != (2, 2));
        assert_eq!(puller.waiting(), 1, "(2, 2) is not");
    }

    #[test]
    fn nothing_is_queued_where_nobody_has_stood() {
        let puller = Puller::new(Arc::new(Store::in_memory()), Path::new("/nonexistent"), 12);
        puller.seed_near([(0, 0)], &nothing_held);
        assert_eq!(puller.waiting(), 0);
    }

    #[test]
    fn each_place_carries_its_own_reach() {
        let puller = Puller::new(Arc::new(Store::in_memory()), Path::new("/nonexistent"), 0);
        puller.visit([((0, 0), 1), ((100, 100), 3)]);
        puller.seed_edge([(1, 0)], &|at| at != (2, 0));
        assert_eq!(puller.waiting(), 0, "two out from a place seen one around is out of reach");
        puller.seed_edge([(101, 100)], &|at| at != (102, 100));
        assert_eq!(puller.waiting(), 1, "two out from a place seen three around is in reach");
    }
}
