//! The map as it currently stands.
//!
//! One value the request threads share, holding what has been loaded off disk
//! and what has been drawn from it. Everything that changes it is behind a lock
//! of its own, because the parts move on different clocks: the world when the
//! mod exports, the palette when an admin joins, the tiles whenever either does.
//!
//! What it holds is here. Noticing that disk has moved is in [`crate::watch`],
//! and what the page is told is in [`crate::feeds`].

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::SystemTime;

use image::RgbImage;

use crate::auth::Sessions;
use crate::cache::{At, Cache};
use crate::columns::{World, columns_dir};
use crate::config::Rules;
use crate::error::{Error, Result};
use crate::files;
use crate::live::Live;
use crate::palette::Palette;
use crate::pending::Pending;
use crate::preferences::Preferences;
use crate::pyramid::{self, TILE};
use crate::render::{Renderer, UNMAPPED};
use crate::log::{say, warn};

/// Which tiles one generation changed, as level and coordinates. `None` means
/// every tile: a new palette recolours the lot, and so does a gap in the history.
pub type Changed = Option<Vec<At>>;

/// How many generations of tile changes to remember. A viewer polls every few
/// seconds and the mod exports every thirty, so this is minutes of slack; past it
/// a viewer is told to repaint everything rather than lied to.
const HISTORY: usize = 128;

pub struct State {
    pub data: PathBuf,
    pub columns: PathBuf,
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
    /// The regions directory's own timestamp, which is the cheap gate. The mod
    /// writes a region beside itself and renames it into place — it must, or a
    /// reader would see half a file — and both of those touch the directory.
    pub seen: Mutex<Option<SystemTime>>,
    /// When each region was last written. The mod only writes a region that
    /// changed, so a timestamp is the whole signal; there is nothing to hash.
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
    history: Mutex<VecDeque<(u64, Changed)>>,
    /// Level 0 tiles whose levels above are out of date. Drained by the builder,
    /// so many changes in one window cost one rebuild rather than many.
    pub stale: Mutex<HashSet<(i32, i32)>>,
    pub cache: Mutex<Cache>,
}

impl State {
    /// Loads what is on disk, without drawing any of it.
    pub fn load(data: &Path, palette: Palette, cache_bytes: usize, rules: Rules) -> Result<Self> {
        let columns = columns_dir(data);
        Ok(Self {
            world: RwLock::new(World::load(data)?),
            palette: RwLock::new(palette),
            seen: Mutex::new(files::modified(&columns)),
            regions: Mutex::new(region_times(&columns)),
            painted: Mutex::new(files::modified(&crate::palette::path_in(data))),
            live: Arc::new(Live::load(data)),
            sessions: Arc::new(Sessions::new()),
            pending: Arc::new(Pending::new()),
            preferences: Arc::new(Preferences::load(data)),
            names: RwLock::new(crate::watch::block_names(data).unwrap_or_default()),
            named: Mutex::new(files::modified(&crate::watch::names_path(data))),
            rules,
            generation: AtomicU64::new(1),
            history: Mutex::new(VecDeque::new()),
            stale: Mutex::new(HashSet::new()),
            cache: Mutex::new(Cache::new(cache_bytes)),
            sea_level: std::sync::atomic::AtomicI32::new(crate::facts::read(data).sea_level),
            data: data.to_path_buf(),
            columns,
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
    }

    pub fn generation(&self) -> u64 {
        self.generation.load(Ordering::Relaxed)
    }

    /// Records a generation and what it changed, then returns the new number.
    pub fn bump(&self, tiles: Changed) -> u64 {
        let generation = self.generation.fetch_add(1, Ordering::Relaxed) + 1;
        if let Ok(mut history) = self.history.lock() {
            history.push_back((generation, tiles));
            while history.len() > HISTORY {
                history.pop_front();
            }
        }
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

        // A gap between what the viewer saw and what is still remembered.
        match history.front() {
            Some((oldest, _)) if *oldest <= since + 1 => {}
            _ => return None,
        }

        let mut tiles = Vec::new();
        for (_, changed) in history.iter().filter(|(at, _)| *at > since) {
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
    pub fn tile(&self, at: At) -> Result<Vec<u8>> {
        if let Ok(mut cache) = self.cache.lock()
            && let Some(bytes) = cache.get(&at)
        {
            return Ok(bytes);
        }

        let bytes = if at.0 == 0 { self.finest(at.1, at.2)? } else { self.stored(at)? };

        if let Ok(mut cache) = self.cache.lock() {
            cache.insert(at, bytes.clone());
        }
        Ok(bytes)
    }

    /// Level 0, which is drawn from the world rather than stored.
    ///
    /// A palette with no colours in it would blank the finest level while the
    /// rest of the pyramid went on showing the world, which reads as a map that
    /// breaks when you zoom in — it was taken for that three times. Growing the
    /// level above instead is what makes the viewer fall back to a coarse map
    /// rather than an empty one.
    fn finest(&self, tx: i32, tz: i32) -> Result<Vec<u8>> {
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
                let image =
                    Renderer::new(&world, &palette, self.sea_level()).render(tx * TILE as i32, tz * TILE as i32, TILE);
                return pyramid::encode(&image);
            }
        }

        let grown = pyramid::from_above(&self.data, 0, tx, tz, TILE, self.levels()).ok_or_else(
            || Error::Empty("the palette has no colours and no level above has this ground".to_owned()),
        )?;
        pyramid::encode(&grown)
    }

    /// A level above zero, which the builder has already drawn.
    ///
    /// Never made on demand: a coarse tile is four of the level below, so making
    /// one here would make every tile beneath it — a thousand renders for a level
    /// five, while somebody waits.
    fn stored(&self, (level, tx, tz): At) -> Result<Vec<u8>> {
        let image = pyramid::read(&self.data, level, tx, tz).ok_or_else(|| {
            Error::Empty(format!("level {level} tile ({tx}, {tz}) is not built yet"))
        })?;
        pyramid::encode(&image)
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
        if pyramid::levels_built(&self.data) < levels
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

        let mut repainted: Vec<At> = changed.iter().map(|&(x, z)| (0, x, z)).collect();

        for level in 1..=levels {
            let parents: HashSet<(i32, i32)> =
                changed.iter().map(|&(x, z)| pyramid::ancestor(1, x, z)).collect();

            for &(px, pz) in &parents {
                let below = pyramid::children(px, pz).map(|(cx, cz)| {
                    if level == 1 {
                        self.level_zero(cx, cz, &mapped)
                    } else {
                        pyramid::read(&self.data, level - 1, cx, cz)
                    }
                });

                if below.iter().all(Option::is_none) {
                    continue;
                }

                let parent = pyramid::downsample(&below, TILE, UNMAPPED);
                if let Err(error) = pyramid::write(&self.data, level, px, pz, &parent) {
                    warn!("{error}");
                }
                repainted.push((level, px, pz));
            }

            changed = parents;
        }

        self.drop_tiles(&repainted);

        if let Ok(palette) = self.palette.read() {
            pyramid::record_palette(&self.data, &palette.fingerprint);
        }

        // One announcement for the whole export: the level 0 tiles the watcher
        // reloaded are in this list too, so a viewer fetches each changed tile
        // once rather than once per level of the pyramid that touched it.
        let generation = self.bump(Some(repainted.clone()));
        say!("{} tiles rebuilt across {levels} levels (generation {generation})", repainted.len());
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

/// When each region on disk was last written.
pub fn region_times(dir: &Path) -> HashMap<(i32, i32), SystemTime> {
    let Ok(paths) = crate::columns::region_files(dir) else {
        return HashMap::new();
    };

    paths
        .into_iter()
        .filter_map(|path| Some((crate::columns::region_coords(&path)?, files::modified(&path)?)))
        .collect()
}
