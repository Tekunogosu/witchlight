//! The ground itself, in the map's own database.
//!
//! This is the map: which chunks there are, which version of a record each one
//! is at, its season, and when each region last changed. A chunk is written only
//! when its record differs from the one stored, so a quiet server writes
//! nothing.
//!
//! Tables: `chunks`, `versions`, `regions`, `visited`, `facts`.

use std::collections::HashMap;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use rusqlite::{OptionalExtension as _, params};

use crate::util::error::{Error, Result};

use super::{Arrived, Held, Store, Stored, Version, inflate, put_one, region_of, seconds};

impl Store {
    /// Returns every chunk somebody stood in lately, as the chunk position, how
    /// far was seen from it, and when, in seconds since the epoch.
    pub fn visited(&self) -> Result<Vec<(i32, i32, i32, u64)>> {
        self.rows(
            "SELECT cx, cz, radius, at FROM visited",
            "reading the visited chunks",
            [],
            |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get::<_, i64>(3)?.max(0) as u64))
            },
        )
    }

    /// Records the places that moved and deletes the ones released, in one
    /// transaction, so a visit costs only the rows it touched.
    pub fn put_visited(&self, stood: &[(i32, i32, i32, u64)], gone: &[(i32, i32)]) -> Result<()> {
        let mut connection = self.lock();
        let transaction = connection
            .transaction()
            .map_err(|error| Error::database("recording where players stood", error))?;
        for &(cx, cz, radius, at) in stood {
            transaction
                .execute(
                    "INSERT INTO visited (cx, cz, radius, at) VALUES (?1, ?2, ?3, ?4)
                     ON CONFLICT (cx, cz) DO UPDATE SET radius = excluded.radius, at = excluded.at",
                    params![cx, cz, radius, at as i64],
                )
                .map_err(|error| Error::database("recording where a player stood", error))?;
        }
        for &(cx, cz) in gone {
            transaction
                .execute("DELETE FROM visited WHERE cx = ?1 AND cz = ?2", params![cx, cz])
                .map_err(|error| Error::database("forgetting where a player stood", error))?;
        }
        transaction
            .commit()
            .map_err(|error| Error::database("recording where players stood", error))
    }

    /// Returns the number of blocks along a chunk's edge, or zero when nothing
    /// has been stored yet.
    pub fn edge(&self) -> Result<usize> {
        let edge: Option<i64> = self
            .lock()
            .query_row("SELECT value FROM facts WHERE name = 'edge'", [], |row| row.get(0))
            .optional()
            .map_err(|error| Error::database("reading the chunk edge", error))?;
        Ok(edge.unwrap_or(0) as usize)
    }

    /// Reports whether nothing has been stored yet.
    pub fn is_empty(&self) -> Result<bool> {
        let count: i64 = self
            .lock()
            .query_row("SELECT COUNT(*) FROM chunks", [], |row| row.get(0))
            .map_err(|error| Error::database("counting chunks", error))?;
        Ok(count == 0)
    }

    /// Returns every chunk, for building the world at startup.
    pub fn chunks(&self) -> Result<Vec<Held>> {
        let connection = self.lock();
        let mut statement = connection
            .prepare(
                "SELECT c.cx, c.cz, c.season, v.record
                 FROM chunks c JOIN versions v ON v.id = c.version",
            )
            .map_err(|error| Error::database("reading the chunks", error))?;

        let rows = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, i32>(0)?,
                    row.get::<_, i32>(1)?,
                    row.get::<_, i64>(2)? as u8,
                    row.get::<_, Vec<u8>>(3)?,
                ))
            })
            .map_err(|error| Error::database("reading the chunks", error))?;

        let mut held = Vec::new();
        for row in rows {
            let (cx, cz, season, packed) =
                row.map_err(|error| Error::database("reading a chunk", error))?;
            held.push(Held { cx, cz, season, record: inflate(&packed)? });
        }
        Ok(held)
    }

    /// Returns every chunk the map holds, in the form the mod reads at startup:
    /// position, season, and the checksum of the current record. That is enough
    /// for the mod to tell a chunk loading again from one that changed, without
    /// keeping a copy of the ground in its own memory.
    pub fn held(&self) -> Result<Vec<(i32, i32, u32, u8)>> {
        self.rows(
            "SELECT c.cx, c.cz, v.crc, c.season
             FROM chunks c JOIN versions v ON v.id = c.version",
            "reading what is held",
            [],
            |row| {
                Ok((
                    row.get::<_, i32>(0)?,
                    row.get::<_, i32>(1)?,
                    row.get::<_, i64>(2)? as u32,
                    row.get::<_, i64>(3)? as u8,
                ))
            },
        )
    }

    /// Stores chunks as they arrived, in one transaction, and reports what each
    /// did to the map.
    ///
    /// A record equal to the current one costs a season update at most. A record
    /// that differs becomes a new version, or reuses an old one when those exact
    /// bytes were current before. A block placed and removed again returns the
    /// chunk to the version it had, and a memory of that chunk still points at
    /// the same row.
    pub fn put_chunks(&self, edge: usize, arrived: &[Arrived], at: SystemTime) -> Result<Vec<Stored>> {
        let at = seconds(at);
        let mut connection = self.lock();
        let transaction = connection
            .transaction()
            .map_err(|error| Error::database("beginning a write", error))?;

        transaction
            .execute(
                "INSERT INTO facts (name, value) VALUES ('edge', ?1)
                 ON CONFLICT (name) DO UPDATE SET value = excluded.value",
                params![edge as i64],
            )
            .map_err(|error| Error::database("recording the chunk edge", error))?;

        let mut stored = Vec::with_capacity(arrived.len());
        for chunk in arrived {
            let one = put_one(&transaction, chunk)?;
            if one.surface_moved() {
                let (rx, rz) = region_of(one.cx, one.cz);
                transaction
                    .execute(
                        "INSERT INTO regions (rx, rz, changed) VALUES (?1, ?2, ?3)
                         ON CONFLICT (rx, rz) DO UPDATE SET changed = excluded.changed",
                        params![rx, rz, at],
                    )
                    .map_err(|error| Error::database("recording when a region changed", error))?;
            }
            stored.push(one);
        }

        transaction.commit().map_err(|error| Error::database("committing a write", error))?;
        Ok(stored)
    }

    /// Changes a chunk's season and nothing else. The ground is unchanged, so no
    /// memory is touched, but the colours change, so the tile is redrawn.
    pub fn set_season(&self, cx: i32, cz: i32, season: u8) -> Result<bool> {
        let changed = self
            .lock()
            .execute(
                "UPDATE chunks SET season = ?3 WHERE cx = ?1 AND cz = ?2 AND season != ?3",
                params![cx, cz, i64::from(season)],
            )
            .map_err(|error| Error::database("moving a chunk's season", error))?;
        Ok(changed > 0)
    }

    /// Returns when the ground in each region last changed.
    pub fn region_times(&self) -> Result<HashMap<(i32, i32), SystemTime>> {
        let read = self.rows(
            "SELECT rx, rz, changed FROM regions",
            "reading when regions changed",
            [],
            |row| {
                Ok((
                    (row.get::<_, i32>(0)?, row.get::<_, i32>(1)?),
                    UNIX_EPOCH + Duration::from_secs(row.get::<_, i64>(2)?.max(0) as u64),
                ))
            },
        )?;
        Ok(read.into_iter().collect())
    }

    /// Returns one version's record, inflated, or `None` when that row is gone.
    pub fn version(&self, version: Version) -> Result<Option<Vec<u8>>> {
        let packed: Option<Vec<u8>> = self
            .lock()
            .query_row("SELECT record FROM versions WHERE id = ?1", params![version], |row| row.get(0))
            .optional()
            .map_err(|error| Error::database("reading a version", error))?;
        packed.map(|packed| inflate(&packed)).transpose()
    }
}
