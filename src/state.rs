//! The map as it currently stands.
//!
//! One value the request threads share, holding what has been loaded off disk
//! and what has been drawn from it. Everything that changes it is behind a lock
//! of its own, because the parts move on different clocks: the world when the
//! mod exports, the palette when an admin joins, the tiles whenever either does.
//!
//! What it holds is here. Terrain arrives through [`State::take_chunks`] from
//! [`crate::apiport`] and [`crate::pull`]; noticing that the palette or the
//! names on disk have moved is in [`crate::watch`], and what the page is told is
//! in [`crate::feeds`].

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::SystemTime;

use image::RgbImage;

use crate::auth::{Keeping, Sessions};
use crate::cache::{At, Cache};
use crate::columns::{Chunk, World, columns_dir};
use crate::events::Events;
use crate::config::Rules;
use crate::error::{Error, Result};
use crate::files;
use crate::history::History;
use crate::levels::Levels;
use crate::live::Live;
use crate::memory::Memory;
use crate::palette::Palette;
use crate::pending::Pending;
use crate::preferences::Preferences;
use crate::pyramid::{self, TILE, TileFormat};
use crate::render::{Renderer, UNMAPPED};
use crate::store::{self, Arrived, Store, Stored, Version};
use crate::log::{say, warn};

/// Which tiles one generation changed, as level and coordinates. `None` means
/// every tile: a new palette recolours the lot, and so does a gap in the history.
pub type Changed = Option<Vec<At>>;

pub struct State {
    pub data: PathBuf,
    /// The zoom levels above the finest, in memory and on disk.
    pub levels: Levels,
    /// The map on disk: every chunk, every remembered version, and what each
    /// person has seen. What `world` holds is this, read at start.
    pub store: Arc<Store>,
    /// What each person remembers of the map, and who shares it with whom.
    pub memory: Arc<Memory>,
    pub world: RwLock<World>,
    /// Reloaded like the world is. A palette can arrive long after start-up —
    /// an admin's client sends one when the server cannot build its own — and
    /// waiting for a restart to notice would make the map look broken.
    pub palette: RwLock<Palette>,
    /// Who is online and every marker, posted by the mod rather than read from a
    /// file it rewrote every couple of seconds.
    pub live: Arc<Live>,
    /// Who has followed a login link. Memory only — see [`crate::auth`].
    pub sessions: Arc<Sessions>,
    /// Markers asked for on the map and waiting for the mod to collect them.
    pub pending: Arc<Pending>,
    /// What each person has set for themselves — their presets, and where their
    /// new markers start. Kept against a uid and written to a file of its own.
    pub preferences: Arc<Preferences>,
    /// What the game calls each block, so that marking something can start from
    /// its name. Empty until the mod has exported one, and reloaded when it does.
    pub names: RwLock<HashMap<String, String>>,
    /// The names file's own timestamp, which is the whole signal that it moved.
    pub named: Mutex<Option<SystemTime>>,
    /// What the operator has said about who markers belong to. Read here only to
    /// tell the page which controls to offer; the mod is what enforces either.
    pub rules: Rules,
    /// The palette file's own timestamp.
    pub painted: Mutex<Option<SystemTime>>,
    /// When the ground in each region last changed, read from the database and
    /// moved as terrain arrives. What decides whether the stored zoom levels
    /// above a region are behind it.
    pub regions: Mutex<HashMap<(i32, i32), SystemTime>>,
    /// Bumped whenever the world actually changes. The viewer watches this and
    /// it versions tile URLs, which is what gets a new map past the browser cache.
    /// Where the world's oceans sit, as the mod last said.
    ///
    /// Held rather than read, because every tile drawn asks for it and it changes
    /// only when a different world is loaded. Refreshed on the same beat the
    /// regions are.
    sea_level: std::sync::atomic::AtomicI32,
    generation: AtomicU64,
    /// Which tiles each generation changed, so a viewer that has fallen a few
    /// generations behind can repaint those and leave the rest of the map alone.
    history: Mutex<History<Changed>>,
    /// Level 0 tiles whose levels above are out of date. Drained by the builder,
    /// so many changes in one window cost one rebuild rather than many.
    pub stale: Mutex<HashSet<(i32, i32)>>,
    /// Tiles served since the last report, and how many of them had to be
    /// drawn or encoded rather than handed out of the cache — see
    /// [`report_serving`](Self::report_serving).
    served: AtomicU64,
    drawn: AtomicU64,
    /// Level 0 tiles that changed and no browser has been told of yet.
    unannounced: Mutex<HashSet<At>>,
    /// Regions in which each person's own memory changed and they have not
    /// been told of yet — announced on the same beat as the tiles.
    unannounced_of: Mutex<HashMap<String, HashSet<(i32, i32)>>>,
    pub cache: Mutex<Cache>,
    /// Remembered chunks as pictures, by chunk, version and season: a reader
    /// away from ground that changed is shown the version they last saw, and a
    /// coarse tile over a long absence holds a hundred of those. Rendering each
    /// from the database on every request was most of what such a tile cost;
    /// a patch is a few kilobytes and the version it shows never changes.
    pub patches: Mutex<HashMap<((i32, i32), Version, u8), Arc<RgbImage>>>,
    /// Every browser waiting to be told of a change. See [`crate::events`].
    pub events: Events,
}

impl State {
    /// Loads what is on disk, without drawing any of it.
    pub fn load(data: &Path, palette: Palette, cache_bytes: usize, rules: Rules) -> Result<Self> {
        let columns = columns_dir(data);

        let store = Store::open(data)?;
        let world = if store.is_empty()? {
            // A database with nothing in it and region files beside it is a
            // server upgraded from a build whose map lived in those files. They
            // are read once, in full, and become the database; from then on the
            // database is the map and the files are what the mod last wrote.
            // Each region is dated by its file, so the zoom levels already built
            // from it are not rebuilt for having been imported.
            let world = World::load(data)?;
            let times = region_times(&columns);
            for (at, chunks) in by_region(&world) {
                let arrived: Vec<Arrived> = chunks
                    .into_iter()
                    .map(|((cx, cz), chunk)| Arrived { cx, cz, season: chunk.season(), record: chunk.record() })
                    .collect();
                let dated = times.get(&at).copied().unwrap_or_else(SystemTime::now);
                store.put_chunks(world.edge, &arrived, dated)?;
            }
            if !world.is_empty() {
                say!(
                    "imported {} chunks from the region files into {}",
                    world.chunks.len(),
                    store::path_in(data).display()
                );
            }
            world
        } else {
            let edge = store.edge()?;
            let mut chunks = HashMap::new();
            for held in store.chunks()? {
                if let Some(chunk) = Chunk::from_record(&held.record, edge, held.season) {
                    chunks.insert((held.cx, held.cz), chunk);
                }
            }
            World::from_chunks(edge, chunks)
        };
        let regions = store.region_times()?;
        let store = Arc::new(store);
        let memory = Arc::new(Memory::load(Arc::clone(&store)));
        let sessions = Arc::new(Sessions::load(Arc::clone(&store), Keeping::from_rules(&rules))?);
        let preferences = Arc::new(Preferences::load(data));
        for (uid, person) in preferences.all() {
            memory.set_shares(&uid, person.share_map_with.iter().copied());
        }

        Ok(Self {
            store,
            memory,
            world: RwLock::new(world),
            palette: RwLock::new(palette),
            regions: Mutex::new(regions),
            painted: Mutex::new(files::modified(&crate::palette::path_in(data))),
            live: Arc::new(Live::load(data, &rules.hidden_groups)),
            sessions,
            pending: Arc::new(Pending::new()),
            preferences,
            names: RwLock::new(crate::watch::block_names(data).unwrap_or_default()),
            named: Mutex::new(files::modified(&crate::watch::names_path(data))),
            rules,
            generation: AtomicU64::new(1),
            history: Mutex::new(History::default()),
            stale: Mutex::new(HashSet::new()),
            served: AtomicU64::new(0),
            drawn: AtomicU64::new(0),
            unannounced: Mutex::new(HashSet::new()),
            unannounced_of: Mutex::new(HashMap::new()),
            cache: Mutex::new(Cache::new(cache_bytes)),
            patches: Mutex::new(HashMap::new()),
            events: Events::default(),
            sea_level: std::sync::atomic::AtomicI32::new(crate::facts::read(data).sea_level),
            data: data.to_path_buf(),
            levels: Levels::new(data),
        })
    }

    /// Where the world's oceans sit.
    #[must_use]
    pub fn sea_level(&self) -> i32 {
        self.sea_level.load(Ordering::Relaxed)
    }

    /// Takes the sea level again, for a world that has said it since start-up.
    pub fn resettle_sea_level(&self) {
        self.sea_level.store(crate::facts::read(&self.data).sea_level, Ordering::Relaxed);
        self.forget_patches();
    }

    /// Forgets every rendered patch: the colours changed under them.
    pub fn forget_patches(&self) {
        if let Ok(mut patches) = self.patches.lock() {
            patches.clear();
        }
    }

    /// Takes chunks that arrived, wherever from: into the database first and
    /// then into the world, so that what is served is never ahead of what is
    /// kept. Says what each did to the map, for whoever remembers the ground.
    ///
    /// The one door terrain comes in by. A record the mod pushed and a column
    /// the puller fetched both pass through here, which is what makes the
    /// database the map rather than one more copy of it.
    pub fn take_chunks(&self, edge: usize, arrived: &[Arrived], at: SystemTime) -> Vec<Stored> {
        if arrived.is_empty() {
            return Vec::new();
        }

        let stored = match self.store.put_chunks(edge, arrived, at) {
            Ok(stored) => stored,
            Err(error) => {
                warn!("could not store {} chunks: {error}", arrived.len());
                return Vec::new();
            }
        };

        if let Ok(mut world) = self.world.write() {
            for chunk in arrived {
                if let Some(read) = Chunk::from_record(&chunk.record, edge, chunk.season) {
                    world.apply_one(chunk.cx, chunk.cz, edge, read);
                }
            }
        }

        if let Ok(mut regions) = self.regions.lock() {
            for one in stored.iter().filter(|one| one.surface_moved()) {
                regions.insert(store::region_of(one.cx, one.cz), at);
            }
        }
        stored
    }

    /// Moves a chunk's season: the year turning under ground that has not.
    /// Says whether anything moved, which is whether the tile wants drawing.
    pub fn take_season(&self, cx: i32, cz: i32, season: u8) -> bool {
        match self.store.set_season(cx, cz, season) {
            Ok(false) => return false,
            Ok(true) => {}
            Err(error) => {
                warn!("could not move the season of ({cx}, {cz}): {error}");
                return false;
            }
        }
        if let Ok(mut world) = self.world.write()
            && let Some(chunk) = world.chunks.get_mut(&(cx, cz))
        {
            chunk.set_season(season);
        }
        true
    }

    /// Says that the ground in these tiles has changed, so that a browser is
    /// told at once and the levels above are rebuilt on their own beat.
    ///
    /// A region is a level 0 tile, and slope shading reads the column to the
    /// west and north of each pixel — so a region drawn also changes the western
    /// edge of the tile east of it and the northern edge of the tile below.
    ///
    /// Level 0 is forgotten here and now, so the next request for it draws the
    /// world as it is — and announced on the next beat of [`announce`](Self::announce),
    /// so that ground arriving a few chunks at a time costs a browser one
    /// repaint of a tile per beat rather than one per arrival. The levels above
    /// are announced by the builder when it has made them, so a viewer is never
    /// sent a coarse tile that is older than the fine one under it.
    pub fn tiles_changed(&self, regions: impl IntoIterator<Item = (i32, i32)>) {
        let mut repaint: Vec<(i32, i32)> =
            regions.into_iter().flat_map(|(rx, rz)| [(rx, rz), (rx + 1, rz), (rx, rz + 1)]).collect();
        repaint.sort_unstable();
        repaint.dedup();
        if repaint.is_empty() {
            return;
        }

        let finest: Vec<At> = repaint.iter().map(|&(x, z)| (0, x, z)).collect();
        self.drop_tiles(&finest);
        self.mark_stale(repaint);
        if let Ok(mut unannounced) = self.unannounced.lock() {
            unannounced.extend(finest);
        }
    }

    /// Tells every browser what has changed since it last said so — which
    /// level 0 tiles for everybody, and which regions for each person alone —
    /// once per beat, however many arrivals and discoveries the beat held.
    /// Nothing is said on a beat where nothing moved, so the map's clock never
    /// turns for nothing. One generation for the lot: forty people exploring
    /// used to turn the clock forty times as often, and every turn was one more
    /// address a browser could not find in its cache.
    pub fn announce(&self) {
        let changed: Vec<At> = {
            let Ok(mut unannounced) = self.unannounced.lock() else { return };
            let mut changed: Vec<At> = unannounced.drain().collect();
            changed.sort_unstable();
            changed
        };
        let personal: Vec<(String, Vec<(i32, i32)>)> = {
            let Ok(mut unannounced) = self.unannounced_of.lock() else { return };
            unannounced
                .drain()
                .map(|(uid, regions)| {
                    let mut regions: Vec<(i32, i32)> = regions.into_iter().collect();
                    regions.sort_unstable();
                    (uid, regions)
                })
                .collect()
        };
        if changed.is_empty() && personal.is_empty() {
            return;
        }
        let generation = self.bump(Some(changed));
        for (uid, regions) in personal {
            self.memory.record(&uid, generation, regions);
        }
    }

    /// The same, for chunks that were just stored — and everybody who was not
    /// there keeps the version they last saw.
    pub fn terrain_changed(&self, stored: &[Stored]) {
        self.tiles_changed(stored.iter().map(|one| store::region_of(one.cx, one.cz)));
        // What moved for each person alone, beside what moved for everybody.
        for (uid, regions) in self.memory.changed(stored) {
            self.memory_changed(&uid, regions);
        }
    }

    /// Notes that one person's own memory changed in these regions: their
    /// composed tiles there are forgotten now, and they are told on the next
    /// beat — see [`announce`](Self::announce).
    fn memory_changed(&self, uid: &str, regions: Vec<(i32, i32)>) {
        self.drop_remembered(uid, &regions);
        if let Ok(mut unannounced) = self.unannounced_of.lock() {
            unannounced.entry(uid.to_owned()).or_default().extend(regions);
        }
    }

    /// Takes what one person has set for themselves, and what follows from it:
    /// whom their map is shared with is read from here by everything that draws
    /// one, so it moves the moment the setting does.
    pub fn keep_person(&self, uid: &str, person: crate::preferences::Person) -> bool {
        let shares: Vec<i32> = person.share_map_with.clone();
        if !self.preferences.set(uid, person) {
            return false;
        }
        self.memory.set_shares(uid, shares);
        true
    }

    /// Takes where somebody is standing: everything within `radius` chunks is
    /// theirs to see, now and from now on.
    pub fn seen_from(&self, uid: &str, x: i32, z: i32, radius: i32) {
        let edge = self.chunk_edge().max(1) as i32;
        let (cx, cz) = (x.div_euclid(edge), z.div_euclid(edge));
        let sight: Vec<(i32, i32)> = crate::columns::disc_of((cx, cz), radius).collect();
        if let Some(regions) = self.memory.saw(uid, &sight) {
            self.memory_changed(uid, regions);
        }
    }

    pub fn generation(&self) -> u64 {
        self.generation.load(Ordering::Relaxed)
    }

    /// Records a generation and what it changed, then returns the new number.
    pub fn bump(&self, tiles: Changed) -> u64 {
        let generation = self.generation.fetch_add(1, Ordering::Relaxed) + 1;
        if let Ok(mut history) = self.history.lock() {
            history.record(generation, tiles, std::time::Instant::now());
        }
        self.events.map_changed();
        generation
    }

    /// What a viewer last at `since` needs to repaint.
    ///
    /// `None` means everything: either the palette changed, or the viewer has
    /// fallen further behind than the history goes and there is no honest way to
    /// tell it which tiles it missed.
    pub fn changes_since(&self, since: u64) -> Changed {
        if since >= self.generation() {
            return Some(Vec::new());
        }

        let history = self.history.lock().ok()?;
        let mut tiles = Vec::new();
        for changed in history.since(since)? {
            tiles.extend(changed.as_ref()?.iter().copied());
        }
        tiles.sort_unstable();
        tiles.dedup();
        Some(tiles)
    }

    pub fn bounds(&self) -> (i32, i32, i32, i32) {
        self.world.read().map_or((0, 0, 0, 0), |world| world.bounds())
    }

    pub fn chunks(&self) -> usize {
        self.world.read().map_or(0, |world| world.chunks.len())
    }

    /// Blocks along a chunk's edge, which is what the viewer draws its grid on.
    /// Zero until something has been exported.
    pub fn chunk_edge(&self) -> usize {
        self.world.read().map_or(0, |world| world.edge)
    }

    /// How many levels this world is wide enough to need.
    pub fn levels(&self) -> u32 {
        let (min_x, min_z, max_x, max_z) = self.bounds();
        let tile = i64::from(TILE);
        let across = (i64::from(max_x) - i64::from(min_x)).div_euclid(tile);
        let down = (i64::from(max_z) - i64::from(min_z)).div_euclid(tile);
        pyramid::levels_for(across, down)
    }

    /// Says how the terrain resolves against whatever palette is loaded now.
    pub fn report_coverage(&self) {
        let (Ok(world), Ok(palette)) = (self.world.read(), self.palette.read()) else {
            return;
        };
        say!("surface {}", Renderer::new(&world, &palette, self.sea_level()).coverage().summary());
    }

    /// One tile as PNG bytes, drawn or read as its level requires.
    pub fn tile(&self, at: At) -> Result<Arc<[u8]>> {
        self.tile_as(at, TileFormat::Png)
    }

    /// One tile as everybody sees it, encoded as one reader asked for.
    pub fn tile_as(&self, at: At, format: TileFormat) -> Result<Arc<[u8]>> {
        let key = Self::cache_key("", format);
        if let Ok(mut cache) = self.cache.lock()
            && let Some(bytes) = cache.get(&key, &at)
        {
            self.counted(false);
            return Ok(bytes);
        }
        self.counted(true);

        let bytes: Arc<[u8]> = match (at.0, format) {
            (0, _) => pyramid::encode_as(&self.finest(at.1, at.2)?, format)?,
            // The stored levels are PNG already, so the exact picture is the
            // bytes as they lie; any other picture of them is made from them.
            (_, TileFormat::Png) => self.stored(at)?,
            (level, _) => {
                let image = self.levels.image(at).ok_or_else(|| {
                    Error::Empty(format!("level {level} tile ({}, {}) is not built yet", at.1, at.2))
                })?;
                pyramid::encode_as(&image, format)?
            }
        }
        .into();

        if let Ok(mut cache) = self.cache.lock() {
            cache.insert(key, at, Arc::clone(&bytes));
        }
        Ok(bytes)
    }

    /// What a tile is cached under: whose it is, and how it is encoded. Two
    /// encodings of one picture never answer for each other.
    #[must_use]
    pub fn cache_key(whose: &str, format: TileFormat) -> String {
        match format {
            TileFormat::Png => whose.to_owned(),
            other => format!("{};{whose}", other.name()),
        }
    }

    /// Level 0, which is drawn from the world rather than stored.
    ///
    /// A palette with no colours in it would blank the finest level while the
    /// rest of the pyramid went on showing the world, which reads as a map that
    /// breaks when you zoom in — it was taken for that three times. Growing the
    /// level above instead is what makes the viewer fall back to a coarse map
    /// rather than an empty one.
    fn finest(&self, tx: i32, tz: i32) -> Result<RgbImage> {
        // Scoped, and the guards let go before anything else is asked of the
        // world. `levels()` reads it too, and a thread that takes a second read
        // lock while holding one may deadlock against a writer that arrived in
        // between — which on this lock is the watcher, every time the mod
        // exports.
        {
            let (Ok(world), Ok(palette)) = (self.world.read(), self.palette.read()) else {
                return Err(Error::Empty("the map is being reloaded".to_owned()));
            };

            if !palette.paints_nothing() {
                return Ok(Renderer::new(&world, &palette, self.sea_level()).render(
                    tx * TILE as i32,
                    tz * TILE as i32,
                    TILE,
                ));
            }
        }

        self.levels.from_above(0, tx, tz, TILE, self.levels()).ok_or_else(
            || Error::Empty("the palette has no colours and no level above has this ground".to_owned()),
        )
    }

    /// A level above zero, which the builder has already drawn.
    ///
    /// Never made on demand: a coarse tile is four of the level below, so making
    /// one here would make every tile beneath it — a thousand renders for a level
    /// five, while somebody waits. Wherever the builder's picture is held right
    /// now is [`Levels`]' business.
    fn stored(&self, at: At) -> Result<Vec<u8>> {
        self.levels.bytes(at)
    }

    /// One level 0 tile as an image, or nothing where the world has no chunks.
    fn level_zero(&self, tx: i32, tz: i32, mapped: &HashSet<(i32, i32)>) -> Option<RgbImage> {
        if !mapped.contains(&(tx, tz)) {
            return None;
        }
        let (Ok(world), Ok(palette)) = (self.world.read(), self.palette.read()) else {
            return None;
        };
        Some(Renderer::new(&world, &palette, self.sea_level()).render(tx * TILE as i32, tz * TILE as i32, TILE))
    }

    /// Rebuilds every level above zero for whatever has changed since last time.
    ///
    /// Bottom up, one level at a time: the tiles that changed at a level decide
    /// which tiles change at the level above, and four of the former make one of
    /// the latter. A region changing therefore costs one tile per level, not one
    /// tile per level per region.
    pub fn build_levels(&self) {
        let Some(mut changed) = self.take_stale() else {
            return;
        };

        let levels = self.levels();

        // A world that has grown past a power of two gains a coarsest level that
        // has never been built. Walking up from what changed builds exactly one
        // tile there and leaves the rest of the level missing — and that level is
        // the one a viewer opens on, so the map reads as empty until something
        // else marks every region stale. The whole pyramid is measured against the
        // world whenever it is shorter than the world needs, which is once per
        // doubling and never in the steady state.
        if self.levels.built() < levels
            && let Ok(regions) = self.regions.lock()
        {
            let behind = pyramid::behind(&self.data, &regions, levels);
            say!("the world now needs {levels} levels — {} regions to rebuild", behind.len());
            changed.extend(behind);
        }

        let Ok(mapped) = self.world.read().map(|world| world.regions().collect())
        else {
            return;
        };

        // The level 0 tiles were announced when the ground arrived; what is
        // announced here is only the levels this built.
        let mut repainted: Vec<At> = Vec::new();
        let now = SystemTime::now();

        for level in 1..=levels {
            let parents: HashSet<(i32, i32)> =
                changed.iter().map(|&(x, z)| pyramid::ancestor(1, x, z)).collect();

            for &(px, pz) in &parents {
                let below = pyramid::children(px, pz).map(|(cx, cz)| {
                    if level == 1 {
                        self.level_zero(cx, cz, &mapped)
                    } else {
                        self.levels.image((level - 1, cx, cz))
                    }
                });

                if below.iter().all(Option::is_none) {
                    continue;
                }

                let parent = pyramid::downsample(&below, TILE, UNMAPPED);
                self.levels.put((level, px, pz), parent, now);
                repainted.push((level, px, pz));
            }

            changed = parents;
        }

        self.drop_tiles(&repainted);

        if let Ok(palette) = self.palette.read() {
            pyramid::record_palette(&self.data, &palette.fingerprint);
        }

        let generation = self.bump(Some(repainted.clone()));
        say!("{} tiles rebuilt across {levels} levels (generation {generation})", repainted.len());
    }

    /// Counts one tile served, and whether it cost a render or an encode.
    pub fn counted(&self, drawn: bool) {
        self.served.fetch_add(1, Ordering::Relaxed);
        if drawn {
            self.drawn.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Says how many tiles went out since it last said, and how many of them
    /// cost a render or an encode. Once a minute, and only where anything did:
    /// the number that says whether a server is serving its cache or drawing
    /// the same tiles over and over.
    pub fn report_serving(&self) {
        let served = self.served.swap(0, Ordering::Relaxed);
        let drawn = self.drawn.swap(0, Ordering::Relaxed);
        if served > 0 {
            say!("{served} tiles served in the last minute, {drawn} of them drawn or encoded");
        }
    }

    /// Writes the level tiles that have waited long enough — see
    /// [`Levels::flush`] for what long enough is.
    pub fn flush_levels(&self) {
        let written = self.levels.flush(SystemTime::now());
        if written > 0 {
            say!("{written} level tiles written, {} still waiting", self.levels.waiting());
        }
    }

    /// The level 0 tiles waiting to have their levels rebuilt, and none left
    /// behind. `None` when there is nothing to do, which is the common tick.
    fn take_stale(&self) -> Option<HashSet<(i32, i32)>> {
        let mut stale = self.stale.lock().ok()?;
        (!stale.is_empty()).then(|| std::mem::take(&mut *stale))
    }

    /// Forgets tiles that have been drawn again.
    pub fn drop_tiles(&self, tiles: &[At]) {
        if let Ok(mut cache) = self.cache.lock() {
            for at in tiles {
                cache.remove(at);
            }
        }
    }

    /// Notes level 0 tiles whose levels above need building again.
    pub fn mark_stale(&self, tiles: impl IntoIterator<Item = (i32, i32)>) {
        if let Ok(mut stale) = self.stale.lock() {
            stale.extend(tiles);
        }
    }
}

/// A world's chunks gathered by the region each sits in.
fn by_region(world: &World) -> HashMap<(i32, i32), Vec<((i32, i32), &Chunk)>> {
    let mut grouped: HashMap<(i32, i32), Vec<((i32, i32), &Chunk)>> = HashMap::new();
    for (&at, chunk) in &world.chunks {
        grouped.entry(store::region_of(at.0, at.1)).or_default().push((at, chunk));
    }
    grouped
}

/// When each region file on disk was last written. Read once, on the start
/// that imports them.
fn region_times(dir: &Path) -> HashMap<(i32, i32), SystemTime> {
    let Ok(paths) = crate::columns::region_files(dir) else {
        return HashMap::new();
    };

    paths
        .into_iter()
        .filter_map(|path| Some((crate::columns::region_coords(&path)?, files::modified(&path)?)))
        .collect()
}

/// What a test needs to stand a map up: a palette with a colour in it, and
/// rules that say nothing surprising. Shared between the modules that build a
/// `State`, so a rule added is a rule every one of them gets.
#[cfg(test)]
pub mod testing {
    use super::*;

    /// Writes a palette naming block 11 grey and block 22 red, and loads it.
    pub fn palette_in(at: &Path) -> Palette {
        std::fs::write(
            crate::palette::path_in(at),
            r##"{"Version":1,"GameVersion":"1.22.7","Source":"client","Fingerprint":"abc",
                "Blocks":{"game:air":{"Id":0,"Rgb":null,"Invisible":true},
                          "game:rock":{"Id":11,"Rgb":"#646464"},
                          "game:brick":{"Id":22,"Rgb":"#c02020"}}}"##,
        )
        .expect("a palette");
        Palette::load(at).expect("it parses")
    }

    pub fn rules(private_map: bool) -> Rules {
        Rules {
            markers_public: false,
            markers_editable: false,
            players_public: true,
            live_refresh_ms: 2000,
            private_map,
            anonymous_spawn: false,
            anonymous_spawn_radius_chunks: 8,
            sight_radius_chunks: 0,
            session_hours: 0,
            sessions_reset_on_restart: false,
            hidden_groups: vec!["xlib".to_owned()],
        }
    }

    /// A map of this build's chunk edge, read from a fresh scratch directory.
    pub fn state_in(at: &Path, private_map: bool) -> State {
        State::load(at, palette_in(at), 1 << 20, rules(private_map)).expect("a start")
    }
}

#[cfg(test)]
mod tests {
    use super::testing::{palette_in, rules};
    use super::*;
    use crate::files::testing::Scratch;

    /// The upgrade path: a server whose map lived in region files starts this
    /// build, and the files become the database. The next start reads the
    /// database alone, so the files may go.
    #[test]
    fn region_files_are_imported_once_and_the_database_is_the_map_after() {
        let held = Scratch::new("state-import");
        let at = held.at();
        let columns = columns_dir(at);
        std::fs::create_dir_all(&columns).unwrap();
        std::fs::write(
            columns.join("r.2.-3.msqr"),
            crate::columns::testing::filed((2, -3), 4, &[(0, 7, 11), (17, 9, 11)], None),
        )
        .unwrap();

        let first = State::load(at, palette_in(at), 1 << 20, rules(false)).expect("a first start");
        assert_eq!(first.chunks(), 2, "both chunks came in off the file");
        assert_eq!(first.store.counts().unwrap().chunks, 2, "and into the database");
        drop(first);

        // The files go, and the map is still there.
        std::fs::remove_dir_all(&columns).unwrap();
        let second = State::load(at, palette_in(at), 1 << 20, rules(false)).expect("a second start");
        assert_eq!(second.chunks(), 2);
        assert_eq!(second.chunk_edge(), 4);
        let world = second.world.read().unwrap();
        assert_eq!(world.column_at(32 * 4, -48 * 4).map(|c| (c.block, c.season)), Some((11, 7)));
        assert_eq!(world.column_at(33 * 4, -47 * 4).map(|c| c.season), Some(9), "the season came back too");
    }

    /// The door every chunk comes in by: what is stored is what is served, and a
    /// chunk that arrives again unchanged is reported as unchanged.
    #[test]
    fn a_chunk_taken_is_in_the_database_and_the_world_alike() {
        let held = Scratch::new("state-take");
        let at = held.at();
        let state = State::load(at, palette_in(at), 1 << 20, rules(false)).expect("an empty start");

        let record = Chunk::filled_with(crate::columns::Column { block: 11, height: 3, temperature: 1, rainfall: 2, season: 5 }, 4).record();
        let stored = state.take_chunks(2, &[Arrived { cx: 1, cz: 1, season: 5, record: record.clone() }], SystemTime::now());
        assert_eq!(stored.len(), 1);
        assert!(stored[0].surface_moved());
        assert_eq!(state.chunks(), 1);
        assert_eq!(state.store.counts().unwrap().chunks, 1);

        let again = state.take_chunks(2, &[Arrived { cx: 1, cz: 1, season: 5, record }], SystemTime::now());
        assert!(!again[0].surface_moved(), "the same bytes are not a change");
    }

    /// Ground arriving in several pieces is one announcement, on the beat.
    #[test]
    fn changed_tiles_are_announced_once_per_beat() {
        let held = Scratch::new("state-announce");
        let at = held.at();
        let state = State::load(at, palette_in(at), 1 << 20, rules(false)).expect("an empty start");
        let before = state.generation();

        state.tiles_changed([(0, 0)]);
        state.tiles_changed([(0, 0)]);
        state.tiles_changed([(3, 3)]);
        assert_eq!(state.generation(), before, "nothing is said until the beat");

        state.announce();
        assert_eq!(state.generation(), before + 1, "three arrivals, one announcement");
        let mut told = state.changes_since(before).expect("within what is remembered");
        told.sort_unstable();
        let mut expected = vec![(0, 0, 0), (0, 1, 0), (0, 0, 1), (0, 3, 3), (0, 4, 3), (0, 3, 4)];
        expected.sort_unstable();
        assert_eq!(told, expected, "each region, and the two tiles its shading reaches into");

        state.announce();
        assert_eq!(state.generation(), before + 1, "a quiet beat says nothing");
    }
}
