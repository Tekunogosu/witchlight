//! What each person has seen, in the map's own database.
//!
//! A person's memory is which chunks they have discovered and where that memory
//! disagrees with the map. A chunk they saw that changed while they were away is
//! a divergence, and it holds the version they saw, so the map can still draw it
//! as they remember it.
//!
//! Tables: `discovered`, `divergences`.

use rusqlite::params;

use crate::util::error::{Error, Result};

use super::{BITSET_BYTES, Counts, Discovered, Divergence, Store};

impl Store {
    /// Returns everything everybody has discovered.
    pub fn discovered(&self) -> Result<Vec<Discovered>> {
        self.rows(
            "SELECT uid, rx, rz, bits FROM discovered",
            "reading what was discovered",
            [],
            |row| {
                // A shorter row was written by an older build and a longer one
                // by a newer build. Read both as far as they go rather than
                // refusing them.
                let bits = row.get::<_, Vec<u8>>(3)?;
                let mut fixed = [0u8; BITSET_BYTES];
                let taken = bits.len().min(BITSET_BYTES);
                fixed[..taken].copy_from_slice(&bits[..taken]);
                Ok(Discovered {
                    uid: row.get(0)?,
                    rx: row.get(1)?,
                    rz: row.get(2)?,
                    bits: fixed,
                })
            },
        )
    }

    /// Records one person's discovered chunks in one region, replacing the
    /// whole bitset.
    pub fn set_discovered(&self, uid: &str, rx: i32, rz: i32, bits: &[u8; BITSET_BYTES]) -> Result<()> {
        self.run(
            "INSERT INTO discovered (uid, rx, rz, bits) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT (uid, rx, rz) DO UPDATE SET bits = excluded.bits",
            "recording a discovery",
            params![uid, rx, rz, &bits[..]],
        )?;
        Ok(())
    }

    /// Returns every place anybody's memory disagrees with the map.
    pub fn divergences(&self) -> Result<Vec<Divergence>> {
        self.rows(
            "SELECT uid, cx, cz, version FROM divergences",
            "reading the divergences",
            [],
            |row| {
                Ok(Divergence {
                    uid: row.get(0)?,
                    cx: row.get(1)?,
                    cz: row.get(2)?,
                    version: row.get(3)?,
                })
            },
        )
    }

    /// Records that people remember chunks at versions the map has moved on
    /// from. Writes them all in one transaction, because one chunk changing under
    /// many absent people is the common case.
    pub fn set_divergences(&self, diverged: &[Divergence]) -> Result<()> {
        if diverged.is_empty() {
            return Ok(());
        }
        let mut connection = self.lock();
        let transaction = connection
            .transaction()
            .map_err(|error| Error::database("beginning a write", error))?;
        for one in diverged {
            transaction
                .execute(
                    "INSERT INTO divergences (uid, cx, cz, version) VALUES (?1, ?2, ?3, ?4)
                     ON CONFLICT (uid, cx, cz) DO NOTHING",
                    params![one.uid, one.cx, one.cz, one.version],
                )
                .map_err(|error| Error::database("recording a divergence", error))?;
        }
        transaction.commit().map_err(|error| Error::database("committing a write", error))
    }

    /// Clears a person's divergences for chunks they have seen again.
    pub fn clear_divergences(&self, uid: &str, chunks: &[(i32, i32)]) -> Result<()> {
        if chunks.is_empty() {
            return Ok(());
        }
        let mut connection = self.lock();
        let transaction = connection
            .transaction()
            .map_err(|error| Error::database("beginning a write", error))?;
        for &(cx, cz) in chunks {
            transaction
                .execute(
                    "DELETE FROM divergences WHERE uid = ?1 AND cx = ?2 AND cz = ?3",
                    params![uid, cx, cz],
                )
                .map_err(|error| Error::database("clearing a divergence", error))?;
        }
        transaction.commit().map_err(|error| Error::database("committing a write", error))
    }

    /// Deletes every version nothing points at, and returns how many went.
    pub fn collect_garbage(&self) -> Result<usize> {
        self.run(
            "DELETE FROM versions
             WHERE id NOT IN (SELECT version FROM chunks)
               AND id NOT IN (SELECT version FROM divergences)",
            "collecting unreferenced versions",
            [],
        )
    }

    // No caller reads this yet. `/witchlight status` answers from the mod's
    // own state and never asks the service. It is kept because it is the read
    // side of a store that has a write side, and the tests exercise it.
    #[allow(dead_code)]
    /// Returns how many chunks, versions and divergences are held, for the log
    /// and for `witchlight status`.
    pub fn counts(&self) -> Result<Counts> {
        let connection = self.lock();
        let count = |table: &str| -> Result<usize> {
            connection
                .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| row.get::<_, i64>(0))
                .map(|n| n as usize)
                .map_err(|error| Error::database(format!("counting {table}"), error))
        };
        Ok(Counts {
            chunks: count("chunks")?,
            versions: count("versions")?,
            divergences: count("divergences")?,
        })
    }
}
