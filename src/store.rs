//! The map's own database: every chunk the service holds, every version of a
//! chunk somebody still remembers, and what each person has seen.
//!
//! One SQLite file beside the map. It replaces two things that used to be
//! written: the mod's region files, which were the terrain, and this service's
//! own whole-world snapshot, which was rewritten in full on a timer whether or
//! not anything had moved. A chunk here is written when it changes and never
//! otherwise, and the file is updated in place, so a quiet server touches the
//! disk for nothing.
//!
//! Nothing here knows what a column means. A record is the bytes the mod sends —
//! `edge * edge` entries of six bytes, see [`crate::columns`] — kept deflated
//! and compared inflated, because two deflate streams of one record are not
//! bytes anyone should compare.
//!
//! Every write is one transaction, so a crash leaves the previous state rather
//! than half of the next one. The connection is behind a lock; the callers are
//! the terrain listener and the renderer, and neither holds it for longer than
//! one statement's worth of work.
//!
//! What the tables mean:
//!
//! - `chunks` is the current map: which version each chunk is at, and its season.
//! - `regions` is when the ground in each region last changed, which is what
//!   decides whether the stored zoom levels above it are behind.
//! - `versions` is every record anybody still points at, current or remembered.
//!   A version nothing points at is deleted by [`Store::collect_garbage`].
//! - `discovered` is which chunks each person has seen, one bit per chunk in a
//!   region, so a whole region's answer is thirty-two bytes.
//! - `divergences` is where a person's memory disagrees with the map: a chunk
//!   they saw that changed while they were not there, and the version they saw.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use rusqlite::{Connection, OptionalExtension as _, params};

use crate::columns::{REGION_CHUNKS, pack, unpack};
use crate::error::{Error, Result};

/// The schema this build writes. A file at another number is a file another
/// build wrote, and refusing it is the honest answer until there is a migration
/// to run.
const SCHEMA: i64 = 3;

/// Chunks in a region, which is how many bits a `discovered` row holds.
const BITS: usize = (REGION_CHUNKS * REGION_CHUNKS) as usize;

/// Bytes in a `discovered` row.
pub const BITSET_BYTES: usize = BITS / 8;

/// Where the database lives, beside the map like everything else this service
/// writes for its own use.
#[must_use]
pub fn path_in(exports: &Path) -> PathBuf {
    exports.join("map.sqlite")
}

/// A version of one chunk, by the row that holds it.
pub type Version = i64;

/// One chunk as it is stored: where, what season, and which version is current.
pub struct Held {
    pub cx: i32,
    pub cz: i32,
    pub season: u8,
    /// The record, inflated: `edge * edge` entries of six bytes.
    pub record: Vec<u8>,
}

/// One chunk as it arrives to be stored.
pub struct Arrived {
    pub cx: i32,
    pub cz: i32,
    pub season: u8,
    /// The record, inflated.
    pub record: Vec<u8>,
}

/// What storing one chunk did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stored {
    pub cx: i32,
    pub cz: i32,
    /// The version the chunk was at before, or nothing where the map had never
    /// held this chunk.
    pub was: Option<Version>,
    /// The version it is at now. Equal to `was` where only the season moved,
    /// which is a change to the map's colours and not to anybody's memory.
    pub now: Version,
}

impl Stored {
    /// Whether the ground itself changed, which is what a memory is about.
    #[must_use]
    pub fn surface_moved(&self) -> bool {
        self.was != Some(self.now)
    }
}

/// One person's discovered chunks in one region.
/// One browser's login, as the database keeps it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Session {
    /// The word the cookie carries.
    pub word: String,
    pub uid: String,
    pub name: String,
    /// When the browser was last seen, to the second.
    pub seen: SystemTime,
}

/// The sessions table, said once for the fresh schema and the upgrade to it.
const SESSIONS_TABLE: &str = "CREATE TABLE sessions (
                             word TEXT PRIMARY KEY,
                             uid TEXT NOT NULL,
                             name TEXT NOT NULL,
                             seen INTEGER NOT NULL
                         ) WITHOUT ROWID;";

/// What the service must still have when the mod is off, said once for the
/// fresh schema and the upgrade to it: the last markers posted, as one row
/// that is replaced when a post differs; everybody's choices, one row each,
/// replaced only for the person who changed theirs; and the chunks players
/// stood in lately, one row each, so a visit costs the rows it moved rather
/// than a file.
const KEPT_TABLES: &str = "CREATE TABLE markers (
                             one INTEGER PRIMARY KEY CHECK (one = 1),
                             body TEXT NOT NULL
                         ) WITHOUT ROWID;
                         CREATE TABLE preferences (
                             uid TEXT PRIMARY KEY,
                             body TEXT NOT NULL
                         ) WITHOUT ROWID;
                         CREATE TABLE visited (
                             cx INTEGER NOT NULL,
                             cz INTEGER NOT NULL,
                             radius INTEGER NOT NULL,
                             at INTEGER NOT NULL,
                             PRIMARY KEY (cx, cz)
                         ) WITHOUT ROWID;";

pub struct Discovered {
    pub uid: String,
    pub rx: i32,
    pub rz: i32,
    pub bits: [u8; BITSET_BYTES],
}

/// One place a person's memory disagrees with the map.
pub struct Divergence {
    pub uid: String,
    pub cx: i32,
    pub cz: i32,
    pub version: Version,
}

pub struct Store {
    connection: Mutex<Connection>,
}

impl Store {
    /// Opens the database, making it where there is none.
    pub fn open(exports: &Path) -> Result<Self> {
        let path = path_in(exports);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|error| Error::io(format!("making {}", parent.display()), error))?;
        }

        let connection = Connection::open(&path)
            .map_err(|error| Error::database(format!("opening {}", path.display()), error))?;

        // Write-ahead logging is what lets the renderer read while the listener
        // writes, and `NORMAL` is the durability a map wants: a crash loses at
        // most the last transaction, never the file.
        connection
            .execute_batch(
                "PRAGMA journal_mode = WAL;
                 PRAGMA synchronous = NORMAL;
                 PRAGMA foreign_keys = ON;",
            )
            .map_err(|error| Error::database("setting up the database", error))?;

        let store = Self { connection: Mutex::new(connection) };
        store.migrate(&path)?;
        Ok(store)
    }

    /// A database in memory, for a test that wants the real thing and no file.
    #[cfg(test)]
    pub fn in_memory() -> Self {
        let connection = Connection::open_in_memory().expect("an in-memory database");
        connection.execute_batch("PRAGMA foreign_keys = ON;").expect("pragmas");
        let store = Self { connection: Mutex::new(connection) };
        store.migrate(Path::new(":memory:")).expect("a fresh schema");
        store
    }

    fn migrate(&self, path: &Path) -> Result<()> {
        let connection = self.lock();
        let version: i64 = connection
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .map_err(|error| Error::database("reading the schema version", error))?;

        match version {
            0 => {
                connection
                    .execute_batch(&format!(
                        "CREATE TABLE chunks (
                             cx INTEGER NOT NULL,
                             cz INTEGER NOT NULL,
                             season INTEGER NOT NULL DEFAULT 0,
                             version INTEGER NOT NULL REFERENCES versions(id),
                             PRIMARY KEY (cx, cz)
                         ) WITHOUT ROWID;
                         CREATE TABLE versions (
                             id INTEGER PRIMARY KEY,
                             cx INTEGER NOT NULL,
                             cz INTEGER NOT NULL,
                             crc INTEGER NOT NULL,
                             record BLOB NOT NULL
                         );
                         CREATE INDEX versions_by_chunk ON versions (cx, cz, crc);
                         CREATE TABLE discovered (
                             uid TEXT NOT NULL,
                             rx INTEGER NOT NULL,
                             rz INTEGER NOT NULL,
                             bits BLOB NOT NULL,
                             PRIMARY KEY (uid, rx, rz)
                         ) WITHOUT ROWID;
                         CREATE TABLE divergences (
                             uid TEXT NOT NULL,
                             cx INTEGER NOT NULL,
                             cz INTEGER NOT NULL,
                             version INTEGER NOT NULL REFERENCES versions(id),
                             PRIMARY KEY (uid, cx, cz)
                         ) WITHOUT ROWID;
                         CREATE INDEX divergences_by_version ON divergences (version);
                         CREATE TABLE regions (
                             rx INTEGER NOT NULL,
                             rz INTEGER NOT NULL,
                             changed INTEGER NOT NULL,
                             PRIMARY KEY (rx, rz)
                         ) WITHOUT ROWID;
                         CREATE TABLE facts (
                             name TEXT PRIMARY KEY,
                             value INTEGER NOT NULL
                         ) WITHOUT ROWID;
                         {SESSIONS_TABLE}
                         {KEPT_TABLES}
                         PRAGMA user_version = {SCHEMA};"
                    ))
                    .map_err(|error| Error::database("creating the schema", error))?;
                Ok(())
            }
            // Schema 1 is schema 2 without the sessions, and schema 2 is schema
            // 3 without what used to be three files beside the map: each is
            // carried forward with everything it holds.
            1 => {
                connection
                    .execute_batch(&format!(
                        "{SESSIONS_TABLE} {KEPT_TABLES} PRAGMA user_version = {SCHEMA};"
                    ))
                    .map_err(|error| Error::database("adding the sessions and kept tables", error))?;
                Ok(())
            }
            2 => {
                connection
                    .execute_batch(&format!("{KEPT_TABLES} PRAGMA user_version = {SCHEMA};"))
                    .map_err(|error| Error::database("adding the kept tables", error))?;
                Ok(())
            }
            SCHEMA => Ok(()),
            other => Err(Error::Parse {
                path: path.to_path_buf(),
                message: format!(
                    "database schema {other} is not one this build reads (it writes {SCHEMA}) — \
                     this is a newer or older witchlight's file"
                ),
            }),
        }
    }

    /// Every browser still logged in.
    pub fn sessions(&self) -> Result<Vec<Session>> {
        let connection = self.lock();
        let mut statement = connection
            .prepare("SELECT word, uid, name, seen FROM sessions")
            .map_err(|error| Error::database("reading the sessions", error))?;
        let rows = statement
            .query_map([], |row| {
                Ok(Session {
                    word: row.get(0)?,
                    uid: row.get(1)?,
                    name: row.get(2)?,
                    seen: UNIX_EPOCH + Duration::from_secs(row.get::<_, i64>(3)?.max(0) as u64),
                })
            })
            .map_err(|error| Error::database("reading the sessions", error))?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|error| Error::database("reading a session", error))
    }

    /// Records one browser's login, whole.
    pub fn put_session(&self, session: &Session) -> Result<()> {
        self.lock()
            .execute(
                "INSERT INTO sessions (word, uid, name, seen) VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT (word) DO UPDATE SET uid = excluded.uid, name = excluded.name, seen = excluded.seen",
                params![session.word, session.uid, session.name, seconds(session.seen)],
            )
            .map_err(|error| Error::database("recording a login", error))?;
        Ok(())
    }

    /// Moves one browser's last-seen time forward.
    pub fn touch_session(&self, word: &str, seen: SystemTime) -> Result<()> {
        self.lock()
            .execute("UPDATE sessions SET seen = ?2 WHERE word = ?1", params![word, seconds(seen)])
            .map_err(|error| Error::database("noting a login was used", error))?;
        Ok(())
    }

    /// Forgets one browser.
    pub fn delete_session(&self, word: &str) -> Result<()> {
        self.lock()
            .execute("DELETE FROM sessions WHERE word = ?1", params![word])
            .map_err(|error| Error::database("forgetting a login", error))?;
        Ok(())
    }

    /// Forgets every browser. Answers how many there were.
    pub fn clear_sessions(&self) -> Result<usize> {
        self.lock()
            .execute("DELETE FROM sessions", [])
            .map_err(|error| Error::database("forgetting every login", error))
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.connection.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// The last markers the mod posted, as the text that arrived, or nothing
    /// where no post has been kept.
    pub fn markers(&self) -> Result<Option<String>> {
        self.lock()
            .query_row("SELECT body FROM markers WHERE one = 1", [], |row| row.get(0))
            .optional()
            .map_err(|error| Error::database("reading the kept markers", error))
    }

    /// Keeps a marker post, whole, in place of the last one.
    pub fn put_markers(&self, body: &str) -> Result<()> {
        self.lock()
            .execute(
                "INSERT INTO markers (one, body) VALUES (1, ?1)
                 ON CONFLICT (one) DO UPDATE SET body = excluded.body",
                params![body],
            )
            .map_err(|error| Error::database("keeping the markers", error))?;
        Ok(())
    }

    /// Everybody's choices, as the text each was kept as, by uid.
    pub fn preferences(&self) -> Result<Vec<(String, String)>> {
        let connection = self.lock();
        let mut statement = connection
            .prepare("SELECT uid, body FROM preferences")
            .map_err(|error| Error::database("reading the preferences", error))?;
        let rows = statement
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .map_err(|error| Error::database("reading the preferences", error))?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|error| Error::database("reading somebody's preferences", error))
    }

    /// Keeps one person's choices, whole, in place of what they had.
    pub fn put_preferences(&self, uid: &str, body: &str) -> Result<()> {
        self.lock()
            .execute(
                "INSERT INTO preferences (uid, body) VALUES (?1, ?2)
                 ON CONFLICT (uid) DO UPDATE SET body = excluded.body",
                params![uid, body],
            )
            .map_err(|error| Error::database("keeping somebody's preferences", error))?;
        Ok(())
    }

    /// Every chunk somebody stood in lately: the chunk, how far was seen from
    /// it, and when, in seconds since the epoch.
    pub fn visited(&self) -> Result<Vec<(i32, i32, i32, u64)>> {
        let connection = self.lock();
        let mut statement = connection
            .prepare("SELECT cx, cz, radius, at FROM visited")
            .map_err(|error| Error::database("reading the visited chunks", error))?;
        let rows = statement
            .query_map([], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get::<_, i64>(3)?.max(0) as u64))
            })
            .map_err(|error| Error::database("reading the visited chunks", error))?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|error| Error::database("reading a visited chunk", error))
    }

    /// Records the places that moved and forgets the ones let go, in one
    /// transaction, so a visit costs the rows it touched and nothing else.
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

    /// Blocks along a chunk's edge, or zero where nothing has been stored yet.
    pub fn edge(&self) -> Result<usize> {
        let edge: Option<i64> = self
            .lock()
            .query_row("SELECT value FROM facts WHERE name = 'edge'", [], |row| row.get(0))
            .optional()
            .map_err(|error| Error::database("reading the chunk edge", error))?;
        Ok(edge.unwrap_or(0) as usize)
    }

    /// Whether nothing has been stored yet.
    pub fn is_empty(&self) -> Result<bool> {
        let count: i64 = self
            .lock()
            .query_row("SELECT COUNT(*) FROM chunks", [], |row| row.get(0))
            .map_err(|error| Error::database("counting chunks", error))?;
        Ok(count == 0)
    }

    /// Every chunk, for building the world at start.
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

    /// Every chunk the map holds, as the mod wants to hear it at start: where,
    /// what season, and the checksum of its current record — enough for the mod
    /// to know a chunk loading again from one that changed, without a copy of
    /// the ground in its own memory.
    pub fn held(&self) -> Result<Vec<(i32, i32, u32, u8)>> {
        let connection = self.lock();
        let mut statement = connection
            .prepare(
                "SELECT c.cx, c.cz, v.crc, c.season
                 FROM chunks c JOIN versions v ON v.id = c.version",
            )
            .map_err(|error| Error::database("reading what is held", error))?;
        let rows = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, i32>(0)?,
                    row.get::<_, i32>(1)?,
                    row.get::<_, i64>(2)? as u32,
                    row.get::<_, i64>(3)? as u8,
                ))
            })
            .map_err(|error| Error::database("reading what is held", error))?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|error| Error::database("reading a held chunk", error))
    }

    /// Takes chunks as they arrived, in one transaction, and says what each one
    /// did to the map.
    ///
    /// A record equal to the one already current costs a season update at most.
    /// A record that differs becomes a new version — or an old one, where the
    /// same bytes were current before: a block placed and taken away again is
    /// the chunk it was, and a memory of that chunk still points at the row it
    /// pointed at.
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

    /// Moves a chunk's season and nothing else. The ground is as it was, so no
    /// memory is touched; the colours are not, so the tile is.
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

    /// When the ground in each region last changed.
    pub fn region_times(&self) -> Result<HashMap<(i32, i32), SystemTime>> {
        let connection = self.lock();
        let mut statement = connection
            .prepare("SELECT rx, rz, changed FROM regions")
            .map_err(|error| Error::database("reading when regions changed", error))?;
        let rows = statement
            .query_map([], |row| Ok(((row.get::<_, i32>(0)?, row.get::<_, i32>(1)?), row.get::<_, i64>(2)?)))
            .map_err(|error| Error::database("reading when regions changed", error))?;
        let mut times = HashMap::new();
        for row in rows {
            let (at, changed) = row.map_err(|error| Error::database("reading a region's time", error))?;
            times.insert(at, UNIX_EPOCH + Duration::from_secs(changed.max(0) as u64));
        }
        Ok(times)
    }

    /// One version's record, inflated, or nothing where that row has gone.
    pub fn version(&self, version: Version) -> Result<Option<Vec<u8>>> {
        let packed: Option<Vec<u8>> = self
            .lock()
            .query_row("SELECT record FROM versions WHERE id = ?1", params![version], |row| row.get(0))
            .optional()
            .map_err(|error| Error::database("reading a version", error))?;
        packed.map(|packed| inflate(&packed)).transpose()
    }

    /// Everything everybody has discovered.
    pub fn discovered(&self) -> Result<Vec<Discovered>> {
        let connection = self.lock();
        let mut statement = connection
            .prepare("SELECT uid, rx, rz, bits FROM discovered")
            .map_err(|error| Error::database("reading what was discovered", error))?;
        let rows = statement
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, i32>(1)?, row.get::<_, i32>(2)?, row.get::<_, Vec<u8>>(3)?))
            })
            .map_err(|error| Error::database("reading what was discovered", error))?;

        let mut held = Vec::new();
        for row in rows {
            let (uid, rx, rz, bits) = row.map_err(|error| Error::database("reading a discovery", error))?;
            let mut fixed = [0u8; BITSET_BYTES];
            let taken = bits.len().min(BITSET_BYTES);
            fixed[..taken].copy_from_slice(&bits[..taken]);
            held.push(Discovered { uid, rx, rz, bits: fixed });
        }
        Ok(held)
    }

    /// Records one person's discovered chunks in one region, whole.
    pub fn set_discovered(&self, uid: &str, rx: i32, rz: i32, bits: &[u8; BITSET_BYTES]) -> Result<()> {
        self.lock()
            .execute(
                "INSERT INTO discovered (uid, rx, rz, bits) VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT (uid, rx, rz) DO UPDATE SET bits = excluded.bits",
                params![uid, rx, rz, &bits[..]],
            )
            .map_err(|error| Error::database("recording a discovery", error))?;
        Ok(())
    }

    /// Every place anybody's memory disagrees with the map.
    pub fn divergences(&self) -> Result<Vec<Divergence>> {
        let connection = self.lock();
        let mut statement = connection
            .prepare("SELECT uid, cx, cz, version FROM divergences")
            .map_err(|error| Error::database("reading the divergences", error))?;
        let rows = statement
            .query_map([], |row| {
                Ok(Divergence {
                    uid: row.get(0)?,
                    cx: row.get(1)?,
                    cz: row.get(2)?,
                    version: row.get(3)?,
                })
            })
            .map_err(|error| Error::database("reading the divergences", error))?;

        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|error| Error::database("reading a divergence", error))
    }

    /// Records that this person remembers a chunk at a version the map has moved
    /// on from — several at once, in one transaction, because one chunk changing
    /// under many absent people is the common shape.
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

    /// Forgets a person's memory of chunks they have seen again.
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

    /// Deletes every version nothing points at any more. Says how many went.
    pub fn collect_garbage(&self) -> Result<usize> {
        self.lock()
            .execute(
                "DELETE FROM versions
                 WHERE id NOT IN (SELECT version FROM chunks)
                   AND id NOT IN (SELECT version FROM divergences)",
                [],
            )
            .map_err(|error| Error::database("collecting unreferenced versions", error))
    }

    /// How many chunks, versions and divergences are held — for the log and for
    /// `witchlight status`.
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

/// What the database holds, by count.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Counts {
    pub chunks: usize,
    pub versions: usize,
    pub divergences: usize,
}

/// Stores one chunk inside a transaction that is already open.
fn put_one(transaction: &rusqlite::Transaction<'_>, chunk: &Arrived) -> Result<Stored> {
    let current: Option<Version> = transaction
        .query_row(
            "SELECT version FROM chunks WHERE cx = ?1 AND cz = ?2",
            params![chunk.cx, chunk.cz],
            |row| row.get(0),
        )
        .optional()
        .map_err(|error| Error::database("reading a chunk's version", error))?;

    let crc = crc_of(&chunk.record);

    // The same bytes may already be a version of this chunk: the current one,
    // where only the season moved, or an older one somebody remembers. Either
    // way the row is reused rather than written again.
    let mut same: Option<Version> = None;
    {
        let mut statement = transaction
            .prepare_cached("SELECT id, record FROM versions WHERE cx = ?1 AND cz = ?2 AND crc = ?3")
            .map_err(|error| Error::database("looking for a matching version", error))?;
        let rows = statement
            .query_map(params![chunk.cx, chunk.cz, crc], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, Vec<u8>>(1)?))
            })
            .map_err(|error| Error::database("looking for a matching version", error))?;
        for row in rows {
            let (id, packed) = row.map_err(|error| Error::database("reading a version", error))?;
            if inflate(&packed)? == chunk.record {
                same = Some(id);
                break;
            }
        }
    }

    let now = match same {
        Some(id) => id,
        None => {
            transaction
                .execute(
                    "INSERT INTO versions (cx, cz, crc, record) VALUES (?1, ?2, ?3, ?4)",
                    params![chunk.cx, chunk.cz, crc, pack(&chunk.record)],
                )
                .map_err(|error| Error::database("storing a version", error))?;
            transaction.last_insert_rowid()
        }
    };

    transaction
        .execute(
            "INSERT INTO chunks (cx, cz, season, version) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT (cx, cz) DO UPDATE SET season = excluded.season, version = excluded.version",
            params![chunk.cx, chunk.cz, i64::from(chunk.season), now],
        )
        .map_err(|error| Error::database("storing a chunk", error))?;

    Ok(Stored { cx: chunk.cx, cz: chunk.cz, was: current, now })
}

fn crc_of(record: &[u8]) -> i64 {
    let mut crc = flate2::Crc::new();
    crc.update(record);
    i64::from(crc.sum())
}

fn inflate(packed: &[u8]) -> Result<Vec<u8>> {
    unpack(packed).ok_or_else(|| Error::Database {
        doing: "inflating a stored record".to_owned(),
        message: "not a deflate stream".to_owned(),
    })
}

/// A moment as the database keeps one: whole seconds since the epoch.
fn seconds(at: SystemTime) -> i64 {
    at.duration_since(UNIX_EPOCH).map_or(0, |since| since.as_secs() as i64)
}

/// Which chunks of a region a bitset names, as the slot each bit stands for.
///
/// The slot is the chunk's position in the region — `dz * REGION_CHUNKS + dx` —
/// the same arithmetic the region format files a chunk under, so a bit and a
/// slot are one number.
#[must_use]
pub fn slot_of(cx: i32, cz: i32) -> usize {
    (cz.rem_euclid(REGION_CHUNKS) * REGION_CHUNKS + cx.rem_euclid(REGION_CHUNKS)) as usize
}

/// Which region a chunk is in.
#[must_use]
pub fn region_of(cx: i32, cz: i32) -> (i32, i32) {
    (cx.div_euclid(REGION_CHUNKS), cz.div_euclid(REGION_CHUNKS))
}

/// Whether a bit is set.
#[must_use]
pub fn bit(bits: &[u8; BITSET_BYTES], slot: usize) -> bool {
    bits[slot / 8] & (1 << (slot % 8)) != 0
}

/// Sets a bit, and says whether it was clear before.
pub fn set_bit(bits: &mut [u8; BITSET_BYTES], slot: usize) -> bool {
    let was = bit(bits, slot);
    bits[slot / 8] |= 1 << (slot % 8);
    !was
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_the_service_must_still_have_is_kept_by_row() {
        let store = Store::in_memory();

        assert_eq!(store.markers().unwrap(), None, "nothing posted yet");
        store.put_markers("{\"Public\":[]}").unwrap();
        store.put_markers("{\"Public\":[1]}").unwrap();
        assert_eq!(store.markers().unwrap().as_deref(), Some("{\"Public\":[1]}"), "one row, the last post");

        store.put_preferences("ada", "{\"a\":1}").unwrap();
        store.put_preferences("bob", "{\"b\":1}").unwrap();
        store.put_preferences("ada", "{\"a\":2}").unwrap();
        let mut people = store.preferences().unwrap();
        people.sort();
        assert_eq!(
            people,
            vec![("ada".to_owned(), "{\"a\":2}".to_owned()), ("bob".to_owned(), "{\"b\":1}".to_owned())],
            "one row per person, replaced in place"
        );

        store.put_visited(&[(1, 2, 8, 100), (3, 4, 8, 100)], &[]).unwrap();
        store.put_visited(&[(1, 2, 12, 200)], &[(3, 4)]).unwrap();
        assert_eq!(store.visited().unwrap(), vec![(1, 2, 12, 200)], "moved rows replaced, gone rows deleted");
    }

    #[test]
    fn a_schema_two_database_gains_the_kept_tables() {
        let connection = Connection::open_in_memory().expect("an in-memory database");
        connection
            .execute_batch(&format!("{SESSIONS_TABLE} PRAGMA user_version = 2;"))
            .expect("a schema 2 database");
        let store = Store { connection: Mutex::new(connection) };
        store.migrate(Path::new(":memory:")).expect("carried forward");
        store.put_markers("[]").unwrap();
        let version: i64 = store.lock().query_row("PRAGMA user_version", [], |row| row.get(0)).unwrap();
        assert_eq!(version, SCHEMA);
    }

    fn record(edge: usize, block: u16) -> Vec<u8> {
        let mut record = Vec::with_capacity(edge * edge * 6);
        for index in 0..edge * edge {
            record.extend_from_slice(&block.to_le_bytes());
            record.extend_from_slice(&(index as i16).to_le_bytes());
            record.push(80);
            record.push(90);
        }
        record
    }

    fn arrived(cx: i32, cz: i32, season: u8, block: u16) -> Arrived {
        Arrived { cx, cz, season, record: record(4, block) }
    }

    #[test]
    fn a_chunk_stored_reads_back_as_it_was() {
        let store = Store::in_memory();
        assert!(store.is_empty().unwrap());

        let stored = store.put_chunks(4, &[arrived(2, -3, 7, 11)], SystemTime::now()).unwrap();
        assert_eq!(stored.len(), 1);
        assert_eq!(stored[0].was, None, "the map had never held it");
        assert!(stored[0].surface_moved());

        let held = store.chunks().unwrap();
        assert_eq!(held.len(), 1);
        assert_eq!((held[0].cx, held[0].cz, held[0].season), (2, -3, 7));

        // What the mod is told at start: the checksum is CRC-32 over the raw
        // record, which is the one the mod's own `Crc32.Of` computes.
        let summary = store.held().unwrap();
        assert_eq!(summary.len(), 1);
        assert_eq!((summary[0].0, summary[0].1, summary[0].3), (2, -3, 7));
        assert_eq!(i64::from(summary[0].2), crc_of(&record(4, 11)));
        assert_eq!(held[0].record, record(4, 11));
        assert_eq!(store.edge().unwrap(), 4);
    }

    #[test]
    fn the_same_record_again_is_not_a_new_version() {
        let store = Store::in_memory();
        let first = store.put_chunks(4, &[arrived(0, 0, 1, 11)], SystemTime::now()).unwrap()[0];
        let again = store.put_chunks(4, &[arrived(0, 0, 2, 11)], SystemTime::now()).unwrap()[0];

        assert_eq!(again.was, Some(first.now));
        assert_eq!(again.now, first.now, "the season moved and nothing else");
        assert!(!again.surface_moved());
        assert_eq!(store.chunks().unwrap()[0].season, 2, "but the season did move");
        assert_eq!(store.counts().unwrap().versions, 1);
    }

    #[test]
    fn a_changed_record_is_a_new_version_and_the_old_one_is_reported() {
        let store = Store::in_memory();
        let first = store.put_chunks(4, &[arrived(0, 0, 1, 11)], SystemTime::now()).unwrap()[0];
        let changed = store.put_chunks(4, &[arrived(0, 0, 1, 22)], SystemTime::now()).unwrap()[0];

        assert_eq!(changed.was, Some(first.now));
        assert_ne!(changed.now, first.now);
        assert!(changed.surface_moved());

        // The old version is still there to be remembered, until nothing does.
        assert_eq!(store.version(first.now).unwrap(), Some(record(4, 11)));
        assert_eq!(store.collect_garbage().unwrap(), 1, "nothing pointed at it");
        assert_eq!(store.version(first.now).unwrap(), None);
    }

    #[test]
    fn a_block_placed_and_taken_away_is_the_version_it_was() {
        let store = Store::in_memory();
        let first = store.put_chunks(4, &[arrived(0, 0, 1, 11)], SystemTime::now()).unwrap()[0];
        store.put_chunks(4, &[arrived(0, 0, 1, 22)], SystemTime::now()).unwrap();
        let back = store.put_chunks(4, &[arrived(0, 0, 1, 11)], SystemTime::now()).unwrap()[0];

        assert_eq!(back.now, first.now, "the same bytes are the same row");
        assert_eq!(store.counts().unwrap().versions, 2);
    }

    #[test]
    fn a_remembered_version_survives_collection_and_goes_when_forgotten() {
        let store = Store::in_memory();
        let first = store.put_chunks(4, &[arrived(0, 0, 1, 11)], SystemTime::now()).unwrap()[0];
        store.put_chunks(4, &[arrived(0, 0, 1, 22)], SystemTime::now()).unwrap();

        store
            .set_divergences(&[Divergence { uid: "ada".into(), cx: 0, cz: 0, version: first.now }])
            .unwrap();
        assert_eq!(store.collect_garbage().unwrap(), 0, "Ada still remembers it");
        assert_eq!(store.divergences().unwrap().len(), 1);

        store.clear_divergences("ada", &[(0, 0)]).unwrap();
        assert_eq!(store.collect_garbage().unwrap(), 1);
        assert!(store.divergences().unwrap().is_empty());
    }

    #[test]
    fn a_region_is_dated_by_its_last_change_of_ground() {
        let store = Store::in_memory();
        let then = UNIX_EPOCH + Duration::from_secs(1_000);
        let later = UNIX_EPOCH + Duration::from_secs(2_000);
        store.put_chunks(4, &[arrived(0, 0, 1, 11)], then).unwrap();
        assert_eq!(store.region_times().unwrap()[&(0, 0)], then);

        // A season alone is not the ground moving.
        store.put_chunks(4, &[arrived(0, 0, 2, 11)], later).unwrap();
        assert_eq!(store.region_times().unwrap()[&(0, 0)], then);
        assert!(store.set_season(0, 0, 3).unwrap());
        assert!(!store.set_season(0, 0, 3).unwrap(), "the same season again is nothing");
        assert_eq!(store.region_times().unwrap()[&(0, 0)], then);

        store.put_chunks(4, &[arrived(0, 0, 2, 22)], later).unwrap();
        assert_eq!(store.region_times().unwrap()[&(0, 0)], later);
    }

    #[test]
    fn what_somebody_discovered_reads_back_whole() {
        let store = Store::in_memory();
        let mut bits = [0u8; BITSET_BYTES];
        assert!(set_bit(&mut bits, slot_of(17, -1)));
        assert!(!set_bit(&mut bits, slot_of(17, -1)), "setting it again changes nothing");

        store.set_discovered("ada", 1, -1, &bits).unwrap();
        let held = store.discovered().unwrap();
        assert_eq!(held.len(), 1);
        assert_eq!((held[0].uid.as_str(), held[0].rx, held[0].rz), ("ada", 1, -1));
        assert!(bit(&held[0].bits, slot_of(17, -1)));
        assert!(!bit(&held[0].bits, slot_of(16, -1)));
    }

    #[test]
    fn a_bit_is_the_slot_the_region_format_files_a_chunk_under() {
        // The corner of region (1, -1) is chunk (16, -16), slot 0; one along and
        // one down is slot 17 — the same arithmetic `columns::chunk_at` inverts.
        assert_eq!(region_of(16, -16), (1, -1));
        assert_eq!(slot_of(16, -16), 0);
        assert_eq!(slot_of(17, -15), 17);
        assert_eq!(slot_of(31, -1), 255);

    }
}
