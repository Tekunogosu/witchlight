//! The exported surface of the world.
//!
//! The map is a directory of regions rather than one file. A region is sixteen
//! chunks on a side, which at a chunk edge of 32 is 512 blocks — one rendered
//! tile at its finest level, and the same square the game calls a map region. A
//! chunk that changes therefore belongs to one file and one tile, so the server
//! mod rewrites what moved instead of the whole map, and this side reloads one
//! region and redraws one tile instead of starting over.
//!
//! Each chunk in a region is compressed on its own, behind a directory of fixed
//! size. Version 4 was one gzip stream over the whole region, which meant the mod
//! could not write one chunk without rewriting the other two hundred and
//! fifty-five — and meant this side had to inflate a quarter of a megabyte to
//! look at one of them.
//!
//! Little endian, version 5. Nothing outside a payload is compressed:
//!
//! ```text
//!   0  magic     "MSQR"
//!   4  version   u16 = 5
//!   6  edge      u16   columns along a chunk's edge
//!   8  regionX   i32
//!  12  regionZ   i32
//!  16  slots     u16   chunks in a region, which is REGION_CHUNKS squared
//!  18  reserved  u16
//!  20  directory slots entries of 16 bytes, in slot order:
//!                  0  offset   u32  from the start of the file; 0 is empty
//!                  4  length   u32  bytes of the deflate stream
//!                  8  checksum u32  CRC-32 of those bytes
//!                 12  season   u8
//!                 13  flags    u8   bit 0: a column here is stored as air
//!                 14  reserved u16
//!      payloads, each a raw deflate stream of edge*edge entries of
//!      u16 blockId, i16 surfaceY, u8 temperature, u8 rainfall
//! ```
//!
//! A chunk's slot is its position in the region — `dz * REGION_CHUNKS + dx` — so a
//! record carries no coordinates and cannot disagree with where it is filed.
//!
//! Payloads are appended and never overwritten, so a file carries bytes nothing
//! points at until the mod packs it down; a reader walks the directory and never
//! the file. A slot whose bytes do not answer to the checksum beside them is read
//! as a chunk the map does not hold, which is what a run that died mid-append
//! leaves behind — and which the mod's own repair then fills in.
//!
//! Season is where the chunk sits in the year. It is per chunk rather than per
//! column because seasons vary by latitude and not across thirty-two blocks, and
//! it lives in the directory so that a year turning costs sixteen bytes a chunk
//! rather than a repacking of the map.
//!
//! Temperature and rainfall are the game's own packing of a column's climate, so
//! the colour maps can be sampled with them unchanged. [`Column::celsius`] and
//! [`Column::wetness`] read them back out for anyone who wants the numbers rather
//! than the colour.

use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};

use crate::error::{Error, Result};
use crate::log::warn;

const MAGIC: &[u8; 4] = b"MSQR";
pub const VERSION: u16 = 5;
const ENTRY_BYTES: usize = 6;
const HEADER_BYTES: usize = 20;
const SLOT_BYTES: usize = 16;

/// Chunks along a region's edge. One rendered tile at the finest level.
pub const REGION_CHUNKS: i32 = 16;

/// How many chunks a region holds, and so how many slots its directory has.
const SLOTS: usize = (REGION_CHUNKS * REGION_CHUNKS) as usize;

/// Where the first payload can start, which is past the directory.
const PAYLOADS_FROM: usize = HEADER_BYTES + SLOTS * SLOT_BYTES;

/// Which chunks a region holds.
///
/// The direction this program needs. A region is a fixed square of chunk
/// coordinates, so which chunks belong to one is arithmetic — where asking every
/// chunk in the world whose region it is means a pass over the whole map to
/// answer a question about one square of it.
pub fn chunks_of((rx, rz): (i32, i32)) -> impl Iterator<Item = (i32, i32)> {
    (0..REGION_CHUNKS)
        .flat_map(move |dz| (0..REGION_CHUNKS).map(move |dx| (rx * REGION_CHUNKS + dx, rz * REGION_CHUNKS + dz)))
}

/// Where the regions live inside the export directory.
#[must_use]
pub fn columns_dir(exports: &Path) -> PathBuf {
    exports.join("columns")
}

/// One column: what is on top, how high, and the climate there.
#[derive(Debug, Clone, Copy)]
pub struct Column {
    pub block: u16,
    pub height: i16,
    pub temperature: u8,
    pub rainfall: u8,
    /// Where this chunk sits in the year, copied from its record.
    pub season: u8,
}

impl Column {
    /// The climate temperature here, in degrees celsius.
    ///
    /// The mod packs it with the game's own `Climate.DescaleTemperature`, which
    /// puts -20 °C at 0 and 40 °C at 255 and clamps outside that; this is that
    /// inverse. It is the world-generation climate rather than the weather: what
    /// grows here, not what it is like outside today.
    #[must_use]
    pub fn celsius(&self) -> f32 {
        f32::from(self.temperature) / 4.25 - 20.0
    }

    /// Rainfall, from bone dry at zero to the wettest the game has at one.
    #[must_use]
    pub fn wetness(&self) -> f32 {
        f32::from(self.rainfall) / 255.0
    }
}

pub struct Chunk {
    pub columns: Vec<Column>,
}

/// One region file, parsed.
pub struct Region {
    pub at: (i32, i32),
    pub edge: usize,
    pub chunks: HashMap<(i32, i32), Chunk>,
}

impl Region {
    /// Reads one region: its directory, and every chunk the directory still
    /// points at.
    ///
    /// The whole file is read once and then walked by the directory, rather than
    /// seeking per chunk. A region is a few hundred kilobytes and this happens
    /// when one changes, so one read and a walk beats two hundred and fifty-six
    /// seeks; what matters is that only the payloads something points at are
    /// inflated, and the bytes left behind by an append are never touched.
    pub fn read(path: &Path) -> Result<Self> {
        let data = std::fs::read(path)
            .map_err(|source| Error::io(format!("reading {}", path.display()), source))?;
        Self::parse(&data, path)
    }

    fn parse(data: &[u8], path: &Path) -> Result<Self> {
        if data.len() < PAYLOADS_FROM || &data[..4] != MAGIC {
            return Err(Error::parse(path, "not a Witchlight region"));
        }

        let version = u16::from_le_bytes([data[4], data[5]]);
        if version != VERSION {
            return Err(Error::parse(
                path,
                format!(
                    "region format version {version} is not supported — \
                     export again with the current server mod"
                ),
            ));
        }

        let edge = u16::from_le_bytes([data[6], data[7]]) as usize;
        if edge == 0 || edge > 64 {
            return Err(Error::parse(path, format!("implausible chunk edge {edge}")));
        }

        let region_x = i32::from_le_bytes([data[8], data[9], data[10], data[11]]);
        let region_z = i32::from_le_bytes([data[12], data[13], data[14], data[15]]);
        let slots = u16::from_le_bytes([data[16], data[17]]) as usize;
        if slots != SLOTS {
            return Err(Error::parse(path, format!("a region of {slots} chunks, not {SLOTS}")));
        }

        let mut chunks = HashMap::new();
        for slot in 0..SLOTS {
            let Some(chunk) = Slot::at(data, slot).and_then(|held| held.columns(data, edge)) else {
                continue;
            };
            chunks.insert(chunk_at(region_x, region_z, slot), chunk);
        }

        Ok(Self { at: (region_x, region_z), edge, chunks })
    }
}

/// Which chunk one slot of a region belongs to. The mod files a chunk by its
/// position in the square, so this and `Regions.ChunkAt` are one arithmetic.
fn chunk_at(region_x: i32, region_z: i32, slot: usize) -> (i32, i32) {
    let slot = slot as i32;
    (region_x * REGION_CHUNKS + slot % REGION_CHUNKS, region_z * REGION_CHUNKS + slot / REGION_CHUNKS)
}

/// One entry of a region's directory: where a chunk's bytes are and what they
/// should come to.
#[derive(Debug, Clone, Copy)]
struct Slot {
    offset: usize,
    length: usize,
    checksum: u32,
    season: u8,
}

impl Slot {
    /// The entry for one slot, or nothing where the slot holds no chunk.
    fn at(data: &[u8], slot: usize) -> Option<Self> {
        let entry = &data[HEADER_BYTES + slot * SLOT_BYTES..];
        let offset = u32::from_le_bytes([entry[0], entry[1], entry[2], entry[3]]) as usize;
        let length = u32::from_le_bytes([entry[4], entry[5], entry[6], entry[7]]) as usize;
        if offset < PAYLOADS_FROM || length == 0 || offset.checked_add(length)? > data.len() {
            return None;
        }

        Some(Self {
            offset,
            length,
            checksum: u32::from_le_bytes([entry[8], entry[9], entry[10], entry[11]]),
            season: entry[12],
        })
    }

    /// This slot's columns, or nothing where its bytes are not the ones the
    /// directory was written about.
    ///
    /// A slot that fails its checksum is read as a chunk the map does not hold.
    /// That is the honest answer — those bytes are a half-written payload from a
    /// run that died before it could point at them — and it is the useful one,
    /// because a chunk the map does not hold is one the mod's repair already
    /// knows how to fetch again.
    fn columns(self, data: &[u8], edge: usize) -> Option<Chunk> {
        let packed = &data[self.offset..self.offset + self.length];
        let mut crc = flate2::Crc::new();
        crc.update(packed);
        if crc.sum() != self.checksum {
            return None;
        }

        let mut record = Vec::with_capacity(edge * edge * ENTRY_BYTES);
        flate2::read::DeflateDecoder::new(packed).read_to_end(&mut record).ok()?;
        if record.len() < edge * edge * ENTRY_BYTES {
            return None;
        }

        let mut columns = Vec::with_capacity(edge * edge);
        for index in 0..edge * edge {
            let entry = &record[index * ENTRY_BYTES..];
            columns.push(Column {
                block: u16::from_le_bytes([entry[0], entry[1]]),
                height: i16::from_le_bytes([entry[2], entry[3]]),
                temperature: entry[4],
                rainfall: entry[5],
                season: self.season,
            });
        }
        Some(Chunk { columns })
    }
}

/// How far a region's exported chunks actually reach.
///
/// Held per region so that the world's own bounds are a walk over a few hundred
/// regions rather than over every chunk in the world. That walk used to happen
/// twice for every region loaded, which on a large world is millions of
/// comparisons per export for two numbers that move when somebody explores.
#[derive(Debug, Clone, Copy)]
struct Extent {
    min: (i32, i32),
    max: (i32, i32),
}

impl Extent {
    /// The extent of one chunk on its own.
    fn around((cx, cz): (i32, i32)) -> Self {
        Self { min: (cx, cz), max: (cx, cz) }
    }

    /// The smallest extent holding both of these.
    fn with(self, other: Self) -> Self {
        Self {
            min: (self.min.0.min(other.min.0), self.min.1.min(other.min.1)),
            max: (self.max.0.max(other.max.0), self.max.1.max(other.max.1)),
        }
    }

    /// The smallest extent holding every one of them, or nothing where there are
    /// none. One widening, however the pieces arrive: a region's chunks widen it
    /// the same way one region's reach widens the world's, and the two written
    /// out separately were two chances to compare the wrong corner.
    fn covering(all: impl IntoIterator<Item = Self>) -> Option<Self> {
        all.into_iter().reduce(Self::with)
    }

    fn of(chunks: &HashMap<(i32, i32), Chunk>) -> Option<Self> {
        Self::covering(chunks.keys().copied().map(Self::around))
    }
}

/// Every exported chunk, addressed by chunk coordinates.
pub struct World {
    pub edge: usize,
    pub chunks: HashMap<(i32, i32), Chunk>,
    /// Which regions are loaded, and how far each one's chunks reach. A region
    /// whose file held no records reaches nowhere, which is `None`.
    regions: HashMap<(i32, i32), Option<Extent>>,
}

impl World {
    /// A world nothing has been exported into yet.
    ///
    /// Named rather than written out, so that a field added here is a field the
    /// tests get too — the last one was added in two places and the second was
    /// noticed only because it would not compile.
    #[must_use]
    pub fn empty() -> Self {
        Self { edge: 0, chunks: HashMap::new(), regions: HashMap::new() }
    }

    /// Whether anything has been exported yet.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.chunks.is_empty()
    }

    /// Loads every region in an export directory.
    pub fn load(exports: &Path) -> Result<Self> {
        let dir = columns_dir(exports);
        let mut world = Self::empty();

        for path in region_files(&dir)? {
            // One unreadable region costs that square, not the run. A region being
            // written as it is read is the common reason, and the next refresh
            // picks it up.
            match Region::read(&path) {
                Ok(region) => world.apply(region),
                Err(error) => warn!("skipping {}: {error}", path.display()),
            }
        }

        // An empty map is a state, not a failure. It is also the state every
        // server is in the moment a format change clears it, and refusing to
        // start then means the map service is down exactly when someone is
        // watching to see whether the upgrade worked.
        Ok(world)
    }

    /// Takes a region's chunks, replacing whatever that square held before.
    pub fn apply(&mut self, region: Region) {
        if self.edge == 0 {
            self.edge = region.edge;
        }
        self.forget(region.at);

        self.regions.insert(region.at, Extent::of(&region.chunks));
        self.chunks.extend(region.chunks);
    }

    /// Drops a region's chunks, for a region file that has gone away.
    ///
    /// The chunks are named rather than searched for. A region is a fixed square
    /// of chunk coordinates, so which ones belong to it is arithmetic — where
    /// asking every chunk in the world whose region it is means a pass over the
    /// whole map for one square of it.
    pub fn forget(&mut self, at: (i32, i32)) {
        if self.regions.remove(&at).is_none() {
            return;
        }
        for chunk in chunks_of(at) {
            self.chunks.remove(&chunk);
        }
    }

    /// Every region that has been loaded.
    pub fn regions(&self) -> impl Iterator<Item = (i32, i32)> + '_ {
        self.regions.keys().copied()
    }

    /// How many there are.
    #[must_use]
    pub fn region_count(&self) -> usize {
        self.regions.len()
    }

    /// The column at a world block position, if that chunk was exported.
    #[must_use]
    pub fn column_at(&self, x: i32, z: i32) -> Option<Column> {
        if self.edge == 0 {
            return None;
        }
        let edge = self.edge as i32;
        let chunk = self.chunks.get(&(x.div_euclid(edge), z.div_euclid(edge)))?;
        let (dx, dz) = (x.rem_euclid(edge) as usize, z.rem_euclid(edge) as usize);
        chunk.columns.get(dz * self.edge + dx).copied()
    }

    /// World bounds in blocks: the area worth drawing.
    #[must_use]
    pub fn bounds(&self) -> (i32, i32, i32, i32) {
        let whole = Extent::covering(self.regions.values().flatten().copied());
        let Some(whole) = whole.filter(|_| !self.chunks.is_empty()) else {
            return (0, 0, 0, 0);
        };
        let edge = self.edge as i32;
        (
            whole.min.0 * edge,
            whole.min.1 * edge,
            (whole.max.0 + 1) * edge,
            (whole.max.1 + 1) * edge,
        )
    }
}

/// Every region file in a directory, with its coordinates taken from its name.
pub fn region_files(dir: &Path) -> Result<Vec<PathBuf>> {
    let paths = crate::files::listing(dir)
        .map_err(|error| Error::io(format!("reading {}", dir.display()), error))?;
    Ok(paths.into_iter().filter(|path| region_coords(path).is_some()).collect())
}

/// `r.{x}.{z}.msqr`, where the numbers are region coordinates and may be negative.
#[must_use]
pub fn region_coords(path: &Path) -> Option<(i32, i32)> {
    let name = path.file_name()?.to_str()?;
    let rest = name.strip_prefix("r.")?.strip_suffix(".msqr")?;
    let (x, z) = rest.split_once('.')?;
    Some((x.parse().ok()?, z.parse().ok()?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write as _;

    /// One region built the way the mod builds one, so that the two halves are
    /// held to the same bytes by something other than two prose descriptions of
    /// them agreeing.
    ///
    /// Takes the chunks to file and, for one of them, whether to spoil its
    /// payload after the checksum was taken over the good bytes — which is what a
    /// run that died part way through an append leaves behind.
    fn filed(at: (i32, i32), edge: usize, chunks: &[(usize, u8, u16)], spoil: Option<usize>)
    -> Vec<u8> {
        let mut file = vec![0u8; PAYLOADS_FROM];
        file[..4].copy_from_slice(MAGIC);
        file[4..6].copy_from_slice(&VERSION.to_le_bytes());
        file[6..8].copy_from_slice(&(edge as u16).to_le_bytes());
        file[8..12].copy_from_slice(&at.0.to_le_bytes());
        file[12..16].copy_from_slice(&at.1.to_le_bytes());
        file[16..18].copy_from_slice(&(SLOTS as u16).to_le_bytes());

        for &(slot, season, block) in chunks {
            let mut record = Vec::with_capacity(edge * edge * ENTRY_BYTES);
            for index in 0..edge * edge {
                record.extend_from_slice(&block.to_le_bytes());
                record.extend_from_slice(&(index as i16).to_le_bytes());
                record.push(80);
                record.push(90);
            }

            let mut packing =
                flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::best());
            packing.write_all(&record).expect("a deflate stream");
            let mut packed = packing.finish().expect("a deflate stream");

            let mut crc = flate2::Crc::new();
            crc.update(&packed);
            let checksum = crc.sum();
            if spoil == Some(slot) {
                packed[0] ^= 0xff;
            }

            let offset = file.len();
            file.extend_from_slice(&packed);

            let entry = HEADER_BYTES + slot * SLOT_BYTES;
            file[entry..entry + 4].copy_from_slice(&(offset as u32).to_le_bytes());
            file[entry + 4..entry + 8].copy_from_slice(&(packed.len() as u32).to_le_bytes());
            file[entry + 8..entry + 12].copy_from_slice(&checksum.to_le_bytes());
            file[entry + 12] = season;
        }
        file
    }

    /// The format, held to the shape both halves write and read it in.
    #[test]
    fn a_region_reads_back_as_the_chunks_that_were_filed_in_it() {
        let path = Path::new("r.2.-3.msqr");
        let held = filed((2, -3), 4, &[(0, 7, 11), (17, 9, 22), (255, 3, 33)], None);
        let read = Region::parse(&held, path).expect("a region this build wrote");

        assert_eq!(read.at, (2, -3));
        assert_eq!(read.edge, 4);
        assert_eq!(read.chunks.len(), 3, "one chunk per filled slot and no more");

        // A slot is a position in the square, so where a chunk lands is arithmetic
        // rather than something the record carries and could get wrong.
        assert!(read.chunks.contains_key(&(32, -48)), "slot 0 is the region's corner");
        assert!(read.chunks.contains_key(&(33, -47)), "slot 17 is one along and one down");
        assert!(read.chunks.contains_key(&(47, -33)), "slot 255 is the far corner");

        let corner = &read.chunks[&(32, -48)];
        assert_eq!(corner.columns.len(), 16);
        assert_eq!(corner.columns[0].block, 11);
        assert_eq!(corner.columns[5].height, 5, "a column is where the record put it");
        assert_eq!(corner.columns[0].season, 7, "the season comes off the directory");
        assert_eq!(read.chunks[&(33, -47)].columns[0].season, 9);
    }

    /// A half-written payload is a chunk the map does not hold, not a chunk of
    /// nonsense. The mod's repair fills one in; a panic here would lose the map.
    #[test]
    fn a_chunk_that_does_not_answer_to_its_checksum_is_read_as_one_that_is_not_there() {
        let path = Path::new("r.0.0.msqr");
        let held = filed((0, 0), 4, &[(0, 1, 11), (1, 1, 22)], Some(1));
        let read = Region::parse(&held, path).expect("a region with one bad slot in it");

        assert_eq!(read.chunks.len(), 1, "the good chunk is still read");
        assert!(read.chunks.contains_key(&(0, 0)));
        assert!(!read.chunks.contains_key(&(1, 0)), "and the spoiled one is simply absent");
    }

    /// The map on disk outlives builds of this program, and a format it cannot
    /// read has to say so rather than be read as whatever it happens to parse as.
    #[test]
    fn a_region_from_another_format_is_refused_rather_than_guessed_at() {
        let path = Path::new("r.0.0.msqr");
        let mut older = filed((0, 0), 4, &[(0, 1, 11)], None);
        older[4..6].copy_from_slice(&4u16.to_le_bytes());
        assert!(Region::parse(&older, path).is_err(), "version 4 is not this format");

        let mut nonsense = filed((0, 0), 4, &[(0, 1, 11)], None);
        nonsense[..4].copy_from_slice(b"XXXX");
        assert!(Region::parse(&nonsense, path).is_err(), "and neither is anything else");

        assert!(Region::parse(&[0u8; 8], path).is_err(), "nor a file shorter than a directory");
    }

    fn column(temperature: u8, rainfall: u8) -> Column {
        Column { block: 0, height: 0, temperature, rainfall, season: 0 }
    }

    /// A region holding one chunk at a named place inside it.
    fn region(at: (i32, i32), chunk: (i32, i32)) -> Region {
        Region {
            at,
            edge: 2,
            chunks: HashMap::from([(chunk, Chunk { columns: vec![column(0, 0); 4] })]),
        }
    }

    /// The claim the viewer's chunk grid rests on.
    ///
    /// The grid's coarsest zoom is `log2(8 / chunk edge)` levels out from the
    /// finest, and it is worked out once when the layer is built. An edge of zero
    /// makes that infinite, and since the layer is never rebuilt the grid would
    /// then be gone for as long as the page stayed open. The viewer is safe
    /// because it builds nothing until the bounds are worth drawing — which is
    /// this pairing, and it lives here rather than in the page that depends on it.
    #[test]
    fn a_world_worth_drawing_always_knows_its_chunk_edge() {
        let mut world = World::empty();
        assert_eq!(world.edge, 0);
        assert_eq!(world.bounds(), (0, 0, 0, 0));

        world.apply(region((0, 0), (0, 0)));
        let (min_x, min_z, max_x, max_z) = world.bounds();
        assert!(world.edge > 0, "a world with terrain in it knows how wide a chunk is");
        assert!(max_x > min_x && max_z > min_z, "and reports bounds the viewer will draw on");

        // The other direction: nothing exported is degenerate bounds, which is
        // what stops the page building a grid it could not fix afterwards.
        world.forget((0, 0));
        assert_eq!(world.bounds(), (0, 0, 0, 0));
    }

    /// The claim `World::forget` rests on.
    ///
    /// It names a region's chunks by arithmetic rather than asking every chunk in
    /// the world which region it is in, so the square it walks has to be exactly
    /// the set that floors back to that region — one chunk missed is terrain that
    /// stays on the map after its file has gone, and one too many is a neighbour's
    /// terrain taken with it.
    #[test]
    fn a_regions_chunks_are_exactly_the_ones_that_floor_back_to_it() {
        // The definition, written out rather than borrowed: negative coordinates
        // floor, which is where this arithmetic goes wrong if it goes wrong.
        let holding =
            |x: i32, z: i32| (x.div_euclid(REGION_CHUNKS), z.div_euclid(REGION_CHUNKS));

        for at in [(0, 0), (3, -2), (-1, -1)] {
            let held: Vec<_> = chunks_of(at).collect();
            assert_eq!(held.len() as i32, REGION_CHUNKS * REGION_CHUNKS);
            for &(x, z) in &held {
                assert_eq!(holding(x, z), at, "chunk ({x}, {z})");
            }

            // And the chunks just outside the square belong to a neighbour.
            let (x, z) = (at.0 * REGION_CHUNKS, at.1 * REGION_CHUNKS);
            for outside in [(x - 1, z), (x, z - 1), (x + REGION_CHUNKS, z), (x, z + REGION_CHUNKS)] {
                assert!(!held.contains(&outside), "{outside:?} is not this region's");
                assert_ne!(holding(outside.0, outside.1), at);
            }
        }
    }

    #[test]
    fn a_region_that_goes_away_takes_its_chunks_and_no_others() {
        let mut world = World::load(Path::new("/nonexistent")).expect("an empty world");
        world.apply(region((0, 0), (1, 1)));
        world.apply(region((1, 0), (REGION_CHUNKS, 0)));
        assert_eq!(world.chunks.len(), 2);
        assert_eq!(world.region_count(), 2);

        world.forget((0, 0));
        assert_eq!(world.region_count(), 1);
        assert!(world.chunks.contains_key(&(REGION_CHUNKS, 0)), "its neighbour stays");
        assert!(!world.chunks.contains_key(&(1, 1)), "and its own chunk goes");

        // A region nobody loaded is not an error and takes nothing with it.
        world.forget((9, 9));
        assert_eq!(world.chunks.len(), 1);
    }

    #[test]
    fn the_worlds_bounds_are_the_chunks_that_exist_rather_than_the_squares_holding_them() {
        let mut world = World::load(Path::new("/nonexistent")).expect("an empty world");
        assert_eq!(world.bounds(), (0, 0, 0, 0), "nothing exported yet");

        // One chunk, two blocks to a chunk edge: the map is that chunk alone and
        // not the sixteen-chunk square its file covers.
        world.apply(region((0, 0), (1, 1)));
        assert_eq!(world.bounds(), (2, 2, 4, 4));

        world.apply(region((-1, -1), (-1, -1)));
        assert_eq!(world.bounds(), (-2, -2, 4, 4));

        world.forget((-1, -1));
        assert_eq!(world.bounds(), (2, 2, 4, 4), "and they come back in when one goes");
    }

    /// The packing is the game's, not this program's, and the game's own
    /// documentation for it states a range it does not use — so these rows come
    /// from calling `Climate.DescaleTemperature` rather than from reading about
    /// it. A byte covers -20 °C to 40 °C, and nothing outside that survives the
    /// export at all.
    #[test]
    fn a_packed_temperature_reads_back_as_degrees() {
        for (byte, celsius) in [(0u8, -20.0), (85, 0.0), (170, 20.0), (255, 40.0)] {
            let read = column(byte, 0).celsius();
            assert!(
                (read - celsius).abs() < 0.2,
                "byte {byte} should be about {celsius} °C, read {read}"
            );
        }
    }

    #[test]
    fn rainfall_runs_from_dry_to_wettest() {
        assert!((column(0, 0).wetness() - 0.0).abs() < f32::EPSILON);
        assert!((column(0, 255).wetness() - 1.0).abs() < f32::EPSILON);
        assert!((column(0, 128).wetness() - 0.5).abs() < 0.01);
    }
}
