//! Where the exports are and where to serve them from.
//!
//! Settings come from a file and command-line flags win over it. The file is
//! written with the defaults on a first run, so there is always something to
//! edit.
//!
//! Which file depends on who started this. Run by hand it is
//! `~/.config/witchlight/config.toml`; started by the server mod it is
//! `witchlight.conf` in the game's `ModConfig` folder, which the mod names with
//! `--config` so that everything about one server's map sits with that server.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

/// What the operator has decided about markers.
///
/// Two switches always read together, both saying who a marker belongs to when
/// nobody has said otherwise. They travel as a pair so the half asking never has
/// one without the other — and because a function taking eight loose arguments is
/// a function taking a settings file badly.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MarkerRules {
    /// Whether a marker nobody has decided about is everyone's.
    pub public: bool,
    /// Whether a marker anybody can see is a marker anybody can change.
    pub public_editable: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    /// The Vintage Story data directory — the server's `--dataPath`. Exports are
    /// read from the `witchlight` folder inside it unless `map_data` says
    /// otherwise.
    pub vs_data: PathBuf,

    /// Where map data is kept. Empty means the `witchlight` folder inside
    /// `vs_data`, which is where it has always been.
    ///
    /// Worth setting where the maps should live somewhere other than beside the
    /// world — a larger disk, a directory a web server already serves.
    pub map_data: PathBuf,

    /// Whether each world gets a directory of its own inside `map_data`.
    ///
    /// Off for a dedicated server, which runs one world and wants its map where
    /// it has always been. On for singleplayer, where every save shares one data
    /// path: without this the second world writes its terrain into the first
    /// world's map at the same coordinates, and every palette and block-name file
    /// is rewritten on each switch because the mod sets differ.
    ///
    /// Read by the mod, which is the only half that knows which world is running.
    /// This half is told the answer with `--exports`, and reads this only to know
    /// where to look when somebody runs `witchlight serve` by hand.
    pub per_world: bool,

    /// Address to listen on. All interfaces by default, so the map is reachable
    /// from the rest of the network without further configuration. Set it to
    /// `127.0.0.1:8080` to keep it on this machine only.
    pub bind: String,

    /// Where the server mod posts who is online and where the markers are.
    ///
    /// Empty means loopback on a port the machine picks, published in `api.json`
    /// beside the map so the mod finds it without being told and two game servers
    /// on one box collide with nothing. Set a `host:port` only for a mod running
    /// on another machine, which is also the one case `api_token` must be set.
    pub api_bind: String,

    /// What the mod must present to post. Empty means a fresh one each start,
    /// written into `api.json` where the mod reads it.
    ///
    /// Only worth setting where that file cannot reach the mod — a mod on another
    /// machine — in which case the same value goes on both sides.
    pub api_token: String,

    /// Whether a marker whose owner has not decided is everyone's.
    ///
    /// Off, so a marker a player drops is theirs until they say otherwise —
    /// somebody's unfinished base is not the server's business by default. An
    /// operator running a map everyone is meant to share turns it on, and every
    /// marker without a decision of its own becomes public.
    ///
    /// The mod reads it too: it decides both what the in-game map shares and what
    /// the web map shows, and those must be the same answer or the two disagree
    /// about who can see what.
    pub markers_public: bool,

    /// Whether a marker anybody can see is a marker anybody can change.
    ///
    /// Off, because being shown something is not being handed it. An operator
    /// running a map the server keeps together — trader routes, roads nobody
    /// owns — turns it on, and a public marker becomes everyone's to correct. A
    /// private marker is never anybody's but its owner's whatever this says.
    ///
    /// The mod reads it too, and it is the mod that enforces it: what the page
    /// makes of this only decides whether an edit is offered.
    pub markers_public_editable: bool,

    /// How many threads render tiles. Zero decides from the machine, capped so
    /// that the game server this usually shares a box with keeps its cores.
    pub threads: usize,

    /// How much memory rendered tiles may occupy before the least used are
    /// dropped. They are rebuilt on demand, so this costs time and not the map.
    pub tile_cache_mb: usize,

    /// Whether the server mod starts this service itself.
    ///
    /// Read by the mod rather than by anything here — it is a setting about who
    /// runs the map, and the only sensible place for it is beside everything else
    /// about the map. Turn it off to run `witchlight serve` yourself, which is what
    /// a map that should outlive the game server wants.
    pub autostart: bool,

    /// Whether the server mod tells a player where the map is when they join.
    ///
    /// Read by the mod, like `autostart`: a map nobody knows the address of is a
    /// map nobody looks at, and the mod is the half that can say so in chat.
    pub announce: bool,

    /// What to tell them, when it is not where this is listening.
    ///
    /// Empty means the address this works out for itself, which is right on a
    /// machine somebody can reach directly and wrong everywhere else: a server on
    /// the open internet is reached at a name, through a proxy, on a port this
    /// never sees. Only an operator knows that address, so only an operator can
    /// set it.
    pub announce_url: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            vs_data: default_vs_data(),
            map_data: PathBuf::new(),
            per_world: false,
            bind: "0.0.0.0:8080".to_owned(),
            api_bind: String::new(),
            api_token: String::new(),
            markers_public: false,
            markers_public_editable: false,
            threads: 0,
            tile_cache_mb: 256,
            autostart: true,
            announce: true,
            announce_url: String::new(),
        }
    }
}

/// What to say about a setting this build no longer has.
///
/// `deny_unknown_fields` is what catches a misspelled setting rather than letting
/// it read as a default, and it catches a renamed one the same way — correctly,
/// and unhelpfully. A settings file older than the build stops the service dead,
/// so the one thing worth saying is which name replaced which.
///
/// One list rather than a check at the point of each rename, so the next one is
/// an entry and not another branch.
fn retired(text: &str) -> String {
    const RETIRED: [(&str, &str); 1] = [(
        "api_socket",
        "api_bind, which is an address rather than a unix socket path. Leave it \
         empty for loopback on a free port, which is where the mod now looks",
    )];

    let mut said = String::new();
    for (was, now) in RETIRED {
        if text.lines().any(|line| line.trim_start().starts_with(was)) {
            said.push_str(&format!("\n\n`{was}` is now {now}."));
        }
    }
    said
}

/// The directories one level down that are maps in their own right.
///
/// Filed per world, one directory each, and a palette is what says a directory
/// is a map. Anything else in there — a stray folder, something half-copied — is
/// not one and is not offered as one.
fn maps_inside(base: &Path) -> Vec<PathBuf> {
    let Ok(listing) = std::fs::read_dir(base) else {
        return Vec::new();
    };
    listing
        .flatten()
        .map(|found| found.path())
        .filter(|path| crate::palette::path_in(path).exists())
        .collect()
}

/// Where the game puts its data when nobody has told it otherwise.
fn default_vs_data() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("VintagestoryData")
}

/// `~/.config/witchlight/config.toml`.
#[must_use]
pub fn default_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("witchlight")
        .join("config.toml")
}

impl Config {
    /// The marker rules this settings file states.
    #[must_use]
    pub fn marker_rules(&self) -> MarkerRules {
        MarkerRules { public: self.markers_public, public_editable: self.markers_public_editable }
    }

    /// Loads `path`. A missing file is not an error — the defaults are a working
    /// configuration — but a malformed one is.
    pub fn load(path: &Path) -> Result<Self> {
        let text = match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Self::default());
            }
            Err(error) => return Err(Error::io(format!("reading {}", path.display()), error)),
        };

        toml::from_str(&text).map_err(|error| {
            Error::parse(path, format!("{error}{}", retired(&text)))
        })
    }

    pub fn write(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|error| Error::io(format!("creating {}", parent.display()), error))?;
        }
        std::fs::write(path, self.to_template())
            .map_err(|error| Error::io(format!("writing {}", path.display()), error))
    }

    /// Where map data is kept, before any per-world directory inside it.
    #[must_use]
    pub fn map_data_dir(&self) -> PathBuf {
        if self.map_data.as_os_str().is_empty() {
            self.vs_data.join("witchlight")
        } else {
            self.map_data.clone()
        }
    }

    /// Which directory holds the map to serve.
    ///
    /// The mod names it outright when it starts this, because the mod is the half
    /// that knows which world is running. Everything here is for a service run by
    /// hand, which has to work it out from what is on disk.
    ///
    /// A palette is what makes a directory a map rather than a folder of them, so
    /// it is what the looking looks for. One directly inside means the map is
    /// there; one a level down means the maps are filed per world, and a single
    /// one of those is not a choice to make anybody type out. Several is, and
    /// saying which were found beats picking one of them.
    pub fn exports(&self, told: Option<&Path>) -> Result<PathBuf> {
        if let Some(told) = told {
            return Ok(told.to_path_buf());
        }

        let base = self.map_data_dir();
        if crate::palette::path_in(&base).exists() {
            return Ok(base);
        }

        // A set of files copied off a server with `scp`, handed straight to
        // `--vs-data`, rather than the data path it was copied out of.
        if self.map_data.as_os_str().is_empty() && crate::palette::path_in(&self.vs_data).exists() {
            return Ok(self.vs_data.clone());
        }

        let mut worlds = maps_inside(&base);
        worlds.sort();
        match worlds.len() {
            0 => Ok(base),
            1 => Ok(worlds.remove(0)),
            _ => Err(Error::config(format!(
                "{} holds {} worlds and nothing said which to serve. \
                 Name one with --exports:\n{}",
                base.display(),
                worlds.len(),
                worlds
                    .iter()
                    .map(|path| format!("  --exports {}", path.display()))
                    .collect::<Vec<_>>()
                    .join("\n")
            ))),
        }
    }

    #[must_use]
    pub fn to_template(&self) -> String {
        let body = toml::to_string_pretty(self).unwrap_or_else(|error| format!("# {error}\n"));
        format!(
            "# witchlight configuration\n\
             # vs_data is the server's --dataPath; exports are read from the\n\
             # `witchlight` folder inside it.\n\
             # map_data overrides that folder. Empty is the one above.\n\
             # per_world files each world's map in a directory of its own inside\n\
             # it. Off for a dedicated server, which runs one world. On for\n\
             # singleplayer, where every save shares one data path and would\n\
             # otherwise write its terrain into the last world's map. Turning it\n\
             # on moves the map already there down into its own directory rather\n\
             # than leaving it to be written over.\n\
             # api_bind is where the mod posts live data. Empty means loopback\n\
             # on a port the machine picks, written to api.json beside the map\n\
             # where the mod reads it. Set a host:port, and api_token to match\n\
             # on both sides, only for a mod on another machine.\n\
             # markers_public decides a marker nobody has chosen for: off keeps\n\
             # one to its owner, on shares it with everybody. Read by the mod as\n\
             # well as here, so the in-game map and the web map agree.\n\
             # markers_public_editable lets anybody change a marker anybody can\n\
             # see. Off, so a public marker is readable by all and writable by\n\
             # its owner; on, the server corrects its own map together.\n\
             # threads is how many requests are answered at once; 0 decides.\n\
             # tile_cache_mb is how much memory rendered tiles may hold.\n\
             # autostart is whether the server mod runs this service itself.\n\
             # Turn it off to run `witchlight serve` by hand, which is what a map\n\
             # that should outlive the game server wants.\n\
             # announce is whether the mod tells a player where the map is when\n\
             # they join. announce_url is what to tell them: empty means the\n\
             # address this works out for itself, which is right on a machine\n\
             # they can reach directly and wrong behind a proxy or a domain.\n\n{body}"
        )
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    /// A directory of one test's own, emptied first so a previous run cannot
    /// answer for this one, and taken away again afterwards.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new(name: &str) -> Self {
            let at = std::env::temp_dir().join(format!("witchlight-config-{name}"));
            let _ = std::fs::remove_dir_all(&at);
            std::fs::create_dir_all(&at).expect("a scratch directory");
            Self(at)
        }

        /// A directory holding a palette, which is what makes one a map.
        fn map(&self, at: &str) -> PathBuf {
            let path = if at.is_empty() { self.0.clone() } else { self.0.join(at) };
            std::fs::create_dir_all(&path).expect("a map directory");
            std::fs::write(crate::palette::path_in(&path), "{}").expect("a palette");
            path
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn at(data: &Path) -> Config {
        Config { map_data: data.to_path_buf(), ..Config::default() }
    }

    #[test]
    fn map_data_is_the_witchlight_folder_in_the_data_path_unless_told_otherwise() {
        let held = Config { vs_data: PathBuf::from("/srv/vs"), ..Config::default() };
        assert_eq!(held.map_data_dir(), PathBuf::from("/srv/vs/witchlight"));

        let moved = Config { map_data: PathBuf::from("/mnt/maps"), ..held };
        assert_eq!(moved.map_data_dir(), PathBuf::from("/mnt/maps"));
    }

    #[test]
    fn a_named_directory_is_served_whatever_else_is_on_disk() {
        // The mod names it, because the mod is the half that knows which world is
        // running. Nothing here may talk it out of that.
        let scratch = Scratch::new("told");
        scratch.map("one");
        scratch.map("two");
        let told = PathBuf::from("/somewhere/else");
        assert_eq!(at(&scratch.0).exports(Some(&told)).expect("the one named"), told);
    }

    #[test]
    fn a_map_directly_inside_is_the_map() {
        let scratch = Scratch::new("flat");
        let flat = scratch.map("");
        assert_eq!(at(&flat).exports(None).expect("the map itself"), flat);
    }

    #[test]
    fn one_world_inside_needs_nobody_to_type_it_out() {
        let scratch = Scratch::new("one-world");
        let world = scratch.map("Ashlands-0c4419ae");
        assert_eq!(at(&scratch.0).exports(None).expect("the only world"), world);
    }

    #[test]
    fn several_worlds_inside_is_a_question_rather_than_a_guess() {
        let scratch = Scratch::new("two-worlds");
        scratch.map("Ashlands-0c4419ae");
        scratch.map("New World-3f8a1c04");
        // A folder that is not a map is not offered as one.
        std::fs::create_dir_all(scratch.0.join("tiles")).expect("a folder");

        let complaint = at(&scratch.0).exports(None).expect_err("a question").to_string();
        assert!(complaint.contains("2 worlds"), "it says how many: {complaint}");
        assert!(complaint.contains("Ashlands-0c4419ae"), "and names them: {complaint}");
        assert!(complaint.contains("--exports"), "and says what to do: {complaint}");
    }

    #[test]
    fn nothing_exported_yet_is_the_directory_the_mod_will_fill() {
        // Every server is here on a first run, and refusing to start then is a
        // map service that is down exactly when somebody is watching it.
        let scratch = Scratch::new("empty");
        assert_eq!(at(&scratch.0).exports(None).expect("somewhere to look"), scratch.0);
    }
}
