//! What the mod last posted, in the map's own database.
//!
//! The markers and everybody's preferences are held whole, as the text that
//! arrived, so a service that restarts serves what it had rather than nothing
//! until the mod posts again.
//!
//! Tables: `markers`, `preferences`.

use rusqlite::{OptionalExtension as _, params};

use crate::util::error::{Error, Result};

use super::Store;

impl Store {
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
        self.run(
            "INSERT INTO markers (one, body) VALUES (1, ?1)
             ON CONFLICT (one) DO UPDATE SET body = excluded.body",
            "keeping the markers",
            params![body],
        )?;
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
        self.run(
            "INSERT INTO preferences (uid, body) VALUES (?1, ?2)
             ON CONFLICT (uid) DO UPDATE SET body = excluded.body",
            "keeping somebody's preferences",
            params![uid, body],
        )?;
        Ok(())
    }
}
