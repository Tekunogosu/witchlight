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
    /// Whether each person is shown the map as they last saw it rather than the
    /// map as it is. See [`crate::memory`].
    pub private_map: bool,
    /// Under a private map, whether the ground around spawn is shown to
    /// everybody, a browser with no session included.
    pub anonymous_spawn: bool,
    /// How far from spawn that reaches, in chunks each way.
    pub anonymous_spawn_radius_chunks: i32,
    /// How far a player sees, in chunks each way. Zero means the game's own
    /// chunk radius, as the mod reports it.
    pub sight_radius_chunks: i32,
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
    /// Whether the map draws the claims the world made for itself.
    ///
    /// Not a permission but a third question about the same subject, which is
    /// why it sits in this table rather than beside the other switches: a claim
    /// round a trader camp or a story structure has an owner's name on it and no
    /// owner behind it, and it exists from the moment the world generated that
    /// ground rather than from the moment anybody found it.
    ///
    /// Off. A web map is the one place those boundaries can all be read at once,
    /// from a chair, without going anywhere — so drawing them is handing every
    /// reader the location of every trader on the server, which is a thing the
    /// game does not otherwise give anybody. An operator who wants them turns
    /// this on.
    ///
    /// Enforced by the mod, and by leaving them out of what it sends rather than
    /// by the page declining to draw them. A claim that reached a browser is a
    /// claim anybody may read out of it, so this can only mean anything on the
    /// side that decides what to send.
    pub worldgen: bool,
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
            // Where a trader camp is, is not already everybody's, which is what
            // makes this the one of the three that starts closed. The game tells
            // a client about a claim it is standing near; the map would tell a
            // reader about every one at once.
            worldgen: false,
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

    /// Whether each person is shown the map as they last saw it.
    ///
    /// On. A public server is not one map: it is a map per person, of what that
    /// person has been near, and ground that changed while they were away stays
    /// as they remember it until they go back. Off, everybody is shown the same
    /// map, which is what a server of friends wants and what the map always did.
    ///
    /// The mod reads it too: while it is on, where a player stands is their
    /// group's to see and nobody else's, whatever `players_public` says.
    pub private_map: bool,

    /// Whether the ground around spawn is everybody's to see under a private
    /// map — a browser with no session included, which is otherwise shown
    /// nothing at all.
    pub anonymous_spawn: bool,

    /// How far from spawn that reaches, in chunks each way. Eight is a square
    /// half a kilometre across.
    pub anonymous_spawn_radius_chunks: i32,

    /// How far a player sees, in chunks as the crow flies: what standing
    /// somewhere adds to their map. Zero means each player's own view distance
    /// as the game granted it, which is as far as it loads chunks for them —
    /// or the server's `MaxChunkRadius` where the mod is too old to say.
    pub sight_radius_chunks: i32,

    /// How long the page leaves between asking where everybody is, in
    /// milliseconds, where it has to ask at all.
    ///
    /// One second. The page is told of changes as they happen — see
    /// `events.rs` — and asks on this clock only while that is not working:
    /// a proxy that will not hold a request open, or a service with too many
    /// browsers waiting already. Then this is the whole of how fresh the live
    /// half of the map is. Players, markers, claims and whether a marker just asked for
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

    /// How long the server mod leaves between writing what the terrain has done,
    /// in milliseconds.
    ///
    /// Ten seconds, which is what it always was. This is the map's own coalescing
    /// knob: a chunk changed six times inside one beat is written once, so raising
    /// it trades how current the terrain is against how often the disk is touched.
    /// A server whose map is watched while people build wants it low; one on a
    /// drive somebody is trying not to wear out wants it high.
    ///
    /// The number matters far less than it did. A chunk that moves now costs its
    /// own kilobyte or so rather than the quarter-megabyte square it sits in — see
    /// the mod's `Regions` — so ten seconds is affordable where it used not to be.
    ///
    /// Read and enforced by the mod, which is the half that does the writing —
    /// nothing here acts on it, as with `autostart` and `announce`. It holds the
    /// number between one second and ten minutes: an export runs on the server's
    /// own tick, so a gap of nothing is the game doing this instead of the world,
    /// and past ten minutes a map is not a picture of a world people are in.
    pub export_interval_ms: u64,

    /// How far around a player the terrain puller may fill in, in chunks as
    /// the crow flies, where the mod has not said how far that player sees.
    ///
    /// Zero means the game server's own `MaxChunkRadius` — the furthest the
    /// game loads chunks for anybody, which is what an in-game map could ever
    /// have shown a player standing there. Backfilling any further than that
    /// draws ground the generator laid down and nobody could have walked to,
    /// which is not the shape a map of what has been explored should have.
    ///
    /// A mod that reports each player's own view distance makes this the
    /// fallback for a player it has not reported yet. Set it past zero only to
    /// draw wider than the game showed anyone — worth doing on a server whose
    /// operator wants the web map more generous than the client, never worth
    /// doing by accident.
    pub backfill_radius_chunks: i32,

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
            private_map: true,
            anonymous_spawn: true,
            anonymous_spawn_radius_chunks: 8,
            sight_radius_chunks: 0,
            players_public: true,
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
            private_map: self.private_map,
            anonymous_spawn: self.anonymous_spawn,
            anonymous_spawn_radius_chunks: self.anonymous_spawn_radius_chunks,
            sight_radius_chunks: self.sight_radius_chunks,
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

    /// The settings file this writes: every value serde knows how to write, with
    /// what it is for standing over it.
    ///
    /// The values come from serde and the notes from <see>NOTES</see>, laid over
    /// each other by walking what was written. Neither half can invent a setting
    /// the other has not heard of: a value with no note is caught by
    /// `every_setting_written_says_what_it_is_for`, and a note for a setting that
    /// no longer exists is never reached and is caught by the same test.
    #[must_use]
    pub fn to_template(&self) -> String {
        let body = toml::to_string_pretty(self).unwrap_or_else(|error| format!("# {error}\n"));
        let mut file = String::from(HEADER);

        // Which table the settings being written belong to, so that a name inside
        // one is looked up as `commands.export` rather than as a top-level
        // `export` that would mean something else.
        let mut table = String::new();

        for line in body.lines() {
            let text = line.trim();
            if text.is_empty() {
                continue;
            }

            if let Some(name) = text.strip_prefix('[').and_then(|rest| rest.strip_suffix(']')) {
                table = format!("{name}.");
                file.push_str(&noted(text));
            } else {
                let key = text.split('=').next().unwrap_or_default().trim();
                file.push_str(&noted(&format!("{table}{key}")));
            }

            file.push_str(text);
            file.push('\n');
        }

        file
    }
}

/// What the file says about itself before it says anything about a setting.
///
/// Two lines, because everything else a reader needs is beside the line it is
/// about. This says only what cannot be: which half of witchlight acts on what
/// they are reading.
const HEADER: &str = "\
# witchlight configuration
#
# Written by the map service, and read by both halves of witchlight: a note
# saying the mod reads a setting means the game server is what acts on it.
";

/// What one setting is for, as it is written above that setting.
///
/// Keyed by the name serde writes — a setting inside a table by `table.key`, and
/// a table itself by its own bracketed name — so a setting that moves into a
/// table takes its note with it rather than losing it.
///
/// Written beside the settings rather than in a block at the head of the file.
/// A note a screen away from the line it is about is a note nobody reads and a
/// line nobody dares change, and the block had grown to sixty lines of prose
/// standing between an operator and the first thing they came to edit.
///
/// A table rather than prose, because a table can be checked: see
/// `every_setting_written_says_what_it_is_for`, which is what stops a setting
/// being added and reaching an operator unexplained. The keys under `[bars]` are
/// the operator's own names and are the one thing in the file with nothing to
/// say about them; the note on the table says what they all are.
const NOTES: &[(&str, &str)] = &[
    (
        "vs_data",
        "The game server's `--dataPath`. Exports are read from the `witchlight`\n\
         folder inside it.",
    ),
    (
        "map_data",
        "Where the map is kept instead. Worth setting where it should live\n\
         somewhere other than beside the world — a larger disk, a directory a web\n\
         server already serves. Empty is the folder above.",
    ),
    (
        "per_world",
        "Files each world's map in a directory of its own inside that folder. Off\n\
         for a dedicated server, which runs one world and wants its map where it\n\
         has always been. On for singleplayer, where every save shares one data\n\
         path and the second world would otherwise write its terrain into the\n\
         first world's map. Turning it on moves the map already there down into\n\
         its own directory rather than leaving it to be written over. Read by the\n\
         mod, which is the only half that knows which world is running.",
    ),
    (
        "bind",
        "Where the map is served. Every address this machine has, so it is\n\
         reachable from the rest of the network without further configuration;\n\
         `127.0.0.1:8080` keeps it to this machine alone.",
    ),
    (
        "api_bind",
        "Where the mod posts who is online and where the markers and claims are.\n\
         Empty means loopback on a port the machine picks, published in api.json\n\
         beside the map so the mod finds it without being told and two game\n\
         servers on one box collide with nothing. Set a host:port only for a mod\n\
         running on another machine.",
    ),
    (
        "api_token",
        "What the mod must present to post. Empty means a fresh one each start,\n\
         written into api.json where the mod reads it — so this is only worth\n\
         setting where that file cannot reach the mod, and then the same value\n\
         goes on both sides.",
    ),
    (
        "markers_public",
        "What a marker nobody has chosen for is: off keeps one to its owner, on\n\
         shares it with everybody. Read by the mod as well as here, so the\n\
         in-game map and the web map agree.",
    ),
    (
        "markers_public_editable",
        "Whether anybody may change a marker anybody can see. Off, so a public\n\
         marker is readable by all and writable by its owner; on, the server\n\
         corrects its own map together. A private marker is never anybody's but\n\
         its owner's either way.",
    ),
    (
        "players_public",
        "Whether where somebody is standing is everybody's to see. On; turn it off\n\
         and a player shows on the map to their own group and to nobody else. How\n\
         many are online is still said either way. Read and enforced by the mod,\n\
         which is the half that knows the groups.",
    ),
    (
        "private_map",
        "Whether each person is shown the map as they last saw it. On, and a\n\
         public server is a map per person: what they have been near, with ground\n\
         that changed while they were away kept as they remember it until they\n\
         go back. Off, everybody is shown the same map. Read by the mod too: while\n\
         this is on a player's position is their group's to see and nobody else's.",
    ),
    (
        "anonymous_spawn",
        "Under a private map, whether the ground around spawn is everybody's to\n\
         see, a browser nobody has logged in on included. On. Off, a browser with\n\
         no session is shown nothing until somebody logs in on it.",
    ),
    (
        "anonymous_spawn_radius_chunks",
        "How far from spawn that reaches, in chunks each way. 8 is a square half a\n\
         kilometre across.",
    ),
    (
        "sight_radius_chunks",
        "How far a player sees, in chunks as the crow flies: standing somewhere\n\
         adds this much around them to their map. 0 uses each player's own view\n\
         distance as the game granted it, which is as far as it loads chunks\n\
         for them.",
    ),
    (
        "live_refresh_ms",
        "How long the page leaves between asking where everybody is, in\n\
         milliseconds, where it has to ask at all. The page is told of changes\n\
         as they happen and asks on this clock only while that is not working;\n\
         then players, markers and claims all arrive on this one beat. Anything\n\
         below 250 is served as 250 and anything above 60000 as 60000.",
    ),
    (
        "export_interval_ms",
        "How long the server mod leaves between writing what the terrain has\n\
         done, in milliseconds. This is the map's coalescing knob: a chunk\n\
         changed six times inside one beat is written once, so raising it trades\n\
         how current the terrain is against how often the disk is touched.\n\
         Anything below 1000 is used as 1000 and anything above 600000 as\n\
         600000, and a world save exports whatever the gap was holding. Read by\n\
         the mod, which is the half that does the writing.",
    ),
    (
        "backfill_radius_chunks",
        "How far around a player the terrain puller may fill in, in chunks,\n\
         where the mod has not said how far that player sees. 0 uses the game\n\
         server's own MaxChunkRadius — the furthest it loads chunks for anybody,\n\
         so the map never draws ground no in-game map could have shown. Set\n\
         past 0 to draw wider than the game itself ever showed anyone.",
    ),
    (
        "threads",
        "How many requests are answered at once. 0 decides from the machine, held\n\
         back so that the game server this usually shares a box with keeps cores\n\
         of its own.",
    ),
    (
        "tile_cache_mb",
        "How much memory rendered tiles may hold before the least used are\n\
         dropped. They are rebuilt on demand, so this costs time and not the map.",
    ),
    (
        "autostart",
        "Whether the server mod runs this service itself. Turn it off to run\n\
         `witchlight serve` by hand, which is what a map that should outlive the\n\
         game server wants.",
    ),
    (
        "announce",
        "Whether the mod tells a player where the map is when they join.",
    ),
    (
        "announce_url",
        "What to tell them. Empty means the address this works out for itself,\n\
         which is right on a machine they can reach directly and wrong behind a\n\
         proxy, a domain or NAT.",
    ),
    (
        "[commands]",
        "Who may run each `wl` command in game. `admin` and `player` are the two\n\
         that answer most servers; any privilege the game knows — controlserver,\n\
         chat, commandplayer — works too, and a name the game does not know is\n\
         refused to everyone but an admin, so a typo locks a command rather than\n\
         opening it. Read by the mod, which is the half that knows who is an\n\
         admin.",
    ),
    ("commands.login", "A link that signs your own browser in as you."),
    ("commands.mark", "A marker where you are looking."),
    ("commands.portrait", "Asking a client for a picture of its player."),
    ("commands.palette", "Asking a client for a block colour palette."),
    ("commands.icons", "Asking a client for the pictures markers are drawn with."),
    ("commands.export", "Writing the surface of every loaded chunk."),
    ("commands.status", "The whole of what state the map is in."),
    ("commands.service", "Starting and stopping the map service."),
    (
        "[claims]",
        "What the map does with the land claims. The first two are spelled the way\n\
         [commands] are and answer two different questions, because they are two:\n\
         seeing where a claim is tells somebody whether they may build there, and\n\
         drawing one takes land. Read and enforced by the mod.",
    ),
    (
        "claims.view",
        "Who may see where the claims are. Open, because the game already sends\n\
         every claim to every client — a map that hid them would tell players less\n\
         than the game does.",
    ),
    (
        "claims.create",
        "Who may draw a new one from the map. What the game asks of `/land claim`,\n\
         so the map is never a way round a rule the server already has; narrowing\n\
         it narrows the map alone.",
    ),
    (
        "claims.worldgen",
        "Whether the map draws the claims the world made for itself — the\n\
         perimeters round trader camps and story structures, which carry an\n\
         owner's name and no owner. Off, because those exist from the moment the\n\
         ground generated, and drawing them hands every reader the location of\n\
         every trader on the server. Turn it on for a map that shows the lot.",
    ),
    (
        "[bars]",
        "A bar on each player's card beside their health and their food, read off\n\
         that player's own entity — which is where a mod giving players mana or\n\
         stamina already keeps it, and which the server can read without knowing\n\
         anything about the mod. Each entry is `name | value attribute | maximum\n\
         attribute | colour | group`, and the key is only a name for the entry. A\n\
         bar is drawn only for a player who has that attribute with a maximum\n\
         above zero, so one nothing on this server uses simply never appears.\n\
         Left out, the group is taken from an installed mod whose id is in the\n\
         attribute's own name. The two below are what a stock Rustbound Magic\n\
         uses.",
    ),
];

/// One setting's note, as the lines that stand over it, or nothing where it has
/// none.
///
/// A blank line before every note, without exception: a rule with an exception is
/// a file that reads as though it were formatted by hand and got tired. What has
/// no note — the operator's own names under `[bars]` — is a list, and a list
/// reads better closed up anyway.
fn noted(key: &str) -> String {
    let Some((_, note)) = NOTES.iter().find(|(name, _)| *name == key) else {
        return String::new();
    };

    let mut said = String::from("\n");
    for line in note.lines() {
        said.push_str("# ");
        said.push_str(line);
        said.push('\n');
    }
    said
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
        // The one of the three that starts closed, because it is the one the
        // game does not already give: a client is told about the claim it is
        // standing near, never about every trader camp at once.
        assert!(!held.worldgen, "the world's own perimeters are not drawn unless asked for");
    }

    /// The one setting here that lands in a browser's timer.
    #[test]
    fn the_live_beat_is_held_to_a_gap_a_browser_can_keep_up_with() {
        let told = |ms| Config { live_refresh_ms: ms, ..Config::default() }.rules().live_refresh_ms;
        assert_eq!(Config::default().live_refresh_ms, 1000, "one second: the clock the page falls back to");
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

    /// Every setting an operator is handed says what it is for, and nothing says
    /// what it is for about a setting they are not handed.
    ///
    /// The two halves of the file — the values serde writes and the notes written
    /// beside them — are held apart, which is what lets each be edited without the
    /// other. This is what stops them drifting: a field added to `Config` reaches
    /// an operator unexplained, and a note left behind by a setting that has gone
    /// is a note that will never again be read by anyone but its author.
    ///
    /// The names under `[bars]` are the exception, and the only one. They are the
    /// operator's own words rather than settings this program has ever heard of,
    /// so there is nothing here that could have a note about them; the note on the
    /// table itself says what all of them are.
    #[test]
    fn every_setting_written_says_what_it_is_for() {
        let template = Config::default().to_template();
        let mut table = String::new();
        let mut unexplained = Vec::new();
        let mut explained = Vec::new();
        let mut previous = "";

        for line in template.lines() {
            let text = line.trim();
            if text.is_empty() || text.starts_with('#') {
                previous = text;
                continue;
            }

            let name = match text.strip_prefix('[').and_then(|rest| rest.strip_suffix(']')) {
                Some(named) => {
                    table = format!("{named}.");
                    text.to_owned()
                }
                None => format!("{table}{}", text.split('=').next().unwrap_or_default().trim()),
            };

            if previous.starts_with('#') {
                explained.push(name);
            } else if table != "bars." {
                unexplained.push(name);
            }
            previous = text;
        }

        assert!(
            unexplained.is_empty(),
            "these reach an operator with nothing said about them: {unexplained:?}"
        );

        let stale: Vec<_> = NOTES
            .iter()
            .map(|(name, _)| *name)
            .filter(|name| !explained.iter().any(|written| written == name))
            .collect();
        assert!(stale.is_empty(), "these notes are about nothing the file holds: {stale:?}");
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
