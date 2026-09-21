//! The plugin register, in the map's own database.
//!
//! A plugin registers a shape, and that registration decides whether its own
//! database is opened, rebuilt or left alone on the next start. This also holds
//! who each person shares a plugin's rows with. That is kept per plugin rather
//! than beside who they share the map with, because showing where somebody
//! explored is not the same as showing what they found there.
//!
//! Tables: `plugins`, `plugin_shares`.

use std::collections::{HashMap, HashSet};
use std::time::SystemTime;

use rusqlite::{OptionalExtension as _, params};

use crate::util::error::{Error, Result};

use super::{Store, seconds};

impl Store {
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
        self.run(
            "INSERT OR REPLACE INTO plugins (id, shape, declared, enabled, at) \
             VALUES (?1, ?2, ?3, 1, ?4)",
            format!("keeping what {id} registered"),
            rusqlite::params![id, shape, declared, seconds(at)],
        )
        .map(|_| ())
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
        self.run("UPDATE plugins SET enabled = 0 WHERE id = ?1", format!("marking {id} gone"), [id]).map(|_| ())
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
}
