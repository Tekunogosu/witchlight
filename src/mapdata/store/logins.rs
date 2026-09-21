//! Browser logins, in the map's own database.
//!
//! A session is one browser that has signed in. It is kept on disk so a service
//! that restarts does not sign everybody out.
//!
//! Table: `sessions`.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use rusqlite::params;

use crate::util::error::Result;

use super::{Session, Store, seconds};

impl Store {
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
        self.run(
            "INSERT INTO sessions (word, uid, name, seen) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT (word) DO UPDATE SET uid = excluded.uid, name = excluded.name, seen = excluded.seen",
            "recording a login",
            params![session.word, session.uid, session.name, seconds(session.seen)],
        )?;
        Ok(())
    }

    /// Moves one browser's last-seen time forward.
    pub fn touch_session(&self, word: &str, seen: SystemTime) -> Result<()> {
        self.run("UPDATE sessions SET seen = ?2 WHERE word = ?1", "noting a login was used", params![word, seconds(seen)])?;
        Ok(())
    }

    /// Deletes one browser's session.
    pub fn delete_session(&self, word: &str) -> Result<()> {
        self.run("DELETE FROM sessions WHERE word = ?1", "forgetting a login", params![word])?;
        Ok(())
    }

    /// Deletes every browser session and returns how many there were.
    pub fn clear_sessions(&self) -> Result<usize> {
        self.run("DELETE FROM sessions", "forgetting every login", [])
    }
}
