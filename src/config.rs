//! Defines the settings file, its location, and how it is read and written.
//!
//! Settings are read from a TOML file. Command-line flags override the file. On
//! the first run the file is written with the defaults, so there is always a file
//! to edit.
//!
//! The file's location depends on who started the service. Run by hand, the
//! service reads `~/.config/witchlight/config.toml`. Started by the server mod,
//! it reads `witchlight.conf` in the game's `ModConfig` folder, which the mod
//! passes with `--config`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::util::error::{Error, Result};

/// Holds the settings the web page and the live feeds need from the config.
///
/// This is the subset of `Config` that decides which controls the page offers and
/// how the service scopes what each viewer is sent. The values are copied out of
/// `Config` by `Config::rules`, which also clamps `live_refresh_ms`.
///
/// The mod enforces the visibility rules on its own copy of these settings. The
/// service uses them to decide what to send and what to show.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rules {
    /// When true, a new marker whose owner has not chosen a visibility is
    /// visible to everyone. When false, only its owner sees it.
    pub allow_public_markers: bool,
    /// When true, any signed-in player may edit a public marker. When false,
    /// only the owner may edit it.
    pub allow_editing_public_markers: bool,
    /// When true, every player's position is shown to every viewer. When
    /// false, a player's position is shown only to members of their own group.
    pub show_players_to_everyone: bool,
    /// The interval between live polls from the page, in milliseconds. Already
    /// clamped to the range `REFRESH_FLOOR_MS..=REFRESH_CEILING_MS`.
    pub live_refresh_ms: u64,
    /// When true, each player sees only the terrain they have explored. When
    /// false, every viewer sees the whole map. See [`crate::mapdata::memory`].
    pub personal_maps: bool,
    /// When true and `personal_maps` is on, the terrain around spawn is shown to
    /// every viewer, including a browser that is not signed in.
    pub show_spawn_to_guests: bool,
    /// The radius of the spawn area that `show_spawn_to_guests` reveals, in
    /// chunks.
    pub spawn_radius_chunks: i32,
    /// The radius a player reveals around themselves, in chunks. Zero means the
    /// view distance the game granted that player, as reported by the mod.
    pub sight_radius_chunks: i32,
    /// How long a browser session stays valid after its last request, in hours.
    /// Zero means sessions never expire.
    pub session_hours: u64,
    /// When true, every browser session is invalidated when the service starts.
    pub invalidate_sessions_on_restart: bool,
    /// Player group names the map ignores. Compared case-insensitively. See
    /// [`Config::hidden_groups`].
    pub hidden_groups: Vec<String>,
}

/// The privilege required to run each `/witchlight` command in game.
///
/// Each value is a Vintage Story privilege code such as `controlserver`, `chat`
/// or `commandplayer`. The shorthand `admin` and `player` cover most servers.
/// The mod checks these when a command is typed; the service does not read them.
///
/// Each value controls who may start the request. It does not control which
/// client answers it. The mod asks whichever client can answer, and only an
/// admin's palette or icon set may replace one already chosen. See the mod's
/// `PaletteExchange` and `IconExchange`.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct Commands {
    /// `/witchlight login`: sends the player a link that signs their browser in.
    pub login: String,
    /// `/witchlight mark`: places a marker on the block the player is looking at.
    pub mark: String,
    /// `/witchlight portrait`: asks a client for a picture of its player.
    pub portrait: String,
    /// `/witchlight palette`: asks a client for the block colour palette.
    pub palette: String,
    /// `/witchlight icons`: asks a client for the marker icons.
    pub icons: String,
    /// `/witchlight export`: writes the surface of every loaded chunk.
    pub export: String,
    /// `/witchlight status`: reports the state of the map and the service.
    pub status: String,
    /// `/witchlight service`: starts and stops the map service.
    pub service: String,
}

/// Controls what the map does with land claims.
///
/// `view` and `create` are privilege codes, written the same way as `[commands]`.
/// `worldgen` is a switch. The mod enforces all three by deciding what it sends
/// to the service. The service keeps the mod's answer so that no browser is sent
/// a claim its viewer may not see, and the page uses it to decide whether to
/// offer the claim buttons.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct Claims {
    /// The privilege required to see claims on the map.
    pub view: String,
    /// The privilege required to draw a new claim on the map.
    pub create: String,
    /// When true, the map draws the claims the world generator created, such as
    /// the perimeters around trader camps and story structures. When false, it
    /// does not.
    ///
    /// Default false. Those claims reveal the location of every trader on the
    /// server, which the game does not otherwise give any player.
    pub worldgen: bool,
}

impl Default for Claims {
    fn default() -> Self {
        Self {
            // The game already sends every claim to every client, so the map
            // shows them to every player by default.
            view: PLAYER.to_owned(),
            // The same privilege the game requires for `/land claim`, so the map
            // cannot grant land to anyone the game would refuse.
            create: Privilege::CLAIM_LAND.to_owned(),
            worldgen: false,
        }
    }
}

/// Privilege codes from the game that this service names directly.
///
/// Only the codes used as defaults are listed, so the settings file, the template
/// and the tests spell each one the same way.
pub struct Privilege;

impl Privilege {
    /// The privilege Vintage Story requires for `/land claim`.
    pub const CLAIM_LAND: &'static str = "claimland";
}

/// The shortest live poll interval the page is ever told, in milliseconds.
///
/// A value below this would have the browser ask again the instant it was
/// answered. Values below the floor are clamped rather than refused, so a number
/// typed in seconds by mistake still gives a working map.
pub const REFRESH_FLOOR_MS: u64 = 250;

/// The longest live poll interval the page is ever told, in milliseconds.
///
/// The marker form waits for confirmation on this interval, and past one minute
/// it has given up before the confirmation arrives.
pub const REFRESH_CEILING_MS: u64 = 60_000;

/// The shorthand privilege that means "server admins".
pub const ADMIN: &str = "admin";

/// The shorthand privilege that means "every player".
pub const PLAYER: &str = "player";

impl Default for Commands {
    fn default() -> Self {
        Self {
            login: PLAYER.to_owned(),
            mark: PLAYER.to_owned(),
            portrait: PLAYER.to_owned(),
            palette: ADMIN.to_owned(),
            icons: PLAYER.to_owned(),
            export: ADMIN.to_owned(),
            status: ADMIN.to_owned(),
            service: ADMIN.to_owned(),
        }
    }
}

/// Holds every setting in the configuration file.
///
/// The doc comment on each field describes the setting. The text written into the
/// operator's file comes from `NOTES`, and a test checks that every field has a
/// note.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    /// The Vintage Story data directory, which is the game server's `--dataPath`.
    /// Map data is read from the `witchlight` folder inside it unless `map_data`
    /// names another location.
    pub vs_data: PathBuf,

    /// The directory that holds the map data. Empty means the `witchlight`
    /// folder inside `vs_data`. Set it to keep the map on another disk or in a
    /// directory a web server already serves.
    pub map_data: PathBuf,

    /// When true, each world's map is kept in its own subdirectory of
    /// `map_data`. When false, the map is kept directly in `map_data`.
    ///
    /// Default true. Every singleplayer save shares one data path, and without
    /// this a second world would overwrite the first world's map. Turning it on
    /// moves an existing map into its own subdirectory.
    ///
    /// The mod reads this and passes the chosen directory to the service with
    /// `--exports`. The service reads it only when run by hand.
    pub per_world: bool,

    /// The address and port the map is served on. Default `0.0.0.0:8080`, which
    /// listens on every interface. Use `127.0.0.1:8080` to serve only this
    /// machine.
    pub bind: String,

    /// The address and port the mod posts live data to: player positions,
    /// markers and claims.
    ///
    /// Empty means loopback on a free port, published in `api.json` beside the
    /// map so the mod finds it on its own. Set a `host:port` only when the mod
    /// runs on another machine, in which case `api_token` must also be set.
    pub api_bind: String,

    /// The bearer token the mod must present when posting live data. Empty means
    /// a new token is generated on each start and written to `api.json` for the
    /// mod to read. Set it only when the mod runs on another machine, and set
    /// the same value on both sides.
    pub api_token: String,

    /// When true, a new marker whose owner has not chosen a visibility is
    /// visible to everyone. When false, only its owner sees it.
    ///
    /// Default false. Read by both the mod and the service, so the in-game map
    /// and the web map agree.
    pub allow_public_markers: bool,

    /// When true, any signed-in player may edit a public marker. When false,
    /// only the marker's owner may edit it. A private marker can only ever be
    /// edited by its owner.
    ///
    /// Default false. The mod enforces it. The page reads it to decide whether
    /// to offer the edit controls.
    pub allow_editing_public_markers: bool,

    /// When true, every player's position is shown to every viewer of the map.
    /// When false, a player's position is shown only to members of their own
    /// player group. The number of players online is shown to everyone either
    /// way.
    ///
    /// Default true. `personal_maps` overrides it: while personal maps are on,
    /// positions are always restricted to the player's own group.
    ///
    /// The mod enforces it, because the mod is the half that knows the groups.
    pub show_players_to_everyone: bool,

    /// Player group names the map ignores. A group listed here is never offered
    /// for sharing a map, never counted, and never shown in the Group tab of the
    /// player list.
    ///
    /// Default `["xlib"]`. Some mods put every player into one group, and a
    /// group that contains everyone is not useful for sharing. Names are compared
    /// case-insensitively.
    pub hidden_groups: Vec<String>,

    /// When true, each player sees only the terrain they have explored, as it
    /// looked when they last saw it. When false, every viewer sees the whole map
    /// as it is now.
    ///
    /// Default true. Read by both halves. While it is on, the mod restricts each
    /// player's position to their own group regardless of
    /// `show_players_to_everyone`.
    pub personal_maps: bool,

    /// When true and `personal_maps` is on, the terrain around spawn is shown to
    /// every viewer, including a browser that is not signed in. When false, a
    /// browser that is not signed in sees nothing until someone signs in.
    ///
    /// Default true.
    pub show_spawn_to_guests: bool,

    /// The radius of the spawn area that `show_spawn_to_guests` reveals, in
    /// chunks. Default 8, which is a square about half a kilometre across.
    pub spawn_radius_chunks: i32,

    /// The radius a player reveals around themselves as they move, in chunks.
    /// Zero means the view distance the game granted that player, which is as
    /// far as the game loads chunks for them. Default 0.
    pub sight_radius_chunks: i32,

    /// How long a browser session stays valid after its last request, in hours.
    /// Zero means sessions never expire: a session lasts until the browser signs
    /// out or `invalidate_sessions_on_restart` clears it.
    ///
    /// Default 0. Set it on a server where a browser left signed in on a shared
    /// machine is a bigger concern than asking for a login link again.
    pub session_hours: u64,

    /// When true, every browser session is invalidated when the service starts,
    /// and every viewer must sign in again. When false, sessions are kept in the
    /// map's database and survive a restart.
    ///
    /// Default false.
    pub invalidate_sessions_on_restart: bool,

    /// The interval between live polls from the page, in milliseconds.
    ///
    /// Default 1000. The page is normally told of changes as they happen through
    /// `/events`, and polls on this interval only when that connection is not
    /// available, for example behind a proxy that does not hold requests open.
    /// Players, markers and claims all arrive on the same poll.
    ///
    /// Clamped to `REFRESH_FLOOR_MS..=REFRESH_CEILING_MS` by `Config::rules`.
    /// The page receives the value when it is served, so a change takes effect
    /// after the service restarts and the page reloads.
    pub live_refresh_ms: u64,

    /// The interval between terrain exports by the mod, in milliseconds.
    ///
    /// Default 10000. A chunk that changes several times within one interval is
    /// written once, so a larger value means less disk activity and a less
    /// current map. The mod clamps it to the range 1000 to 600000, and a world
    /// save exports whatever the interval was holding.
    ///
    /// Read by the mod only.
    pub export_interval_ms: u64,

    /// The radius around a player that the terrain puller may fill in, in
    /// chunks, when the mod has not reported that player's view distance.
    ///
    /// Default 0, which means the game server's `MaxChunkRadius`. That is the
    /// furthest the game loads chunks for anyone, so the map never draws ground
    /// no in-game map could have shown. Set it above zero to draw wider than the
    /// game shows its players.
    pub backfill_radius_chunks: i32,

    /// The number of threads that render tiles. Zero picks a count from the
    /// CPU count, leaving cores for the game server that usually shares the
    /// machine. Default 0.
    pub threads: usize,

    /// The memory budget for rendered tiles, in megabytes. When the budget is
    /// exceeded the least recently used tiles are dropped and rendered again on
    /// demand. Default 256.
    pub tile_cache_mb: usize,

    /// When true, the server mod starts and stops this service itself. When
    /// false, the operator runs `witchlight serve` by hand, which lets the map
    /// outlive the game server.
    ///
    /// Default true. Read by the mod only.
    pub autostart: bool,

    /// When true, the mod tells each player the map's address in chat when they
    /// join. Default true. Read by the mod only.
    pub announce: bool,

    /// The address the mod announces. Empty means the address the service works
    /// out for itself, which is correct on a LAN and wrong behind a proxy, a
    /// domain name or NAT. Set it to the address players actually use.
    pub announce_url: String,

    /// The privilege required to run each `/witchlight` command in game.
    ///
    /// Tables must come after every plain setting in a TOML file, so this and
    /// the tables below it are the last fields.
    pub commands: Commands,

    /// Controls who may see land claims, who may draw one, and whether
    /// generated claims are drawn.
    pub claims: Claims,

    /// Defines extra bars on each player's card, beside health and food.
    ///
    /// A mod that gives players a resource such as mana or stamina stores it as
    /// attributes on the player entity. Each entry here names those attributes,
    /// and the mod reads them the same way it reads health and hunger. The value
    /// format is `name | value attribute | maximum attribute | colour | group`.
    /// The key is only a label for the entry.
    ///
    /// The group is the heading the bar is filed under in the accessibility
    /// window. When the group is left out, the mod uses the id of an installed
    /// mod that appears in the attribute's name, if there is one.
    ///
    /// A bar is drawn only for a player who has the attribute with a maximum
    /// above zero. An entry for a mod that is not installed draws nothing.
    pub bars: BTreeMap<String, String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            vs_data: default_vs_data(),
            map_data: PathBuf::new(),
            per_world: true,
            bind: "0.0.0.0:8080".to_owned(),
            api_bind: String::new(),
            api_token: String::new(),
            allow_public_markers: false,
            allow_editing_public_markers: false,
            personal_maps: true,
            show_spawn_to_guests: true,
            spawn_radius_chunks: 8,
            sight_radius_chunks: 0,
            session_hours: 0,
            invalidate_sessions_on_restart: false,
            show_players_to_everyone: true,
            hidden_groups: vec!["xlib".to_owned()],
            live_refresh_ms: 1000,
            export_interval_ms: 10_000,
            backfill_radius_chunks: 0,
            threads: 0,
            tile_cache_mb: 256,
            autostart: true,
            announce: true,
            announce_url: String::new(),
            commands: Commands::default(),
            claims: Claims::default(),
            // The two bars a stock Rustbound Magic install provides. The keys
            // sort alphabetically in the file, so `mana` comes before `mana_exp`.
            bars: [
                (
                    "mana".to_owned(),
                    "Mana | entitybehavior-resource-currentmana_rm \
                     | entitybehavior-resource-totalmaxmana_rm | #7c5cff | Rustbound Magic"
                        .to_owned(),
                ),
                (
                    "mana_exp".to_owned(),
                    "Magic | entitybehavior-resource-currentexptonextmaxmanalevel_rm \
                     | entitybehavior-resource-maxexptonextmaxmanalevel_rm | #d8a24a | Rustbound Magic"
                        .to_owned(),
                ),
            ]
            .into_iter()
            .collect(),
        }
    }
}

/// Builds the text appended to a parse error when the file uses a retired key.
///
/// `deny_unknown_fields` rejects a renamed key the same way it rejects a
/// misspelled one. This function checks the file text for each retired name and
/// says which key replaced it, so the operator knows what to change.
fn retired(text: &str) -> String {
    const RETIRED: [(&str, &str); 8] = [
        (
            "api_socket",
            "api_bind, which is an address rather than a unix socket path. Leave it \
             empty for loopback on a free port, which is where the mod now looks",
        ),
        ("markers_public", "allow_public_markers"),
        ("markers_public_editable", "allow_editing_public_markers"),
        ("players_public", "show_players_to_everyone"),
        ("private_map", "personal_maps"),
        ("anonymous_spawn_radius_chunks", "spawn_radius_chunks"),
        ("anonymous_spawn", "show_spawn_to_guests"),
        ("sessions_reset_on_restart", "invalidate_sessions_on_restart"),
    ];

    let mut said = String::new();
    for (was, now) in RETIRED {
        // A key is matched as a whole word, so `anonymous_spawn` does not also
        // match `anonymous_spawn_radius_chunks`.
        let used = text.lines().any(|line| {
            let line = line.trim_start();
            line.starts_with(was)
                && line[was.len()..].chars().next().is_none_or(|next| next == ' ' || next == '=')
        });
        if used {
            said.push_str(&format!("\n\n`{was}` is now {now}."));
        }
    }
    said
}

/// Lists the subdirectories of `base` that are maps.
///
/// A directory is a map when it contains a palette file. Any other directory is
/// ignored.
fn maps_inside(base: &Path) -> Vec<PathBuf> {
    let Ok(listing) = std::fs::read_dir(base) else {
        return Vec::new();
    };
    listing
        .flatten()
        .map(|found| found.path())
        .filter(|path| crate::render::palette::path_in(path).exists())
        .collect()
}

/// Returns the game's default data directory.
fn default_vs_data() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("VintagestoryData")
}

/// Returns the default configuration file path,
/// `~/.config/witchlight/config.toml`.
#[must_use]
pub fn default_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("witchlight")
        .join("config.toml")
}

impl Config {
    /// Copies the settings the page and the feeds need into a `Rules`, clamping
    /// `live_refresh_ms` to its allowed range.
    #[must_use]
    pub fn rules(&self) -> Rules {
        Rules {
            allow_public_markers: self.allow_public_markers,
            allow_editing_public_markers: self.allow_editing_public_markers,
            show_players_to_everyone: self.show_players_to_everyone,
            // Clamped here so the page and the tests see the same number.
            live_refresh_ms: self.live_refresh_ms.clamp(REFRESH_FLOOR_MS, REFRESH_CEILING_MS),
            personal_maps: self.personal_maps,
            show_spawn_to_guests: self.show_spawn_to_guests,
            spawn_radius_chunks: self.spawn_radius_chunks,
            sight_radius_chunks: self.sight_radius_chunks,
            session_hours: self.session_hours,
            invalidate_sessions_on_restart: self.invalidate_sessions_on_restart,
            hidden_groups: self.hidden_groups.clone(),
        }
    }

    /// Loads the configuration from `path`. A missing file yields the defaults.
    /// A file that cannot be read or parsed is an error.
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

    /// Writes the configuration to `path` as a commented TOML file, creating the
    /// parent directory if needed.
    pub fn write(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|error| Error::io(format!("creating {}", parent.display()), error))?;
        }
        std::fs::write(path, self.to_template())
            .map_err(|error| Error::io(format!("writing {}", path.display()), error))
    }

    /// Returns the directory that holds the map data, before any per-world
    /// subdirectory.
    #[must_use]
    pub fn map_data_dir(&self) -> PathBuf {
        if self.map_data.as_os_str().is_empty() {
            self.vs_data.join("witchlight")
        } else {
            self.map_data.clone()
        }
    }

    /// Resolves the directory that holds the map to serve.
    ///
    /// `told` is the `--exports` flag, which the mod always passes. When it is
    /// absent the service looks on disk: a palette directly inside the map data
    /// directory means the map is there; a palette one level down means maps are
    /// filed per world. One world is served without being named. Several worlds
    /// is an error that lists them, so the operator can name one.
    pub fn exports(&self, told: Option<&Path>) -> Result<PathBuf> {
        if let Some(told) = told {
            return Ok(told.to_path_buf());
        }

        let base = self.map_data_dir();
        if crate::render::palette::path_in(&base).exists() {
            return Ok(base);
        }

        // Handles a map directory copied off a server and passed as
        // `--vs-data` instead of the data path it came from.
        if self.map_data.as_os_str().is_empty() && crate::render::palette::path_in(&self.vs_data).exists() {
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
}

mod template;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::files::testing::Scratch;

    /// Creates a directory holding a palette. A palette is what makes a
    /// directory a map.
    fn map(held: &Scratch, at: &str) -> PathBuf {
        let path = if at.is_empty() { held.at().to_path_buf() } else { held.at().join(at) };
        std::fs::create_dir_all(&path).expect("a map directory");
        std::fs::write(crate::render::palette::path_in(&path), "{}").expect("a palette");
        path
    }

    fn at(data: &Path) -> Config {
        Config { map_data: data.to_path_buf(), ..Config::default() }
    }

    #[test]
    fn the_commands_an_operator_never_touched_are_the_ones_the_mod_would_have_chosen() {
        let held = Config::default().commands;
        assert_eq!(held.export, ADMIN, "writing the world is an operator's");
        assert_eq!(held.status, ADMIN);
        assert_eq!(held.service, ADMIN, "so is starting and stopping the map");
        assert_eq!(held.login, PLAYER, "a link to your own page is your own");
        assert_eq!(held.mark, PLAYER);
        assert_eq!(held.palette, ADMIN, "repainting every block on the map is an operator's to start");
        assert_eq!(held.icons, PLAYER);
        assert_eq!(held.portrait, PLAYER);
    }

    #[test]
    fn the_claim_gates_start_where_the_game_already_stands() {
        let held = Config::default().claims;
        // The game sends every claim to every client, so a map that hid them
        // would tell players less than the game does.
        assert_eq!(held.view, PLAYER, "where a claim is, is already everybody's");
        // The map must never be a way round a rule the server already has.
        assert_eq!(
            held.create,
            Privilege::CLAIM_LAND,
            "taking land through the map asks what `/land claim` asks"
        );
        // This one starts closed. The game tells a client about the claim it
        // is standing near, never about every trader camp at once.
        assert!(!held.worldgen, "the world's own perimeters are not drawn unless asked for");
    }

    /// Checks the clamp on the only setting here that becomes a browser timer.
    #[test]
    fn the_live_beat_is_held_to_a_gap_a_browser_can_keep_up_with() {
        let told = |ms| Config { live_refresh_ms: ms, ..Config::default() }.rules().live_refresh_ms;
        assert_eq!(Config::default().live_refresh_ms, 1000, "one second: the clock the page falls back to");
        assert_eq!(told(500), 500, "what an operator asked for is what the page is told");
        // A gap of nothing makes the browser ask again the instant it is
        // answered.
        assert_eq!(told(0), REFRESH_FLOOR_MS);
        // A number typed in seconds by mistake gives a fast map instead.
        assert_eq!(told(2), REFRESH_FLOOR_MS);
        // Past a minute the marker form gives up before the poll that would
        // have confirmed it.
        assert_eq!(told(600_000), REFRESH_CEILING_MS);
    }

    #[test]
    fn the_written_template_reads_back_as_what_wrote_it() {
        // The tables must serialise after every plain setting, or the settings
        // below them are read as part of the table. This test enforces that
        // field order.
        let held = Config {
            commands: Commands { export: "commandplayer".to_owned(), ..Commands::default() },
            claims: Claims { view: ADMIN.to_owned(), ..Claims::default() },
            ..Config::default()
        };
        let read: Config =
            toml::from_str(&held.to_template()).expect("the template this just wrote");
        assert_eq!(read.commands, held.commands);
        assert_eq!(read.claims, held.claims, "and so does every other table");
        assert_eq!(read.announce_url, held.announce_url, "and nothing fell into the table");
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
        // The mod names the directory, because only the mod knows which world
        // is running. Nothing here may override it.
        let scratch = Scratch::new("config-told");
        map(&scratch, "one");
        map(&scratch, "two");
        let told = PathBuf::from("/somewhere/else");
        assert_eq!(at(scratch.at()).exports(Some(&told)).expect("the one named"), told);
    }

    #[test]
    fn a_map_directly_inside_is_the_map() {
        let scratch = Scratch::new("config-flat");
        let flat = map(&scratch, "");
        assert_eq!(at(&flat).exports(None).expect("the map itself"), flat);
    }

    #[test]
    fn one_world_inside_needs_nobody_to_type_it_out() {
        let scratch = Scratch::new("config-one-world");
        let world = map(&scratch, "Ashlands-0c4419ae");
        assert_eq!(at(scratch.at()).exports(None).expect("the only world"), world);
    }

    #[test]
    fn several_worlds_inside_is_a_question_rather_than_a_guess() {
        let scratch = Scratch::new("config-two-worlds");
        map(&scratch, "Ashlands-0c4419ae");
        map(&scratch, "New World-3f8a1c04");
        // A folder that is not a map must not be offered as one.
        std::fs::create_dir_all(scratch.at().join("tiles")).expect("a folder");

        let complaint = at(scratch.at()).exports(None).expect_err("a question").to_string();
        assert!(complaint.contains("2 worlds"), "it says how many: {complaint}");
        assert!(complaint.contains("Ashlands-0c4419ae"), "and names them: {complaint}");
        assert!(complaint.contains("--exports"), "and says what to do: {complaint}");
    }

    #[test]
    fn nothing_exported_yet_is_the_directory_the_mod_will_fill() {
        // Every server is in this state on a first run. Refusing to start then
        // would take the map service down exactly when it is being watched.
        let scratch = Scratch::new("config-empty");
        assert_eq!(at(scratch.at()).exports(None).expect("somewhere to look"), scratch.at());
    }
}
