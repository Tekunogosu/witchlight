# Architecture

This document covers how Witchlight is put together: configuration in full,
the data pipeline, the HTTP interface, rendering, the viewer's internals, the
codebase layout, and how to run its test suite. For what the project is and
how to get it running, see [README.md](README.md).

Witchlight is the renderer half of a pair. It does not read the game's save
file directly and does not require Vintage Story to be installed. All map
data comes from exports written by the companion server mod,
[`witchlight-csharp`](https://github.com/Tekunogosu/witchlight-csharp). The
mod reads game state; Witchlight turns that data into pixels and serves it
over HTTP. Splitting the two this way
means a Vintage Story update can only ever break the mod half, never the
renderer.

**Both halves ship as one release and always carry the same version
number**, even when only one side changed. The mod's `package.sh` reads the
version out of the built witchlight binary and refuses to package a pair
that disagrees — see that repository's `ARCHITECTURE.md` for the mechanism.

## Configuration

Settings live in `~/.config/witchlight/config.toml`, written with defaults
on first run, or wherever `-c` points — which is how the server mod points
witchlight at its own `ModConfig/witchlight.conf`.

```toml
vs_data = "/home/vintagestory/data"   # the server's --dataPath
map_data = ""                         # where maps are kept; empty means <vs_data>/witchlight
per_world = true                      # keep each world's map in its own subdirectory
bind = "0.0.0.0:8080"                 # every interface; 127.0.0.1:8080 for this machine only
api_bind = ""                         # where the mod posts live data; empty means loopback on a free port
api_token = ""                        # the token the mod must present; empty means a new one each start
allow_public_markers = false          # new markers are visible to everyone (true) or only their owner (false)
allow_editing_public_markers = false  # any player may edit a public marker (true) or only its owner (false)
show_players_to_everyone = true       # positions are shown to everyone (true) or only to the player's group (false)
hidden_groups = ["xlib"]              # player groups the map ignores
personal_maps = true                  # each player sees only the terrain they have explored
show_spawn_to_guests = true           # under personal maps, spawn is shown to everyone, signed in or not
spawn_radius_chunks = 8               # the radius of that spawn area, in chunks
sight_radius_chunks = 0               # the radius a player reveals, in chunks; 0 means their view distance
session_hours = 0                     # how long a browser stays signed in; 0 means forever
invalidate_sessions_on_restart = false # sign every browser out when the service starts
live_refresh_ms = 1000                # the poll interval the page falls back to
export_interval_ms = 10000            # the interval between terrain exports by the mod
backfill_radius_chunks = 0            # how far around a player the map may fill in; 0 means the game's limit
threads = 0                           # tile rendering threads; 0 picks from the CPU count
tile_cache_mb = 256                   # memory budget for rendered tiles
autostart = true                      # the server mod starts the map service itself
announce = true                       # the mod tells joining players the map's address
announce_url = ""                     # the address to announce; empty means work it out

[commands]                            # the privilege required to run each /witchlight command
login = "player"                      # a link that signs your browser in
mark = "player"                       # a marker on the block you are looking at
portrait = "player"                   # ask a client for a picture of its player
palette = "admin"                     # ask a client for the block colour palette
icons = "player"                      # ask a client for the marker icons
export = "admin"                      # write the surface of every loaded chunk
status = "admin"                      # report the state of the map and the service
service = "admin"                     # start and stop the map service

[claims]                              # what the map does with land claims
view = "player"                       # the privilege required to see claims
create = "claimland"                  # the privilege required to draw a claim
worldgen = false                      # draw the claims the world generator created
```

This is the config file trimmed to one line per setting. The file Witchlight
writes carries a full explanatory comment above each one, so editing it needs
nothing else open beside it — the sample above is here to show the shape at a
glance.

`autostart`, `announce`, `announce_url`, `[commands]`, `[claims]`, and
`[bars]` are settings that Witchlight itself never reads. They govern who
starts the map, who is told about it, and who may request data from a game
client — questions about the mod/service pair rather than about either half
alone, so they live in this one shared config instead of two separate ones.

Flags override the config file and apply once, unless `-S` is given to save
them. `-p` prints the resolved configuration.

### Personal maps

`personal_maps` suits a public server: each player sees only the terrain
they've explored, as it looked the last time they saw it. Turn it off and
every viewer sees the whole map as it is right now.

`show_spawn_to_guests` and `spawn_radius_chunks` control what a signed-out
browser sees under personal maps: the terrain around spawn, and nothing else.
`sight_radius_chunks` is the radius a player reveals as they move; 0 uses the
player's own view distance.

### Live refresh

`live_refresh_ms` is the interval the page falls back to when it has to poll
instead of being pushed an update — see [Serving](#serving) below for the
normal path. Values are clamped to between 250 and 60000 ms. A config change
here takes effect once the service has restarted and the page reloaded.

### Player status bars

`[bars]` adds a bar to each player's card, beside health and food:

```toml
[bars]
mana = "Mana | entitybehavior-resource-currentmana_rm | entitybehavior-resource-totalmaxmana_rm | #7c5cff"
```

Any mod that stores a resource on the player's own entity, in the same
watched attributes the game uses for health and hunger, can be read this way
with zero coupling — Witchlight reads a number off the entity without linking
against the mod that put it there. Each entry is
`name | value attribute | maximum attribute | colour | group`. The key names
the entry; entries are drawn in the order the file lists them.

The `group` field controls which section a bar is filed under in the
accessibility panel's **Bar display**, where a reader can switch bars on and
off individually — that section only lists bars a server has actually sent.
If `group` is omitted, Witchlight looks for an installed mod whose id appears
in the attribute's own name and files the bar there. That only works for a
mod that names its attributes after itself, so bundled entries for mods that
don't (like the two shown above, for
[Rustbound Magic](https://mods.vintagestory.at/rustboundmagic)) name their
group explicitly.

A settings file with no `[bars]` section is given the two entries above by
default, the same way a file with no `[commands]` section gets the command
defaults below — upgrading an old config and seeing nothing new here means
the feature doesn't apply, not that it's broken. An explicitly emptied
`[bars]` section means "show none."

A bar is drawn only for a player who has the attribute, with a nonzero
maximum. A missing bar just means that player, or that server, doesn't use
that mod — showing nothing is the correct behavior, and it's also what makes
listing an attribute nobody on a given server has completely free.

### Command privileges

`[commands]` accepts `admin`, `player`, or any privilege the game itself
knows (`controlserver`, `chat`, `commandplayer`, etc.), which lets a server
delegate a command to its moderators instead of only admins or everyone. An
unrecognized name is refused to everyone but an admin, and logged as an
error, so a typo locks a command down rather than opening it up. The
defaults split commands that change server state from commands that only
answer a question about the caller.

Each setting controls who may *initiate* a request, not who may *receive*
its result — the mod always asks whichever client can answer, and incoming
answers are trusted equally regardless of who asked, except that only an
admin's palette or marker-icon submission may replace what's already stored;
anyone else's only fills gaps. `wl status` prints the privilege table
currently in force. A config file written before `[commands]` existed
implies the defaults; Witchlight never rewrites a config file just to add a
section of defaults it's already following (`witchlight -c <file> -S` will
do that explicitly, at the cost of any comments in the file).

### Land claims

`[claims]` uses the same privilege syntax as `[commands]`, and answers two
separate questions: seeing where a claim is (whether you may build there) and
drawing a new one (taking land). A server can reasonably let everyone see
every claim boundary while restricting who may create one.

`view` defaults to `player` because the game already sends every claim to
every client and draws the boundary for anyone holding the right tool — a map
that hid this would tell players less than the game itself does. `create`
defaults to `claimland`, matching what the game requires of `/land claim`
directly. **The map is never a way around a permission the server already
enforces** — narrowing `create` narrows the map's own UI only, and the mod
still checks the game's own privilege, `allowLandClaiming`, the role's
allowance and minimum size, existing claim count, and overlap with other
claims. Any claim the map lets through is one `/land claim` would also allow.

`worldgen` controls a third case that isn't a permission at all: the game
protects trader camps, story structures, and dungeons with land claims that
carry an owner's name but no real owner, generated with the world itself.
Drawing them by default would broadcast every trader location on the server,
so this defaults to `false`. The mod omits these from what it sends, rather
than having the map filter them client-side, because any claim that reaches
a browser is one a user could read directly out of the page. `wl status`
reports how many claims the map is drawing versus how many the server has,
so the gap is visible in-game.

### Networking

Set `announce_url` for any server players can't reach directly (behind a
proxy, a domain, or NAT). Left empty, Witchlight announces the address it can
determine for itself, which is correct on a LAN and wrong otherwise.

`bind` defaults to every interface, so the map is reachable on the local
network as soon as it starts. On startup Witchlight logs every address it's
actually reachable at, and writes them to `service.json` next to the
export — that's how the server mod knows what to log and what to tell a
joining player.

Exposing the map to the open internet requires a further step: put it behind
a reverse proxy that terminates TLS. **Witchlight has no authentication of
its own** — anything reachable on the map port can read terrain, player
positions, and every marker along with its owner's name.

### Data layout

`vs_data` is the game's data directory; exports are read from the
`witchlight` subfolder inside it, unless `map_data` names a different
location. A directory containing `palette.json` directly is also accepted,
so files copied off a server with `scp` need no extra flag. Whichever path is
used is logged on startup.

`per_world` (on by default) puts each world's map in its own subdirectory,
named after the world. Turn it off for a dedicated server that should serve
its one map directly from the data folder, without a subdirectory — this
matters because every singleplayer save otherwise shares one data path and
would overwrite the previous world's map. Nothing is shared between these
per-world directories, even identical files, since a file written once and
left alone costs nothing, while re-deriving a shared file on every world
switch costs real disk I/O.

The server mod names the export directory outright, with `--exports`, since
it's the half that knows which world is currently running. Running Witchlight
by hand only requires this when `per_world` is on and more than one world has
been exported — with a single export, that one is served automatically; with
several, the log names which one it picked rather than guessing silently.

## Startup output

```
witchlight 0.2.0
witchlight: reading /srv/vs/data/witchlight
witchlight: 635 chunks, 864x1024 blocks
witchlight: palette from client, 45418 blocks, 26 colour maps (game 1.22.7)
witchlight: surface 96% painted, 4% nothing to draw, 0% waiting on a colour, 0% unknown blocks
witchlight: serving on http://192.168.1.158:8080  (on your network)
```

The version is logged on every run, not only with `--version`.
**`palette from server` vs. `from client`** identifies which machine's game
assets supplied the block colors — a dedicated server's own palette is
nearly empty, which is the most common cause of a blank-looking map.

The **surface** line classifies every exported column, distinguishing "no
terrain" from "no palette entry." `0% painted, 100% nothing to draw` means
there's plenty of terrain and no colors assigned to it yet. Coverage below
25% is additionally logged as a warning on stderr.

**Waiting on a colour** identifies a third, narrower case: columns holding a
block the palette knows draws something, but has no color recorded for.
These are drawn as bare earth rather than as unexplored ground, because
they're known terrain — the map is missing a color, not missing world data.
The mod resolves this automatically by asking a connected player's client;
Witchlight only reports the gap, it can't close it.

**Nothing to draw** is drawn the same way but counted separately, and that
distinction matters differently to different readers: the column was
exported and its height is known, but the topmost block has nothing
visible — air over an otherwise-solid column, or one of the invisible
placeholder blocks a large structure uses beside its real geometry. To an
operator, the two cases are different (one will resolve itself, the other
never will); to a viewer looking at the map, both simply read as ground.
**Only a column that was never exported reads as true absence.**

A flat, chunk-aligned square of that "nothing to draw" color in the middle of
otherwise-finished terrain indicates something else: a chunk the mod
exported while the server had already unloaded its block data, keeping only
the flat heightmap above it, so every position read as air. See `Readable`
in the mod's `Columns.cs`, which now withholds such a chunk instead of
exporting it as solid sky. Chunks exported before that fix self-repair the
next time they're loaded and re-exported; before the fix, they appeared as
black specks through otherwise normal terrain.

Palette reloads mid-session are logged the same way:

```
witchlight: palette reloaded from disk — 45418 blocks, source client (generation 7, tiles dropped)
witchlight: surface 96% painted, 4% nothing to draw, 0% waiting on a colour, 0% unknown blocks
```

## Data sources

| File | Contents |
|---|---|
| `palette.json` | every block: its in-world id, an average color or colorless-type, and which color maps tint it |
| `colormaps/*.png` | the game's lookup images, sampled by climate and by season |
| `map.sqlite` | **written by Witchlight**: every chunk it holds, every remembered version of one, per-player exploration state, active sessions, recent markers, presets, and last-known player positions |
| `tiles/{level}/…` | **written by Witchlight**: cached zoom levels above one-block-per-pixel |

Terrain data belongs to Witchlight alone. The mod detects when a chunk's
blocks change and posts the updated surface over the API channel within
roughly a quarter second — six bytes per column, deflated — and Witchlight
writes it to the database, holds it in memory, and pushes it to every
connected browser looking at that area. A chunk is only written when it
changes, and the file is updated in place, so an idle server does no disk
I/O. The database is a single SQLite file, compiled into the binary, so no
external database server is required.

A "region" is 16×16 chunks, which at a 32-block chunk edge is 512 blocks —
exactly one tile at the finest zoom level, matching the game's own map
region size. This means a chunk change only ever invalidates one tile.

Older versions of Witchlight stored the map as a directory of region files
(`columns/r.{x}.{z}.msqr`), written by the mod and watched by the service. A
service that starts with an empty database and finds those files performs a
one-time import into the database; the format is still documented at the
head of `src/columns.rs` for that purpose. Those files may be deleted once
the import is logged as complete.

**Personal maps**, in detail: under `personal_maps`, what a given player sees
is derived from two facts the database tracks about them — see
`src/memory.rs`. Every chunk within sight of anywhere they've stood is
*discovered*, tracked one bit per chunk. A discovered chunk that changed
while the player was away becomes a *divergence*: a stored pointer to the
version they last saw, kept in the database as long as anyone still points
at it. A player who leaves an area keeps seeing it as it was; a player who
returns has their divergence cleared and sees current state; many players
who all remember the same old version of an area share a single stored copy.
Each player's tile is composed on request, from live memory only, blending
unexplored ground (blank) with remembered ground (from their own recorded
version). Sharing a personal map with others is opt-in and per-group, set in
each player's own settings: enabling a group means everyone in it sees what
that player has explored, as they last saw it. The area around spawn is
visible to everyone, including signed-out browsers, out to
`spawn_radius_chunks`.

**The palette and color maps reload while the service is running.** On a
dedicated server the palette typically arrives from an admin's client some
time after startup; without hot-reloading, the map would show nothing until
manually restarted.

Player positions and markers are never read from a file. The mod posts them
over the **API channel** — a second listener on loopback, on whatever port
is free, with its port and access token written to `api.json` next to the
map so the mod can find it unattended. Positions are held in memory only,
since a stale position on disk is worthless. Markers are the exception: they
are persisted to the database whenever a post differs from the last known
value, so the map still has something to show when the game server is
offline. The API channel accepts writes, which is why it's kept off the
public map port. Witchlight never reads a file it doesn't also write, so an
empty map means nothing has been posted yet, not that something is missing.
The full interface is documented in
[API.md](https://github.com/Tekunogosu/witchlight-csharp/blob/main/API.md).

## Rendering

Each pixel represents one block. A column's block id maps to a base color
via the palette.

A block the palette can't resolve is still drawn, in one of three ways
depending on what the palette says about it. An id the palette has never
seen is drawn in **loud magenta** — this indicates a bug in the export
pipeline, not a fact about the world. A block the palette says draws nothing
(air, an invisible helper block) is drawn as **bare ground**, matching
reality. A block the palette says should draw something, but has no color
recorded for, is drawn as **earth**, with normal slope shading — this
represents a color the map is waiting on, and freshly dug soil should read
as ground while it waits rather than as something else. (These three cases
used to be collapsed into two, which meant freshly dug soil shared a color
with unexplored terrain.)

Grass, leaves, and water ship as **greyscale masks** in the game's own
assets, so a tinted block's color comes from multiplying that mask by its
climate and season color maps — climate sampled at the column's own
temperature and rainfall, season sampled at the chunk's position in the
year. Both lookups mirror the game's own shader, `colormap.vsh`. The game
adds per-position noise on the season axis to avoid a flat-colored forest; a
hash of block position substitutes for that here, so a given block renders
the same color on every run.

Per-column tinting is the part most other map tools skip — they bake a
single tint into the palette, sampled wherever the exporting player happened
to stand, so a desert and a rainforest render as the same color.

Relief shading compares each column's height against its northern and
western neighbors, lighting the world from the northwest, matching standard
map convention.

## Serving

Tiles are 512×512 at one pixel per block, rendered on request and cached
afterward — startup is free, and only the portion of the map someone
actually views gets rendered.

New terrain increments a **generation** counter, exposed in `/info.json` and
in tile URLs as `?v=N`. That's what invalidates the browser cache for
changed tiles; tiles are otherwise marked immutable and cached for a year
(privately, since a personal-map tile is specific to one viewer), while the
page itself and both live-data feeds are served `no-store`.

| Route | Description |
|---|---|
| `/` | the viewer: drag to pan, scroll to zoom |
| `/tiles/{level}/{x}/{z}.png` | one tile, versioned by `?v=`, rendered per-viewer |
| `/info.json` | bounds, chunk edge, chunk count, and generation, as seen by the requester |
| `/events?since=&live=` | long-poll: holds until the map or live feed has changed, then returns what changed |
| `/block.json?x=&z=` | metadata for one block: code, surface height, climate |
| `/live.json` | players and markers, from memory. A player entry includes `Facing` (degrees clockwise from north) and `Portrait` (the name of their rendered portrait image) with `PortraitAt` (when it was generated) |
| `/icons.json`, `/icons/{name}.svg` | marker icon assets |
| `/portraits/{name}.png` | a player's rendered portrait, generated client-side by their own game client. Request with `?v={PortraitAt}` — the filename is stable per player and doesn't change when the image does |

The page holds an open request to `/events`, answered as soon as something
changes — a long poll: an ordinary HTTP request the server holds open until
there's something to report, which the page immediately reissues once
answered. The response carries only the tiles that changed since the page's
last known generation, so activity in one part of a large server repaints
only the affected tiles. Each tile is swapped in only after its replacement
has finished decoding, so the map never flashes blank mid-update. If the
long-poll connection is refused or can't stay open, the page falls back to
polling `/info.json` every two seconds and `/live.json` on the
`live_refresh_ms` interval. Players are drawn as cyan dots labeled with their
name; markers as diamonds in their owner's color, titled above and owner
labeled below — a death marker is always titled "You died here," so the
owner label is what identifies whose it is.

## The viewer

Built on [Leaflet](https://leafletjs.com/), vendored into the binary rather
than loaded from a CDN, so the service ships as one file that works offline
and doesn't expose visitor information to a third party. UI icons come from
[Phosphor](https://phosphoricons.com/), also vendored: filled silhouettes,
matching the style of the game's own waypoint icons. Only the icons actually
used are compiled in; see `src/chrome.rs` for that list, and
`src/vendor/README.md` for exact vendored versions and how to update them.

### Block inspector

The picker (the framed block preview below the zoom control) inspects one
block at a time rather than an area. The cursor stays a normal arrow rather
than becoming a crosshair, since a crosshair centers on — and obscures — the
exact block it's identifying. Instead, the target block is outlined, which
at one pixel per block is sufficient. Selecting a block queries
`/block.json` for its code, the height a player standing there would read,
and the climate that column generated with. An unexported column reports
that explicitly instead of fabricating a block. The reported data always
matches what the renderer used for that pixel, so the label and the color
never disagree.

### Marker list

The marker list answers "what's out there," which the map alone can't once
there are forty markers scattered across a million blocks. It's split by
visibility, searchable by name or owner, and sortable by any column —
including distance from spawn or from the viewer's own current position,
and by owner or visibility. Distance updates live as the viewer moves, but
sort order is fixed at the time the list is drawn — a list that resorted
itself every couple of seconds would move rows out from under the cursor.
Typing in the search box also enlarges matching markers on the map itself,
which is the other half of "finding" one.

**Bulk edit.** Toggling it adds a checkbox column to the list, with a
select-all box in the header; the button row beneath the list then acts on
the checked rows instead of the full list — one consistent rule across all
four actions, each of which reports how many rows it affected. Two of the
four are new: a delete button with a confirmation step, and *Apply preset*,
which opens a searchable preset list and overwrites every checked marker's
name, icon, color, and block pattern from the chosen preset. Location and
visibility are untouched by this, since those are properties of an
individual marker rather than of a marker type.

**Right-clicking a marker opens it**, whether or not it belongs to the
viewer. Their own marker opens as an editable form: name, color, icon,
visibility, and a delete button. Someone else's opens as a read-only record,
with the two actions that are still available to any viewer: pinning it, and
saving it as a personal preset. The marker list opens the same dialog from a
row.

**Every marker records which block it was placed on**, read from the game
when created and re-read whenever it moves. Presets created from a marker
inherit that pattern automatically, since a preset with no block pattern
matches nothing. Markers created before this feature existed fall back to
whatever block the map itself has recorded at that location.

**Pinning** adds the marker to the viewer's own in-game map as a pinned
waypoint, kept against the map edge instead of scrolling off. This is a
purely personal, client-side choice — pinning someone else's marker doesn't
affect anyone else's view. Any marker visible to a viewer can be pinned,
since visibility is the only relevant permission. For the viewer's own
markers, this is the same pin flag the game's waypoint UI already exposes,
so the two stay in sync; for others' markers, the mod stores this choice
alongside the rest of that viewer's visibility preferences.

Markers can also be scaled up for readability — see *Marker size* in the
accessibility panel, alongside color and colorblindness settings. It scales
the same base size the game itself uses, so a marker's position and its drop
shadow scale together; the search-highlight enlargement applies on top of
whatever size is configured.

### Layout

Every UI button lives in a single column along the left edge: identity,
display settings, markers, claims, zoom, and the block picker, in that
order. (The zoom control used to be centered on that edge separately, which
created two independently-sliding stacks that could overlap on a short
window — this was fixed by unifying them into one column.)

The world clock sits in the opposite corner, next to the online-player
list — status information rather than a control, and a readout placed in
the middle of a column of buttons reads as another button to press. It
takes over that corner's full space when no one is online, since the player
list collapses to nothing.

### Land claims

Claims render as their own toggleable layer, switchable from either the tool
column or the settings panel (the two controls are kept in sync). Each
claim renders as one shaded rectangle per contiguous area, so a claim built
from several adjoining boxes keeps its actual boundary shape rather than
being wrapped in a single bounding box; clicking one shows its owner, name,
and vertical extent. Each owner gets a distinct color, derived from their
own identity so it's consistent across every viewer and session (though
similar hues can still collide, which is why ownership is always also shown
in text in the popup and the list). Claims are only rendered on explored
ground: a claim extending past the explored boundary is clipped to it, and a
claim entirely outside it isn't rendered at all — rendering a boundary over
unexplored black space would imply the map knows something it doesn't. Which
claims a given viewer receives is filtered server-side against a
visibility list the mod supplies, so a viewer without access to a claim
receives no data for it at all, rather than data the client is asked not to
display.

Players permitted to claim land get an additional button that **draws a new
claim**: drag once across the map, with a crosshair cursor and the map view
locked in place, then a confirmation dialog appears showing the dragged
area. The dialog previews the claim's volume in cubic meters and the
remaining allowance it would leave, and rejects an obviously oversized claim
before spending a round trip on it — final validation still happens
server-side; this only avoids a pointless wait for a claim that was never
going to succeed.

The claim form is a column of labeled sections rather than a row of
fields: name, area (as *West*/*North*/*East*/*South*), depth, and
permissions. Permissions expose exactly what `/land claim grant` offers in
the game: the two global everyone-permissions, plus a list of individually
permitted builders — nothing broader, since anything broader wouldn't
actually be enforced by the game.

A third button lists every claim on the server: what exists, who owns it,
where it is, and edit access for claims the viewer controls — renaming,
changing permissions, and abandoning a claim all happen from this list. The
claim boundary itself is display-only here; moving one requires validating
against every other claim and the owner's remaining allowance, information
the map can't show ahead of time, so redrawing a boundary means creating a
new claim rather than editing the old one.

Depth is a required, explicit field rather than inferred, even though the
map is a top-down view — claim volume is measured in cubic meters and depth
is the dominant factor in that volume, so defaulting to full world height
would let a small footprint use a huge allowance. It defaults to a band
around the ground level under the selected area's center, matching a typical
base's needs; *All* is available for claims that genuinely want full height.

### Chunk grid

An optional overlay outlining chunk boundaries (32 blocks, matching the
game's own chunk size). Off by default, rendered faint enough not to
obscure terrain, and automatically hidden below roughly 8 pixels per chunk,
where a grid stops being legible and starts being visual noise. It is
deliberately not clipped to the explored area — seeing exactly where
exploration stops is one of its main uses.

## Codebase layout

One module per subject; shared utilities live outside anything that first
needed them. Roughly outside-in reading order:

| | |
|---|---|
| `main.rs` | CLI entry point and subcommand dispatch |
| `config.rs` | settings file plus CLI flag overrides |
| `server.rs` | startup: load state, bind, spawn worker threads |
| `routes.rs` | public HTTP route table |
| `apiport.rs` `api.rs` | the private API channel the mod posts to, and its published address |
| `state.rs` | shared state across request threads |
| `store.rs` | the map's own database: chunks, remembered versions, per-player exploration state |
| `memory.rs` | per-player exploration memory and sharing rules |
| `scope.rs` | what a given viewer is shown: full map or personal memory |
| `events.rs` | push notifications to connected browsers |
| `watch.rs` | detecting new palette or block-name data from the mod |
| `feeds.rs` | JSON responses served to the page |
| `viewer.rs` `viewer/` | the page itself: markup, styling, and script bundling |
| `chrome.rs` | which vendored icons are compiled into the binary |
| `columns.rs` `pyramid.rs` `render.rs` `palette.rs` `color.rs` | rendering pipeline: region files to pixels |
| `live.rs` `pending.rs` `preferences.rs` `auth.rs` `facts.rs` `wire.rs` | protocol between the mod and the service |
| `http.rs` `urls.rs` `cache.rs` `net.rs` `files.rs` `random.rs` `error.rs` | generic utilities with no map-specific knowledge |

The last row imports nothing from the rows above it. That's what keeps those
modules reusable, and it's worth preserving when adding new code.

## Testing

```sh
cargo test                     # everything, including the viewer's own tests
node tests/viewer.mjs          # just the viewer tests, with verbose output
./tests/zoom-sweep.py URL      # visual regression check against a running map, via a real browser
```

The viewer is plain JavaScript and is tested as JavaScript: `tests/viewer.mjs`
imports the functions directly from `src/viewer/*.js` and exercises them,
rather than maintaining a separate reimplementation to test against — a
reimplementation can pass while the real page is broken, which is how a
missing clamp once shipped in `draw` undetected. The test file list is read
from `src/viewer.rs`, so any script added to the page is automatically
covered. `cargo test` shells out to `node` for this and reports clearly if
it's missing.

`zoom-sweep.py` requires a running map and a real browser, so it's excluded
from `cargo test`. It drives a headless Chromium instance across the full
zoom range and checks that colors actually reach the screen — the map has
gone blank past a certain zoom level four separate times, for four unrelated
root causes, and in each case the viewer's own internal math was correct and
nothing else caught it. This test checks the one thing all four failures had
in common: whether the rendered image actually arrives.

## Known limitations

- **No authentication on the map port.** Anyone who can reach it sees
  everything except markers their owners marked private (those are filtered
  server-side before the response is sent, since a client can't be trusted
  to hide data it's already received). The separate API channel does accept
  writes, so it's restricted to loopback and requires the token from
  `api.json`.
- **Sessions are in-memory only.** A service restart costs every signed-in
  user one re-click of their login link. Witchlight doesn't otherwise
  persist anything about a user it wasn't explicitly told to.
