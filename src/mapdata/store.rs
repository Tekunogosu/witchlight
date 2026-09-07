//! Stores the map in one SQLite file beside the exports.
//!
//! The database holds every chunk the service has, every version of a chunk
//! somebody still remembers, and what each person has seen. A chunk is written
//! only when it changes, and the file is updated in place, so a quiet server
//! writes nothing.
//!
//! Nothing in this module interprets a column. A record is the bytes the mod
//! sends, `edge * edge` entries of six bytes each. See
//! [`crate::render::columns`]. Records are stored deflated and compared
//! inflated, because two deflate streams of one record need not be equal byte
//! for byte.
//!
//! Every write is one transaction, so a crash leaves the previous state rather
//! than half of the next one. The connection sits behind a lock. Its callers are
//! the terrain listener and the renderer, and neither holds it longer than one
//! statement.
//!
//! The tables:
//!
//! - `chunks` is the current map. It holds which version each chunk is at and
//!   its season.
//! - `regions` records when the ground in each region last changed, which
//!   decides whether the stored zoom levels above it are behind.
//! - `versions` holds every record anybody still points at, current or
//!   remembered. [`Store::collect_garbage`] deletes a version nothing points at.
//! - `discovered` holds which chunks each person has seen, one bit per chunk in
//!   a region, so a whole region fits in thirty-two bytes.
//! - `divergences` records where a person's memory disagrees with the map: a
//!   chunk they saw that changed while they were away, and the version they saw.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use rusqlite::{Connection, OptionalExtension as _, params};

use crate::render::columns::{pack, unpack};
use crate::util::error::{Error, Result};
use crate::util::log::say;

pub use crate::render::columns::{BITSET_BYTES, bit, region_of, set_bit, slot_of};

/// The schema version this build writes. A file at a higher number was written
/// by a newer build, and opening it is refused.
const SCHEMA: i64 = 5;

/// Returns the path of the database, which sits beside the map.
#[must_use]
pub fn path_in(exports: &Path) -> PathBuf {
    exports.join("map.sqlite")
}

/// Identifies one version of one chunk by the row that holds it.
pub type Version = i64;

/// Holds one chunk as it is stored, with its position, season and record.
pub struct Held {
    pub cx: i32,
    pub cz: i32,
    pub season: u8,
    /// The inflated record, `edge * edge` entries of six bytes.
    pub record: Vec<u8>,
}

/// Holds one chunk as it arrives to be stored.
pub struct Arrived {
    pub cx: i32,
    pub cz: i32,
    pub season: u8,
    /// The record, inflated.
    pub record: Vec<u8>,
}

/// Reports what storing one chunk did to the map.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stored {
    pub cx: i32,
    pub cz: i32,
    /// The version the chunk was at before, or `None` when the map had never
    /// held this chunk.
    pub was: Option<Version>,
    /// The version the chunk is at now. It equals `was` when only the season
    /// moved, which changes the map's colours but nobody's memory.
    pub now: Version,
}

impl Stored {
    /// Reports whether the ground itself changed, as opposed to the season.
    #[must_use]
    pub fn surface_moved(&self) -> bool {
        self.was != Some(self.now)
    }
}

/// Holds one browser's login, as the database keeps it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Session {
    /// The token the cookie carries.
    pub word: String,
    pub uid: String,
    pub name: String,
    /// When the browser was last seen, rounded to the second.
    pub seen: SystemTime,
}

/// Defines the sessions table. Used by both the fresh schema and the migration
/// that adds it.
const SESSIONS_TABLE: &str = "CREATE TABLE sessions (
                             word TEXT PRIMARY KEY,
                             uid TEXT NOT NULL,
                             name TEXT NOT NULL,
                             seen INTEGER NOT NULL
                         ) WITHOUT ROWID;";

/// Defines the tables the service must still have when the mod is off. Used by
/// both the fresh schema and the migration that adds them.
///
/// `markers` holds the last marker post in one row, replaced when a post
/// differs. `preferences` holds one row per person, replaced only for the person
/// who changed theirs. `visited` holds one row per chunk players stood in
/// lately, so a visit costs the rows it moved rather than a whole file.
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

/// Defines the tables recording which plugins have registered and what shape
/// each last declared.
///
/// These live here rather than in each plugin's own database, so what is
/// installed can be answered without opening every plugin's file. That includes
/// a removed plugin, whose rows are kept and reported rather than dropped.
///
/// `shape` is the fingerprint of the last declaration. It says whether a plugin
/// registering again is the same plugin.
const PLUGINS_TABLE: &str = "CREATE TABLE plugins (
                             id TEXT PRIMARY KEY,
                             shape TEXT NOT NULL,
                             declared TEXT NOT NULL DEFAULT '',
                             enabled INTEGER NOT NULL,
                             at INTEGER NOT NULL
                         ) WITHOUT ROWID;
                         CREATE TABLE plugin_shares (
                             plugin TEXT NOT NULL,
                             uid TEXT NOT NULL,
                             said INTEGER NOT NULL,
                             PRIMARY KEY (plugin, uid, said)
                         ) WITHOUT ROWID;";

/// Holds one person's discovered chunks in one region, as a bitset.
pub struct Discovered {
    pub uid: String,
    pub rx: i32,
    pub rz: i32,
    pub bits: [u8; BITSET_BYTES],
}

/// Records one place where a person's memory disagrees with the map.
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
    /// Opens the database, creating it if there is none.
    pub fn open(exports: &Path) -> Result<Self> {
        let path = path_in(exports);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|error| Error::io(format!("making {}", parent.display()), error))?;
        }

        let connection = Connection::open(&path)
            .map_err(|error| Error::database(format!("opening {}", path.display()), error))?;

        // Write-ahead logging lets the renderer read while the listener writes.
        // `NORMAL` synchronous means a crash loses at most the last
        // transaction, never the file.
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

    /// Opens an in-memory database, for tests that want a real database and no
    /// file.
    #[cfg(test)]
    pub fn in_memory() -> Self {
        let connection = Connection::open_in_memory().expect("an in-memory database");
        connection.execute_batch("PRAGMA foreign_keys = ON;").expect("pragmas");
        let store = Self { connection: Mutex::new(connection) };
        store.migrate(Path::new(":memory:")).expect("a fresh schema");
        store
    }

    /// Copies the database as it stands, before a migration changes it.
    ///
    /// The backup is named for the schema it holds, not the one about to be
    /// written, because someone rolling back is asking which file returns them
    /// to where they were. Several migrations leave several files rather than
    /// overwriting one.
    ///
    /// It uses `VACUUM INTO` rather than a file copy. The database is in WAL
    /// mode, so recent writes live in `map.sqlite-wal` until a checkpoint folds
    /// them in and the bytes of `map.sqlite` alone are not the database.
    /// `VACUUM INTO` writes one consistent file whatever state the log is in.
    ///
    /// A backup that cannot be written stops the migration. Refusing to start is
    /// recoverable; a schema changed with no way back is not.
    fn back_up_before_migrating(&self, path: &Path, from: i64) -> Result<()> {
        let backup = path.with_extension(format!("schema{from}.bak"));

        // An earlier run of this migration already wrote a backup. Keep it. It
        // holds the database as it was when last known good. Overwriting it here
        // would replace that with whatever a half-finished attempt left behind.
        if backup.exists() {
            say!("keeping the schema {from} backup already beside the map");
            return Ok(());
        }

        self.lock()
            .execute("VACUUM INTO ?1", [&backup.to_string_lossy().as_ref()])
            .map_err(|error| Error::database(format!("backing up schema {from} to {}", backup.display()), error))?;

        say!("schema {from} backed up to {}", backup.display());
        Ok(())
    }

    fn migrate(&self, path: &Path) -> Result<()> {
        let version: i64 = self
            .lock()
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .map_err(|error| Error::database("reading the schema version", error))?;

        // Back up before the migration's first write, because a migration that
        // fails halfway has already changed the file. Only back up when the
        // schema is about to move. A database already at this build's number is
        // not written to, and a fresh one at 0 holds nothing worth keeping.
        if version != 0 && version != SCHEMA && !path.starts_with(":memory:") {
            self.back_up_before_migrating(path, version)?;
        }

        let connection = self.lock();

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
                         {PLUGINS_TABLE}
                         PRAGMA user_version = {SCHEMA};"
                    ))
                    .map_err(|error| Error::database("creating the schema", error))?;
                Ok(())
            }
            // Schema 1 lacks the sessions table. Schema 2 lacks the kept
            // tables. Schema 3 lacks the plugin register. Each arm adds what its
            // schema is missing and keeps everything already there.
            1 => {
                connection
                    .execute_batch(&format!(
                        "{SESSIONS_TABLE} {KEPT_TABLES} {PLUGINS_TABLE} PRAGMA user_version = {SCHEMA};"
                    ))
                    .map_err(|error| Error::database("adding the sessions and kept tables", error))?;
                Ok(())
            }
            2 => {
                connection
                    .execute_batch(&format!(
                        "{KEPT_TABLES} {PLUGINS_TABLE} PRAGMA user_version = {SCHEMA};"
                    ))
                    .map_err(|error| Error::database("adding the kept tables", error))?;
                Ok(())
            }
            3 => {
                connection
                    .execute_batch(&format!("{PLUGINS_TABLE} PRAGMA user_version = {SCHEMA};"))
                    .map_err(|error| Error::database("adding the plugin register", error))?;
                Ok(())
            }
            // Schema 4 keeps only a plugin's shape fingerprint, which says
            // whether a shape moved but not what it is, so nothing could be
            // served until the mod registered again. The new column is added
            // empty. A plugin whose shape is not yet known is one the service
            // waits to hear from.
            4 => {
                connection
                    .execute_batch(&format!(
                        "ALTER TABLE plugins ADD COLUMN declared TEXT NOT NULL DEFAULT '';
                         PRAGMA user_version = {SCHEMA};"
                    ))
                    .map_err(|error| Error::database("keeping what each plugin declared", error))?;
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

    /// Returns one plugin's shape fingerprint and whether it is still
    /// installed, or `None` when it has never registered. These are the two
    /// questions asked before a plugin's own database is opened.
    pub fn plugin(&self, id: &str) -> Result<Option<(String, bool)>> {
        self.lock()
            .query_row("SELECT shape, enabled FROM plugins WHERE id = ?1", [id], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)? != 0))
            })
            .optional()
            .map_err(|error| Error::database(format!("reading what {id} registered"), error))
    }

    // No caller reads this yet. `/witchlight status` answers from the mod's
    // own state and never asks the service. It is kept because it is the read
    // side of a store that has a write side, and the tests exercise it.
    #[allow(dead_code)]
    /// Returns every plugin that has ever registered, installed or not.
    ///
    /// Ordered by id, so an operator reading the status twice sees the same
    /// list in the same order.
    pub fn plugins(&self) -> Result<Vec<(String, String, bool)>> {
        self.rows(
            "SELECT id, shape, enabled FROM plugins ORDER BY id",
            "reading the plugin register",
            [],
            |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, i64>(2)? != 0))
            },
        )
    }

    /// Records that a plugin registered, with the shape it declared.
    ///
    /// Replaces any existing row, because registering is the plugin stating what
    /// it is now. Also marks the plugin installed, which covers a plugin that
    /// was removed and has come back.
    pub fn keep_plugin(&self, id: &str, shape: &str, declared: &str, at: SystemTime) -> Result<()> {
        self.lock()
            .execute(
                "INSERT OR REPLACE INTO plugins (id, shape, declared, enabled, at) \
                 VALUES (?1, ?2, ?3, 1, ?4)",
                rusqlite::params![id, shape, declared, seconds(at)],
            )
            .map(|_| ())
            .map_err(|error| Error::database(format!("keeping what {id} registered"), error))
    }

    /// Returns every installed plugin's declaration, as the shape it last sent.
    ///
    /// The service opens its plugin databases from this at startup, so a plugin
    /// serves its rows from the moment the map is up rather than waiting for the
    /// game server to register it again.
    ///
    /// A plugin registered by an older build has no declaration stored and does
    /// not appear here. It is served once it registers.
    pub fn declared_plugins(&self) -> Result<Vec<(String, String, String)>> {
        self.rows(
            "SELECT id, shape, declared FROM plugins \
             WHERE enabled = 1 AND declared <> '' ORDER BY id",
            "reading what plugins declared",
            [],
            |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?))
            },
        )
    }

    // No caller reads this yet. `/witchlight status` answers from the mod's
    // own state and never asks the service. It is kept because it is the read
    // side of a store that has a write side, and the tests exercise it.
    #[allow(dead_code)]
    /// Marks a plugin as no longer installed.
    ///
    /// Its register row and its data rows both stay. An operator decides what to
    /// do with the data of a plugin they removed. The row is marked rather than
    /// deleted so the status can report that the data is still there.
    pub fn forget_plugin(&self, id: &str) -> Result<()> {
        self.lock()
            .execute("UPDATE plugins SET enabled = 0 WHERE id = ?1", [id])
            .map(|_| ())
            .map_err(|error| Error::database(format!("marking {id} gone"), error))
    }

    /// Returns which groups one person shares one plugin's rows with.
    ///
    /// This is kept per plugin rather than beside `share_map_with`. Showing
    /// where somebody explored is not the same as showing what they found there,
    /// and one control over both would share more than a person expected.
    pub fn plugin_shared_with(&self, plugin: &str, uid: &str) -> Result<Vec<i32>> {
        self.rows(
            "SELECT said FROM plugin_shares WHERE plugin = ?1 AND uid = ?2 ORDER BY said",
            "reading who a plugin is shared with",
            params![plugin,
            uid], |row| row.get::<_, i32>(0),
        )
    }

    /// Replaces which groups one person shares one plugin's rows with.
    ///
    /// Writes the whole set in one transaction. A half-written set would leave a
    /// person sharing with a group they had just deselected.
    pub fn keep_plugin_shares(&self, plugin: &str, uid: &str, groups: &[i32]) -> Result<()> {
        let mut connection = self.lock();
        let deal = connection
            .transaction()
            .map_err(|error| Error::database("keeping who a plugin is shared with", error))?;
        deal.execute("DELETE FROM plugin_shares WHERE plugin = ?1 AND uid = ?2", params![plugin, uid])
            .map_err(|error| Error::database("clearing who a plugin was shared with", error))?;
        for group in groups {
            deal.execute(
                "INSERT OR REPLACE INTO plugin_shares (plugin, uid, said) VALUES (?1, ?2, ?3)",
                params![plugin, uid, group],
            )
            .map_err(|error| Error::database("keeping who a plugin is shared with", error))?;
        }
        deal.commit()
            .map_err(|error| Error::database("keeping who a plugin is shared with", error))?;
        Ok(())
    }

    /// Returns everybody who shares one plugin's rows with anybody, keyed by
    /// uid.
    ///
    /// Read at startup and whenever a share is written, so deciding whose rows a
    /// reader may see is a lookup in memory rather than a query per request.
    pub fn plugin_shares(&self, plugin: &str) -> Result<HashMap<String, HashSet<i32>>> {
        let rows = self.rows(
            "SELECT uid, said FROM plugin_shares WHERE plugin = ?1",
            "reading a plugin's shares",
            [plugin],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, i32>(1)?)),
        )?;

        let mut held: HashMap<String, HashSet<i32>> = HashMap::new();
        for (uid, group) in rows {
            held.entry(uid).or_default().insert(group);
        }
        Ok(held)
    }

    /// Returns every browser still logged in.
    pub fn sessions(&self) -> Result<Vec<Session>> {
        self.rows(
            "SELECT word, uid, name, seen FROM sessions",
            "reading the sessions",
            [],
            |row| {
                Ok(Session {
                    word: row.get(0)?,
                    uid: row.get(1)?,
                    name: row.get(2)?,
                    seen: UNIX_EPOCH + Duration::from_secs(row.get::<_, i64>(3)?.max(0) as u64),
                })
            },
        )
    }

    /// Records one browser's login.
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

    /// Deletes one browser's session.
    pub fn delete_session(&self, word: &str) -> Result<()> {
        self.lock()
            .execute("DELETE FROM sessions WHERE word = ?1", params![word])
            .map_err(|error| Error::database("forgetting a login", error))?;
        Ok(())
    }

    /// Deletes every browser session and returns how many there were.
    pub fn clear_sessions(&self) -> Result<usize> {
        self.lock()
            .execute("DELETE FROM sessions", [])
            .map_err(|error| Error::database("forgetting every login", error))
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.connection.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Runs a query and returns every row, built by `read`.
    ///
    /// `doing` names the work for the error message, once rather than at each of
    /// the three steps that can fail.
    fn rows<T>(
        &self,
        sql: &str,
        doing: &'static str,
        params: impl rusqlite::Params,
        read: impl FnMut(&rusqlite::Row<'_>) -> rusqlite::Result<T>,
    ) -> Result<Vec<T>> {
        let failed = |error| Error::database(doing, error);
        let connection = self.lock();
        let mut statement = connection.prepare(sql).map_err(failed)?;
        let rows = statement.query_map(params, read).map_err(failed)?;
        rows.collect::<std::result::Result<Vec<T>, _>>().map_err(failed)
    }

    /// Returns the last marker post the mod sent, as the text that arrived, or
    /// `None` when no post has been stored.
    pub fn markers(&self) -> Result<Option<String>> {
        self.lock()
            .query_row("SELECT body FROM markers WHERE one = 1", [], |row| row.get(0))
            .optional()
            .map_err(|error| Error::database("reading the kept markers", error))
    }

    /// Stores a marker post whole, replacing the last one.
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

    /// Returns everybody's preferences as stored text, keyed by uid.
    pub fn preferences(&self) -> Result<Vec<(String, String)>> {
        self.rows("SELECT uid, body FROM preferences", "reading the preferences", [], |row| {
            Ok((row.get(0)?, row.get(1)?))
        })
    }

    /// Stores one person's preferences whole, replacing what they had.
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
        self.lock()
            .execute(
                "INSERT INTO discovered (uid, rx, rz, bits) VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT (uid, rx, rz) DO UPDATE SET bits = excluded.bits",
                params![uid, rx, rz, &bits[..]],
            )
            .map_err(|error| Error::database("recording a discovery", error))?;
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
        self.lock()
            .execute(
                "DELETE FROM versions
                 WHERE id NOT IN (SELECT version FROM chunks)
                   AND id NOT IN (SELECT version FROM divergences)",
                [],
            )
            .map_err(|error| Error::database("collecting unreferenced versions", error))
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

/// Counts what the database holds.
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

    // These bytes may already be a version of this chunk, either the current
    // one when only the season moved, or an older one somebody remembers. Reuse
    // that row rather than writing it again.
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

/// Converts a time to how the database stores one, as whole seconds since the
/// epoch.
fn seconds(at: SystemTime) -> i64 {
    at.duration_since(UNIX_EPOCH).map_or(0, |since| since.as_secs() as i64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::files::testing::Scratch;

    /// Checks that a migration keeps a copy of what it is about to change,
    /// named for the schema that copy holds. That is the number the build being
    /// rolled back to reads, so it is the number that identifies the file to
    /// restore.
    #[test]
    fn a_migration_leaves_the_old_schema_beside_the_map() {
        let scratch = Scratch::new("store-migration-backup");
        let path = path_in(scratch.at());

        // Build a schema 2 database, which is everything but the three tables
        // schema 3 adds.
        Store::open(scratch.at()).expect("a fresh database");
        {
            let connection = Connection::open(&path).expect("the database");
            connection
                .execute_batch(
                    "DROP TABLE markers;
                     DROP TABLE preferences;
                     DROP TABLE visited;
                     DROP TABLE plugins;
                     DROP TABLE plugin_shares;
                     PRAGMA user_version = 2;",
                )
                .expect("a database as schema 2 left it");
        }

        let backup = path.with_extension("schema2.bak");
        assert!(!backup.exists(), "nothing has been migrated yet");

        Store::open(scratch.at()).expect("the migration to schema 3");

        assert!(backup.exists(), "the schema it came from is kept beside the map");
        let kept = Connection::open(&backup).expect("the backup opens");
        let version: i64 = kept.query_row("PRAGMA user_version", [], |row| row.get(0)).expect("its version");
        assert_eq!(version, 2, "the backup holds the schema it is named for, not the one migrated to");

        // A second migration must not replace the copy made when the database
        // was last known good with whatever a later attempt left behind.
        std::fs::write(&backup, b"not a database").expect("standing in for an older backup");
        {
            let connection = Connection::open(&path).expect("the database");
            connection.execute_batch("PRAGMA user_version = 2;").expect("back to schema 2");
        }
        let _ = Store::open(scratch.at());
        assert_eq!(
            std::fs::read(&backup).expect("the backup still there"),
            b"not a database",
            "an existing backup is kept, not written over"
        );
    }

    /// Checks that opening a database already at this build's schema writes
    /// nothing, so there is nothing to back up.
    #[test]
    fn opening_a_current_database_keeps_no_copy() {
        let scratch = Scratch::new("store-no-needless-backup");

        Store::open(scratch.at()).expect("a fresh database");
        Store::open(scratch.at()).expect("opening it again");

        let copies: Vec<_> = std::fs::read_dir(scratch.at())
            .expect("the directory")
            .filter_map(std::result::Result::ok)
            .filter(|entry| entry.path().extension().is_some_and(|end| end == "bak"))
            .collect();
        assert!(copies.is_empty(), "a database that did not migrate leaves no backup");
    }

    /// Checks that what a plugin declared is stored, so the service can open it
    /// on its next start without waiting to be registered again.
    ///
    /// A shape fingerprint alone says whether a shape moved but not what the
    /// shape is, which is not enough to open a plugin from the register.
    #[test]
    fn the_register_keeps_what_a_plugin_declared() {
        let store = Store::in_memory();
        let now = SystemTime::now();
        let shape = r#"{"columns":{"x":"int"},"key":["x"],"scope":"owner"}"#;

        store.keep_plugin("wl-heatmap", "abc123", shape, now).unwrap();

        let declared = store.declared_plugins().unwrap();
        assert_eq!(declared.len(), 1, "an installed plugin that declared a shape is answered for");
        assert_eq!(declared[0].0, "wl-heatmap");
        assert_eq!(declared[0].1, "abc123", "its fingerprint, so opening it reads as no change");
        assert_eq!(declared[0].2, shape, "and the declaration itself, which is what it is opened from");

        // A removed plugin is not opened. Its rows stay where they are, and an
        // operator decides what to do with them.
        store.forget_plugin("wl-heatmap").unwrap();
        assert!(
            store.declared_plugins().unwrap().is_empty(),
            "a plugin that is no longer installed is not opened at start"
        );

        // A plugin registered by an older build has no declaration and is not
        // opened from the register. It is served once it registers.
        store.keep_plugin("wl-older", "999", "", now).unwrap();
        assert!(
            store.declared_plugins().unwrap().iter().all(|(id, _, _)| id != "wl-older"),
            "a plugin whose shape was never kept is waited on rather than guessed at"
        );
    }

    /// Checks that the register distinguishes an installed plugin from one that
    /// was, which is what lets a plugin be removed without losing its rows.
    #[test]
    fn the_register_remembers_a_plugin_that_has_been_removed() {
        let store = Store::in_memory();
        let now = SystemTime::now();

        assert_eq!(store.plugin("wl-heatmap").unwrap(), None, "nothing has registered");
        assert!(store.plugins().unwrap().is_empty());

        store.keep_plugin("wl-heatmap", "abc123", "{}", now).unwrap();
        assert_eq!(store.plugin("wl-heatmap").unwrap(), Some(("abc123".to_owned(), true)));

        // Registering again with a new shape replaces the stored declaration,
        // because registering is the plugin stating what it is now.
        store.keep_plugin("wl-heatmap", "def456", "{}", now).unwrap();
        assert_eq!(store.plugin("wl-heatmap").unwrap(), Some(("def456".to_owned(), true)));

        // Once removed, the plugin is still known and still listed, but no
        // longer installed.
        store.forget_plugin("wl-heatmap").unwrap();
        assert_eq!(
            store.plugin("wl-heatmap").unwrap(),
            Some(("def456".to_owned(), false)),
            "what it declared is remembered so its rows can still be spoken for"
        );

        // Reinstalling restores it without losing what it declared.
        store.keep_plugin("wl-heatmap", "def456", "{}", now).unwrap();
        assert_eq!(store.plugin("wl-heatmap").unwrap(), Some(("def456".to_owned(), true)));

        store.keep_plugin("wl-other", "999", "{}", now).unwrap();
        let listed = store.plugins().unwrap();
        assert_eq!(listed.len(), 2);
        assert_eq!(listed[0].0, "wl-heatmap", "listed by name, the same order twice running");
        assert_eq!(listed[1].0, "wl-other");
    }

    /// Checks that sharing is stored per plugin and per person, and that
    /// replacing it writes the whole set rather than a series of edits.
    #[test]
    fn who_a_plugin_is_shared_with_is_kept_per_person() {
        let store = Store::in_memory();

        assert!(store.plugin_shared_with("wl-heatmap", "ada").unwrap().is_empty(), "nobody shares by default");

        store.keep_plugin_shares("wl-heatmap", "ada", &[4, 1]).unwrap();
        assert_eq!(store.plugin_shared_with("wl-heatmap", "ada").unwrap(), vec![1, 4], "sorted, and both kept");

        // The set is replaced whole, so a group taken off is no longer shared
        // with rather than lingering because only additions were written.
        store.keep_plugin_shares("wl-heatmap", "ada", &[4]).unwrap();
        assert_eq!(store.plugin_shared_with("wl-heatmap", "ada").unwrap(), vec![4]);

        store.keep_plugin_shares("wl-heatmap", "ada", &[]).unwrap();
        assert!(store.plugin_shared_with("wl-heatmap", "ada").unwrap().is_empty(), "and may be taken off entirely");

        // One person's setting must not affect another's, and one plugin's must
        // not affect another's.
        store.keep_plugin_shares("wl-heatmap", "ada", &[1]).unwrap();
        store.keep_plugin_shares("wl-heatmap", "bob", &[2]).unwrap();
        store.keep_plugin_shares("wl-other", "ada", &[3]).unwrap();
        assert_eq!(store.plugin_shared_with("wl-heatmap", "ada").unwrap(), vec![1]);
        assert_eq!(store.plugin_shared_with("wl-heatmap", "bob").unwrap(), vec![2]);
        assert_eq!(store.plugin_shared_with("wl-other", "ada").unwrap(), vec![3]);

        let all = store.plugin_shares("wl-heatmap").unwrap();
        assert_eq!(all.len(), 2, "everybody sharing this plugin, and nobody sharing another");
        assert_eq!(all["ada"], HashSet::from([1]));
        assert_eq!(all["bob"], HashSet::from([2]));
    }

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

        // The checksum the mod is told at startup is CRC-32 over the raw
        // record, matching what the mod's own `Crc32.Of` computes.
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

        // The old version stays until nothing remembers it.
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

        // A season change alone must not count as the ground moving.
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
        // The corner of region (1, -1) is chunk (16, -16), which is slot 0. One
        // along and one down is slot 17. `columns::chunk_at` inverts this same
        // arithmetic.
        assert_eq!(region_of(16, -16), (1, -1));
        assert_eq!(slot_of(16, -16), 0);
        assert_eq!(slot_of(17, -15), 17);
        assert_eq!(slot_of(31, -1), 255);

    }
}
