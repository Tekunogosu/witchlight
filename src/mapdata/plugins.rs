//! Stores and serves the rows plugins keep.
//!
//! This service never runs plugin code. A plugin is a name, a declared shape for
//! the rows it keeps, and files it shipped. This module holds those rows and
//! answers for them. A plugin cannot make the map wrong, because the tiles are
//! drawn without asking whether a plugin exists.
//!
//! Each plugin gets its own database under `plugins/{id}/data.sqlite`. It is not
//! a table in the map's database, because that file carries one schema number
//! and refuses any it does not recognise, so a plugin adding a table there would
//! make the map unopenable by a build without that plugin. Nor is it one shared
//! file for every plugin: SQLite takes one write lock per file, and ten plugins
//! behind one lock made a single commit wait 629ms, where a file each held the
//! worst wait to 31ms. Deleting a folder is then the whole of uninstalling.
//!
//! The registry of which plugins exist lives in the map's own database, so what
//! is installed can be answered without opening every plugin's file.
//!
//! No SQL a plugin wrote ever reaches SQLite. A plugin declares columns from a
//! closed set of types, under names checked against the same rule every other
//! name in this service follows. Every statement is composed here and every
//! value is bound. That lets a plugin describe its own storage without being
//! trusted.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use rusqlite::Connection;

use crate::util::error::{Error, Result};
use crate::util::log::say;

/// The schema version of a plugin's own database.
///
/// This number is unrelated to the map's. Only this module reads a plugin's
/// file, and the two schemas change for different reasons.
const SCHEMA: i64 = 1;

/// The maximum number of columns a plugin's table may have.
const MOST_COLUMNS: usize = 32;

/// Defines the table naming each owner, so a row can say whose it is.
///
/// This is a table rather than a column on every row. A name is one fact about a
/// person, not a fact repeated on each of the thousands of rows they may have,
/// and it changes when they rename. The mod sends the name with every post
/// because only the mod knows it. The page can name only players who are online,
/// so a row shared by somebody who has logged off would otherwise show a bare
/// uid.
const OWNERS_TABLE: &str = "CREATE TABLE owners (
                             uid TEXT PRIMARY KEY,
                             name TEXT NOT NULL
                         ) WITHOUT ROWID;";

/// Names the types a column may hold.
///
/// The set is closed, because these type names are written into a `CREATE
/// TABLE`. What reaches SQL is this service's own spelling of whichever variant
/// a plugin asked for, never the plugin's own word.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Int,
    Real,
    Text,
    Bool,
}

impl Kind {
    /// Returns how SQLite spells this type. These are this service's own
    /// words, never a plugin's.
    const fn sql(self) -> &'static str {
        match self {
            Self::Int | Self::Bool => "INTEGER",
            Self::Real => "REAL",
            Self::Text => "TEXT",
        }
    }
}

/// Decides who may see a plugin's rows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Scope {
    /// Each row belongs to one person, who may share it with a group.
    Owner,
    /// Every row belongs to everybody, like the terrain.
    World,
}

/// Describes what a plugin says its rows look like.
///
/// Columns are ordered rather than hashed, so the same declaration always
/// produces the same table and the same fingerprint. A shape that compared
/// unequal to itself because a hash map iterated differently would migrate on
/// every start.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
pub struct Shape {
    /// Holds every column, keyed by name.
    pub columns: BTreeMap<String, Kind>,
    /// Names the columns that identify a row. The service prepends `owner_uid`
    /// and a plugin never names it here.
    pub key: Vec<String>,
    /// Names the columns a reader may ask ranges of. An index is built over
    /// these.
    #[serde(default)]
    pub ranged: Vec<String>,
    pub scope: Scope,
}

impl Shape {
    /// Checks that this is a valid shape, and describes what is wrong when it
    /// is not.
    ///
    /// This runs before anything is created and before anything is compared.
    /// Every other function here writes a plugin's column names into SQL, and
    /// this is the one place that decides they may be written.
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
            if !crate::util::urls::is_stored_name(name) {
                return Err(format!(
                    "{name} is not a column name: lowercase letters, digits, _ and - only"
                ));
            }
            // The service prepends this column to every table, so a plugin
            // declaring it would declare it twice.
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

    /// Returns a fingerprint of this shape.
    ///
    /// Comparing fingerprints rather than shapes makes deciding whether a plugin
    /// changed one string comparison against what the registry stored, and stops
    /// the stored record of a past registration from drifting from what it
    /// described.
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

    /// Builds the `CREATE TABLE` statement this shape asks for.
    ///
    /// `owner_uid` always comes first, both as a column and in the key. The
    /// service decides who a row belongs to, not the plugin, and leading the key
    /// with it makes "everything this person may see" a lookup rather than a
    /// scan. A world-scoped plugin still has the column, holding the empty
    /// string, so one table shape serves both scopes.
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

        // Build one index over the ranged columns in the order they were
        // named, so a reader asking about a corner of the world reads only that
        // corner.
        if !self.ranged.is_empty() {
            sql.push_str(&format!(
                "\nCREATE INDEX data_ranged ON data (owner_uid, {});",
                self.ranged.join(", ")
            ));
        }

        sql
    }
}

/// Holds one plugin's own database.
pub struct Plugin {
    id: String,
    shape: Shape,
    connection: Mutex<Connection>,
}

impl Plugin {
    /// Opens a plugin's database, creating it if there is none, and brings its
    /// table up to the declared shape.
    ///
    /// When the shape has only gained columns, the table is altered in place and
    /// existing rows keep what they had. Any other change, such as a moved key, a
    /// changed type, or a removed column, is refused. Rebuilding the table would
    /// mean deciding which rows to keep, which only the plugin can decide, so the
    /// rows are left alone.
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

    /// Creates the table this plugin's shape asks for.
    fn create(&self, path: &Path) -> Result<()> {
        self.lock()
            .execute_batch(&format!("{}\n{OWNERS_TABLE}\nPRAGMA user_version = {SCHEMA};", self.shape.create()))
            .map_err(|error| Error::database(format!("making {}'s table", self.id), error))?;
        say!("plugin {}: keeping its rows in {}", self.id, path.display());
        Ok(())
    }

    /// Applies a shape that changed since this database was made.
    fn change(&self, path: &Path, was: Option<&str>) -> Result<()> {
        let held = self.columns_held()?;
        let wanted: Vec<&String> = self.shape.columns.keys().collect();

        // Only additions can be applied in place. SQLite will not drop a column
        // that is part of a primary key, and rebuilding the table means deciding
        // what happens to rows that no longer fit, which is the plugin's
        // decision.
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
            // Nothing was added and nothing was lost, so what changed is a
            // key, a type or the scope. None of those can be altered under
            // existing rows.
            return Err(Error::config(format!(
                "plugin {} declares the same columns in a different shape — a key, a type \
                 or who may see them. None of those can be changed under rows that are \
                 already there. Its data is untouched at {}",
                self.id,
                path.display()
            )));
        }

        // Back up before the first change. Altering a plugin's table has no way
        // back without a copy. The backup is named for the fingerprint it holds,
        // so which copy belongs to which registration is unambiguous.
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

    /// Copies a plugin's database before its shape is changed.
    ///
    /// It uses `VACUUM INTO` rather than a file copy, for the same reason the
    /// map's own backup does. The database is in WAL mode, so the bytes of the
    /// file alone are not the database until the log is folded in.
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

    /// Returns which columns the table has on disk.
    ///
    /// This asks the database rather than deriving the answer from the registry.
    /// The registry says what a plugin last declared, this says what it actually
    /// got, and a change is only safe once the two are compared.
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

    /// Returns the rows this reader may see, as JSON.
    ///
    /// `sources` names whose rows to answer with. The service works it out from
    /// the session and who shares with whom, never from anything the reader or
    /// the plugin sent. A world-scoped plugin ignores it, because its rows
    /// belong to everybody. An owner-scoped plugin with no sources returns
    /// nothing, which is what a stranger is shown.
    ///
    /// `asked` names ranges. Every column in it is checked against the ones the
    /// plugin declared ranged, and both ends are bound rather than written into
    /// the SQL, so a query string decides which rows come back and never what
    /// the statement is.
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
            // Refuse a column that exists but was not declared ranged, rather
            // than scanning for it. The index that makes a range cheap is built
            // from that declaration, and answering without one stalls the map
            // under load.
            if !self.shape.ranged.iter().any(|named| named == column) {
                return Err(Error::config(format!(
                    "{} did not declare {column} as a column a range may be asked of",
                    self.id
                )));
            }
            // Writing the name here is safe because it matched one the plugin
            // declared, and a declaration is checked before it becomes a table.
            wheres.push(format!("data.{column} BETWEEN ? AND ?"));
            values.push(Box::new(*low));
            values.push(Box::new(*high));
        }

        let columns: Vec<&str> = self.shape.columns.keys().map(String::as_str).collect();
        // Use a left join, because a row whose owner has no stored name is
        // still a row. The name arrives with a post, and a row can predate one.
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
            // Include the owner's name, so a page can say whose a row is
            // without having seen that player online.
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

    /// Stores rows a plugin sent.
    ///
    /// The whole batch is one transaction. A batch that failed halfway would
    /// leave a plugin unable to say what it had stored.
    ///
    /// `owner` is the service's answer about whose rows these are, taken from
    /// what the mod knows and not from the row itself. A world-scoped plugin
    /// stores the empty string, so one table shape serves both scopes.
    pub fn write(&self, owner: &str, called: &str, rows: &[serde_json::Value]) -> Result<usize> {
        let columns: Vec<&str> = self.shape.columns.keys().map(String::as_str).collect();
        let holes = std::iter::repeat_n("?", columns.len() + 1).collect::<Vec<_>>().join(", ");
        let sql = format!("INSERT OR REPLACE INTO data (owner_uid, {}) VALUES ({holes})", columns.join(", "));

        let owner = if self.shape.scope == Scope::World { "" } else { owner };

        let mut connection = self.lock();
        let deal = connection
            .transaction()
            .map_err(|error| Error::database(format!("keeping {}'s rows", self.id), error))?;

        // Store what this owner is called now, which is what a reader is shown.
        // Written on every post rather than once, so a player who renames is
        // named the way they are named today.
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

    /// Deletes one row, identified by the key that names it.
    ///
    /// The key arrives as the values of the plugin's own key columns, in the
    /// order it declared them. The delete is scoped to an owner, because a
    /// reader may only remove their own rows.
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

    // No caller reads this yet, for the reason given beside `Store::counts`.
    #[allow(dead_code)]
    /// Returns how many rows this plugin keeps, for the status an operator
    /// reads.
    pub fn rows(&self) -> Result<i64> {
        self.lock()
            .query_row("SELECT COUNT(*) FROM data", [], |row| row.get(0))
            .map_err(|error| Error::database(format!("counting {}'s rows", self.id), error))
    }
}

/// Returns the directory holding every plugin's own directory.
#[must_use]
pub fn plugins_dir(exports: &Path) -> PathBuf {
    exports.join("plugins")
}

/// Joins everything one plugin runs on the page into a single script.
///
/// A plugin keeps whatever it needs beyond one file in `scripts/`. Those are
/// read in name order and joined ahead of its `viewer.js`. Name order means
/// there is no declared list, so a plugin that wants a particular order says so
/// by naming its files rather than keeping a second list in step with the
/// directory.
///
/// The result is wrapped in a scope of its own, which is why it is joined here
/// rather than served as several addresses. Every script the page loads
/// otherwise shares one scope, so two plugins that each declare `const draw` at
/// the top level produce a `SyntaxError` that stops the second dead. Inside the
/// wrapper a plugin's files see each other and nothing of theirs reaches out.
///
/// The entry point comes last, so its top-level code can use whatever the rest
/// of the plugin declared.
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

    // Put `use strict` inside the wrapper rather than at the top of the file,
    // so it covers this plugin alone and not the page or any other plugin.
    Some(format!("(function () {{\n'use strict';\n{}\n}})();\n", parts.join("\n")))
}

/// Holds every plugin this service is keeping rows for.
///
/// A plugin's database is opened when it registers and then kept open, because
/// opening it per request would open a file per request. One lock guards this
/// map, which tracks which plugins exist rather than their rows. Each plugin's
/// own database has its own lock, which is why each has its own file.
#[derive(Default)]
pub struct Plugins {
    open: Mutex<BTreeMap<String, std::sync::Arc<Plugin>>>,
}

impl Plugins {
    /// Registers a plugin. Opens its database, creating it if there is none,
    /// and adds any new column to a table that was already there.
    ///
    /// The name is checked against the same rule every other stored name
    /// follows before it becomes a directory, because this is where a plugin's
    /// word first becomes a path.
    pub fn register(&self, root: &Path, id: &str, shape: &Shape, was: Option<&str>) -> Result<()> {
        if !crate::util::urls::is_stored_name(id) {
            return Err(Error::config(format!(
                "{id} is not a plugin name: lowercase letters, digits, _ and - only"
            )));
        }

        let plugin = Plugin::open(root, id, shape, was)?;
        self.lock().insert(id.to_owned(), std::sync::Arc::new(plugin));
        Ok(())
    }

    /// Returns one plugin, or `None` when none by that name has registered.
    #[must_use]
    pub fn get(&self, id: &str) -> Option<std::sync::Arc<Plugin>> {
        self.lock().get(id).cloned()
    }

    /// Returns the names of every plugin currently holding rows.
    #[must_use]
    pub fn names(&self) -> Vec<String> {
        self.lock().keys().cloned().collect()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, BTreeMap<String, std::sync::Arc<Plugin>>> {
        self.open.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

/// Converts one value out of a row into the JSON the page is handed.
///
/// The value is read as the type the plugin declared rather than as whatever
/// SQLite stored, so a column declared a number is always a number on the page.
/// A value that will not read as its type becomes `null`, which is what a column
/// added to a table with existing rows holds.
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

/// Converts one value a plugin sent into the type it declared.
///
/// A plugin's JSON is not trusted to match the declared shape. A number arriving
/// where text was declared is stored as text. A value that cannot be read as its
/// type at all is stored as null rather than refused, so one bad field in a batch
/// of thousands costs that field and not the batch.
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
    use crate::util::files::testing::Scratch;

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

    /// Checks the guard on the only thing a plugin sends that becomes SQL. The
    /// declaration must be rejected unless every name is safe to write.
    #[test]
    fn a_shape_is_checked_before_it_can_become_a_table() {
        assert!(shape().check().is_ok());

        let named = |name: &str| Shape {
            columns: [(name.to_owned(), Kind::Int)].into_iter().collect(),
            key: vec![name.to_owned()],
            ranged: vec![],
            scope: Scope::World,
        };

        // Each of these would otherwise be written into a CREATE TABLE exactly
        // as it arrived.
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

    /// Checks that the fingerprint changes whenever the shape does.
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

        // Opening it again with the shape it was made with must change
        // nothing.
        let again = Plugin::open(&root, "wl-heatmap", &shape(), Some(&shape().fingerprint()));
        assert!(again.is_ok(), "an unchanged shape is not a migration");
    }

    /// Checks that a column added is carried onto the existing table, while any
    /// other shape change is refused with the rows left in place.
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

        // A copy must have been kept before the table was altered, named for
        // the shape it holds rather than the one it moved to.
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

        // After a refusal the rows must still be there, which is the point of
        // refusing rather than rebuilding.
        let untouched = Plugin::open(&root, "wl-heatmap", &wider, Some(&wider.fingerprint()))
            .expect("the shape it actually has");
        assert_eq!(untouched.rows().expect("still there"), 1, "a refusal costs no rows");
    }

    /// Checks that a reader is answered with what they may see and nothing
    /// else, without the plugin having to arrange it.
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

        // The service decides whose rows to draw from and hands that in. The
        // plugin is never asked.
        let both = plugin.read(&["ada".to_owned(), "bob".to_owned()], &[]).expect("shared");
        assert!(both.contains("ada-copper") && both.contains("bob-gold"));

        assert_eq!(plugin.read(&[], &[]).expect("a stranger"), "[]", "nobody's rows are everybody's");
    }

    /// Checks that a row says whose it is by name and not only by uid.
    ///
    /// The page can name only players who are online, so a row shared by
    /// somebody who has logged off would otherwise show a bare uid.
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

        // A rename must apply to every row that owner has.
        plugin
            .write("uid-7f3a", "Adamarie", &[serde_json::json!({ "x": 9, "y": 9, "z": 9, "code": "gold" })])
            .expect("another find")
            ;
        let after = plugin.read(&["uid-7f3a".to_owned()], &[]).expect("read back");
        assert_eq!(after.matches("Adamarie").count(), 2, "both rows say the new name");
        assert!(!after.contains("\"Ada\""), "and none says the old one");
    }

    /// Checks that a world-scoped plugin keeps everybody's rows and that a
    /// reader with no session still sees them.
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

    /// Checks that a range is answered only for a column declared ranged, and
    /// returns only the rows inside it.
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

        // `y` is a column but was not declared ranged, so the request must be
        // refused rather than answered with a scan.
        assert!(
            plugin.read(&["ada".to_owned()], &[("y".to_owned(), 0, 2)]).is_err(),
            "a range is only answered where an index was asked for"
        );
        assert!(
            plugin.read(&["ada".to_owned()], &[("nowhere".to_owned(), 0, 1)]).is_err(),
            "and only of a column that exists"
        );
    }

    /// Checks that a value is stored as the type its column was declared, so
    /// the page is handed numbers where numbers were promised.
    #[test]
    fn a_value_is_kept_as_the_type_its_column_was_declared() {
        let scratch = Scratch::new("plugins-types");
        let root = plugins_dir(scratch.at());
        let plugin = Plugin::open(&root, "wl-heatmap", &shape(), None).expect("a plugin");

        // Send a number as a string, and omit one field.
        plugin
            .write("ada", "Ada", &[serde_json::json!({ "x": "42", "y": 2, "z": 3 })])
            .expect("what a plugin actually sends");

        let said = plugin.read(&["ada".to_owned()], &[]).expect("read back");
        assert!(said.contains("\"x\":42"), "a number, not a string: {said}");
        assert!(said.contains("\"code\":null"), "a field nobody sent is null: {said}");
    }

    /// Checks that a row is deleted by the key the plugin named it with, and
    /// only by its owner.
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

    /// Checks that a plugin spread across several files becomes one script in a
    /// scope of its own.
    #[test]
    fn a_plugins_scripts_are_joined_behind_a_scope_of_their_own() {
        let scratch = Scratch::new("plugins-bundle");
        let root = plugins_dir(scratch.at());
        let mine = root.join("wl-heatmap");
        std::fs::create_dir_all(mine.join("scripts")).expect("the plugin");

        std::fs::write(mine.join("viewer.js"), "start(draw, panel);").expect("the entry");
        std::fs::write(mine.join("scripts/10-draw.js"), "const draw = 1;").expect("one part");
        std::fs::write(mine.join("scripts/20-panel.js"), "const panel = 2;").expect("another");
        // This is not a script and must not be swept in with them.
        std::fs::write(mine.join("scripts/notes.md"), "nothing to run").expect("a stray file");

        let made = bundle(&root, "wl-heatmap").expect("a bundle");

        assert!(made.starts_with("(function () {"), "wrapped in a scope of its own");
        assert!(made.contains("'use strict';"), "and strict inside that scope");
        assert!(made.trim_end().ends_with("})();"), "and closed");
        assert!(!made.contains("nothing to run"), "only the scripts");

        // Scripts come in name order, and the entry point last so it can use
        // what the rest declared.
        let drawn = made.find("const draw").expect("the first part");
        let panelled = made.find("const panel").expect("the second");
        let started = made.find("start(draw, panel);").expect("the entry");
        assert!(drawn < panelled, "scripts in the order their names put them");
        assert!(panelled < started, "and the entry point after all of them");

        // Each file is named in the bundle, so an error in one identifies it.
        assert!(made.contains("// wl-heatmap/scripts/10-draw.js"));
        assert!(made.contains("// wl-heatmap/viewer.js"));

        // A plugin with no `scripts/` directory is still a plugin.
        let plain = root.join("wl-plain");
        std::fs::create_dir_all(&plain).expect("another plugin");
        std::fs::write(plain.join("viewer.js"), "const draw = 3;").expect("its entry");
        let simple = bundle(&root, "wl-plain").expect("one file is a bundle too");
        assert!(simple.contains("const draw = 3;"));

        // Two plugins may each declare the same name. The wrapper makes those
        // two scopes. Unwrapped they would be a SyntaxError that stops the
        // second dead.
        assert!(made.contains("const draw = 1;") && simple.contains("const draw = 3;"));

        assert!(bundle(&root, "wl-nothing").is_none(), "a plugin with no script is not served one");
    }

    /// Checks that the service's own column is prepended and leads the key,
    /// whatever the plugin declared.
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
