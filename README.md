# mapstique

Renders and serves a browsable map of a Vintage Story world.

This is the renderer half. It never reads a save file and does not need the game
installed: everything it draws comes from exports written by the companion server
mod in [`mapstique-csharp`](../../mapstique-csharp). The mod knows the game; this
knows pixels.

## Tests

```sh
cargo test                     # everything, including the viewer's
node tests/viewer.mjs          # just the viewer's, with its output
```

The viewer is JavaScript and is tested as JavaScript. `tests/viewer.mjs` lifts the
functions out of `src/viewer.html` by name and runs them, rather than keeping a
second copy to check — a copy passes happily while the page it stands for is
broken, which is how the clamp was once removed from `draw` without a single
check noticing. `cargo test` shells out to `node`, and says so if it is missing.

## Running

The usual way is not to. The [server mod](../../mapstique-csharp) carries this
binary inside its archive and starts it once the world is ready, so installing the
mod installs the map — its settings are then `mapstique.conf` in the game's
`ModConfig` folder and everything it prints goes to `Logs/mapstique-service.log`.
Set `autostart = false` there to run it yourself instead, which is what a map that
should stay up while the game server is down wants.

Run by hand it looks like this:

```sh
mapstique                                   # serve, using the saved settings
mapstique -d /srv/vs/data                   # point it at a server's --dataPath
mapstique -d /srv/vs/data -S                # ...and remember that
mapstique render --out map.png              # one PNG of everything exported
```

Settings live in `~/.config/mapstique/config.toml`, written with the defaults on a
first run — or wherever `-c` names, which is how the server mod points this at its
own `ModConfig/mapstique.conf`:

```toml
vs_data = "/home/vintagestory/data"   # the server's --dataPath
bind = "0.0.0.0:8080"                 # every interface; 127.0.0.1 for this machine only
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

`vs_data` is the game's data directory and exports are read from the `mapstique`
folder inside it. A directory holding `palette.json` directly is accepted too, so
files copied off a server with `scp` need no second flag. Whichever it picks is
printed on start.

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
mapstique 0.2.0
mapstique: reading /srv/vs/data/mapstique
mapstique: 635 chunks, 864x1024 blocks
mapstique: palette from client, 45418 blocks, 26 colour maps (game 1.22.7)
mapstique: surface 96% painted, 4% nothing to draw, 0% unknown blocks
mapstique: serving on http://192.168.1.158:8080  (on your network)
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
mapstique: palette reloaded from disk — 45418 blocks, source client (generation 7, tiles dropped)
mapstique: surface 96% painted, 4% nothing to draw, 0% unknown blocks
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

The format still moves while Mapstique is alpha. A region this build cannot read
is skipped and said so on stderr, and the mod clears a map it cannot read on start
rather than upgrading it — so an upgrade costs the explored area, which comes back
as players move through it.

**All three are reloaded while the service runs.** That matters as much for the
palette as the terrain: on a dedicated server the palette arrives from an admin's
client some time after start-up, and a service that read it once would show nothing
until restarted.

Players and markers do not come from a file. The mod posts them to the **API
socket** — by default a unix socket in `/tmp` named after the export directory,
which both sides work out for themselves — and they are held in memory, because a
position is worthless by the time a disk has finished with it. Markers are the
exception and are written to `markers.json` when they arrive, so the map still has
something to show when the game server is off. That socket takes writes, which is
why it is not on the map port. Nothing here reads a file this build does not
write, so an empty map means nothing was posted rather than nothing was found.
Every interface is written up in [API.md](../../mapstique-csharp/API.md).

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
| `/` | the viewer: drag to pan, scroll to zoom, `F` to fit |
| `/tiles/{x}/{z}.png` | one tile, versioned by `?v=` |
| `/info.json` | bounds, chunk count, generation |
| `/live.json` | players and markers, from memory. A player carries `Portrait`, the name of their picture, where they have sent one |
| `/icons.json`, `/icons/{name}.svg` | the pictures markers are drawn with |
| `/portraits/{name}.png` | a picture a player's own client drew of their seraph |

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
is looking at the map. See `src/vendor/README.md` for the exact release it came
from and how to move version.

## Rough edges

- **Zoom is browser-side.** Tiles render at one pixel per block and are scaled in
  the viewer; a real pyramid with downsampled levels would be sharper zoomed out.
- **The tile cache is unbounded.** It grows with the area anyone has looked at.
  Reloads only drop the tiles a changed region touches, so it is otherwise kept.
- **No authentication.** Anything that can reach the map port sees everything. The
  API socket is separate, and takes writes, so it is a unix socket rather than a
  port.
