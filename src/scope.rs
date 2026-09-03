//! What one reader is shown of the map.
//!
//! With `private_map` off, everybody is shown the whole map, and this is a
//! pass-through to the tiles the service already draws. With it on, a reader is
//! shown the map as their memory has it — see [`crate::memory`] — plus the
//! ground around spawn as it is, which the operator may open to anybody: a
//! browser with no session is shown that disc and nothing else.
//!
//! A tile for a reader is the tile everybody gets, with two things done to it:
//! ground the reader has never been near is painted as unexplored, and ground
//! they remember differently is drawn again from what they remember. Neither
//! is written to disk. The global tiles are the only pictures kept; a reader's
//! are composed when asked for and held in the same memory cache as everything
//! else, keyed by whose view they are.
//!
//! What a chunk is to a reader is one question with three answers —
//! [`Shown`] — and every route that draws or names ground asks it here rather
//! than reading the memory itself. A tile, the block inspector and the map's
//! own bounds must agree about what a reader may see, and three readings of the
//! same tables would be three chances to disagree.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use image::RgbImage;

use crate::cache::At;
use crate::columns::{Chunk, REGION_CHUNKS, World, chunks_of};
use crate::error::{Error, Result};
use crate::memory::View;
use crate::pyramid::{self, TILE};
use crate::render::{Renderer, UNMAPPED};
use crate::state::{Changed, State};
use crate::store::{self, Version};

/// The ground around spawn, in chunks each way, that is shown as it is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Disc {
    pub cx: i32,
    pub cz: i32,
    pub radius: i32,
}

impl Disc {
    fn holds(&self, (cx, cz): (i32, i32)) -> bool {
        (cx - self.cx).abs() <= self.radius && (cz - self.cz).abs() <= self.radius
    }

    /// Every region the disc reaches into.
    fn regions(&self) -> impl Iterator<Item = (i32, i32)> + '_ {
        let (min, max) = ((self.cx - self.radius, self.cz - self.radius), (self.cx + self.radius, self.cz + self.radius));
        let (rx0, rz0) = store::region_of(min.0, min.1);
        let (rx1, rz1) = store::region_of(max.0, max.1);
        (rz0..=rz1).flat_map(move |rz| (rx0..=rx1).map(move |rx| (rx, rz)))
    }
}

/// Whose map this is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Scope {
    /// The whole map, as everybody sees it when the map is not private.
    Whole,
    /// The map as one reader's memory has it, plus the spawn disc where the
    /// operator has opened one.
    Remembered { view: View, spawn: Option<Disc> },
}

impl Scope {
    /// What this scope's tiles are cached under. Empty for the whole map, so
    /// the tiles everybody shares are keyed as they always were.
    #[must_use]
    pub fn key(&self) -> String {
        match self {
            Self::Whole => String::new(),
            Self::Remembered { view, spawn } => {
                let mut key = String::from("m:");
                key.push_str(&view.sources.join(","));
                if let Some(disc) = spawn {
                    key.push_str(&format!(";s:{},{},{}", disc.cx, disc.cz, disc.radius));
                }
                key
            }
        }
    }

    #[must_use]
    pub fn is_whole(&self) -> bool {
        matches!(self, Self::Whole)
    }
}

/// What one chunk is to one reader.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shown {
    /// Never been near it: drawn as unexplored, named as nothing.
    Hidden,
    /// Seen as it is now.
    Current,
    /// Seen, and changed since: drawn from the version they last saw.
    Remembered(Version),
}

impl State {
    /// What a reader with this session is shown.
    #[must_use]
    pub fn scope_for(&self, uid: Option<&str>) -> Scope {
        if !self.rules.private_map {
            return Scope::Whole;
        }

        let spawn = self.rules.anonymous_spawn.then(|| {
            let facts = crate::facts::read(&self.data);
            let edge = self.chunk_edge().max(1) as i32;
            Disc {
                cx: facts.spawn_x.div_euclid(edge),
                cz: facts.spawn_z.div_euclid(edge),
                radius: self.rules.anonymous_spawn_radius_chunks.max(0),
            }
        });

        Scope::Remembered { view: self.memory.view(uid), spawn }
    }

    /// What one chunk is to a scope.
    fn shown(&self, scope: &Scope, chunk: (i32, i32)) -> Shown {
        match scope {
            Scope::Whole => Shown::Current,
            Scope::Remembered { view, spawn } => {
                if spawn.is_some_and(|disc| disc.holds(chunk)) {
                    return Shown::Current;
                }
                let region = store::region_of(chunk.0, chunk.1);
                let discovered = self
                    .memory
                    .discovered_in(view, region)
                    .is_some_and(|bits| store::bit(&bits, store::slot_of(chunk.0, chunk.1)));
                if !discovered {
                    return Shown::Hidden;
                }
                match self.memory.remembered(view, chunk) {
                    Some(version) => Shown::Remembered(version),
                    None => Shown::Current,
                }
            }
        }
    }

    /// Every region a scope has anything shown in.
    fn regions_shown(&self, scope: &Scope) -> HashSet<(i32, i32)> {
        match scope {
            Scope::Whole => self.world.read().map(|world| world.regions().collect()).unwrap_or_default(),
            Scope::Remembered { view, spawn } => {
                let mut regions = self.memory.regions_of(view);
                if let Some(disc) = spawn {
                    regions.extend(disc.regions());
                }
                regions
            }
        }
    }

    /// The map's bounds as a scope sees them, in blocks: the whole map's, or
    /// the reach of what this reader has anything shown in. A reader shown
    /// nothing has degenerate bounds, which is the page's cue to draw no grid.
    #[must_use]
    pub fn bounds_for(&self, scope: &Scope) -> (i32, i32, i32, i32) {
        if scope.is_whole() {
            return self.bounds();
        }
        let edge = self.chunk_edge() as i32;
        let regions = self.regions_shown(scope);
        let Some(first) = regions.iter().next().copied() else {
            return (0, 0, 0, 0);
        };
        let (mut min, mut max) = (first, first);
        for &(rx, rz) in &regions {
            min = (min.0.min(rx), min.1.min(rz));
            max = (max.0.max(rx), max.1.max(rz));
        }
        let span = REGION_CHUNKS * edge;
        (min.0 * span, min.1 * span, (max.0 + 1) * span, (max.1 + 1) * span)
    }

    /// How many chunks a scope is shown. A number about the reader's map, not
    /// about the server's.
    #[must_use]
    pub fn chunks_for(&self, scope: &Scope) -> usize {
        match scope {
            Scope::Whole => self.chunks(),
            Scope::Remembered { view, spawn } => {
                let mut shown: usize = self
                    .regions_shown(scope)
                    .iter()
                    .filter_map(|&region| self.memory.discovered_in(view, region))
                    .map(|bits| bits.iter().map(|byte| byte.count_ones() as usize).sum::<usize>())
                    .sum();
                if let Some(disc) = spawn {
                    let across = usize::try_from(disc.radius * 2 + 1).unwrap_or(0);
                    shown += across * across;
                }
                shown
            }
        }
    }

    /// Which tiles a scope needs to draw again since `since`: what changed for
    /// everybody, and what changed in this reader's own memory. `None` means
    /// everything, for the reasons [`State::changes_since`] gives.
    #[must_use]
    pub fn changes_for(&self, scope: &Scope, since: u64) -> Changed {
        let mut tiles = self.changes_since(since)?;
        if let Scope::Remembered { view, .. } = scope {
            let regions = self.memory.changes_since(view, since)?;
            let levels = self.levels();
            for (rx, rz) in regions {
                for level in 0..=levels {
                    let (ax, az) = pyramid::ancestor(level, rx, rz);
                    tiles.push((level, ax, az));
                }
            }
        }
        tiles.sort_unstable();
        tiles.dedup();
        Some(tiles)
    }

    /// One tile as a scope sees it, as PNG bytes.
    pub fn tile_for(&self, scope: &Scope, at: At) -> Result<Arc<[u8]>> {
        if scope.is_whole() {
            return self.tile(at);
        }

        let key = scope.key();
        if let Ok(mut cache) = self.cache.lock()
            && let Some(bytes) = cache.get(&key, &at)
        {
            return Ok(bytes);
        }

        let image = self.compose(scope, at)?;
        let bytes: Arc<[u8]> = pyramid::encode(&image)?.into();

        if let Ok(mut cache) = self.cache.lock() {
            cache.insert(key, at, Arc::clone(&bytes));
        }
        Ok(bytes)
    }

    /// The tile everybody gets, with what this reader must not see painted out
    /// and what they remember differently drawn in.
    fn compose(&self, scope: &Scope, at: At) -> Result<RgbImage> {
        let (level, tx, tz) = at;
        let edge = self.chunk_edge().max(1) as i32;
        let span = 1i32 << level;
        let regions: Vec<(i32, i32)> = (0..span)
            .flat_map(|dz| (0..span).map(move |dx| (tx * span + dx, tz * span + dz)))
            .collect();

        // Everything this reader remembers differently inside this tile, and
        // whether anything is shown here at all. Decided before the global tile
        // is fetched, because a tile nobody may see anything of is not worth
        // drawing — it is unexplored, whole.
        let mut remembered: HashMap<(i32, i32), Version> = HashMap::new();
        let mut shown_any = false;
        let mut hidden_any = false;
        let held: HashSet<(i32, i32)> = self.regions_shown(scope).into_iter().filter(|region| regions.contains(region)).collect();
        for &region in &held {
            for chunk in chunks_of(region) {
                match self.shown(scope, chunk) {
                    Shown::Hidden => hidden_any = true,
                    Shown::Current => shown_any = true,
                    Shown::Remembered(version) => {
                        shown_any = true;
                        remembered.insert(chunk, version);
                    }
                }
            }
        }
        if held.len() < regions.len() {
            hidden_any = true;
        }

        let blank = || RgbImage::from_pixel(TILE, TILE, image::Rgb([UNMAPPED.r, UNMAPPED.g, UNMAPPED.b]));
        if !shown_any {
            return Ok(blank());
        }

        // The picture everybody gets. At the finest level it is drawn again
        // where memory differs, from a world with the remembered chunks in it,
        // so that slope shading reads across the seams as it does everywhere.
        let mut image = if level == 0 && !remembered.is_empty() {
            let overlay = self.overlay((tx, tz), &remembered, edge as usize)?;
            let Ok(palette) = self.palette.read() else {
                return Err(Error::Empty("the map is being reloaded".to_owned()));
            };
            Renderer::new(&overlay, &palette, self.sea_level()).render(tx * TILE as i32, tz * TILE as i32, TILE)
        } else {
            let bytes = self.tile(at)?;
            image::load_from_memory(&bytes).map_err(|error| Error::Empty(format!("a stored tile would not decode: {error}")))?.to_rgb8()
        };

        if !hidden_any && (level == 0 || remembered.is_empty()) {
            return Ok(image);
        }

        // Paint out what is hidden. A chunk is `edge >> level` pixels across,
        // and past level five less than one: a pixel there is shown if any
        // chunk in it is, so the mask is built from what is shown rather than
        // from what is not.
        let px_per_chunk = (edge >> level).max(1) as u32;
        let chunks_per_px = (1i32 << level) / edge.max(1);
        let mut shown = vec![false; (TILE * TILE) as usize];
        let origin = (tx * span * REGION_CHUNKS, tz * span * REGION_CHUNKS);
        let to_px = |c: i32, o: i32| -> u32 {
            if chunks_per_px > 1 { ((c - o) / chunks_per_px) as u32 } else { ((c - o) as u32) * px_per_chunk }
        };
        for &region in &held {
            for chunk in chunks_of(region) {
                if self.shown(scope, chunk) == Shown::Hidden {
                    continue;
                }
                let (px, pz) = (to_px(chunk.0, origin.0), to_px(chunk.1, origin.1));
                for dz in 0..px_per_chunk {
                    for dx in 0..px_per_chunk {
                        if let Some(cell) = shown.get_mut(((pz + dz) * TILE + px + dx) as usize) {
                            *cell = true;
                        }
                    }
                }
            }
        }
        for (index, pixel) in image.pixels_mut().enumerate() {
            if !shown[index] {
                *pixel = image::Rgb([UNMAPPED.r, UNMAPPED.g, UNMAPPED.b]);
            }
        }

        // Above the finest level a remembered chunk is drawn on its own and
        // shrunk into place: at these sizes the seam a lone chunk's shading
        // leaves is under a pixel.
        if level > 0 {
            for (&chunk, &version) in &remembered {
                let Some(patch) = self.patch(chunk, version, edge as usize) else { continue };
                let (px, pz) = (to_px(chunk.0, origin.0), to_px(chunk.1, origin.1));
                blit_shrunk(&mut image, &patch, px, pz, px_per_chunk);
            }
        }

        Ok(image)
    }

    /// A world of one region and its border, with the remembered chunks in
    /// place of the current ones.
    fn overlay(&self, region: (i32, i32), remembered: &HashMap<(i32, i32), Version>, edge: usize) -> Result<World> {
        let Ok(world) = self.world.read() else {
            return Err(Error::Empty("the map is being reloaded".to_owned()));
        };
        let mut chunks: HashMap<(i32, i32), Chunk> = HashMap::new();
        let (x0, z0) = (region.0 * REGION_CHUNKS - 1, region.1 * REGION_CHUNKS - 1);
        for cz in z0..=z0 + REGION_CHUNKS + 1 {
            for cx in x0..=x0 + REGION_CHUNKS + 1 {
                if let Some(chunk) = world.chunks.get(&(cx, cz)) {
                    chunks.insert((cx, cz), chunk.clone());
                }
            }
        }
        for (&at, &version) in remembered {
            let season = world.chunks.get(&at).map_or(0, Chunk::season);
            if let Some(record) = self.store.version(version)?
                && let Some(chunk) = Chunk::from_record(&record, edge, season)
            {
                chunks.insert(at, chunk);
            }
        }
        drop(world);
        Ok(World::from_chunks(edge, chunks))
    }

    /// One remembered chunk drawn alone, at one pixel per block.
    fn patch(&self, chunk: (i32, i32), version: Version, edge: usize) -> Option<RgbImage> {
        let record = self.store.version(version).ok()??;
        let season = self.world.read().ok().and_then(|world| world.chunks.get(&chunk).map(Chunk::season)).unwrap_or(0);
        let one = Chunk::from_record(&record, edge, season)?;
        let world = World::from_chunks(edge, HashMap::from([(chunk, one)]));
        let palette = self.palette.read().ok()?;
        let e = edge as i32;
        Some(Renderer::new(&world, &palette, self.sea_level()).render(chunk.0 * e, chunk.1 * e, edge as u32))
    }

    /// What is at one block, as a scope sees it: nothing where it is hidden,
    /// the remembered column where it is remembered, and the map's own reading
    /// otherwise.
    #[must_use]
    pub fn surface_for(&self, scope: &Scope, x: i32, z: i32) -> Option<crate::render::Surface> {
        let edge = self.chunk_edge().max(1) as i32;
        let chunk = (x.div_euclid(edge), z.div_euclid(edge));
        let version = match self.shown(scope, chunk) {
            Shown::Hidden => return Some(crate::render::Surface::Unmapped),
            Shown::Current => None,
            Shown::Remembered(version) => Some(version),
        };

        let palette = self.palette.read().ok()?;
        match version {
            None => {
                let world = self.world.read().ok()?;
                Some(Renderer::new(&world, &palette, self.sea_level()).surface_at(x, z))
            }
            Some(version) => {
                let record = self.store.version(version).ok()??;
                let season = self.world.read().ok().and_then(|world| world.chunks.get(&chunk).map(Chunk::season)).unwrap_or(0);
                let one = Chunk::from_record(&record, edge as usize, season)?;
                let world = World::from_chunks(edge as usize, HashMap::from([(chunk, one)]));
                Some(Renderer::new(&world, &palette, self.sea_level()).surface_at(x, z))
            }
        }
    }

    /// Forgets every composed tile that read one person's memory in any of
    /// these regions. The tiles everybody shares are untouched: their ground
    /// did not move, only what this person remembers of it.
    pub fn drop_remembered(&self, uid: &str, regions: &[(i32, i32)]) {
        let Ok(mut cache) = self.cache.lock() else { return };
        cache.remove_where(|key, (level, x, z)| {
            if key.is_empty() || !key.split(|c| c == ':' || c == ',' || c == ';').any(|part| part == uid) {
                return false;
            }
            let span = 1i32 << level;
            regions.iter().any(|&(rx, rz)| (rx.div_euclid(span), rz.div_euclid(span)) == (*x, *z))
        });
    }
}

/// Draws `patch` into `into` at `(px, pz)`, shrunk to `size` pixels a side by
/// averaging. `size` divides the patch evenly at every level this is used at:
/// a chunk is a power of two blocks across, and so is a level's scale.
fn blit_shrunk(into: &mut RgbImage, patch: &RgbImage, px: u32, pz: u32, size: u32) {
    let step = (patch.width() / size).max(1);
    let count = step * step;
    for dz in 0..size {
        for dx in 0..size {
            let mut total = [0u32; 3];
            for sz in 0..step {
                for sx in 0..step {
                    let pixel = patch.get_pixel(dx * step + sx, dz * step + sz).0;
                    for channel in 0..3 {
                        total[channel] += u32::from(pixel[channel]);
                    }
                }
            }
            if px + dx < into.width() && pz + dz < into.height() {
                into.put_pixel(
                    px + dx,
                    pz + dz,
                    image::Rgb([(total[0] / count) as u8, (total[1] / count) as u8, (total[2] / count) as u8]),
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::files::testing::Scratch;
    use crate::state::testing::state_in;
    use crate::store::Arrived;
    use std::time::SystemTime;

    const EDGE: usize = 32;

    fn record(block: u16) -> Vec<u8> {
        let mut record = Vec::with_capacity(EDGE * EDGE * 6);
        for _ in 0..EDGE * EDGE {
            record.extend_from_slice(&block.to_le_bytes());
            record.extend_from_slice(&100i16.to_le_bytes());
            record.push(128);
            record.push(128);
        }
        record
    }

    fn ground(state: &State, chunk: (i32, i32), block: u16) {
        let stored = state.take_chunks(
            EDGE,
            &[Arrived { cx: chunk.0, cz: chunk.1, season: 0, record: record(block) }],
            SystemTime::now(),
        );
        state.terrain_changed(&stored);
    }

    fn pixel(png: &[u8], x: u32, z: u32) -> [u8; 3] {
        image::load_from_memory(png).expect("a png").to_rgb8().get_pixel(x, z).0
    }

    const DARK: [u8; 3] = [UNMAPPED.r, UNMAPPED.g, UNMAPPED.b];

    /// Two chunks of rock. Ada has been near one of them; she is shown that one
    /// and nothing else, and the whole map is shown to a scope of the whole.
    #[test]
    fn a_reader_is_shown_what_they_have_been_near_and_nothing_else() {
        let held = Scratch::new("scope-hidden");
        let state = state_in(held.at(), true);
        ground(&state, (0, 0), 11);
        ground(&state, (1, 0), 11);
        state.seen_from("ada", 0, 0, 0);

        let ada = state.scope_for(Some("ada"));
        let tile = state.tile_for(&ada, (0, 0, 0)).expect("a tile");
        assert_ne!(pixel(&tile, 5, 5), DARK, "chunk (0, 0) is hers to see");
        assert_eq!(pixel(&tile, 40, 5), DARK, "chunk (1, 0) is not");
        assert_eq!(pixel(&tile, 300, 300), DARK, "and nor is ground nobody exported");

        let whole = state.tile_for(&Scope::Whole, (0, 0, 0)).expect("a tile");
        assert_ne!(pixel(&whole, 40, 5), DARK, "everybody's map has the second chunk");

        let stranger = state.scope_for(None);
        let nothing = state.tile_for(&stranger, (0, 0, 0)).expect("a tile");
        assert_eq!(pixel(&nothing, 5, 5), DARK, "a stranger is shown nothing without a spawn disc");
        assert_eq!(state.bounds_for(&stranger), (0, 0, 0, 0));
        assert_eq!(state.bounds_for(&ada), (0, 0, 512, 512));
        assert_eq!(state.chunks_for(&ada), 1);
    }

    /// Ada saw rock, went away, and the rock became brick. Her tile still shows
    /// rock; the whole map shows brick; and when she returns, so does hers.
    #[test]
    fn a_reader_is_shown_what_they_remember_until_they_return() {
        let held = Scratch::new("scope-remembered");
        let state = state_in(held.at(), true);
        ground(&state, (0, 0), 11);
        state.seen_from("ada", 0, 0, 0);
        let rock = pixel(&state.tile_for(&Scope::Whole, (0, 0, 0)).unwrap(), 5, 5);

        state.seen_from("ada", 320, 320, 0);
        ground(&state, (0, 0), 22);
        let brick = pixel(&state.tile_for(&Scope::Whole, (0, 0, 0)).unwrap(), 5, 5);
        assert_ne!(rock, brick);

        let ada = state.scope_for(Some("ada"));
        assert_eq!(pixel(&state.tile_for(&ada, (0, 0, 0)).unwrap(), 5, 5), rock, "Ada remembers rock");

        // The inspector agrees with the picture.
        let named = state.block(&ada, 5, 5).expect("a block");
        assert!(named.contains("\"block\":11"), "{named}");
        assert!(state.block(&ada, 40, 5).expect("a block").contains("unmapped"));

        state.seen_from("ada", 0, 0, 0);
        assert_eq!(pixel(&state.tile_for(&ada, (0, 0, 0)).unwrap(), 5, 5), brick, "and sees brick once back");
    }

    /// The same at a coarser level: the stored tile is masked at chunk
    /// resolution and a remembered chunk is drawn shrunk into its place.
    #[test]
    fn a_coarser_tile_is_masked_and_patched_at_chunk_resolution() {
        let held = Scratch::new("scope-coarse");
        let state = state_in(held.at(), true);
        ground(&state, (0, 0), 11);
        ground(&state, (1, 0), 11);
        // Far enough away that the world is three tiles across and earns a
        // level above the finest.
        ground(&state, (40, 40), 11);
        state.seen_from("ada", 0, 0, 0);
        state.build_levels();
        assert!(state.levels() >= 1);

        let ada = state.scope_for(Some("ada"));
        let coarse = state.tile_for(&ada, (1, 0, 0)).expect("a level 1 tile");
        assert_ne!(pixel(&coarse, 3, 3), DARK, "chunk (0, 0) is 16 pixels wide here");
        assert_eq!(pixel(&coarse, 20, 3), DARK, "chunk (1, 0) is painted out");

        // Away, and the ground changes: the patch drawn in is the remembered one.
        state.seen_from("ada", 900, 900, 0);
        ground(&state, (0, 0), 22);
        state.build_levels();
        let whole = pixel(&state.tile_for(&Scope::Whole, (1, 0, 0)).unwrap(), 3, 3);
        let remembered = pixel(&state.tile_for(&ada, (1, 0, 0)).unwrap(), 3, 3);
        assert_ne!(whole, remembered, "the whole map shows brick where Ada is shown rock");
    }

    /// The spawn disc is shown as it is to everybody, a stranger included.
    #[test]
    fn the_spawn_disc_is_everybodys() {
        let held = Scratch::new("scope-spawn");
        let mut state = state_in(held.at(), true);
        state.rules.anonymous_spawn = true;
        state.rules.anonymous_spawn_radius_chunks = 1;
        ground(&state, (0, 0), 11);
        ground(&state, (3, 0), 11);

        let stranger = state.scope_for(None);
        let tile = state.tile_for(&stranger, (0, 0, 0)).expect("a tile");
        assert_ne!(pixel(&tile, 5, 5), DARK, "spawn is at the origin without a world.json");
        assert_eq!(pixel(&tile, 100, 5), DARK, "chunk (3, 0) is outside the disc");
        assert!(state.chunks_for(&stranger) >= 9);
    }

    #[test]
    fn a_disc_reaches_the_regions_around_spawn() {
        // Spawn at chunk (512, 512) is the corner of region (32, 32); a radius
        // of two reaches one region back on each axis.
        let disc = Disc { cx: 512, cz: 512, radius: 2 };
        let regions: HashSet<(i32, i32)> = disc.regions().collect();
        assert_eq!(regions, HashSet::from([(31, 31), (32, 31), (31, 32), (32, 32)]));
        assert!(disc.holds((510, 514)));
        assert!(!disc.holds((509, 512)));
    }

    #[test]
    fn a_scopes_key_names_whose_view_it_is() {
        assert_eq!(Scope::Whole.key(), "");
        let mine = Scope::Remembered {
            view: View { sources: vec!["ada".into(), "bob".into()] },
            spawn: Some(Disc { cx: 1, cz: 2, radius: 3 }),
        };
        assert_eq!(mine.key(), "m:ada,bob;s:1,2,3");
        assert_eq!(Scope::Remembered { view: View::default(), spawn: None }.key(), "m:");
    }

    #[test]
    fn a_patch_shrinks_by_averaging() {
        let mut into = RgbImage::from_pixel(4, 4, image::Rgb([0, 0, 0]));
        let mut patch = RgbImage::from_pixel(4, 4, image::Rgb([100, 0, 0]));
        patch.put_pixel(0, 0, image::Rgb([0, 0, 0]));
        blit_shrunk(&mut into, &patch, 2, 2, 2);
        assert_eq!(into.get_pixel(2, 2).0, [75, 0, 0], "three red and one black");
        assert_eq!(into.get_pixel(3, 3).0, [100, 0, 0]);
        assert_eq!(into.get_pixel(0, 0).0, [0, 0, 0], "outside the patch is untouched");
    }
}
