//! This service's own picture of the world, saved and loaded whole.
//!
//! Not the mod's export — that is [`crate::columns::Region`], a format the mod
//! owns and appends to as the game runs. This is the opposite shape: everything
//! this service holds in memory, written out in one pass on its own clock and
//! read back in one pass at start, so a restart does not have to replay every
//! region file to rebuild what a running service already knew. A missing or
//! unreadable snapshot is not a fault — the region files are still there, and
//! [`crate::columns::World::load`] rebuilds from them exactly as it always has.
//!
//! One gzip stream, little endian:
//!
//! ```text
//! header  "WLSN", u16 version, i32 edge, u32 chunk_count
//! chunk   i32 chunk_x, i32 chunk_z, then edge*edge entries of
//!         u16 block, i16 height, u8 temperature, u8 rainfall, u8 season
//! ```
//!
//! The record layout matches [`crate::columns::Column`] field for field, so
//! reading one is the same six-byte stride the region format already uses.

use std::collections::HashMap;
use std::io::{Read, Write as _};
use std::path::{Path, PathBuf};

use crate::columns::{Chunk, Column, World};
use crate::error::{Error, Result};
use crate::files;

const MAGIC: &[u8; 4] = b"WLSN";
const VERSION: u16 = 1;
/// Magic (4) + version (2) + edge (4) + chunk count (4).
const HEADER_BYTES: usize = 14;
const ENTRY_BYTES: usize = 7;

/// Where the snapshot lives, beside the map rather than inside `columns/` — it is
/// this service's own file and not one the mod's own region-packing code should
/// ever have reason to look at.
#[must_use]
pub fn path_in(exports: &Path) -> PathBuf {
    exports.join("service-snapshot.bin")
}

/// Writes every chunk `world` holds. Nothing is written for a world that has
/// loaded nothing yet — an empty snapshot would otherwise overwrite a real one
/// left from a previous run if this races a `World::load` that has not finished.
pub fn write(exports: &Path, world: &World) -> Result<()> {
    if world.is_empty() {
        return Ok(());
    }

    let edge = world.edge;
    let chunks = &world.chunks;

    let mut body = Vec::with_capacity(HEADER_BYTES + chunks.len() * (8 + edge * edge * ENTRY_BYTES));
    body.extend_from_slice(MAGIC);
    body.extend_from_slice(&VERSION.to_le_bytes());
    body.extend_from_slice(&(edge as i32).to_le_bytes());
    body.extend_from_slice(&(chunks.len() as u32).to_le_bytes());

    for (&(cx, cz), chunk) in chunks {
        body.extend_from_slice(&cx.to_le_bytes());
        body.extend_from_slice(&cz.to_le_bytes());
        for column in &chunk.columns {
            body.extend_from_slice(&column.block.to_le_bytes());
            body.extend_from_slice(&column.height.to_le_bytes());
            body.push(column.temperature);
            body.push(column.rainfall);
            body.push(column.season);
        }
    }

    let mut gzipped = Vec::new();
    flate2::write::GzEncoder::new(&mut gzipped, flate2::Compression::fast())
        .write_all(&body)
        .and_then(|()| Ok(()))
        .map_err(|error: std::io::Error| Error::io("compressing the snapshot".to_owned(), error))?;

    let path = path_in(exports);
    files::replace(&path, &gzipped)
        .map_err(|error| Error::io(format!("writing {}", path.display()), error))
}

/// Reads a snapshot back into a world, or `None` where there is none to read —
/// which is every server before its first save under this version, and is
/// answered by falling back to [`crate::columns::World::load`].
#[must_use]
pub fn read(exports: &Path) -> Option<World> {
    let file = std::fs::File::open(path_in(exports)).ok()?;
    let mut body = Vec::new();
    flate2::read::GzDecoder::new(file).read_to_end(&mut body).ok()?;
    parse(&body)
}

fn parse(data: &[u8]) -> Option<World> {
    if data.len() < HEADER_BYTES || &data[..4] != MAGIC {
        return None;
    }

    let version = u16::from_le_bytes([data[4], data[5]]);
    if version != VERSION {
        return None;
    }

    let edge = i32::from_le_bytes([data[6], data[7], data[8], data[9]]) as usize;
    let count = u32::from_le_bytes([data[10], data[11], data[12], data[13]]) as usize;
    if edge == 0 || edge > 64 {
        return None;
    }

    let stride = 8 + edge * edge * ENTRY_BYTES;
    let mut chunks = HashMap::with_capacity(count);
    let mut at = HEADER_BYTES;

    for _ in 0..count {
        if at + stride > data.len() {
            break;
        }

        let cx = i32::from_le_bytes([data[at], data[at + 1], data[at + 2], data[at + 3]]);
        let cz = i32::from_le_bytes([data[at + 4], data[at + 5], data[at + 6], data[at + 7]]);
        let mut columns = Vec::with_capacity(edge * edge);

        for index in 0..edge * edge {
            let entry = at + 8 + index * ENTRY_BYTES;
            columns.push(Column {
                block: u16::from_le_bytes([data[entry], data[entry + 1]]),
                height: i16::from_le_bytes([data[entry + 2], data[entry + 3]]),
                temperature: data[entry + 4],
                rainfall: data[entry + 5],
                season: data[entry + 6],
            });
        }

        chunks.insert((cx, cz), Chunk { columns });
        at += stride;
    }

    Some(World::from_chunks(edge, chunks))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::files::testing::Scratch;

    fn column(block: u16) -> Column {
        Column { block, height: 5, temperature: 100, rainfall: 50, season: 3 }
    }

    #[test]
    fn a_world_written_and_read_back_holds_the_same_columns() {
        let held = Scratch::new("snapshot-roundtrip");
        let mut chunks = HashMap::new();
        chunks.insert((2, -3), Chunk { columns: vec![column(7); 4] });
        let world = World::from_chunks(2, chunks);

        write(held.at(), &world).expect("a snapshot writes");
        let read = read(held.at()).expect("and reads back");

        assert_eq!(read.edge, 2);
        assert_eq!(read.column_at(4, -6).map(|c| c.block), Some(7));
    }

    #[test]
    fn nothing_is_written_for_a_world_that_has_loaded_nothing() {
        let held = Scratch::new("snapshot-empty");
        write(held.at(), &World::empty()).expect("writing nothing is not an error");
        assert!(!path_in(held.at()).exists());
    }

    #[test]
    fn a_missing_snapshot_reads_as_none_rather_than_an_error() {
        let held = Scratch::new("snapshot-missing");
        assert!(read(held.at()).is_none());
    }
}
