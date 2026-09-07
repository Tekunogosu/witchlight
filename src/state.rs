//! Holds the map's current state.
//!
//! [`State`] is the one value the request threads share. It holds what was
//! loaded off disk and what has been drawn from it. Each mutable part sits
//! behind its own lock, because the parts change on different schedules: the
//! world when the mod exports, the palette when an admin joins, and the tiles
//! whenever either does.
//!
//! Terrain arrives through [`State::take_chunks`] from
//! [`crate::protocol::apiport`] and [`crate::protocol::pull`].
//! [`crate::protocol::watch`] notices when the palette or the block names on
//! disk change. [`crate::web::feeds`] builds what the page is told.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::SystemTime;

use image::RgbImage;

use crate::protocol::auth::{Keeping, Sessions};
use crate::util::cache::{At, Cache};
use crate::render::columns::{Chunk, World, columns_dir};
use crate::web::events::Events;
use crate::config::Rules;
use crate::util::error::{Error, Result};
use crate::util::files;
use crate::util::history::History;
use crate::render::levels::Levels;
use crate::protocol::live::Live;
use crate::mapdata::memory::Memory;
use crate::render::palette::Palette;
use crate::protocol::pending::Pending;
use crate::protocol::preferences::Preferences;
use crate::render::pyramid::{self, TILE, TileFormat};
use crate::render::tiles::{Renderer, UNMAPPED};
use crate::mapdata::store::{self, Arrived, Store, Stored, Version};
use crate::util::log::{say, warn};

/// Lists the tiles one generation changed, as level and coordinates. `None`
/// means every tile, which is what a new palette or a gap in the history causes.
pub type Changed = Option<Vec<At>>;

/// One drawn chunk in the patch cache: where it is, which version it shows, and
/// the season it was drawn in. All three decide the picture, so all three key it.
pub type PatchKey = ((i32, i32), Version, u8);

/// A chunk together with the coordinate it sits at.
type PlacedChunk<'a> = ((i32, i32), &'a Chunk);

pub struct State {
    pub data: PathBuf,
    /// The zoom levels above the finest, in memory and on disk.
    pub levels: Levels,
    /// The map on disk, holding every chunk, every remembered version, and
    /// what each person has seen. `world` is this, read at startup.
    pub store: Arc<Store>,
    /// Records what each person remembers of the map, and who shares it with
    /// whom.
    pub memory: Arc<Memory>,
    pub world: RwLock<World>,
    /// The block colour palette, reloaded like the world is. A palette can
    /// arrive long after startup, because an admin's client sends one when the
    /// server cannot build its own. Waiting for a restart to notice would leave
    /// the map looking broken.
    pub palette: RwLock<Palette>,
    /// Holds who is online and every marker. The mod posts these rather than
    /// rewriting a file every couple of seconds.
    pub live: Arc<Live>,
    /// Tracks who has followed a login link. Held in memory only. See
    /// [`crate::protocol::auth`].
    pub sessions: Arc<Sessions>,
    /// Holds markers asked for on the map that the mod has not yet collected.
    pub pending: Arc<Pending>,
    /// Holds every plugin keeping rows here. It is empty until a plugin
    /// registers. Registration is the only thing that adds to it. Nothing is
    /// loaded from disk and no plugin code runs in this process.
    pub plugins: Arc<crate::mapdata::plugins::Plugins>,
    /// Records which groups each person shares each plugin's rows with, keyed
    /// by plugin. It is read from the database when a plugin registers and then
    /// held, so deciding whose rows a reader may see costs a lookup rather than
    /// a query.
    plugin_shares: Mutex<HashMap<String, HashMap<String, HashSet<i32>>>>,
    /// Holds each person's own settings, such as their presets and where their
    /// new markers start. Keyed by uid and written to its own file.
    pub preferences: Arc<Preferences>,
    /// Maps each block code to the name the game shows, so marking something
    /// can start from its name. It is empty until the mod exports the names, and
    /// is reloaded when the mod rewrites them.
    pub names: RwLock<HashMap<String, String>>,
    /// The block names file's modification time. A change to it is the only
    /// signal that the names were rewritten.
    pub named: Mutex<Option<SystemTime>>,
    /// Holds the operator's visibility and ownership settings. This service
    /// reads them only to decide which controls the page offers. The mod
    /// enforces them.
    pub rules: Rules,
    /// The palette file's own timestamp.
    pub painted: Mutex<Option<SystemTime>>,
    /// Records when the ground in each region last changed. It is read from the
    /// database and updated as terrain arrives. Comparing it against a stored
    /// zoom level decides whether that level is behind its region.
    pub regions: Mutex<HashMap<(i32, i32), SystemTime>>,
    /// The height of the world's oceans, as the mod last reported it.
    ///
    /// Held in memory rather than read per use, because every tile drawn asks
    /// for it and it changes only when a different world is loaded. It is
    /// refreshed on the same schedule as the region times.
    sea_level: std::sync::atomic::AtomicI32,
    /// Increments whenever the world changes. The viewer watches this, and it
    /// versions tile URLs so a new map gets past the browser cache.
    generation: AtomicU64,
    /// Records which tiles each generation changed, so a viewer a few
    /// generations behind repaints only those and leaves the rest alone.
    history: Mutex<History<Changed>>,
    /// Holds level 0 tiles whose levels above are out of date. The builder
    /// drains it, so many changes in one window cost one rebuild.
    pub stale: Mutex<HashSet<(i32, i32)>>,
    /// Counts tiles served since the last report, and how many had to be drawn
    /// or encoded rather than served from the cache. See
    /// [`report_serving`](Self::report_serving).
    served: AtomicU64,
    drawn: AtomicU64,
    /// Holds level 0 tiles that changed and that no browser has been told of.
    unannounced: Mutex<HashSet<At>>,
    /// Holds, per person, the regions in which their own memory changed and
    /// they have not been told. These are announced with the tiles.
    unannounced_of: Mutex<HashMap<String, HashSet<(i32, i32)>>>,
    pub cache: Mutex<Cache>,
    /// Caches remembered chunks as images, keyed by chunk, version and season.
    ///
    /// A reader away from ground that changed sees the version they last saw,
    /// and a coarse tile over a long absence holds a hundred such versions.
    /// Rendering each from the database per request dominated the cost of such a
    /// tile. A cached image is a few kilobytes and the version it shows never
    /// changes.
    pub patches: Mutex<HashMap<PatchKey, Arc<RgbImage>>>,
    /// Holds every browser waiting to be told of a change. See
    /// [`crate::web::events`].
    pub events: Events,
}

impl State {
    /// Loads what is on disk, without drawing any of it.
    pub fn load(data: &Path, palette: Palette, cache_bytes: usize, rules: Rules) -> Result<Self> {
        let columns = columns_dir(data);

        let store = Store::open(data)?;
        let world = if store.is_empty()? {
            // An empty database with region files beside it means a server
            // upgraded from a build whose map lived in those files. Read them
            // once in full into the database. From then on the database is the
            // map and the files are only what the mod last wrote. Each region
            // keeps its file's date, so zoom levels already built from it are
            // not rebuilt just because it was imported.
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
        let preferences = Arc::new(Preferences::load(Arc::clone(&store)));
        let live = Arc::new(Live::load(Arc::clone(&store), &rules.hidden_groups));
        for (uid, person) in preferences.all() {
            memory.set_shares(&uid, person.share_map_with.iter().copied());
        }

        let state = Self {
            store,
            memory,
            world: RwLock::new(world),
            palette: RwLock::new(palette),
            regions: Mutex::new(regions),
            painted: Mutex::new(files::modified(&crate::render::palette::path_in(data))),
            live,
            sessions,
            pending: Arc::new(Pending::new()),
            plugins: Arc::new(crate::mapdata::plugins::Plugins::default()),
            plugin_shares: Mutex::new(HashMap::new()),
            preferences,
            names: RwLock::new(crate::protocol::watch::block_names(data).unwrap_or_default()),
            named: Mutex::new(files::modified(&crate::protocol::watch::names_path(data))),
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
            sea_level: std::sync::atomic::AtomicI32::new(crate::mapdata::facts::read(data).sea_level),
            data: data.to_path_buf(),
            levels: Levels::new(data),
        };

        state.recall_plugins();
        Ok(state)
    }

    /// Opens every plugin the register lists, using the shape it last declared.
    ///
    /// This runs at startup rather than waiting for the mod. A plugin registers
    /// only when the game server tells it to, which happens after the map is
    /// already serving, so a reader who opened the map first would see nothing
    /// from that plugin until then.
    ///
    /// A plugin that fails to open is logged and skipped. The map serves either
    /// way.
    fn recall_plugins(&self) {
        let declared = match self.store.declared_plugins() {
            Ok(found) => found,
            Err(error) => {
                warn!("could not read what plugins declared: {error}");
                return;
            }
        };

        let root = crate::mapdata::plugins::plugins_dir(&self.data);
        for (id, was, declaration) in declared {
            let Ok(shape) = serde_json::from_str::<crate::mapdata::plugins::Shape>(&declaration) else {
                warn!("plugin {id}: what it declared is not a shape this build reads");
                continue;
            };

            // `was` is the plugin's own fingerprint, so opening it here is
            // never read as a shape that changed.
            if let Err(error) = self.plugins.register(&root, &id, &shape, Some(was.as_str())) {
                warn!("plugin {id}: {error}");
                continue;
            }

            self.recall_plugin_shares(&id);
            say!("plugin {id}: opened from the register");
        }
    }

    /// Returns whose rows of one plugin a reader may see: their own, plus those
    /// of everyone who shared that plugin with a group the reader is in.
    ///
    /// [`Memory::view`] answers the same question for terrain, and the two
    /// answers are kept separate on purpose. Sharing where you explored is not
    /// sharing what you found there, so deriving this from the terrain shares
    /// would let a shared map leak someone's ore.
    ///
    /// Returns empty for a reader with no session, which is what a stranger
    /// sees.
    #[must_use]
    pub fn plugin_sources(&self, plugin: &str, uid: Option<&str>) -> Vec<String> {
        let Some(uid) = uid.filter(|uid| !uid.is_empty()) else {
            return Vec::new();
        };

        // The mod decides group membership and has already filtered out the
        // game's own chat channels. See `PlayerFeed.Joined` in the mod.
        let holds = |group: &i32| self.memory.group_holds(*group, uid);
        match self.plugin_shares.lock() {
            Ok(held) => match held.get(plugin) {
                Some(shares) => crate::mapdata::memory::sources_from(shares, uid, holds),
                None => vec![uid.to_owned()],
            },
            Err(_) => vec![uid.to_owned()],
        }
    }

    /// Stores one person's sharing settings for one plugin.
    pub fn keep_plugin_shares(&self, plugin: &str, uid: &str, groups: &[i32]) -> Result<()> {
        self.store.keep_plugin_shares(plugin, uid, groups)?;
        if let Ok(mut held) = self.plugin_shares.lock() {
            held.entry(plugin.to_owned())
                .or_default()
                .insert(uid.to_owned(), groups.iter().copied().collect());
        }
        Ok(())
    }

    /// Reads one plugin's shares from the database into memory.
    ///
    /// This runs once, when the plugin registers. Every later change goes
    /// through [`State::keep_plugin_shares`], which writes both copies.
    pub fn recall_plugin_shares(&self, plugin: &str) {
        match self.store.plugin_shares(plugin) {
            Ok(shares) => {
                if let Ok(mut held) = self.plugin_shares.lock() {
                    held.insert(plugin.to_owned(), shares);
                }
            }
            Err(error) => warn!("could not read who shares {plugin}: {error}"),
        }
    }

    /// Returns the height of the world's oceans.
    #[must_use]
    pub fn sea_level(&self) -> i32 {
        self.sea_level.load(Ordering::Relaxed)
    }

    /// Re-reads the sea level, for a world that reported it after startup.
    pub fn resettle_sea_level(&self) {
        self.sea_level.store(crate::mapdata::facts::read(&self.data).sea_level, Ordering::Relaxed);
        self.forget_patches();
    }

    /// Drops every cached patch image, because the colours under them changed.
    pub fn forget_patches(&self) {
        if let Ok(mut patches) = self.patches.lock() {
            patches.clear();
        }
    }

    /// Stores chunks that arrived, into the database first and then into the
    /// world, so what is served is never ahead of what is kept. Returns what
    /// each chunk did to the map.
    ///
    /// All terrain enters through this function. A record the mod pushed and a
    /// column the puller fetched both pass through here, which keeps the
    /// database authoritative.
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

    /// Changes a chunk's season, leaving its terrain alone. Returns true when
    /// the season actually changed, which means the tile needs redrawing.
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

    /// Records that the ground in these tiles changed, so browsers are told and
    /// the levels above are rebuilt on their own schedule.
    ///
    /// A region is a level 0 tile. Slope shading reads the column to the west
    /// and north of each pixel, so redrawing a region also changes the western
    /// edge of the tile east of it and the northern edge of the tile below.
    ///
    /// Level 0 is dropped from the cache immediately, so the next request for it
    /// draws the world as it is. It is announced on the next call to
    /// [`announce`](Self::announce), so terrain arriving a few chunks at a time
    /// costs a browser one repaint per beat rather than one per arrival. The
    /// builder announces the levels above once it has built them, so a viewer is
    /// never sent a coarse tile older than the fine one under it.
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

    /// Tells every browser what changed since the last call: which level 0
    /// tiles for everybody, and which regions for each person alone.
    ///
    /// One call announces everything the interval accumulated, under a single
    /// generation. A beat where nothing moved announces nothing, so the
    /// generation counter never advances for nothing. Each advance invalidates
    /// every browser's cached tile addresses, so batching matters.
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

    /// Announces chunks that were just stored. Anyone who was not there keeps
    /// the version they last saw.
    pub fn terrain_changed(&self, stored: &[Stored]) {
        self.tiles_changed(stored.iter().map(|one| store::region_of(one.cx, one.cz)));
        // Record what moved for each person alone, beside what moved for
        // everybody.
        for (uid, regions) in self.memory.changed(stored) {
            self.memory_changed(&uid, regions);
        }
    }

    /// Records that one person's own memory changed in these regions. Their
    /// composed tiles there are dropped now, and they are told on the next call
    /// to [`announce`](Self::announce).
    fn memory_changed(&self, uid: &str, regions: Vec<(i32, i32)>) {
        self.drop_remembered(uid, &regions);
        if let Ok(mut unannounced) = self.unannounced_of.lock() {
            unannounced.entry(uid.to_owned()).or_default().extend(regions);
        }
    }

    /// Stores one person's own settings and applies what follows from them.
    /// Everything that draws a map reads the share list from here, so a change
    /// takes effect at once.
    pub fn keep_person(&self, uid: &str, person: crate::protocol::preferences::Person) -> bool {
        let shares: Vec<i32> = person.share_map_with.clone();
        if !self.preferences.set(uid, person) {
            return false;
        }
        self.memory.set_shares(uid, shares);
        true
    }

    /// Records where somebody is standing. Everything within `radius` chunks
    /// becomes theirs to see, from now on.
    pub fn seen_from(&self, uid: &str, x: i32, z: i32, radius: i32) {
        let edge = self.chunk_edge().max(1) as i32;
        let (cx, cz) = (x.div_euclid(edge), z.div_euclid(edge));
        let sight: Vec<(i32, i32)> = crate::render::columns::disc_of((cx, cz), radius).collect();
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

    /// Returns the tiles a viewer last at generation `since` needs to repaint.
    ///
    /// Returns `None` to mean every tile. That happens when the palette changed,
    /// or when the viewer has fallen further behind than the history reaches and
    /// there is no way to tell which tiles it missed.
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

    /// Returns the number of blocks along a chunk's edge, which the viewer
    /// draws its grid on. Returns zero until something has been exported.
    pub fn chunk_edge(&self) -> usize {
        self.world.read().map_or(0, |world| world.edge)
    }

    /// Returns how many zoom levels this world is wide enough to need.
    pub fn levels(&self) -> u32 {
        let (min_x, min_z, max_x, max_z) = self.bounds();
        let tile = i64::from(TILE);
        let across = (i64::from(max_x) - i64::from(min_x)).div_euclid(tile);
        let down = (i64::from(max_z) - i64::from(min_z)).div_euclid(tile);
        pyramid::levels_for(across, down)
    }

    /// Logs how much of the terrain the currently loaded palette can colour.
    pub fn report_coverage(&self) {
        let (Ok(world), Ok(palette)) = (self.world.read(), self.palette.read()) else {
            return;
        };
        say!("surface {}", Renderer::new(&world, &palette, self.sea_level()).coverage().summary());
    }

    /// Returns one tile as PNG bytes, drawing or reading it as its level
    /// requires.
    pub fn tile(&self, at: At) -> Result<Arc<[u8]>> {
        self.tile_as(at, TileFormat::Png)
    }

    /// Returns one tile as everybody sees it, in the requested encoding.
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
            // The stored levels are already PNG, so serve the bytes as they
            // lie. Any other encoding is derived from them.
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

    /// Returns the cache key for a tile, combining whose it is with how it is
    /// encoded. Two encodings of one picture never answer for each other.
    #[must_use]
    pub fn cache_key(whose: &str, format: TileFormat) -> String {
        match format {
            TileFormat::Png => whose.to_owned(),
            other => format!("{};{whose}", other.name()),
        }
    }

    /// Renders one level 0 tile from the world. Level 0 is never stored.
    ///
    /// A palette with no colours would blank the finest level while the rest of
    /// the pyramid still showed the world, which reads as a map that breaks when
    /// you zoom in. Scaling up the level above instead makes the viewer fall
    /// back to a coarse map rather than an empty one.
    fn finest(&self, tx: i32, tz: i32) -> Result<RgbImage> {
        // Scope the guards so they drop before anything else reads the world.
        // `levels()` also takes a read lock, and taking a second read lock while
        // holding one can deadlock against a writer that arrived in between. On
        // this lock the writer is the watcher, which runs every time the mod
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

    /// Returns a level above zero, which the builder has already drawn.
    ///
    /// These are never built on demand. A coarse tile is four of the level
    /// below, so building one here would build every tile beneath it, which is a
    /// thousand renders for a level five while a request waits. [`Levels`]
    /// decides where the builder's image is held.
    fn stored(&self, at: At) -> Result<Vec<u8>> {
        self.levels.bytes(at)
    }

    /// Returns one level 0 tile as an image, or `None` where the world has no
    /// chunks.
    fn level_zero(&self, tx: i32, tz: i32, mapped: &HashSet<(i32, i32)>) -> Option<RgbImage> {
        if !mapped.contains(&(tx, tz)) {
            return None;
        }
        let (Ok(world), Ok(palette)) = (self.world.read(), self.palette.read()) else {
            return None;
        };
        Some(Renderer::new(&world, &palette, self.sea_level()).render(tx * TILE as i32, tz * TILE as i32, TILE))
    }

    /// Rebuilds every level above zero for whatever changed since the last call.
    ///
    /// Works bottom up, one level at a time. The tiles that changed at a level
    /// decide which tiles change at the level above, and four of the former make
    /// one of the latter. A region changing therefore costs one tile per level,
    /// not one tile per level per region.
    pub fn build_levels(&self) {
        let Some(mut changed) = self.take_stale() else {
            return;
        };

        let levels = self.levels();

        // A world that has grown past a power of two gains a coarsest level
        // that was never built. Walking up from what changed would build exactly
        // one tile there and leave the rest of the level missing. That level is
        // the one a viewer opens on, so the map would read as empty until
        // something else marked every region stale. Measure the whole pyramid
        // against the world whenever it is shorter than the world needs, which
        // happens once per doubling and never in the steady state.
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

        // The level 0 tiles were announced when the ground arrived. Announce
        // only the levels this call built.
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

    /// Logs how many tiles went out since the last call, and how many cost a
    /// render or an encode. Called once a minute, and logs nothing when no tile
    /// was served. The ratio shows whether a server is serving its cache or
    /// redrawing the same tiles.
    pub fn report_serving(&self) {
        let served = self.served.swap(0, Ordering::Relaxed);
        let drawn = self.drawn.swap(0, Ordering::Relaxed);
        if served > 0 {
            say!("{served} tiles served in the last minute, {drawn} of them drawn or encoded");
        }
    }

    /// Writes the level tiles that have waited long enough. [`Levels::flush`]
    /// defines the wait.
    pub fn flush_levels(&self) {
        let written = self.levels.flush(SystemTime::now());
        if written > 0 {
            say!("{written} level tiles written, {} still waiting", self.levels.waiting());
        }
    }

    /// Takes and clears the level 0 tiles waiting to have their levels rebuilt.
    /// Returns `None` when there is nothing to do, which is the common case.
    fn take_stale(&self) -> Option<HashSet<(i32, i32)>> {
        let mut stale = self.stale.lock().ok()?;
        (!stale.is_empty()).then(|| std::mem::take(&mut *stale))
    }

    /// Drops redrawn tiles from the cache.
    pub fn drop_tiles(&self, tiles: &[At]) {
        if let Ok(mut cache) = self.cache.lock() {
            for at in tiles {
                cache.remove(at);
            }
        }
    }

    /// Marks level 0 tiles whose levels above need rebuilding.
    pub fn mark_stale(&self, tiles: impl IntoIterator<Item = (i32, i32)>) {
        if let Ok(mut stale) = self.stale.lock() {
            stale.extend(tiles);
        }
    }
}

/// Groups a world's chunks by the region each sits in.
fn by_region(world: &World) -> HashMap<(i32, i32), Vec<PlacedChunk<'_>>> {
    let mut grouped: HashMap<(i32, i32), Vec<PlacedChunk<'_>>> = HashMap::new();
    for (&at, chunk) in &world.chunks {
        grouped.entry(store::region_of(at.0, at.1)).or_default().push((at, chunk));
    }
    grouped
}

/// Returns when each region file on disk was last written. Read once, on the
/// startup that imports them.
fn region_times(dir: &Path) -> HashMap<(i32, i32), SystemTime> {
    let Ok(paths) = crate::render::columns::region_files(dir) else {
        return HashMap::new();
    };

    paths
        .into_iter()
        .filter_map(|path| Some((crate::render::columns::region_coords(&path)?, files::modified(&path)?)))
        .collect()
}

/// Provides what a test needs to stand a map up: a palette with colours in it
/// and a default set of rules. Shared between the modules that build a `State`,
/// so a rule added here reaches all of them.
#[cfg(test)]
pub mod testing {
    use super::*;

    /// Writes a palette naming block 11 grey and block 22 red, and loads it.
    pub fn palette_in(at: &Path) -> Palette {
        std::fs::write(
            crate::render::palette::path_in(at),
            r##"{"Version":1,"GameVersion":"1.22.7","Source":"client","Fingerprint":"abc",
                "Blocks":{"game:air":{"Id":0,"Rgb":null,"Invisible":true},
                          "game:rock":{"Id":11,"Rgb":"#646464"},
                          "game:brick":{"Id":22,"Rgb":"#c02020"}}}"##,
        )
        .expect("a palette");
        Palette::load(at).expect("it parses")
    }

    pub fn rules(personal_maps: bool) -> Rules {
        Rules {
            allow_public_markers: false,
            allow_editing_public_markers: false,
            show_players_to_everyone: true,
            live_refresh_ms: 2000,
            personal_maps,
            show_spawn_to_guests: false,
            spawn_radius_chunks: 8,
            sight_radius_chunks: 0,
            session_hours: 0,
            invalidate_sessions_on_restart: false,
            hidden_groups: vec!["xlib".to_owned()],
        }
    }

    /// Builds a map with this build's chunk edge, from a fresh scratch
    /// directory.
    pub fn state_in(at: &Path, personal_maps: bool) -> State {
        State::load(at, palette_in(at), 1 << 20, rules(personal_maps)).expect("a start")
    }
}

#[cfg(test)]
mod tests {
    use super::testing::{palette_in, rules};
    use super::*;
    use crate::util::files::testing::Scratch;

    /// Checks the upgrade path. A server whose map lived in region files starts
    /// this build, and the files become the database. The next start reads the
    /// database alone, so the files can be deleted.
    #[test]
    fn region_files_are_imported_once_and_the_database_is_the_map_after() {
        let held = Scratch::new("state-import");
        let at = held.at();
        let columns = columns_dir(at);
        std::fs::create_dir_all(&columns).unwrap();
        std::fs::write(
            columns.join("r.2.-3.msqr"),
            crate::render::columns::testing::filed((2, -3), 4, &[(0, 7, 11), (17, 9, 11)], None),
        )
        .unwrap();

        let first = State::load(at, palette_in(at), 1 << 20, rules(false)).expect("a first start");
        assert_eq!(first.chunks(), 2, "both chunks came in off the file");
        assert_eq!(first.store.counts().unwrap().chunks, 2, "and into the database");
        drop(first);

        // Delete the files. The map must still be there.
        std::fs::remove_dir_all(&columns).unwrap();
        let second = State::load(at, palette_in(at), 1 << 20, rules(false)).expect("a second start");
        assert_eq!(second.chunks(), 2);
        assert_eq!(second.chunk_edge(), 4);
        let world = second.world.read().unwrap();
        assert_eq!(world.column_at(32 * 4, -48 * 4).map(|c| (c.block, c.season)), Some((11, 7)));
        assert_eq!(world.column_at(33 * 4, -47 * 4).map(|c| c.season), Some(9), "the season came back too");
    }

    /// Checks that what is stored is what is served, and that a chunk arriving
    /// again unchanged is reported as unchanged.
    #[test]
    fn a_chunk_taken_is_in_the_database_and_the_world_alike() {
        let held = Scratch::new("state-take");
        let at = held.at();
        let state = State::load(at, palette_in(at), 1 << 20, rules(false)).expect("an empty start");

        let record = Chunk::filled_with(crate::render::columns::Column { block: 11, height: 3, temperature: 1, rainfall: 2, season: 5 }, 4).record();
        let stored = state.take_chunks(2, &[Arrived { cx: 1, cz: 1, season: 5, record: record.clone() }], SystemTime::now());
        assert_eq!(stored.len(), 1);
        assert!(stored[0].surface_moved());
        assert_eq!(state.chunks(), 1);
        assert_eq!(state.store.counts().unwrap().chunks, 1);

        let again = state.take_chunks(2, &[Arrived { cx: 1, cz: 1, season: 5, record }], SystemTime::now());
        assert!(!again[0].surface_moved(), "the same bytes are not a change");
    }

    /// Checks that ground arriving in several pieces produces one announcement.
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
