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

/// What came of asking the game to load a column.
enum Loadable {
    /// The savegame has no such column, so nothing will ever answer for it.
    Absent,
    /// The game was asked to load it, so a later step may find it answerable.
    Asked,
}

/// How many times one column is asked for before the service stops asking.
///
/// A column the mod cannot answer for is asked again after the game has been
/// told to load it, because the load is what makes the next ask answerable. A
/// column that is still unanswered after this many rounds is one the game
/// cannot produce, and asking forever would poll a chunk that is never coming
/// for as long as the service runs. The count is per column and only rises on a
/// round that failed, so ordinary ground costs one attempt.
const MOST_ATTEMPTS: u32 = 5;

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
    /// How many times each column has been asked for, and never answered.
    ///
    /// This is what stops one column being asked for forever, and what lets a
    /// column be asked again at all: an entry is what `offer` reads to decide
    /// whether a column is new, in flight or given up on. A column that arrives
    /// is removed, so the map growing over it costs nothing.
    tried: Mutex<HashMap<(i32, i32), u32>>,
    /// The columns already reported as given up on, so the log says so once
    /// rather than on every pass that offers them again.
    said: Mutex<HashSet<(i32, i32)>>,
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
            tried: Mutex::new(HashMap::new()),
            said: Mutex::new(HashSet::new()),
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
        // A column is queued once per round of asking. An entry means it is
        // already queued or has been asked for and not answered; `retry` is what
        // puts one back after the game has been told to load it, and that is the
        // only way a column asked for once is asked for again. Past the cap it
        // is left alone, so a column the game cannot produce stops costing a
        // request on every pass that draws something beside it.
        match tried.get(&at) {
            Some(&attempts) if attempts >= MOST_ATTEMPTS => return,
            Some(_) => return,
            None => {
                tried.insert(at, 0);
            }
        }
        queue.push_back(at);
    }

    /// Puts a column back on the queue after the game has been asked to load it,
    /// unless it has been asked for too many times already.
    ///
    /// The load is what makes the next ask answerable, so a column dropped
    /// without this is a hole nothing fills: `offer` refuses a column it has an
    /// entry for, so nothing beside it can put it back either.
    ///
    /// Returns false once the column has been given up on, so the caller can say
    /// so.
    fn retry(&self, at: (i32, i32)) -> bool {
        let Ok(mut tried) = self.tried.lock() else { return false };
        let attempts = tried.entry(at).or_insert(0);
        *attempts += 1;
        if *attempts >= MOST_ATTEMPTS {
            return false;
        }
        drop(tried);
        let Ok(mut queue) = self.far.lock() else { return true };
        if queue.len() < MAX_QUEUED {
            queue.push_back(at);
        }
        true
    }

    /// Forgets that a column was ever asked for, so it is offered afresh.
    ///
    /// A column that arrived is no longer owed anything, and one that the map
    /// later loses is asked for again like ground nobody has drawn.
    fn arrived(&self, at: (i32, i32)) {
        if let Ok(mut tried) = self.tried.lock() {
            tried.remove(&at);
        }
        if let Ok(mut said) = self.said.lock() {
            said.remove(&at);
        }
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
    /// or not saved, are asked for again on a later step, once the game has been
    /// told to load them. `tried` counts those rounds so a column the game
    /// cannot produce is given up on rather than asked for forever.
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
                Ok(Some((edge, told, chunk))) => {
                    // The season the mod read off its own calendar for this
                    // column. A mod too old to send one leaves the chunk with
                    // whatever it had, which for ground never drawn before is
                    // the year's start until the mod's next season pass says
                    // otherwise — the behaviour before the season was carried.
                    let season = told.unwrap_or_else(|| {
                        state
                            .world
                            .read()
                            .ok()
                            .and_then(|world| world.chunks.get(&(cx, cz)).map(Chunk::season))
                            .unwrap_or(0)
                    });
                    self.arrived((cx, cz));
                    arrived.push((edge, crate::mapdata::store::Arrived { cx, cz, season, record: chunk.record() }));
                }
                Ok(None) => {
                    // The mod does not have this column loaded. Ask the game for
                    // it and queue the column again, because the load is what
                    // makes the next ask answerable.
                    match self.request_load(&endpoint, cx, cz) {
                        // The savegame has no such column. No amount of asking
                        // makes one, so this is not a failure to retry: it is
                        // ground the world does not have, and the map is right
                        // to leave it blank. Said at debug rather than as a
                        // warning, because the edge of a world is not a fault.
                        Loadable::Absent => {
                            self.given_up((cx, cz));
                        }
                        Loadable::Asked => {
                            if !self.retry((cx, cz)) {
                                self.gave_up(cx, cz);
                            }
                        }
                    }
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

    /// Stops a column being asked for again, saying nothing.
    ///
    /// For a column the savegame does not have: the map is right to leave it
    /// blank and an operator has nothing to fix, so this is bookkeeping rather
    /// than a report.
    fn given_up(&self, at: (i32, i32)) {
        if let Ok(mut tried) = self.tried.lock() {
            tried.insert(at, MOST_ATTEMPTS);
        }
    }

    /// Says once that a column has been given up on, and what to do about it.
    ///
    /// This is the only sign an operator gets that a square of the map will stay
    /// unexplored-looking with nobody near it, so it names the column, says why
    /// it stopped, and says the one thing that fixes it. Said once per column:
    /// the queue offers a given-up column again whenever something is drawn
    /// beside it, and a warning per pass would bury the first one.
    fn gave_up(&self, cx: i32, cz: i32) {
        if let Ok(mut said) = self.said.lock() {
            if !said.insert((cx, cz)) {
                return;
            }
        }
        warn!(
            "gave up asking for the ground at chunk ({cx}, {cz}) after {MOST_ATTEMPTS} tries: \
             the game server would not load it, so it stays blank on the map. \
             Walking a player through it draws it. If it stays blank after that, \
             the column is not in the game's own save."
        );
    }

    /// Fetches one column, or returns `None` when the mod has nothing loaded to
    /// answer with.
    fn fetch_column(
        &self, endpoint: &Endpoint, cx: i32, cz: i32,
    ) -> Result<Option<(usize, Option<u8>, Chunk)>, String> {
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
        Ok(Some((edge, parsed.season, chunk)))
    }

    /// Asks the mod to load a column it does not currently hold, so the game's
    /// own chunk-loaded event hands it to the exporter and it arrives by the
    /// push path.
    ///
    /// This only asks for a column the savegame already has. Loading one it does
    /// not have would generate it, and the map must never make the world bigger.
    fn request_load(&self, endpoint: &Endpoint, cx: i32, cz: i32) -> Loadable {
        match self.exists(endpoint, cx, cz) {
            Ok(true) => {}
            Ok(false) => return Loadable::Absent,
            Err(error) => {
                warn!("could not ask the mod whether ({cx}, {cz}) exists: {error}");
                // Whether the world has this column is unknown, so this is not
                // an answer that it has none. Counted as an attempt like any
                // other, which is what bounds the asking.
                return Loadable::Asked;
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
        Loadable::Asked
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
    /// Where the column sits in the year, as the mod read it off the game's own
    /// calendar. Absent from a mod older than the field, and a column with no
    /// season keeps whatever the map had for it.
    #[serde(rename = "Season", default)]
    season: Option<u8>,
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
    fn a_column_the_mod_cannot_answer_for_is_asked_again() {
        let puller = Puller::new(Arc::new(Store::in_memory()), Path::new("/nonexistent"), 0);
        puller.visit([((0, 0), 100)]);
        puller.seed_near([(0, 0)], &nothing_held);
        let queued = puller.waiting();
        assert!(queued > 0, "the column is queued to begin with");

        // Draining the queue is what a step does before asking. Nothing may be
        // re-offered from outside, which is the bug this guards: `offer` refuses
        // a column it has already seen, so only `retry` can put one back.
        let batch = puller.next_batch(queued);
        assert_eq!(puller.waiting(), 0, "and drained by the asking");
        puller.seed_near([(0, 0)], &nothing_held);
        assert_eq!(puller.waiting(), 0, "a fresh offer alone does not put it back");

        assert!(puller.retry(batch[0]), "a column not yet given up on goes back on the queue");
        assert_eq!(puller.waiting(), 1, "so a later step asks for it again");
    }

    #[test]
    fn a_column_nothing_answers_for_is_given_up_on() {
        let puller = Puller::new(Arc::new(Store::in_memory()), Path::new("/nonexistent"), 0);
        puller.visit([((0, 0), 100)]);
        let at = (0, 0);
        puller.seed_near([at], &nothing_held);
        puller.next_batch(puller.waiting());

        // Every round after the first drains the queue again, the way a step
        // does. The last one must refuse, or an unanswerable column is asked
        // for as long as the service runs.
        let mut answered = 0;
        for _ in 0..MOST_ATTEMPTS * 2 {
            if !puller.retry(at) {
                break;
            }
            answered += 1;
            puller.next_batch(puller.waiting());
        }
        assert!(answered < (MOST_ATTEMPTS * 2) as usize, "the asking stops");
        assert_eq!(answered, (MOST_ATTEMPTS - 1) as usize, "after the attempts it is allowed");
        assert_eq!(puller.waiting(), 0, "and nothing is left queued for it");

        // And it stays given up on however much is drawn beside it.
        puller.seed_near([at], &nothing_held);
        puller.seed_edge([at], &nothing_held);
        assert_eq!(puller.waiting(), 0, "a given-up column is not offered again");
    }

    #[test]
    fn a_column_that_arrives_is_asked_for_again_if_the_map_loses_it() {
        let puller = Puller::new(Arc::new(Store::in_memory()), Path::new("/nonexistent"), 0);
        puller.visit([((0, 0), 100)]);
        let at = (0, 0);
        puller.seed_near([at], &nothing_held);
        puller.next_batch(puller.waiting());

        puller.arrived(at);
        puller.seed_near([at], &nothing_held);
        assert!(puller.waiting() > 0, "ground the map lost is asked for like ground never drawn");
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
