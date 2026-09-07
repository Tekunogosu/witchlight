//! The configuration file as it is written out, with a note above each setting.
//!
//! The values come from serde and the notes from [`NOTES`]. Every note states
//! what the setting controls, what each value does, the default, and which half
//! of Witchlight reads it, because this text is what an operator edits.

use super::Config;

impl Config {
    /// Renders the configuration as a TOML file with a note above each setting.
    ///
    /// The values come from serde and the notes from `NOTES`. The test
    /// `every_setting_written_says_what_it_is_for` checks that every written
    /// setting has a note and that every note matches a written setting.
    #[must_use]
    pub fn to_template(&self) -> String {
        let body = toml::to_string_pretty(self).unwrap_or_else(|error| format!("# {error}\n"));
        let mut file = String::from(HEADER);

        // The table the current line belongs to, so a key inside `[commands]` is
        // looked up as `commands.export` rather than as a top-level `export`.
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

/// The comment block at the top of the written configuration file.
const HEADER: &str = "\
# Witchlight configuration
#
# This file is written by the map service and read by both the map service and
# the server mod. Each note says which half acts on the setting.
";

/// The note written above each setting in the configuration file.
///
/// Keys match the names serde writes. A setting inside a table is keyed as
/// `table.key`, and a table itself by its bracketed name. The keys under `[bars]`
/// are the operator's own labels and have no notes; the note on the table
/// describes them all.
const NOTES: &[(&str, &str)] = &[
    (
        "vs_data",
        "The Vintage Story data directory (the game server's --dataPath).\n\
         Map data is read from the `witchlight` folder inside it unless map_data\n\
         is set.",
    ),
    (
        "map_data",
        "The directory that holds the map data. Empty means the `witchlight`\n\
         folder inside vs_data. Set it to keep the map on another disk or in a\n\
         directory a web server already serves.",
    ),
    (
        "per_world",
        "true: each world's map is kept in its own subdirectory of map_data.\n\
         false: the map is kept directly in map_data.\n\
         Default: true. Every singleplayer save shares one data path, and without\n\
         this a second world would overwrite the first world's map. Turning it on\n\
         moves an existing map into its own subdirectory. Read by the mod.",
    ),
    (
        "bind",
        "The address and port the map is served on. 0.0.0.0:8080 listens on every\n\
         interface. 127.0.0.1:8080 serves only this machine.",
    ),
    (
        "api_bind",
        "The address and port the mod posts live data to (player positions,\n\
         markers and claims). Empty means loopback on a free port, published in\n\
         api.json beside the map so the mod finds it on its own. Set a host:port\n\
         only when the mod runs on another machine.",
    ),
    (
        "api_token",
        "The token the mod must present when posting live data. Empty means a new\n\
         token is generated on each start and written to api.json for the mod to\n\
         read. Set it only when the mod runs on another machine, and set the same\n\
         value on both sides.",
    ),
    (
        "allow_public_markers",
        "Controls who can see a new marker when its owner has not chosen.\n\
         true: new markers are visible to everyone. false: only their owner sees\n\
         them. Default: false. Read by both the mod and the map service.",
    ),
    (
        "allow_editing_public_markers",
        "true: any signed-in player may edit a public marker. false: only the\n\
         owner may edit it. A private marker can only ever be edited by its\n\
         owner. Default: false. Enforced by the mod.",
    ),
    (
        "show_players_to_everyone",
        "true: every player's position is shown to every viewer. false: a\n\
         player's position is shown only to members of their own player group.\n\
         The number of players online is shown to everyone either way.\n\
         Default: true. While personal_maps is on, positions are always\n\
         restricted to the player's own group. Enforced by the mod.",
    ),
    (
        "hidden_groups",
        "Player group names the map ignores. A group listed here is never offered\n\
         for sharing a map and never shown in the Group tab of the player list.\n\
         Some mods (xlib, for example) put every player into one group, and a\n\
         group that contains everyone is not useful for sharing. Names are\n\
         compared case-insensitively.",
    ),
    (
        "personal_maps",
        "true: each player sees only the terrain they have explored, as it looked\n\
         when they last saw it. false: every viewer sees the whole map as it is\n\
         now. Default: true. Read by both halves. While it is on, the mod\n\
         restricts each player's position to their own group regardless of\n\
         show_players_to_everyone.",
    ),
    (
        "show_spawn_to_guests",
        "Applies only while personal_maps is on. true: the terrain around spawn\n\
         is shown to every viewer, including a browser that is not signed in.\n\
         false: a browser that is not signed in sees nothing. Default: true.",
    ),
    (
        "spawn_radius_chunks",
        "The radius of the spawn area that show_spawn_to_guests reveals, in\n\
         chunks. Default: 8, which is a square about half a kilometre across.",
    ),
    (
        "sight_radius_chunks",
        "The radius a player reveals around themselves as they move, in chunks.\n\
         0 means the view distance the game granted that player. Default: 0.",
    ),
    (
        "session_hours",
        "How long a browser session stays valid after its last request, in hours.\n\
         0 means sessions never expire: a session lasts until the browser signs\n\
         out or invalidate_sessions_on_restart clears it. Default: 0.",
    ),
    (
        "invalidate_sessions_on_restart",
        "true: every browser session is invalidated when the map service starts,\n\
         and every viewer must sign in again. false: sessions are kept in the\n\
         map's database and survive a restart. Default: false.",
    ),
    (
        "live_refresh_ms",
        "The interval between live polls from the web page, in milliseconds. The\n\
         page is normally told of changes as they happen and polls on this\n\
         interval only when that connection is unavailable, for example behind\n\
         a proxy that does not hold requests open. Values below 250 are treated\n\
         as 250 and values above 60000 as 60000. Default: 1000.",
    ),
    (
        "export_interval_ms",
        "The interval between terrain exports, in milliseconds. A chunk that\n\
         changes several times within one interval is written once, so a larger\n\
         value means less disk activity and a less current map. Values below 1000\n\
         are treated as 1000 and values above 600000 as 600000. A world save\n\
         exports immediately. Default: 10000. Read by the mod.",
    ),
    (
        "backfill_radius_chunks",
        "The radius around a player that the map may fill in, in chunks, when the\n\
         mod has not reported that player's view distance. 0 means the game\n\
         server's MaxChunkRadius, so the map never draws ground no in-game map\n\
         could have shown. Set it above 0 to draw wider than the game shows its\n\
         players. Default: 0.",
    ),
    (
        "threads",
        "The number of threads that render tiles. 0 picks a count from the CPU\n\
         count, leaving cores for the game server that usually shares the\n\
         machine. Default: 0.",
    ),
    (
        "tile_cache_mb",
        "The memory budget for rendered tiles, in megabytes. When it is exceeded,\n\
         the least recently used tiles are dropped and rendered again on demand.\n\
         Default: 256.",
    ),
    (
        "autostart",
        "true: the server mod starts and stops the map service itself. false: run\n\
         `witchlight serve` by hand, which lets the map outlive the game server.\n\
         Default: true. Read by the mod.",
    ),
    (
        "announce",
        "true: the mod tells each player the map's address in chat when they\n\
         join. false: it does not. Default: true. Read by the mod.",
    ),
    (
        "announce_url",
        "The address the mod announces. Empty means the address the map service\n\
         works out for itself, which is correct on a LAN and wrong behind a\n\
         proxy, a domain name or NAT. Set it to the address players actually use.",
    ),
    (
        "[commands]",
        "The privilege required to run each /witchlight command in game. `admin`\n\
         and `player` cover most servers. Any privilege code the game knows also\n\
         works, such as controlserver, chat or commandplayer. A code the game does\n\
         not know locks the command to admins. Read by the mod.",
    ),
    ("commands.login", "/witchlight login: sends you a link that signs your browser in."),
    ("commands.mark", "/witchlight mark: places a marker on the block you are looking at."),
    ("commands.portrait", "/witchlight portrait: asks a client for a picture of its player."),
    ("commands.palette", "/witchlight palette: asks a client for the block colour palette."),
    ("commands.icons", "/witchlight icons: asks a client for the marker icons."),
    ("commands.export", "/witchlight export: writes the surface of every loaded chunk."),
    ("commands.status", "/witchlight status: reports the state of the map and the service."),
    ("commands.service", "/witchlight service: starts and stops the map service."),
    (
        "[claims]",
        "What the map does with land claims. view and create are privilege codes,\n\
         written the same way as [commands]. Read and enforced by the mod.",
    ),
    (
        "claims.view",
        "The privilege required to see claims on the map. Default: player, because\n\
         the game already sends every claim to every client.",
    ),
    (
        "claims.create",
        "The privilege required to draw a new claim on the map. Default: claimland,\n\
         the same privilege the game requires for /land claim.",
    ),
    (
        "claims.worldgen",
        "true: the map draws the claims the world generator created, such as the\n\
         perimeters around trader camps and story structures. false: it does not.\n\
         Default: false, because those claims reveal the location of every trader\n\
         on the server.",
    ),
    (
        "[bars]",
        "Extra bars on each player's card, beside health and food. Each entry names\n\
         attributes on the player entity that a mod stores a resource in, such as\n\
         mana or stamina. The value format is:\n\
         name | value attribute | maximum attribute | colour | group\n\
         The key is only a label for the entry. The group is the heading the bar\n\
         is filed under; left out, the mod uses the id of an installed mod that\n\
         appears in the attribute's name. A bar is drawn only for a player who has\n\
         the attribute with a maximum above zero, so an entry for a mod that is\n\
         not installed draws nothing. The two entries below are for Rustbound\n\
         Magic.",
    ),
];

/// Formats the note for `key` as comment lines with a blank line before them.
/// Returns an empty string when the key has no note.
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

    /// The values serde writes and the notes written beside them are held apart,
    /// so each can be edited without the other. This test is what stops them
    /// drifting. A field added to `Config` would otherwise reach an operator with
    /// nothing said about it, and a note left behind by a setting that has gone
    /// would never be read by anyone again.
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
}
