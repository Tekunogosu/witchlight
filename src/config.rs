//! Where the exports are and where to serve them from.
//!
//! Settings come from `~/.config/mapstique/config.toml`, and command-line flags
//! win over the file. The file is written with the defaults on a first run, so
//! there is always something to edit.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    /// The Vintage Story data directory — the server's `--dataPath`. Exports are
    /// read from the `mapstique` folder inside it.
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
}

impl Default for Config {
    fn default() -> Self {
        Self {
            vs_data: default_vs_data(),
            bind: "0.0.0.0:8080".to_owned(),
            api_socket: String::new(),
            threads: 0,
        }
    }
}

/// Where the game puts its data when nobody has told it otherwise.
fn default_vs_data() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("VintagestoryData")
}

/// `~/.config/mapstique/config.toml`.
#[must_use]
pub fn default_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("mapstique")
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
    /// The mod writes into `<vs data>/mapstique`, which is what `vs_data` points
    /// at. A directory that holds the exports directly is accepted too, so a set
    /// of files copied off a server with `scp` works without a second flag.
    #[must_use]
    pub fn exports(&self) -> PathBuf {
        let nested = self.vs_data.join("mapstique");
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
            "# mapstique configuration\n\
             # vs_data is the server's --dataPath; exports are read from the\n\
             # `mapstique` folder inside it.\n\
             # api_socket is where the mod posts live data. Empty means a unix\n\
             # socket in /tmp named after this folder, which is where the mod\n\
             # looks; a host:port is accepted instead.\n\
             # threads is how many requests are answered at once; 0 decides.\n\n{body}"
        )
    }
}
