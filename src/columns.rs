//! The exported surface of the world.
//!
//! The map is a directory of regions rather than one file. A region is sixteen
//! chunks on a side, which at a chunk edge of 32 is 512 blocks — one rendered
//! tile at its finest level, and the same square the game calls a map region. A
//! chunk that changes therefore belongs to one file and one tile, so the server
//! mod rewrites what moved instead of the whole map, and this side reloads one
//! region and redraws one tile instead of starting over.
//!
//! Each region file is a gzip stream. Inside it, little endian, version 4:
//!
//! ```text
//! header   "MSQR", u16 version, u16 columns per chunk edge,
//!          i32 regionX, i32 regionZ, i32 chunks
//! record   i32 chunkX, i32 chunkZ, u8 season, u8 reserved, then edge*edge
//!          entries of u16 blockId, i16 surfaceY, u8 temperature, u8 rainfall
//! ```
//!
//! Season is where the chunk sits in the year. It is per chunk rather than per
//! column because seasons vary by latitude and not across thirty-two blocks.
//!
//! Temperature and rainfall are the game's own packing of a column's climate, so
//! the colour maps can be sampled with them unchanged. [`Column::celsius`] and
//! [`Column::wetness`] read them back out for anyone who wants the numbers rather
//! than the colour.
//!
//! Records are fixed size, so a region is one length check and a stride.

use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};

use crate::error::{Error, Result};

const MAGIC: &[u8; 4] = b"MSQR";
pub const VERSION: u16 = 4;
const ENTRY_BYTES: usize = 6;
const HEADER_BYTES: usize = 20;
const RECORD_HEADER_BYTES: usize = 10;

/// Chunks along a region's edge. One rendered tile at the finest level.
pub const REGION_CHUNKS: i32 = 16;

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
    /// Reads and decompresses one region.
    pub fn read(path: &Path) -> Result<Self> {
        let file = std::fs::File::open(path)
            .map_err(|source| Error::io(format!("reading {}", path.display()), source))?;

        let mut data = Vec::new();
        flate2::read::GzDecoder::new(file)
            .read_to_end(&mut data)
            .map_err(|source| Error::io(format!("decompressing {}", path.display()), source))?;

        Self::parse(&data, path)
    }

    fn parse(data: &[u8], path: &Path) -> Result<Self> {
        if data.len() < HEADER_BYTES || &data[..4] != MAGIC {
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

        let stride = RECORD_HEADER_BYTES + edge * edge * ENTRY_BYTES;
        let mut chunks = HashMap::new();
        let mut at = HEADER_BYTES;

        while at + stride <= data.len() {
            let record = &data[at..at + stride];
            let cx = i32::from_le_bytes([record[0], record[1], record[2], record[3]]);
            let cz = i32::from_le_bytes([record[4], record[5], record[6], record[7]]);
            let season = record[8];

            let mut columns = Vec::with_capacity(edge * edge);
            for index in 0..edge * edge {
                let entry = &record[RECORD_HEADER_BYTES + index * ENTRY_BYTES..];
                columns.push(Column {
                    block: u16::from_le_bytes([entry[0], entry[1]]),
                    height: i16::from_le_bytes([entry[2], entry[3]]),
                    temperature: entry[4],
                    rainfall: entry[5],
                    season,
                });
            }

            chunks.insert((cx, cz), Chunk { columns });
            at += stride;
        }

        Ok(Self { at: (region_x, region_z), edge, chunks })
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
    fn of(chunks: &HashMap<(i32, i32), Chunk>) -> Option<Self> {
        let mut extent: Option<Self> = None;
        for &(cx, cz) in chunks.keys() {
            extent = Some(match extent {
                None => Self { min: (cx, cz), max: (cx, cz) },
                Some(held) => Self {
                    min: (held.min.0.min(cx), held.min.1.min(cz)),
                    max: (held.max.0.max(cx), held.max.1.max(cz)),
                },
            });
        }
        extent
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
    /// Whether anything has been exported yet.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.chunks.is_empty()
    }

    /// Loads every region in an export directory.
    pub fn load(exports: &Path) -> Result<Self> {
        let dir = columns_dir(exports);
        let mut world = Self {
            edge: 0,
            chunks: HashMap::new(),
            regions: HashMap::new(),
        };

        for path in region_files(&dir)? {
            // One unreadable region costs that square, not the run. A region being
            // written as it is read is the common reason, and the next refresh
            // picks it up.
            match Region::read(&path) {
                Ok(region) => world.apply(region),
                Err(error) => eprintln!("witchlight: skipping {}: {error}", path.display()),
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
        let mut whole: Option<Extent> = None;
        for extent in self.regions.values().flatten() {
            whole = Some(match whole {
                None => *extent,
                Some(held) => Extent {
                    min: (held.min.0.min(extent.min.0), held.min.1.min(extent.min.1)),
                    max: (held.max.0.max(extent.max.0), held.max.1.max(extent.max.1)),
                },
            });
        }

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
        let mut world = World {
            edge: 0,
            chunks: HashMap::new(),
            regions: HashMap::new(),
        };
        assert_eq!(world.edge, 0);
        assert_eq!(world.bounds(), (0, 0, 0, 0));

        world.apply(region((0, 0), (0, 0)));
        let (min_x, min_z, max_x, max_z) = world.bounds();
        assert!(world.edge > 0, "a world with terrain in it knows how wide a chunk is");
        assert!(
            max_x > min_x && max_z > min_z,
            "and reports bounds the viewer will draw on"
        );

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
