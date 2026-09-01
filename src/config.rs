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

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

/// What the operator has decided that the page must be told.
///
/// Who a marker belongs to when nobody has said otherwise, whether where
/// somebody is standing is everybody's to see, and how often to ask for the lot.
/// They travel together so the half asking never has one without the others —
/// and because a function taking eight loose arguments is a function taking a
/// settings file badly.
///
/// Nothing here is enforced here. The mod is the half that decides who is sent
/// what; what these settle is which controls the page offers and how often it
/// asks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rules {
    /// Whether a marker nobody has decided about is everyone's.
    pub markers_public: bool,
    /// Whether a marker anybody can see is a marker anybody can change.
    pub markers_editable: bool,
    /// Whether where a player is standing is everybody's to see. Enforced by the
    /// mod; what this decides here is only what the page is told, so that a short
    /// list of players reads as a server that chose it rather than as a fault.
    pub players_public: bool,
    /// How long the page leaves between asking where everybody is, in
    /// milliseconds. Already held to a gap a browser can keep up with — the
    /// clamping is `Config::rules`, so nothing downstream has to wonder.
    pub live_refresh_ms: u64,
}

/// Which privilege each `wl` command asks of whoever types it.
///
/// A privilege code the game knows — `controlserver`, `chat`, `commandplayer` —
/// with `admin` and `player` spelled out for the two that answer almost every
/// server. The mod is the half that enforces it; nothing here reads these, for
/// the same reason nothing here reads `autostart`.
///
/// The split is between commands that change what the server is doing and
/// commands that answer a question about the person typing them. Exporting the
/// world, reading the map's whole state and starting or stopping the service are
/// an operator's; a link to your own page, a marker where you are standing, and
/// asking a client for the pictures the map draws with are anybody's.
///
/// Loosening one of the last three costs less than it looks: what a client sends
/// back is taken on the same terms whoever asked for it, and only an admin's
/// palette or icon may replace one already chosen — see the mod's
/// `PaletteExchange` and `IconExchange`. So these decide who may ask, not who
/// may repaint the map.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct Commands {
    /// A link that signs your own browser in as you.
    pub login: String,
    /// A marker where you are looking.
    pub mark: String,
    /// Asking a client for a picture of its player.
    pub portrait: String,
    /// Asking a client for a block colour palette.
    pub palette: String,
    /// Asking a client for the pictures markers are drawn with.
    pub icons: String,
    /// Writing the surface of every loaded chunk.
    pub export: String,
    /// What has been exported, and where the palette came from.
    pub status: String,
    /// Starting and stopping the map service.
    pub service: String,
}

/// Who may see the land claims on the map, and who may draw a new one.
///
/// Two privileges rather than one, because they are two different asks. Seeing
/// where a claim is answers "may I build here" and is the sort of thing a server
/// puts on a public map; drawing one takes land, and a server that shows every
/// boundary is not thereby a server where anybody may fence off a valley.
///
/// Written the way `[commands]` is — a privilege the game knows, with `admin`
/// and `player` spelled out for the two that answer most servers — because it is
/// the same question in the same file and a second spelling would be a second
/// thing to explain.
///
/// The mod is the half that enforces both. What the service does with them is
/// hold the lists of who the mod says may, so that a browser is never handed a
/// claim its reader may not see; what the page does with them is decide whether
/// to offer the button.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct Claims {
    /// Who may see where the claims are.
    pub view: String,
    /// Who may draw a new one from the map.
    pub create: String,
}

impl Default for Claims {
    fn default() -> Self {
        Self {
            // Where a claim is, is already everybody's: the game sends every
            // claim to every client and draws the borders for anyone holding the
            // right tool. A map that hid them would be telling players less than
            // the game does, so the default is what they already have and the
            // setting is for a server that wants less.
            view: PLAYER.to_owned(),
            // What the game asks of `/land claim`, and for the same reason. The
            // map must not be a way round a rule the server already has, so this
            // starts as that rule rather than as a looser one — and an operator
            // narrowing it here narrows the map alone, which is the point of its
            // being a setting.
            create: Privilege::CLAIM_LAND.to_owned(),
        }
    }
}

/// A privilege code this service names by hand.
///
/// Only where a default has to be one particular privilege of the game's rather
/// than `admin` or `player`. Spelled once so that the settings file, the template
/// and the tests cannot disagree about it.
pub struct Privilege;

impl Privilege {
    /// What Vintage Story asks of anybody running `/land claim`.
    pub const CLAIM_LAND: &'static str = "claimland";
}

/// The shortest gap the page is ever told to leave between live polls.
///
/// A gap of nothing is a browser asking again the instant it is answered, which
/// is a denial of service written into a settings file — and a quarter second is
/// already faster than the mod posts. Clamped rather than refused, so a number
/// somebody typed in seconds still leaves a working map.
pub const REFRESH_FLOOR_MS: u64 = 250;

/// The longest.
///
/// A map that says where people are once a minute is as slow as one still worth
/// calling live: a marker the page has just asked for is confirmed on this beat,
/// and past a minute the form has given up waiting before the answer arrives.
pub const REFRESH_CEILING_MS: u64 = 60_000;

/// What `admin` is short for.
pub const ADMIN: &str = "admin";

/// What `player` is short for.
pub const PLAYER: &str = "player";

impl Default for Commands {
    fn default() -> Self {
        Self {
            login: PLAYER.to_owned(),
            mark: PLAYER.to_owned(),
            portrait: PLAYER.to_owned(),
            palette: PLAYER.to_owned(),
            icons: PLAYER.to_owned(),
            export: ADMIN.to_owned(),
            status: ADMIN.to_owned(),
            service: ADMIN.to_owned(),
        }
    }
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

    /// Whether where a player is standing is everybody's to see.
    ///
    /// On, which is what a map of a server people play on together is for. An
    /// operator running a server where being findable is not part of the deal
    /// turns it off, and then a player appears to their own group and to nobody
    /// else — how many are on is still said to everyone, because that is a fact
    /// about the server rather than about anybody on it.
    ///
    /// Vintage Story has no setting of its own to follow here. Its server config
    /// says nothing about who may see whom, and the nearest thing in the world
    /// config — `allowMap` — decides whether there is a map at all, which is a
    /// different question. So this is witchlight's own, and it defaults to what
    /// the map has always done.
    ///
    /// The mod reads it and enforces it: it is the half that knows the groups,
    /// and a service holding positions it must not send is a service one bug away
    /// from sending them.
    pub players_public: bool,

    /// How long the page leaves between asking where everybody is, in
    /// milliseconds.
    ///
    /// Two seconds. Players, markers, claims and whether a marker just asked for
    /// has been made all arrive on this one beat, so this number is the whole of
    /// how fresh the live half of the map is. Lower it on a server where people
    /// watch each other move; raise it on one where a browser left open all day
    /// should cost the machine less.
    ///
    /// Milliseconds because the interesting choices sit inside a second of each
    /// other, and seconds would round every one of them to the same number.
    ///
    /// Held between `REFRESH_FLOOR_MS` and `REFRESH_CEILING_MS`, which is what
    /// keeps a zero out of a browser's timer. Read here alone: the page is told
    /// the number when it is served, so a change reaches a browser once the
    /// service has restarted and the page has been reloaded.
    pub live_refresh_ms: u64,

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

    /// Who may run each `wl` command in game.
    ///
    /// Last in the file because it is a table, and a table has to come after
    /// every plain setting or the ones below it read as part of it.
    pub commands: Commands,

    /// Who may see the land claims, and who may draw one from the map.
    ///
    /// A table, so it sits with the other tables at the foot of the file.
    pub claims: Claims,

    /// Extra bars on a player's card, beside their health and their food.
    ///
    /// A mod that gives players a resource — mana, stamina, a level — keeps it
    /// on the player's own entity, where the server can already read it. So this
    /// half needs to know nothing about any mod: an operator names the
    /// attributes and the mod reads whatever is under them, exactly as it
    /// already reads the game's own health and hunger.
    ///
    /// Each value is `name | value attribute | maximum attribute | colour |
    /// group`. The key is only a name for the entry, and the entries are read in
    /// the order they appear in the file.
    ///
    /// The group is what the map files the bar under where a reader switches
    /// bars on and off. Left out, the mod looks for an installed mod whose id
    /// appears in the attribute's own name and uses that — which answers for a
    /// mod that names its attributes after itself and for no other, since an
    /// attribute carries no record of what wrote it.
    ///
    /// **A bar is drawn only for a player who actually has it.** A missing
    /// attribute, or one whose maximum is zero, is a player this does not apply
    /// to — somebody who has not taken up magic, or a server without the mod —
    /// and no bar is the right picture of that. Which is also why naming an
    /// attribute nothing has costs nothing.
    pub bars: BTreeMap<String, String>,
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
            players_public: true,
            live_refresh_ms: 2000,
            threads: 0,
            tile_cache_mb: 256,
            autostart: true,
            announce: true,
            announce_url: String::new(),
            commands: Commands::default(),
            claims: Claims::default(),
            // What a stock Rustbound Magic keeps its two bars under, since that
            // is the mod most likely to be behind this setting being wanted at
            // all. Named rather than detected: this half never sees a mod, and a
            // key it has wrong costs a bar that does not draw rather than
            // anything that breaks. Sorted by key in the file, so `mana` comes
            // before the experience that raises it.
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
    /// What this settings file says about who may see and change what.
    #[must_use]
    pub fn rules(&self) -> Rules {
        Rules {
            markers_public: self.markers_public,
            markers_editable: self.markers_public_editable,
            players_public: self.players_public,
            // Clamped here rather than where it is read, so that the number the
            // page is handed and the number a test asks about are the same one.
            live_refresh_ms: self.live_refresh_ms.clamp(REFRESH_FLOOR_MS, REFRESH_CEILING_MS),
        }
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
             # players_public decides whether where somebody is standing is\n\
             # everybody's to see. On; turn it off and a player shows on the map\n\
             # to their own group and to nobody else. How many are online is\n\
             # still said either way. Read and enforced by the mod, which is the\n\
             # half that knows the groups.\n\
             # live_refresh_ms is the map refresh frequency: how long the page\n\
             # leaves between asking where everybody is, in milliseconds. Players, markers and claims all\n\
             # arrive on this one beat, so it is the whole of how fresh the live\n\
             # half of the map is. Anything below 250 is served as 250 and\n\
             # anything above 60000 as 60000.\n\
             # threads is how many requests are answered at once; 0 decides.\n\
             # tile_cache_mb is how much memory rendered tiles may hold.\n\
             # autostart is whether the server mod runs this service itself.\n\
             # Turn it off to run `witchlight serve` by hand, which is what a map\n\
             # that should outlive the game server wants.\n\
             # announce is whether the mod tells a player where the map is when\n\
             # they join. announce_url is what to tell them: empty means the\n\
             # address this works out for itself, which is right on a machine\n\
             # they can reach directly and wrong behind a proxy or a domain.\n\
             # [commands] is who may run each `wl` command in game. `admin` and\n\
             # `player` are the two that answer most servers; any privilege the\n\
             # game knows — controlserver, chat, commandplayer — works too, and\n\
             # a name the game does not know is refused to everyone but an\n\
             # admin, so a typo locks a command rather than opening it. Read by\n\
             # the mod, which is the half that knows who is an admin.\n\
             # [claims] is who may see the land claims on the map and who may\n\
             # draw a new one from it. Same spelling as [commands]: `admin`,\n\
             # `player`, or any privilege the game knows. Seeing where a claim\n\
             # is starts open, because the game already tells every client;\n\
             # drawing one starts at `claimland`, which is what the game asks\n\
             # of `/land claim`, so the map is never a way round a rule the\n\
             # server already has. Read and enforced by the mod.\n\
             # [bars] adds a bar to each player's card beside their health and\n\
             # their food, read off that player's own entity — which is where a\n\
             # mod giving players mana or stamina already keeps it, and which\n\
             # the server can read without knowing anything about the mod.\n\
             # Each entry is `name | value attribute | maximum attribute |\n\
             # colour`, and the key is only a name for the entry. A bar is drawn\n\
             # only for a player who has that attribute with a maximum above\n\
             # zero, so one nothing on this server uses simply never appears.\n\
             # A fifth part groups the bar where a reader switches them on and\n\
             # off; left out, the mod looks for an installed mod whose id is in\n\
             # the attribute's own name. The two below are what a stock\n\
             # Rustbound Magic uses.\n\n{body}"
        )
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::files::testing::Scratch;

    /// A directory holding a palette, which is what makes one a map.
    fn map(held: &Scratch, at: &str) -> PathBuf {
        let path = if at.is_empty() { held.at().to_path_buf() } else { held.at().join(at) };
        std::fs::create_dir_all(&path).expect("a map directory");
        std::fs::write(crate::palette::path_in(&path), "{}").expect("a palette");
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
        assert_eq!(held.palette, PLAYER);
        assert_eq!(held.icons, PLAYER);
        assert_eq!(held.portrait, PLAYER);
    }

    #[test]
    fn the_claim_gates_start_where_the_game_already_stands() {
        let held = Config::default().claims;
        // The game sends every claim to every client, so a map that hid them
        // would tell players less than the game does.
        assert_eq!(held.view, PLAYER, "where a claim is, is already everybody's");
        // And the map must never be a way round a rule the server already has.
        assert_eq!(
            held.create,
            Privilege::CLAIM_LAND,
            "taking land through the map asks what `/land claim` asks"
        );
    }

    /// The one setting here that lands in a browser's timer.
    #[test]
    fn the_live_beat_is_held_to_a_gap_a_browser_can_keep_up_with() {
        let told = |ms| Config { live_refresh_ms: ms, ..Config::default() }.rules().live_refresh_ms;
        assert_eq!(Config::default().live_refresh_ms, 2000, "two seconds, as it always was");
        assert_eq!(told(500), 500, "what an operator asked for is what the page is told");
        // A gap of nothing is a browser asking again the instant it is answered.
        assert_eq!(told(0), REFRESH_FLOOR_MS);
        // And somebody who typed the number in seconds gets a fast map rather
        // than that.
        assert_eq!(told(2), REFRESH_FLOOR_MS);
        // Past a minute the form has given up on a marker before the beat that
        // would have confirmed it.
        assert_eq!(told(600_000), REFRESH_CEILING_MS);
    }

    #[test]
    fn the_written_template_reads_back_as_what_wrote_it() {
        // The table has to serialise after every plain setting, or the settings
        // below it are read as part of it. This is the check that keeps the
        // field last rather than a comment asking the next person to.
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
        // The mod names it, because the mod is the half that knows which world is
        // running. Nothing here may talk it out of that.
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
        // A folder that is not a map is not offered as one.
        std::fs::create_dir_all(scratch.at().join("tiles")).expect("a folder");

        let complaint = at(scratch.at()).exports(None).expect_err("a question").to_string();
        assert!(complaint.contains("2 worlds"), "it says how many: {complaint}");
        assert!(complaint.contains("Ashlands-0c4419ae"), "and names them: {complaint}");
        assert!(complaint.contains("--exports"), "and says what to do: {complaint}");
    }

    #[test]
    fn nothing_exported_yet_is_the_directory_the_mod_will_fill() {
        // Every server is here on a first run, and refusing to start then is a
        // map service that is down exactly when somebody is watching it.
        let scratch = Scratch::new("config-empty");
        assert_eq!(at(scratch.at()).exports(None).expect("somewhere to look"), scratch.at());
    }
}
