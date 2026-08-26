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

/// Which region a chunk belongs to. Negative coordinates floor, as they must.
#[must_use]
pub fn region_of(chunk_x: i32, chunk_z: i32) -> (i32, i32) {
    (chunk_x.div_euclid(REGION_CHUNKS), chunk_z.div_euclid(REGION_CHUNKS))
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
            return Err(Error::parse(path, "not a Mapstique region"));
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

/// Every exported chunk, addressed by chunk coordinates.
pub struct World {
    pub edge: usize,
    pub chunks: HashMap<(i32, i32), Chunk>,
    pub regions: Vec<(i32, i32)>,
    min: (i32, i32),
    max: (i32, i32),
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
            regions: Vec::new(),
            min: (i32::MAX, i32::MAX),
            max: (i32::MIN, i32::MIN),
        };

        for path in region_files(&dir)? {
            // One unreadable region costs that square, not the run. A region being
            // written as it is read is the common reason, and the next refresh
            // picks it up.
            match Region::read(&path) {
                Ok(region) => world.apply(region),
                Err(error) => eprintln!("mapstique: skipping {}: {error}", path.display()),
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
        if !self.regions.contains(&region.at) {
            self.regions.push(region.at);
        }
        self.chunks.extend(region.chunks);
        self.rebound();
    }

    /// Drops a region's chunks, for a region file that has gone away.
    pub fn forget(&mut self, at: (i32, i32)) {
        self.chunks.retain(|&(cx, cz), _| region_of(cx, cz) != at);
        self.regions.retain(|held| *held != at);
        self.rebound();
    }

    fn rebound(&mut self) {
        let (mut min, mut max) = ((i32::MAX, i32::MAX), (i32::MIN, i32::MIN));
        for &(cx, cz) in self.chunks.keys() {
            min = (min.0.min(cx), min.1.min(cz));
            max = (max.0.max(cx), max.1.max(cz));
        }
        self.min = min;
        self.max = max;
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
        if self.chunks.is_empty() {
            return (0, 0, 0, 0);
        }
        let edge = self.edge as i32;
        (
            self.min.0 * edge,
            self.min.1 * edge,
            (self.max.0 + 1) * edge,
            (self.max.1 + 1) * edge,
        )
    }
}

/// Every region file in a directory, with its coordinates taken from its name.
pub fn region_files(dir: &Path) -> Result<Vec<PathBuf>> {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        // The mod makes this directory when it first exports. Until then there is
        // nothing to read and nothing wrong.
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(Error::io(format!("reading {}", dir.display()), error)),
    };

    let mut paths: Vec<PathBuf> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| region_coords(path).is_some())
        .collect();
    paths.sort();
    Ok(paths)
}

/// `r.{x}.{z}.msqr`, where the numbers are region coordinates and may be negative.
#[must_use]
pub fn region_coords(path: &Path) -> Option<(i32, i32)> {
    let name = path.file_name()?.to_str()?;
    let rest = name.strip_prefix("r.")?.strip_suffix(".msqr")?;
    let (x, z) = rest.split_once('.')?;
    Some((x.parse().ok()?, z.parse().ok()?))
}
