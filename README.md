# witchlight

Renders and serves a browsable map of a Vintage Story world.

This is the renderer half. It never reads a save file and does not need the game
installed: everything it draws comes from exports written by the companion server
mod in [`witchlight-csharp`](../../witchlight-csharp). The mod knows the game; this
knows pixels.

## Tests

```sh
cargo test                     # everything, including the viewer's
node tests/viewer.mjs          # just the viewer's, with its output
./tests/zoom-sweep.py URL      # against a running map, in a real browser
```

The viewer is JavaScript and is tested as JavaScript. `tests/viewer.mjs` lifts the
functions out of `src/viewer/*.js` by name and runs them, rather than keeping a
second copy to check — a copy passes happily while the page it stands for is
broken, which is how the clamp was once removed from `draw` without a single
check noticing. It reads the file list out of `src/viewer.rs`, so a script added
to the page is a script the tests see. `cargo test` shells out to `node`, and says
so if it is missing.

`zoom-sweep.py` is the one that needs a map and a browser, so it is not part of
`cargo test`. It drives chromium across the zoom range and counts the colours that
reach the screen. The map has gone blank past some zoom four times, for four
unrelated reasons, and every one of them looked identical from the page and left
the viewer's own arithmetic correct — so this checks the only thing they had in
common, which is that the picture stops arriving.

## Running

The usual way is not to. The [server mod](../../witchlight-csharp) carries this
binary inside its archive and starts it once the world is ready, so installing the
mod installs the map — its settings are then `witchlight.conf` in the game's
`ModConfig` folder and everything it prints goes to `Logs/witchlight-service.log`.
Set `autostart = false` there to run it yourself instead, which is what a map that
should stay up while the game server is down wants.

Run by hand it looks like this:

```sh
witchlight                                   # serve, using the saved settings
witchlight -d /srv/vs/data                   # point it at a server's --dataPath
witchlight -d /srv/vs/data -S                # ...and remember that
witchlight render --out map.png              # one PNG of everything exported
```

Settings live in `~/.config/witchlight/config.toml`, written with the defaults on a
first run — or wherever `-c` names, which is how the server mod points this at its
own `ModConfig/witchlight.conf`:

```toml
vs_data = "/home/vintagestory/data"   # the server's --dataPath
map_data = ""                         # where maps are kept; empty means <vs_data>/witchlight
per_world = false                     # a directory per world inside it
bind = "0.0.0.0:8080"                 # every interface; 127.0.0.1 for this machine only
api_bind = ""                         # where the mod posts; empty means loopback on a free port
api_token = ""                        # what it must present; empty means a fresh one each start
markers_public = false                # whether a marker nobody has chosen for is everyone's
autostart = true                      # whether the server mod starts this itself
announce = true                       # whether it tells joining players where the map is
announce_url = ""                     # and what to tell them; empty means work it out
```

`autostart`, `announce` and `announce_url` are the settings here that this program
never reads. They say who starts the map and who is told about it, which are
questions about the pair rather than about either half, so they sit with everything
else about the map instead of in a file of their own.

Set `announce_url` on any server a player cannot reach directly. Left empty, the
address given out is the one this machine can see for itself, which is right on a
LAN and wrong behind a proxy, a domain or NAT.

Flags win over the file and stay one-off unless `-S` is given, so a quick look at
another world does not rewrite your setup. `-p` prints the resolved settings.

`vs_data` is the game's data directory and exports are read from the `witchlight`
folder inside it, unless `map_data` names somewhere else. A directory holding
`palette.json` directly is accepted too, so files copied off a server with `scp`
need no second flag. Whichever it picks is printed on start.

`per_world` files each world's map in a directory of its own inside that folder,
named after the world. A dedicated server runs one world and leaves this off; the
mod turns it on for singleplayer, where every save shares one data path and would
otherwise write its terrain into the last world's map. Nothing is shared between
those directories, not even the files that would be identical — one written once
and left alone costs nothing to keep, while one rewritten every time you switch
between a world with no mods and a world with fifty costs a disk.

The mod names the directory outright with `--exports` when it starts this, because
it is the half that knows which world is running. By hand it is only needed when
`per_world` is on and more than one world has been exported; with one, that one is
served, and with several the service says which it found rather than guessing.

It binds every interface by default, so the map is reachable from the rest of your
network as soon as it starts. On start it prints the addresses it can actually be
reached at, since `0.0.0.0` is not something anyone can type into a browser, and
writes them to `service.json` beside the export — the address on the network first,
because that is the one worth giving somebody else. That file is how the server mod
knows what to put in its own log and what to tell a player who joins.

Reaching it from the open internet is a further step: put it behind a reverse
proxy that terminates TLS. The service has no authentication, and everything it
serves — terrain, player positions, every marker with its owner's name — is
readable by anyone who can reach the port.

## What it tells you

```
witchlight 0.2.0
witchlight: reading /srv/vs/data/witchlight
witchlight: 635 chunks, 864x1024 blocks
witchlight: palette from client, 45418 blocks, 26 colour maps (game 1.22.7)
witchlight: surface 96% painted, 4% nothing to draw, 0% unknown blocks
witchlight: serving on http://192.168.1.158:8080  (on your network)
```

The version comes first on every run, because `--version` only helps if you think
to ask. **`palette from server` versus `from client`** says which machine's assets
the colours came from — a dedicated server's own palette is nearly empty, and that
is the usual reason a map looks blank. The **surface** line walks every exported
column and classifies it, which separates "no terrain" from "no palette": a map
reading `0% painted, 100% nothing to draw` has plenty of terrain and no colours for
it. Below 25% painted it says so explicitly on stderr.

Reloads are narrated the same way, so a palette arriving mid-session is visible:

```
witchlight: palette reloaded from disk — 45418 blocks, source client (generation 7, tiles dropped)
witchlight: surface 96% painted, 4% nothing to draw, 0% unknown blocks
```

## What it reads

| File | What it is |
|---|---|
| `palette.json` | every block: its id in this world, an average colour, and which colour maps tint it |
| `colormaps/*.png` | the game's lookup images, sampled by climate and by season |
| `columns/r.{x}.{z}.msqr` | per chunk column: top block, its height, the climate there, and the chunk's season |
| `tiles/{level}/…` | **written here**, the zoom levels above one block per pixel |

The map is a directory of regions rather than one file. A region is eight chunks
on a side, which at a chunk edge of 32 is 256 blocks — **exactly one tile**. A
chunk that changes therefore belongs to one region file and one tile, so the mod
rewrites what moved rather than the whole map, and this side reloads one region
and drops one tile rather than starting over.

Each region is a gzip stream of fixed-size records after a 20-byte header,
documented in `src/columns.rs`; the compression runs between five and eight times
on real exports. Chunks accumulate across exports, so the map keeps everything the
server has ever had loaded.

The format still moves while Witchlight is alpha. A region this build cannot read
is skipped and said so on stderr, and the mod clears a map it cannot read on start
rather than upgrading it — so an upgrade costs the explored area, which comes back
as players move through it.

**All three are reloaded while the service runs.** That matters as much for the
palette as the terrain: on a dedicated server the palette arrives from an admin's
client some time after start-up, and a service that read it once would show nothing
until restarted.

Players and markers do not come from a file. The mod posts them on the **API
channel** — a second listener on loopback, on whatever port the machine had free,
whose port and token are written to `api.json` beside the map so the mod finds it
without being told. They are held in memory, because a position is worthless by
the time a disk has finished with it. Markers are the exception and are written to
`markers.json` when they arrive, so the map still has something to show when the
game server is off. That channel takes writes, which is why it is not on the map
port. Nothing here reads a file this build does not write, so an empty map means
nothing was posted rather than nothing was found. Every interface is written up in
[API.md](../../witchlight-csharp/API.md).

## How a pixel is decided

One pixel is one block. The column's block id gives a base colour from the palette.

Grass, leaves and water ship as **greyscale masks** in the game's assets, so a
tinted block is multiplied by its colour maps: the climate map sampled at that
column's own temperature and rainfall, and the season map at the chunk's position
in the year. Both are the same lookups the game's own shader does, taken from
`colormap.vsh`. The season map's second axis is per-position noise in the game,
stopping a forest being one flat colour; a hash of the block position stands in for
it, so the same block is the same colour on every render.

Per-column tinting is the part the established map mods do not do — they bake one
tint into the palette, sampled wherever the exporting player happened to be
standing, so a desert and a rainforest come out the same colour.

Relief comes from comparing each column's height with its northern and western
neighbours, lighting the world from the north-west the way every game map does.

## Serving

Tiles are 256×256 at one pixel per block, rendered **on request** and then cached,
so start-up costs nothing and only the part of the world someone looks at is drawn.

Every request stats the export; if the timestamp moved, the file is read and hashed,
and only a genuinely different hash triggers a reload — otherwise the mod's
30-second rewrite would re-render everything for nothing. A real reload bumps a
**generation** counter, which appears in `/info.json` and in tile URLs as `?v=N`.
That is what gets new terrain past the browser cache; tiles are otherwise marked
immutable and cached for a year, while the page and both feeds are `no-store`.

| Route | |
|---|---|
| `/` | the viewer: drag to pan, scroll to zoom |
| `/tiles/{x}/{z}.png` | one tile, versioned by `?v=` |
| `/info.json` | bounds, chunk edge, chunk count, generation |
| `/block.json?x=&z=` | what is at one block: its code, its surface height, its climate |
| `/live.json` | players and markers, from memory. A player carries `Facing`, which way they are looking in degrees clockwise from north, and `Portrait`, the name of their picture, with `PortraitAt`, when it was drawn, where they have sent one |
| `/icons.json`, `/icons/{name}.svg` | the pictures markers are drawn with |
| `/portraits/{name}.png` | a picture a player's own client drew of their seraph. Ask with `?v={PortraitAt}`: the name is the player's and does not change when the picture does |

The page polls `/info.json` every 5 seconds and `/live.json` every 2. It asks with
`?since=` and gets back the tiles that actually changed, so a server where someone
is building repaints one square rather than the map, and each tile is swapped only
once its replacement has decoded — the old one stays up meanwhile, so the map never
blinks. Players draw
as cyan dots with their name; markers as diamonds in their owner's colour, with the
title above and the owner below — every death marker is titled "You died here", so
the owner is the only thing that says whose it is.

## The viewer

[Leaflet](https://leafletjs.com/), vendored into the binary rather than fetched
from a CDN, so the service stays one file that works offline and tells nobody who
is looking at the map. The marks its furniture wears are vendored beside it, from
[Phosphor](https://phosphoricons.com/): filled silhouettes, which is what the
game's own waypoint marks are, so the two read as one set. Only the six the page
uses are compiled in, and `src/chrome.rs` is the list. See `src/vendor/README.md`
for the exact releases both came from and how to move version.

The **picker** — the block in a frame, below the zoom control — reads one block rather than
looking at all of them. It leaves the pointer an arrow, since a crosshair centres
on the block it is naming and so covers it. It outlines the block instead, which
at one pixel per block is the whole point of the tool, and asks `/block.json` what
is there:
the block's code, the height a player standing on it would read, and the climate
that column was generated with. A column nobody has exported says so instead of
naming a block that is not there. The reading is the same one the renderer made
for that pixel, so the words and the colour underneath them always agree.

The **chunk grid** outlines the chunks the export names, which the game puts at 32
blocks. It is off until switched on, faint enough to read the terrain through, and
it stops being drawn below eight pixels to a chunk — past that a grid stops being
a grid and becomes a wash. It is deliberately not clipped to the explored area:
seeing where that stops, squarely, is half of what a grid is for.

## How it is laid out

One subject to a file, and a utility never sits inside the system that first
needed it. Reading order, roughly outside in:

| | |
|---|---|
| `main.rs` | the command line, and what each subcommand does |
| `config.rs` | the settings file, and the flags laid over it |
| `server.rs` | bringing the map up: read what is there, bind, hand out threads |
| `routes.rs` | what the public port answers, and to what |
| `apiport.rs` `api.rs` | the private channel the mod posts on, and where it is published |
| `state.rs` | what the request threads share |
| `watch.rs` | noticing that the mod has written something |
| `feeds.rs` | the JSON the page asks for |
| `viewer.rs` `viewer/` | the page: markup, style, and eleven scripts joined in order |
| `chrome.rs` | which marks the furniture wears, and which of the vendored pack reach the binary |
| `columns.rs` `pyramid.rs` `render.rs` `palette.rs` `color.rs` | the map itself, from region file to pixel |
| `live.rs` `pending.rs` `preferences.rs` `auth.rs` `facts.rs` | what the two halves say to each other |
| `http.rs` `urls.rs` `cache.rs` `net.rs` `files.rs` `random.rs` `error.rs` | utilities, which know nothing about maps |

Nothing in the last row imports anything above it. That is the whole of what
keeps them reusable, and it is worth checking before adding to one.

## Rough edges

- **No authentication on the map port.** Anything that can reach it sees
  everything, apart from markers their owners kept private — those are filtered
  before the page is answered, since a browser cannot be asked to hide what it
  has been handed. The API channel is separate and takes writes, so it answers
  only on loopback and only to a caller carrying the token from `api.json`.
- **Sessions live in memory.** A restart of the service costs everyone one click
  of one login link, and the map keeps nothing about anybody it was not asked to.
