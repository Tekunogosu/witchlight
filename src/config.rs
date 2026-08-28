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

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    /// The Vintage Story data directory — the server's `--dataPath`. Exports are
    /// read from the `witchlight` folder inside it.
    pub vs_data: PathBuf,

    /// Address to listen on. All interfaces by default, so the map is reachable
    /// from the rest of the network without further configuration. Set it to
    /// `127.0.0.1:8080` to keep it on this machine only.
    pub bind: String,

    /// Where the server mod posts who is online and where the markers are. Empty
    /// means a socket in `/tmp` named after the export directory, which is where
    /// the mod looks unless it has been told otherwise. A `host:port` is accepted
    /// for a mod running on another machine; a path is taken as a socket.
    pub api_socket: String,

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
            bind: "0.0.0.0:8080".to_owned(),
            api_socket: String::new(),
            threads: 0,
            tile_cache_mb: 256,
            autostart: true,
            announce: true,
            announce_url: String::new(),
        }
    }
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

        toml::from_str(&text).map_err(|error| Error::parse(path, error.to_string()))
    }

    pub fn write(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|error| Error::io(format!("creating {}", parent.display()), error))?;
        }
        std::fs::write(path, self.to_template())
            .map_err(|error| Error::io(format!("writing {}", path.display()), error))
    }

    /// Where the mod's exports actually are.
    ///
    /// The mod writes into `<vs data>/witchlight`, which is what `vs_data` points
    /// at. A directory that holds the exports directly is accepted too, so a set
    /// of files copied off a server with `scp` works without a second flag.
    #[must_use]
    pub fn exports(&self) -> PathBuf {
        let nested = self.vs_data.join("witchlight");
        if nested.join("palette.json").exists() {
            return nested;
        }
        if self.vs_data.join("palette.json").exists() {
            return self.vs_data.clone();
        }
        nested
    }

    #[must_use]
    pub fn to_template(&self) -> String {
        let body = toml::to_string_pretty(self).unwrap_or_else(|error| format!("# {error}\n"));
        format!(
            "# witchlight configuration\n\
             # vs_data is the server's --dataPath; exports are read from the\n\
             # `witchlight` folder inside it.\n\
             # api_socket is where the mod posts live data. Empty means a unix\n\
             # socket in /tmp named after this folder, which is where the mod\n\
             # looks; a host:port is accepted instead.\n\
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
