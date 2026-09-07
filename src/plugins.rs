//! What a plugin keeps, and where.
//!
//! A plugin is not code this service runs. It is a name, a declared shape for
//! the rows it keeps, and files it shipped — and the whole of what this module
//! does is hold those rows and answer for them. Nothing here loads anything, and
//! a plugin cannot make the map itself wrong: the tiles are drawn without ever
//! asking whether a plugin exists.
//!
//! Each plugin gets a database of its own, under `plugins/{id}/data.sqlite`.
//! Not a table in the map's database, because that file carries one schema
//! number and refuses any it does not recognise — a plugin adding a table there
//! would be a plugin that makes the map unopenable by a build without it. Not
//! one shared file for every plugin either: SQLite takes one write lock per
//! file, and ten plugins behind one lock made a single commit wait 629ms where
//! a file each held the worst wait to 31ms. A map that stutters is worse than
//! a backfill that takes longer, and deleting a folder is the whole of
//! uninstalling.
//!
//! The registry of which plugins exist lives in the map's own database, so that
//! what is installed can be answered without opening every plugin's file.
//!
//! **No SQL a plugin wrote ever reaches SQLite.** A plugin declares columns from
//! a closed set of types and names that are checked against the same rule every
//! other name in this service follows; every statement is composed here and
//! every value is bound. That is what lets a plugin describe its own storage
//! without being trusted.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use rusqlite::Connection;

use crate::error::{Error, Result};
use crate::log::say;

/// The schema a plugin's own database is at.
///
/// Its own number, unrelated to the map's: a plugin's file is only ever read by
/// this module, and the two move for different reasons.
const SCHEMA: i64 = 1;

/// How deep a plugin's own table may go. A declaration past this is a plugin
/// asking for something it should keep in its own shape instead.
const MOST_COLUMNS: usize = 32;

/// What each owner is called, so a row can say whose it is.
///
/// A table rather than a column on every row: a name is one fact about a person
/// and not a fact about each of the thousands of rows they may have, and one
/// that changes when they rename. The mod sends it with every post because the
/// mod is the half that knows it — the page can only name players who are
/// online, so a reading shared by somebody who has logged off would otherwise
/// show a uid and tell the reader nothing.
const OWNERS_TABLE: &str = "CREATE TABLE owners (
                             uid TEXT PRIMARY KEY,
                             name TEXT NOT NULL
                         ) WITHOUT ROWID;";

/// What a column may hold.
///
/// A closed set, because the type names are written into a `CREATE TABLE` and
/// the only safe way to put a plugin's word into SQL is to not put it there:
/// what is written is this service's own spelling of whichever of these was
/// asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Int,
    Real,
    Text,
    Bool,
}

impl Kind {
    /// How SQLite spells it. This service's own words, never a plugin's.
    const fn sql(self) -> &'static str {
        match self {
            Self::Int | Self::Bool => "INTEGER",
            Self::Real => "REAL",
            Self::Text => "TEXT",
        }
    }
}

/// Who may see a plugin's rows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Scope {
    /// Each row belongs to one person, who may share it with a group.
    Owner,
    /// Everybody's, like the terrain.
    World,
}

/// What a plugin says its rows look like.
///
/// Ordered rather than hashed, so that the same declaration always produces the
/// same table and the same fingerprint — a shape that compared unequal to itself
/// because a map iterated differently would migrate on every start.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
pub struct Shape {
    /// Every column, by name.
    pub columns: BTreeMap<String, Kind>,
    /// Which of them identify a row. `owner_uid` is prepended to this by the
    /// service and is never named here.
    pub key: Vec<String>,
    /// Which of them a reader may ask ranges of. An index is built for these.
    #[serde(default)]
    pub ranged: Vec<String>,
    pub scope: Scope,
}

impl Shape {
    /// Whether this is a shape at all, and what is wrong where it is not.
    ///
    /// Read before anything is created and before anything is compared, because
    /// every other function here writes a plugin's column names into SQL and
    /// this is the one place that decides they may be.
    pub fn check(&self) -> std::result::Result<(), String> {
        if self.columns.is_empty() {
            return Err("a shape with no columns keeps nothing".to_owned());
        }
        if self.columns.len() > MOST_COLUMNS {
            return Err(format!("a shape may have {MOST_COLUMNS} columns at most"));
        }
        if self.key.is_empty() {
            return Err("a shape needs a key, so a row can be replaced rather than repeated".to_owned());
        }

        for name in self.columns.keys() {
            if !crate::urls::is_stored_name(name) {
                return Err(format!(
                    "{name} is not a column name: lowercase letters, digits, _ and - only"
                ));
            }
            // The service's own column, prepended to every table here. A plugin
            // declaring it would be declaring it twice.
            if name == "owner_uid" {
                return Err("owner_uid is the service's own column and is added for you".to_owned());
            }
        }

        for named in self.key.iter().chain(self.ranged.iter()) {
            if !self.columns.contains_key(named) {
                return Err(format!("{named} is named but is not one of the columns"));
            }
        }

        Ok(())
    }

    /// What this shape is, as one word.
    ///
    /// Compared rather than the shape itself so that "has this plugin changed"
    /// is one string equality against what the registry kept, and so that what
    /// is stored about a past registration cannot drift from what it described.
    #[must_use]
    pub fn fingerprint(&self) -> String {
        let mut hash = 0xcbf2_9ce4_8422_2325_u64;
        let mut eat = |bytes: &[u8]| {
            for byte in bytes {
                hash ^= u64::from(*byte);
                hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
            }
        };

        for (name, kind) in &self.columns {
            eat(name.as_bytes());
            eat(kind.sql().as_bytes());
        }
        for named in &self.key {
            eat(b"k");
            eat(named.as_bytes());
        }
        for named in &self.ranged {
            eat(b"r");
            eat(named.as_bytes());
        }
        eat(match self.scope {
            Scope::Owner => b"owner",
            Scope::World => b"world",
        });

        format!("{hash:016x}")
    }

    /// The table this shape asks for.
    ///
    /// `owner_uid` first and always, and first in the key: who a row belongs to
    /// is the service's answer and not a plugin's, and a key that leads with it
    /// is what makes "everything this person may see" a lookup rather than a
    /// scan. A plugin that keeps everybody's rows still has the column, holding
    /// the empty string, so that one table shape answers both scopes.
    fn create(&self) -> String {
        let mut sql = String::from("CREATE TABLE data (\n  owner_uid TEXT NOT NULL");
        for (name, kind) in &self.columns {
            sql.push_str(&format!(",\n  {name} {}", kind.sql()));
        }
        sql.push_str(",\n  PRIMARY KEY (owner_uid");
        for named in &self.key {
            sql.push_str(&format!(", {named}"));
        }
        sql.push_str(")\n) WITHOUT ROWID;");

        // One index over the ranged columns, in the order they were named, so a
        // reader asking about a corner of the world reads that corner.
        if !self.ranged.is_empty() {
            sql.push_str(&format!(
                "\nCREATE INDEX data_ranged ON data (owner_uid, {});",
                self.ranged.join(", ")
            ));
        }

        sql
    }
}

/// One plugin's own database.
pub struct Plugin {
    id: String,
    shape: Shape,
    connection: Mutex<Connection>,
}

impl Plugin {
    /// What this plugin is called.
    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }

    /// What it said its rows look like.
    #[must_use]
    pub fn shape(&self) -> &Shape {
        &self.shape
    }

    /// Opens a plugin's database, making it where there is none, and brings the
    /// table it holds up to the shape declared.
    ///
    /// Where the shape has only gained columns the table is altered in place and
    /// the rows already there keep what they had. Where anything else moved —
    /// a key, a type, a column gone — this refuses rather than guessing which
    /// rows to keep: the plugin knows what its data means and this never will,
    /// so the answer is to say so and leave the rows alone.
    pub fn open(root: &Path, id: &str, shape: &Shape, was: Option<&str>) -> Result<Self> {
        if let Err(wrong) = shape.check() {
            return Err(Error::config(format!("{id} declared a shape this service cannot make: {wrong}")));
        }

        let directory = root.join(id);
        std::fs::create_dir_all(&directory)
            .map_err(|error| Error::io(format!("making {}", directory.display()), error))?;

        let path = directory.join("data.sqlite");
        let fresh = !path.exists();

        let connection = Connection::open(&path)
            .map_err(|error| Error::database(format!("opening {}", path.display()), error))?;
        connection
            .execute_batch("PRAGMA journal_mode = WAL; PRAGMA synchronous = NORMAL;")
            .map_err(|error| Error::database(format!("setting up {}", path.display()), error))?;

        let plugin = Self { id: id.to_owned(), shape: shape.clone(), connection: Mutex::new(connection) };

        if fresh {
            plugin.create(&path)?;
        } else if was != Some(shape.fingerprint().as_str()) {
            plugin.change(&path, was)?;
        }

        Ok(plugin)
    }

    /// The table, as this plugin's shape asks for it.
    fn create(&self, path: &Path) -> Result<()> {
        self.lock()
            .execute_batch(&format!("{}\n{OWNERS_TABLE}\nPRAGMA user_version = {SCHEMA};", self.shape.create()))
            .map_err(|error| Error::database(format!("making {}'s table", self.id), error))?;
        say!("plugin {}: keeping its rows in {}", self.id, path.display());
        Ok(())
    }

    /// A shape that has moved since this database was made.
    fn change(&self, path: &Path, was: Option<&str>) -> Result<()> {
        let held = self.columns_held()?;
        let wanted: Vec<&String> = self.shape.columns.keys().collect();

        // Only additions, and only at the end of the row. Anything else — a
        // column gone, a type changed, a key moved — cannot be done in place:
        // SQLite will not drop a column that is part of a primary key, and
        // rebuilding the table means deciding what happens to rows that no
        // longer fit, which is the plugin's decision and not this service's.
        let added: Vec<&&String> = wanted.iter().filter(|name| !held.contains(**name)).collect();
        let lost: Vec<&String> = held.iter().filter(|name| !self.shape.columns.contains_key(*name)).collect();

        if !lost.is_empty() {
            return Err(Error::config(format!(
                "plugin {} declares a shape that drops {} — this service will not \
                 decide what happens to rows that no longer fit. Read the old rows with \
                 Query, write them back with StoreMany, or register under a new name. \
                 Its data is untouched at {}",
                self.id,
                lost.iter().map(|name| name.as_str()).collect::<Vec<_>>().join(", "),
                path.display()
            )));
        }

        if added.is_empty() {
            // Nothing was added and nothing was lost, so what moved is a key, a
            // type or the scope — none of which can be altered under rows.
            return Err(Error::config(format!(
                "plugin {} declares the same columns in a different shape — a key, a type \
                 or who may see them. None of those can be changed under rows that are \
                 already there. Its data is untouched at {}",
                self.id,
                path.display()
            )));
        }

        // Kept before the first change: a plugin whose table is altered has no
        // way back to what it was, and the copy is what makes rolling one back
        // possible at all. Named for the fingerprint it holds, so which copy
        // answers which registration is not a guess.
        self.back_up(path, was)?;

        let connection = self.lock();
        for name in &added {
            let kind = self.shape.columns[**name];
            connection
                .execute_batch(&format!("ALTER TABLE data ADD COLUMN {name} {};", kind.sql()))
                .map_err(|error| Error::database(format!("adding {name} to {}", self.id), error))?;
        }

        say!(
            "plugin {}: {} added, rows already there keep what they had",
            self.id,
            added.iter().map(|name| name.as_str()).collect::<Vec<_>>().join(", ")
        );
        Ok(())
    }

    /// A copy of a plugin's database, before its shape is changed.
    ///
    /// `VACUUM INTO` rather than a copy of the file, for the reason the map's own
    /// backup uses it: the database is in WAL mode and the bytes of the file
    /// alone are not the database until the log is folded in.
    fn back_up(&self, path: &Path, was: Option<&str>) -> Result<()> {
        let backup = path.with_extension(format!("shape{}.bak", was.unwrap_or("unknown")));
        if backup.exists() {
            return Ok(());
        }

        self.lock()
            .execute("VACUUM INTO ?1", [&backup.to_string_lossy().as_ref()])
            .map_err(|error| {
                Error::database(format!("backing up {} to {}", self.id, backup.display()), error)
            })?;

        say!("plugin {}: kept a copy at {}", self.id, backup.display());
        Ok(())
    }

    /// Which columns the table actually has, as it stands on disk.
    ///
    /// Asked of the database rather than worked out from what was registered:
    /// the registry says what a plugin last declared, and this says what it got,
    /// and a change is only safe to make when those two are compared.
    fn columns_held(&self) -> Result<Vec<String>> {
        let connection = self.lock();
        let mut statement = connection
            .prepare("SELECT name FROM pragma_table_info('data') WHERE name <> 'owner_uid'")
            .map_err(|error| Error::database(format!("reading {}'s columns", self.id), error))?;
        let held = statement
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(|error| Error::database(format!("reading {}'s columns", self.id), error))?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|error| Error::database(format!("reading {}'s columns", self.id), error))?;
        Ok(held)
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.connection.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Rows this reader may see, as JSON.
    ///
    /// `sources` is whose rows to answer with, worked out by the service from
    /// the session and who shares with whom — never from anything the reader or
    /// the plugin said. A world-scoped plugin ignores it, because its rows are
    /// everybody's; an owner-scoped one with no sources answers with nothing,
    /// which is what a stranger is shown.
    ///
    /// `asked` names ranges. Every column in it is checked against the ones the
    /// plugin declared ranged, and both ends are bound rather than written, so
    /// what arrives in a query string decides which rows come back and never
    /// what the statement is.
    pub fn read(&self, sources: &[String], asked: &[(String, i64, i64)]) -> Result<String> {
        let mut wheres = Vec::new();
        let mut values: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

        if self.shape.scope == Scope::Owner {
            if sources.is_empty() {
                return Ok("[]".to_owned());
            }
            let holes = std::iter::repeat_n("?", sources.len()).collect::<Vec<_>>().join(", ");
            wheres.push(format!("data.owner_uid IN ({holes})"));
            for source in sources {
                values.push(Box::new(source.clone()));
            }
        }

        for (column, low, high) in asked {
            // Named ranged by the plugin, or not answered. A column that exists
            // but was not declared ranged is refused rather than scanned: the
            // index that makes a range cheap is built from that declaration, and
            // answering without one is how a map goes quiet under load.
            if !self.shape.ranged.iter().any(|named| named == column) {
                return Err(Error::config(format!(
                    "{} did not declare {column} as a column a range may be asked of",
                    self.id
                )));
            }
            // The name is safe to write because it matched one the plugin
            // declared, and a declaration is checked before it becomes a table.
            wheres.push(format!("data.{column} BETWEEN ? AND ?"));
            values.push(Box::new(*low));
            values.push(Box::new(*high));
        }

        let columns: Vec<&str> = self.shape.columns.keys().map(String::as_str).collect();
        // Left, because a row whose owner has no name kept is still a row: the
        // name arrives with a post and a row could predate one.
        let sql = format!(
            "SELECT data.owner_uid, COALESCE(owners.name, ''), {} FROM data \
             LEFT JOIN owners ON owners.uid = data.owner_uid{}",
            columns.iter().map(|name| format!("data.{name}")).collect::<Vec<_>>().join(", "),
            if wheres.is_empty() {
                String::new()
            } else {
                format!(" WHERE {}", wheres.join(" AND "))
            }
        );

        let connection = self.lock();
        let mut statement = connection
            .prepare(&sql)
            .map_err(|error| Error::database(format!("asking {} for its rows", self.id), error))?;

        let bound: Vec<&dyn rusqlite::ToSql> = values.iter().map(std::convert::AsRef::as_ref).collect();
        let mut rows = statement
            .query(bound.as_slice())
            .map_err(|error| Error::database(format!("asking {} for its rows", self.id), error))?;

        let mut out = String::from("[");
        let mut first = true;
        while let Some(row) = rows
            .next()
            .map_err(|error| Error::database(format!("reading {}'s rows", self.id), error))?
        {
            if !first {
                out.push(',');
            }
            first = false;
            out.push_str("{\"Owner\":");
            let owner: String = row.get(0).unwrap_or_default();
            out.push_str(&serde_json::to_string(&owner).unwrap_or_else(|_| "\"\"".to_owned()));
            // What that owner is called, so a page can say whose a row is
            // without having to have seen them online.
            out.push_str(",\"OwnerName\":");
            let called: String = row.get(1).unwrap_or_default();
            out.push_str(&serde_json::to_string(&called).unwrap_or_else(|_| "\"\"".to_owned()));
            for (at, name) in columns.iter().enumerate() {
                out.push_str(&format!(",{}:", serde_json::to_string(name).unwrap_or_default()));
                out.push_str(&value_json(row, at + 2, self.shape.columns[*name]));
            }
            out.push('}');
        }
        out.push(']');
        Ok(out)
    }

    /// Rows a plugin has sent, kept.
    ///
    /// One transaction for the lot: a batch that failed halfway would leave a
    /// plugin unable to say what it had stored, and the whole point of sending
    /// many at once is that the answer is about all of them.
    ///
    /// `owner` is the service's answer about whose these are, taken from what
    /// the mod knows and not from the row. A world-scoped plugin stores the
    /// empty string, so one table shape answers both scopes.
    pub fn write(&self, owner: &str, called: &str, rows: &[serde_json::Value]) -> Result<usize> {
        let columns: Vec<&str> = self.shape.columns.keys().map(String::as_str).collect();
        let holes = std::iter::repeat_n("?", columns.len() + 1).collect::<Vec<_>>().join(", ");
        let sql = format!("INSERT OR REPLACE INTO data (owner_uid, {}) VALUES ({holes})", columns.join(", "));

        let owner = if self.shape.scope == Scope::World { "" } else { owner };

        let mut connection = self.lock();
        let deal = connection
            .transaction()
            .map_err(|error| Error::database(format!("keeping {}'s rows", self.id), error))?;

        // What this owner is called now, which is what a reader is shown. Written
        // on every post rather than once, so a player who renames is named the
        // way they are named today.
        if !owner.is_empty() && !called.is_empty() {
            deal.execute(
                "INSERT OR REPLACE INTO owners (uid, name) VALUES (?1, ?2)",
                rusqlite::params![owner, called],
            )
            .map_err(|error| Error::database(format!("keeping who {} belongs to", self.id), error))?;
        }
        {
            let mut statement = deal
                .prepare_cached(&sql)
                .map_err(|error| Error::database(format!("keeping {}'s rows", self.id), error))?;

            for row in rows {
                let mut values: Vec<Box<dyn rusqlite::ToSql>> = vec![Box::new(owner.to_owned())];
                for name in &columns {
                    values.push(bound_value(row.get(*name), self.shape.columns[*name]));
                }
                let bound: Vec<&dyn rusqlite::ToSql> =
                    values.iter().map(std::convert::AsRef::as_ref).collect();
                statement
                    .execute(bound.as_slice())
                    .map_err(|error| Error::database(format!("keeping a row of {}'s", self.id), error))?;
            }
        }
        deal.commit()
            .map_err(|error| Error::database(format!("keeping {}'s rows", self.id), error))?;
        Ok(rows.len())
    }

    /// One row taken away, by the key that names it.
    ///
    /// The key arrives as the values of the plugin's own key columns, in the
    /// order it declared them, so what a plugin calls a row is what it is asked
    /// to give up. Scoped to an owner, because a reader may only take away what
    /// is theirs.
    pub fn forget(&self, owner: &str, key: &[&str]) -> Result<bool> {
        if key.len() != self.shape.key.len() {
            return Ok(false);
        }

        let mut wheres = vec!["owner_uid = ?".to_owned()];
        let mut values: Vec<Box<dyn rusqlite::ToSql>> = vec![Box::new(owner.to_owned())];
        for (column, given) in self.shape.key.iter().zip(key) {
            wheres.push(format!("{column} = ?"));
            values.push(bound_value(
                Some(&serde_json::Value::String((*given).to_owned())),
                self.shape.columns[column],
            ));
        }

        let sql = format!("DELETE FROM data WHERE {}", wheres.join(" AND "));
        let bound: Vec<&dyn rusqlite::ToSql> = values.iter().map(std::convert::AsRef::as_ref).collect();
        let gone = self
            .lock()
            .execute(&sql, bound.as_slice())
            .map_err(|error| Error::database(format!("taking a row from {}", self.id), error))?;
        Ok(gone > 0)
    }

    /// How many rows this plugin keeps, for the status the operator reads.
    pub fn rows(&self) -> Result<i64> {
        self.lock()
            .query_row("SELECT COUNT(*) FROM data", [], |row| row.get(0))
            .map_err(|error| Error::database(format!("counting {}'s rows", self.id), error))
    }
}

/// Where every plugin's own directory lives.
#[must_use]
pub fn plugins_dir(exports: &Path) -> PathBuf {
    exports.join("plugins")
}

/// Everything one plugin runs on the page, as one script.
///
/// A plugin worth writing outgrows one file — the map's own viewer is twenty-two
/// of them — so a plugin keeps whatever else it needs in `scripts/`, read in
/// name order and joined ahead of its `viewer.js`. Name order rather than a list
/// it declares: a plugin that wants a particular order says so by naming its
/// files, which is a thing it can see in its own directory rather than a second
/// place to keep in step with the first.
///
/// **Wrapped in a scope of its own**, which is the whole reason this is joined
/// here rather than served as several addresses. Every script the page loads
/// otherwise shares one scope: two plugins that each declare `const draw` at the
/// top level are not one quietly overwriting the other but a `SyntaxError` that
/// stops the second dead. Inside the wrapper a plugin's files see each other and
/// nothing of theirs reaches out, so what a plugin calls things is its own
/// business and no other plugin's problem.
///
/// The entry point is last, so that what it does at the top level can use
/// whatever the rest of the plugin declared.
#[must_use]
pub fn bundle(root: &Path, id: &str) -> Option<String> {
    let directory = root.join(id);
    let entry = std::fs::read_to_string(directory.join("viewer.js")).ok()?;

    let mut parts = Vec::new();
    if let Ok(listing) = std::fs::read_dir(directory.join("scripts")) {
        let mut named: Vec<PathBuf> = listing
            .filter_map(std::result::Result::ok)
            .map(|found| found.path())
            .filter(|path| path.extension().is_some_and(|end| end == "js"))
            .collect();
        named.sort();
        for path in named {
            if let Ok(body) = std::fs::read_to_string(&path) {
                let name = path.file_name().unwrap_or_default().to_string_lossy().into_owned();
                parts.push(format!("// {id}/scripts/{name}\n{body}"));
            }
        }
    }
    parts.push(format!("// {id}/viewer.js\n{entry}"));

    // `use strict` inside the wrapper rather than at the top of the file: a
    // directive here covers this plugin and says nothing about the page or about
    // anybody else's plugin.
    Some(format!("(function () {{\n'use strict';\n{}\n}})();\n", parts.join("\n")))
}

/// Every plugin this service is holding rows for.
///
/// Opened when a plugin registers and kept, because a database opened per
/// request is a file opened per request. Behind one lock rather than each
/// plugin's own, since what this guards is which plugins exist and not their
/// rows — a plugin's own database has its own lock, which is the whole reason
/// each has a file of its own.
#[derive(Default)]
pub struct Plugins {
    open: Mutex<BTreeMap<String, std::sync::Arc<Plugin>>>,
}

impl Plugins {
    /// A plugin has said what it is. Opens its database, makes it where there is
    /// none, and carries an added column onto a table that was already there.
    ///
    /// The name is read to the same rule every other stored name follows before
    /// it becomes a directory, because this is where a plugin's word first
    /// becomes a path.
    pub fn register(&self, root: &Path, id: &str, shape: &Shape, was: Option<&str>) -> Result<()> {
        if !crate::urls::is_stored_name(id) {
            return Err(Error::config(format!(
                "{id} is not a plugin name: lowercase letters, digits, _ and - only"
            )));
        }

        let plugin = Plugin::open(root, id, shape, was)?;
        self.lock().insert(id.to_owned(), std::sync::Arc::new(plugin));
        Ok(())
    }

    /// One plugin, or nothing where none by that name has registered.
    #[must_use]
    pub fn get(&self, id: &str) -> Option<std::sync::Arc<Plugin>> {
        self.lock().get(id).cloned()
    }

    /// Every plugin currently holding rows, by name.
    #[must_use]
    pub fn names(&self) -> Vec<String> {
        self.lock().keys().cloned().collect()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, BTreeMap<String, std::sync::Arc<Plugin>>> {
        self.open.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

/// One value out of a row, as the JSON the page is handed.
///
/// Read as the type the plugin declared rather than as whatever SQLite happens
/// to have stored, so that a column said to be a number is a number on the page
/// and not sometimes a string. A value that will not read as its type is `null`,
/// which is what a column added to a table with rows already in it holds.
fn value_json(row: &rusqlite::Row<'_>, at: usize, kind: Kind) -> String {
    match kind {
        Kind::Int => row.get::<_, Option<i64>>(at).ok().flatten().map_or("null".to_owned(), |v| v.to_string()),
        Kind::Real => row
            .get::<_, Option<f64>>(at)
            .ok()
            .flatten()
            .map_or("null".to_owned(), |v| serde_json::Number::from_f64(v).map_or("null".to_owned(), |n| n.to_string())),
        Kind::Bool => row
            .get::<_, Option<i64>>(at)
            .ok()
            .flatten()
            .map_or("null".to_owned(), |v| if v == 0 { "false".to_owned() } else { "true".to_owned() }),
        Kind::Text => row
            .get::<_, Option<String>>(at)
            .ok()
            .flatten()
            .map_or("null".to_owned(), |v| serde_json::to_string(&v).unwrap_or_else(|_| "null".to_owned())),
    }
}

/// One value a plugin sent, as the type it declared.
///
/// A plugin's JSON is not trusted to be the shape it said: a number arriving
/// where text was declared is stored as text, and a value that cannot be read as
/// its type at all is stored as null rather than refused, since one bad field in
/// a batch of thousands should cost that field and not the batch.
fn bound_value(value: Option<&serde_json::Value>, kind: Kind) -> Box<dyn rusqlite::ToSql> {
    let Some(value) = value else {
        return Box::new(None::<i64>);
    };

    match kind {
        Kind::Int => Box::new(value.as_i64().or_else(|| value.as_str().and_then(|s| s.parse().ok()))),
        Kind::Real => Box::new(value.as_f64().or_else(|| value.as_str().and_then(|s| s.parse().ok()))),
        Kind::Bool => Box::new(value.as_bool().map(i64::from).or_else(|| value.as_i64().map(|v| i64::from(v != 0)))),
        Kind::Text => Box::new(match value {
            serde_json::Value::String(said) => Some(said.clone()),
            serde_json::Value::Null => None,
            other => Some(other.to_string()),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::files::testing::Scratch;

    fn shape() -> Shape {
        Shape {
            columns: [
                ("x".to_owned(), Kind::Int),
                ("y".to_owned(), Kind::Int),
                ("z".to_owned(), Kind::Int),
                ("code".to_owned(), Kind::Text),
            ]
            .into_iter()
            .collect(),
            key: vec!["x".to_owned(), "y".to_owned(), "z".to_owned()],
            ranged: vec!["x".to_owned(), "z".to_owned()],
            scope: Scope::Owner,
        }
    }

    /// The declaration is the only thing a plugin says that becomes SQL, so what
    /// it will and will not accept is the whole of the guard around that.
    #[test]
    fn a_shape_is_checked_before_it_can_become_a_table() {
        assert!(shape().check().is_ok());

        let named = |name: &str| Shape {
            columns: [(name.to_owned(), Kind::Int)].into_iter().collect(),
            key: vec![name.to_owned()],
            ranged: vec![],
            scope: Scope::World,
        };

        // Every one of these is a column name that would otherwise be written
        // into a CREATE TABLE exactly as it arrived.
        for bad in [
            "x, y); DROP TABLE data; --",
            "x\"",
            "x'",
            "x y",
            "X",
            "x.y",
            "",
            "../x",
        ] {
            assert!(named(bad).check().is_err(), "{bad} must not be a column name");
        }

        assert!(named("owner_uid").check().is_err(), "the service's own column is not a plugin's");

        let mut no_key = shape();
        no_key.key.clear();
        assert!(no_key.check().is_err(), "a row that cannot be identified cannot be replaced");

        let mut stranger = shape();
        stranger.key = vec!["nowhere".to_owned()];
        assert!(stranger.check().is_err(), "a key names a column that exists");

        let mut ranged = shape();
        ranged.ranged = vec!["nowhere".to_owned()];
        assert!(ranged.check().is_err(), "so does a ranged column");
    }

    /// What is compared to decide whether a plugin has changed.
    #[test]
    fn a_fingerprint_says_when_a_shape_has_moved() {
        assert_eq!(shape().fingerprint(), shape().fingerprint(), "the same shape twice");

        let mut wider = shape();
        wider.columns.insert("found_at".to_owned(), Kind::Int);
        assert_ne!(shape().fingerprint(), wider.fingerprint(), "a column added");

        let mut retyped = shape();
        retyped.columns.insert("code".to_owned(), Kind::Int);
        assert_ne!(shape().fingerprint(), retyped.fingerprint(), "a column's type");

        let mut rekeyed = shape();
        rekeyed.key = vec!["x".to_owned(), "z".to_owned()];
        assert_ne!(shape().fingerprint(), rekeyed.fingerprint(), "a key");

        let mut shared = shape();
        shared.scope = Scope::World;
        assert_ne!(shape().fingerprint(), shared.fingerprint(), "who may see them");
    }

    #[test]
    fn a_plugin_gets_a_database_of_its_own() {
        let scratch = Scratch::new("plugins-own-database");
        let root = plugins_dir(scratch.at());

        let plugin = Plugin::open(&root, "wl-heatmap", &shape(), None).expect("a new plugin");
        assert_eq!(plugin.rows().expect("no rows yet"), 0);
        assert!(root.join("wl-heatmap/data.sqlite").exists(), "in its own folder");

        // Opening it again with the shape it was made with changes nothing.
        let again = Plugin::open(&root, "wl-heatmap", &shape(), Some(&shape().fingerprint()));
        assert!(again.is_ok(), "an unchanged shape is not a migration");
    }

    /// The split the design rests on: a column added is carried, anything else
    /// is refused with the rows left where they are.
    #[test]
    fn a_shape_may_gain_a_column_and_may_not_lose_one() {
        let scratch = Scratch::new("plugins-shape-change");
        let root = plugins_dir(scratch.at());
        let first = shape();

        let plugin = Plugin::open(&root, "wl-heatmap", &first, None).expect("a new plugin");
        plugin
            .lock()
            .execute("INSERT INTO data (owner_uid, x, y, z, code) VALUES ('uid1', 1, 2, 3, 'copper')", [])
            .expect("a row worth keeping");
        drop(plugin);

        let mut wider = first.clone();
        wider.columns.insert("found_at".to_owned(), Kind::Int);
        let grown = Plugin::open(&root, "wl-heatmap", &wider, Some(&first.fingerprint()))
            .expect("a column added is carried");
        assert_eq!(grown.rows().expect("still there"), 1, "the row already there is kept");
        let kept: String = grown
            .lock()
            .query_row("SELECT code FROM data", [], |row| row.get(0))
            .expect("what it held");
        assert_eq!(kept, "copper", "and keeps what it had");
        drop(grown);

        // A copy was kept before the table was altered, named for the shape it
        // holds rather than the one it was moved to.
        let backup = root.join("wl-heatmap").join(format!("data.shape{}.bak", first.fingerprint()));
        assert!(backup.exists(), "a copy is kept before a shape is changed");

        let mut narrower = wider.clone();
        narrower.columns.remove("code");
        let refused = Plugin::open(&root, "wl-heatmap", &narrower, Some(&wider.fingerprint()));
        assert!(refused.is_err(), "a column lost is refused");

        let mut rekeyed = wider.clone();
        rekeyed.key = vec!["x".to_owned()];
        assert!(
            Plugin::open(&root, "wl-heatmap", &rekeyed, Some(&wider.fingerprint())).is_err(),
            "a key moved is refused"
        );

        // Refused, and the rows are still there — which is the whole point of
        // refusing rather than rebuilding.
        let untouched = Plugin::open(&root, "wl-heatmap", &wider, Some(&wider.fingerprint()))
            .expect("the shape it actually has");
        assert_eq!(untouched.rows().expect("still there"), 1, "a refusal costs no rows");
    }

    /// The whole of why the service holds a plugin's rows rather than the plugin
    /// holding them: a reader is answered with what they may see and nothing
    /// else, and no plugin had to be trusted to arrange that.
    #[test]
    fn a_reader_is_answered_with_their_own_rows_and_nobody_elses() {
        let scratch = Scratch::new("plugins-scoping");
        let root = plugins_dir(scratch.at());
        let plugin = Plugin::open(&root, "wl-heatmap", &shape(), None).expect("a plugin");

        plugin
            .write("ada", "Ada", &[serde_json::json!({ "x": 1, "y": 2, "z": 3, "code": "ada-copper" })])
            .expect("ada's find");
        plugin
            .write("bob", "Bob", &[serde_json::json!({ "x": 9, "y": 8, "z": 7, "code": "bob-gold" })])
            .expect("bob's find");

        let ada = plugin.read(&["ada".to_owned()], &[]).expect("what ada sees");
        assert!(ada.contains("ada-copper"), "her own");
        assert!(!ada.contains("bob-gold"), "and not his");

        let bob = plugin.read(&["bob".to_owned()], &[]).expect("what bob sees");
        assert!(bob.contains("bob-gold") && !bob.contains("ada-copper"));

        // Sharing is the service's answer about whose rows to draw from, handed
        // in rather than asked of the plugin.
        let both = plugin.read(&["ada".to_owned(), "bob".to_owned()], &[]).expect("shared");
        assert!(both.contains("ada-copper") && both.contains("bob-gold"));

        assert_eq!(plugin.read(&[], &[]).expect("a stranger"), "[]", "nobody's rows are everybody's");
    }

    /// A row says whose it is by name, not only by uid.
    ///
    /// The page can only name players who are online, so a reading shared by
    /// somebody who has logged off would otherwise show a uid and tell the
    /// reader nothing.
    #[test]
    fn a_row_can_say_who_it_belongs_to() {
        let scratch = Scratch::new("plugins-owner-names");
        let root = plugins_dir(scratch.at());
        let plugin = Plugin::open(&root, "wl-heatmap", &shape(), None).expect("a plugin");

        plugin
            .write("uid-7f3a", "Ada", &[serde_json::json!({ "x": 1, "y": 2, "z": 3, "code": "copper" })])
            .expect("a find");

        let said = plugin.read(&["uid-7f3a".to_owned()], &[]).expect("read back");
        assert!(said.contains("\"OwnerName\":\"Ada\""), "named, not only identified: {said}");
        assert!(said.contains("\"Owner\":\"uid-7f3a\""), "and still identified");

        // A rename is what they are called now, on every row they have.
        plugin
            .write("uid-7f3a", "Adamarie", &[serde_json::json!({ "x": 9, "y": 9, "z": 9, "code": "gold" })])
            .expect("another find")
            ;
        let after = plugin.read(&["uid-7f3a".to_owned()], &[]).expect("read back");
        assert_eq!(after.matches("Adamarie").count(), 2, "both rows say the new name");
        assert!(!after.contains("\"Ada\""), "and none says the old one");
    }

    /// A world-scoped plugin keeps everybody's rows, and a reader with no
    /// session still sees them — which is the difference the scope makes.
    #[test]
    fn a_world_scoped_plugin_answers_everyone() {
        let scratch = Scratch::new("plugins-world-scope");
        let root = plugins_dir(scratch.at());
        let mut shared = shape();
        shared.scope = Scope::World;

        let plugin = Plugin::open(&root, "wl-landmarks", &shared, None).expect("a plugin");
        plugin
            .write("ada", "Ada", &[serde_json::json!({ "x": 1, "y": 2, "z": 3, "code": "a-tower" })])
            .expect("a landmark");

        assert!(plugin.read(&[], &[]).expect("a stranger").contains("a-tower"));
    }

    /// Ranges are the reason the store is typed rather than a blob, so what may
    /// be asked of one is checked and what comes back is only what was asked for.
    #[test]
    fn a_range_is_answered_only_where_it_was_declared() {
        let scratch = Scratch::new("plugins-ranges");
        let root = plugins_dir(scratch.at());
        let plugin = Plugin::open(&root, "wl-heatmap", &shape(), None).expect("a plugin");

        plugin
            .write(
                "ada",
                "Ada",
                &[
                    serde_json::json!({ "x": 10, "y": 1, "z": 10, "code": "near" }),
                    serde_json::json!({ "x": 5000, "y": 1, "z": 5000, "code": "far" }),
                ],
            )
            .expect("two finds");

        let near = plugin
            .read(&["ada".to_owned()], &[("x".to_owned(), -100, 100), ("z".to_owned(), -100, 100)])
            .expect("a corner of the world");
        assert!(near.contains("near"), "what is in the corner asked for");
        assert!(!near.contains("far"), "and not what is outside it");

        // `y` is a column but was not declared ranged, so asking is refused
        // rather than answered with a scan.
        assert!(
            plugin.read(&["ada".to_owned()], &[("y".to_owned(), 0, 2)]).is_err(),
            "a range is only answered where an index was asked for"
        );
        assert!(
            plugin.read(&["ada".to_owned()], &[("nowhere".to_owned(), 0, 1)]).is_err(),
            "and only of a column that exists"
        );
    }

    /// Values arrive from a plugin as JSON and are stored as the type declared,
    /// so a page is handed numbers where numbers were promised.
    #[test]
    fn a_value_is_kept_as_the_type_its_column_was_declared() {
        let scratch = Scratch::new("plugins-types");
        let root = plugins_dir(scratch.at());
        let plugin = Plugin::open(&root, "wl-heatmap", &shape(), None).expect("a plugin");

        // A number sent as a string, and a missing field.
        plugin
            .write("ada", "Ada", &[serde_json::json!({ "x": "42", "y": 2, "z": 3 })])
            .expect("what a plugin actually sends");

        let said = plugin.read(&["ada".to_owned()], &[]).expect("read back");
        assert!(said.contains("\"x\":42"), "a number, not a string: {said}");
        assert!(said.contains("\"code\":null"), "a field nobody sent is null: {said}");
    }

    /// A row is taken away by the key the plugin named it with, and only by its
    /// owner.
    #[test]
    fn a_row_is_forgotten_by_its_own_key() {
        let scratch = Scratch::new("plugins-forget");
        let root = plugins_dir(scratch.at());
        let plugin = Plugin::open(&root, "wl-heatmap", &shape(), None).expect("a plugin");

        plugin
            .write("ada", "Ada", &[serde_json::json!({ "x": 1, "y": 2, "z": 3, "code": "copper" })])
            .expect("a find");
        assert_eq!(plugin.rows().unwrap(), 1);

        assert!(!plugin.forget("bob", &["1", "2", "3"]).unwrap(), "not his to take");
        assert_eq!(plugin.rows().unwrap(), 1, "and so still there");

        assert!(plugin.forget("ada", &["1", "2", "3"]).unwrap(), "hers to take");
        assert_eq!(plugin.rows().unwrap(), 0);

        assert!(!plugin.forget("ada", &["1", "2"]).unwrap(), "a key is every column of it");
    }

    /// A plugin across several files is one script, in a scope of its own.
    #[test]
    fn a_plugins_scripts_are_joined_behind_a_scope_of_their_own() {
        let scratch = Scratch::new("plugins-bundle");
        let root = plugins_dir(scratch.at());
        let mine = root.join("wl-heatmap");
        std::fs::create_dir_all(mine.join("scripts")).expect("the plugin");

        std::fs::write(mine.join("viewer.js"), "start(draw, panel);").expect("the entry");
        std::fs::write(mine.join("scripts/10-draw.js"), "const draw = 1;").expect("one part");
        std::fs::write(mine.join("scripts/20-panel.js"), "const panel = 2;").expect("another");
        // Not a script, and not to be swept in with them.
        std::fs::write(mine.join("scripts/notes.md"), "nothing to run").expect("a stray file");

        let made = bundle(&root, "wl-heatmap").expect("a bundle");

        assert!(made.starts_with("(function () {"), "wrapped in a scope of its own");
        assert!(made.contains("'use strict';"), "and strict inside that scope");
        assert!(made.trim_end().ends_with("})();"), "and closed");
        assert!(!made.contains("nothing to run"), "only the scripts");

        // Name order, and the entry last so it may use what the rest declared.
        let drawn = made.find("const draw").expect("the first part");
        let panelled = made.find("const panel").expect("the second");
        let started = made.find("start(draw, panel);").expect("the entry");
        assert!(drawn < panelled, "scripts in the order their names put them");
        assert!(panelled < started, "and the entry point after all of them");

        // Each file is named in the bundle, so an error in one says which.
        assert!(made.contains("// wl-heatmap/scripts/10-draw.js"));
        assert!(made.contains("// wl-heatmap/viewer.js"));

        // A plugin with no `scripts/` is still a plugin.
        let plain = root.join("wl-plain");
        std::fs::create_dir_all(&plain).expect("another plugin");
        std::fs::write(plain.join("viewer.js"), "const draw = 3;").expect("its entry");
        let simple = bundle(&root, "wl-plain").expect("one file is a bundle too");
        assert!(simple.contains("const draw = 3;"));

        // Two plugins may each declare the same name. Wrapped, that is two
        // scopes; unwrapped it is a SyntaxError that stops the second dead.
        assert!(made.contains("const draw = 1;") && simple.contains("const draw = 3;"));

        assert!(bundle(&root, "wl-nothing").is_none(), "a plugin with no script is not served one");
    }

    /// The service's own column is prepended, and leads the key, whatever the
    /// plugin said.
    #[test]
    fn who_a_row_belongs_to_is_the_services_answer() {
        let sql = shape().create();
        assert!(sql.contains("owner_uid TEXT NOT NULL"), "always there");
        assert!(sql.contains("PRIMARY KEY (owner_uid, x, y, z)"), "and leads the key");
        assert!(sql.contains("CREATE INDEX data_ranged ON data (owner_uid, x, z)"), "ranges are indexed");

        let mut unranged = shape();
        unranged.ranged.clear();
        assert!(!unranged.create().contains("CREATE INDEX"), "and only where asked for");
    }
}
