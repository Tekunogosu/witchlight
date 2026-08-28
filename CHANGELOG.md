# witchlight

The map service. It pairs with the [server mod](../../mapstique-csharp), and the
two **must match on minor version**: minor is the compatibility generation, moved
whenever the file format or the socket protocol changes. Patch is free to differ,
and covers anything that changes only one half.

## Unreleased

### A new portrait shows without a reload

A player who sends a new picture keeps the name they had — it is derived from who
they are, not from what the picture holds — so nothing downstream could tell that
anything had happened. The card compared one name against the same name, decided
nothing had changed, and left the picture alone; the browser, handed an address it
had seen before, was entitled to do the same. The map went on showing the old face
until somebody reloaded the page.

A live player now carries `PortraitAt`, when their picture was written, and the map
asks for `/portraits/{name}.png?v={PortraitAt}`. The address is what the card
compares and what the browser fetches, so a redrawn player changes both. It lands
on the next live poll, a couple of seconds later.

The time moves only when bytes were actually written, so a character taken apart
and put back the same way costs no refetch.

### The project is called Witchlight

Every mapstique here is now witchlight — the crate, the binary, the settings
directory, the socket, the page title, and everything it prints. Nothing about what
it renders or serves changed, which is why the version did not move.

Settings live in `~/.config/witchlight/config.toml` and exports are read from the
`witchlight` folder inside the game's data directory. Both are new paths rather
than renamed ones, so a first run after this writes fresh defaults and the map
rebuilds as the server exports; the old `mapstique` folder and config are left
where they are and want removing by hand.

The region format is untouched — `MSQR` and `.msqr` never carried the old name, and
changing them would have been a format change rather than a rename.

The viewer keeps its settings under `witchlight.settings`, so the panel comes back
at its defaults once.

## 0.16.1

### A tool for reading one block

The 🔍 below the zoom control turns the pointer into a picker. It outlines the
block under it — at one pixel per block the pointer covers whatever it is over, so
being shown which block the map means is most of the tool — and says in the corner
what is there: the block's code, the surface height, and the climate the column
was generated with. `/block.json?x=&z=` is where it comes from.

The pointer stays an arrow while the tool is armed. A crosshair centres on what it
names and so sits on top of it, which is the one thing a tool for looking at a
single block must not do.

The reading is the renderer's own. How a column resolves against the palette had
been decided in two places that happened to agree; it is now decided in one, so
the map cannot name a block it did not draw. A column nobody has exported says
that rather than naming something that is not there.

### A chunk grid

Off until switched on, beside the other things the settings panel shows. It
outlines the chunks the export names, faint enough to read the terrain through,
and stops being drawn below eight pixels to a chunk. It is not clipped to the
explored area — where the explored area stops is one of the things it is for.

### Fixed

`/info.json` says the chunk edge, which the viewer had no way to learn and the
region format has carried all along.

The routes table promised an `F` key that fits the map to the world. There has
never been one.

## 0.16.0

### A player is their portrait, or their initial

The face assembled from three colours is gone, and so is `/skincolors.json` and
everything that fed it. A card shows the picture a player's own client drew of
them, and where there is none it shows their initial — which is what the map shows
now rather than only until somebody runs the command.

There was never anything worth drawing in between. What a seraph looks like exists
only on the machine rendering it; everything a server can read about an appearance
amounts to less than a letter does.

**Both halves must be upgraded together.** The mod no longer sends skin part
colours and this no longer serves them.

## 0.15.0

### Players look like themselves

A player's card shows a picture their own client drew of their seraph — skin,
hair, clothes, armour, whatever they are wearing — in place of the face this used
to assemble out of three colours. `/portraits/{name}.png` serves them, filed under
a name the mod derives from a player uid, which is base64 and carries characters a
path cannot.

The drawn face is still there for anyone who has not sent a picture, and a picture
that will not load falls back to it rather than leaving a broken image in the card.

Portraits are held for a minute rather than an hour: a player who sends a new one
keeps the name they had, so an address that never changes must not be cached as
though it did.

### Fixed

One rule decides whether a name in a URL is only ever a name, rather than one copy
of it per kind of stored file.

## 0.14.0

### It says where it is, so the mod can tell people

The addresses the map answers on are written to `service.json` beside the rest of
the export as soon as it binds. Working out what `0.0.0.0` actually means is this
program's question and it had already answered it for its own log; the mod needed
the same answer and had no way to ask. The address on the network comes first,
because that is the one worth giving somebody else.

`announce` and `announce_url` are new, and like `autostart` this program does not
read them: they say whether the mod tells a joining player where the map is, and
what to tell them. `announce_url` is empty by default, meaning the address worked
out here — which is right on a machine a player can reach directly and wrong for a
server on the open internet, where the map is reached at a name, through a proxy,
on a port this never sees. Only an operator knows that address.

The startup lines now put the address on the network first and mark loopback as
what it is, rather than listing loopback first and annotating the other.

**Both halves must be upgraded together.** The settings file gained two keys, and
a 0.13 service refuses a file it does not recognise every field of. An existing
file is brought up to date, values kept, with:

```sh
witchlight -c <file> --save-config -p
```

## 0.13.0

### The server mod can run this

The map service rides along inside the server mod's archive and is started by it,
so installing the mod installs the map. It stays a separate program — it knows
pixels, the mod knows the game, and a map worth keeping outlives any one game
server — but it is no longer a second thing to fetch, configure and remember to
start.

Settings move with it. Started by the mod, the file is `witchlight.conf` in the
game's `ModConfig` folder, named on the command line, so everything about one
server's map sits with that server rather than in one shared file in a home
directory. Run by hand, `~/.config/witchlight/config.toml` is unchanged.

`autostart` is new, and is the one setting here that this program does not read:
it says whether the mod starts the service unasked. Turn it off to run
`witchlight serve` yourself, which is what a map that should stay up while the game
server is down wants.

Everything the service prints goes to `witchlight-service.log` in the game's own
`Logs` folder, so it can be tailed while it runs and is not interleaved with a
game server's log.

**Both halves must be upgraded together.** The settings file gained a key, and a
0.12 service refuses a file it does not recognise every field of.

## 0.12.1

### Fixed

The map was blank at the zoom it opens on, until an admin joined the game.

A world that grows past a power of two gains a coarsest level. The levels above
zero are rebuilt by walking up from whatever region changed, so exactly one tile
was built at the new level — the one above the region that had just arrived — and
every other tile there had nothing beneath it that had changed, so nothing ever
asked for it. That level is the one a viewer opens on, so the map read as empty.

Restarting did not fix it: the start-up scan asked only whether a region's level 1
tile was current, which it was. An admin joining did fix it, by accident — their
client supplies a palette, a new palette recolours every tile, and rebuilding
every tile filled the level in.

One function now answers what the levels owe the world, comparing a region
against every level above it rather than only the first, and it is asked both at
start and whenever the world turns out to need a level the pyramid does not have.

## 0.12.0

### Faces are drawn from a table of colours

A player's appearance arrives as the names of the parts they chose — `skin4`,
`mossgreen`, `azure` — and `/skincolors.json` says what each name looks like. A
name with no colour falls back to that player's initial.

The table is fetched once and again whenever a player names a part not in it,
which is what happens when a mod adds appearances or when an admin supplies the
colours after the map was already open.

## 0.11.3

### Fixed

The generation was bumped twice for one export: once when the watcher reloaded the
changed regions, again when the builder finished the levels above. The generation
versions every tile URL, so that was the same pixels fetched under two names — and
on a dense world a tile is a third of a megabyte. Whether the second fetch
happened depended on where the five second poll fell, which is why it looked
intermittent.

The builder now announces the whole export at once, including the level 0 tiles
the watcher reloaded.

## 0.11.2

### Fixed

The food bar is green, as it is in the game, rather than amber.

## 0.11.1

### Says when the mod is older than it is

A map counting from absolute zero rather than from spawn looks like a bug in the
map, and nothing on either side looks wrong — so a missing `world.json` is now
said at start, naming the reason.

## 0.11.0

### Coordinates the game would recognise

Vintage Story shows every coordinate a player sees relative to world spawn, while
the world is a million blocks across with spawn near the middle. This map showed
absolute positions — not wrong, but agreeing with nothing a player can compare
them against, so a marker the map called `511900` was `-100` on their screen. The
mod writes where spawn is and the map counts from there. Absolute is a setting,
since that is what region files and tile URLs use.

The corner also reports wherever the pointer is, falling back to the middle of the
view when it leaves.

### Following a player

Clicking a card keeps the map on that player. Zooming does not stop it — looking
closer is not choosing to look elsewhere — but dragging does.

### Nothing on screen is rebuilt because data arrived

Cards, player markers and map markers are built once and afterwards written into.
Everything on the live feed shares a two second beat and a walking player changes
position and food on every one, so anything that rebuilt on change rebuilt
constantly: forty markers and thirteen cards destroyed and recreated twice a
minute. That was the flicker.

### Fixed

- A player whose skin colours could not be read drew as a black face on a black
  background instead of falling back to their initial: the colours arrive as zero,
  which is a perfectly good `#000000`, so the card looked empty rather than plain.
- A long player name pushed its card and bars out past their own border.

## 0.10.0

### Who is online

A card each for the players on the server: a face, their name, and how much health
and food they have. Both readings come from the server, which already knows them,
so nothing is asked of any client. The list scrolls when there are more players
than fit.

The face is drawn from the colours that player chose — skin, hair and eyes, read
from their applied skin parts. It is not a likeness and does not pretend to be:
the game keeps no portrait of anyone, its character screen renders the live entity
into a panel and there is nothing to read back. The colours are enough to tell one
player from another.

### A settings panel

A cogwheel, and toggles for what to show. Kept in the browser rather than on the
server: these are one person's preferences about one screen, and everybody looking
at the same map is entitled to a different answer.

## 0.9.2

### Fixed

Markers were destroyed and rebuilt on every live poll — forty elements twice a
minute, each with a picture to re-resolve. Markers that have not changed are now
left alone entirely, and a player who moved is moved rather than replaced.

This is also why a popup vanished a moment after being opened: it was attached to
an element that no longer existed.

## 0.9.1

### Marker pictures

Waypoints are drawn with the game's own icons, tinted with the colour their owner
chose. A name with no picture falls back to a plain shape, and the set is
refetched when a marker names one that is not known — so a marker mod installed
while the map is open is picked up without a reload.

### Fixed

A tile the service had not built answered `404`, which the viewer had nothing to
put in place of, so the browser drew a broken image. It now draws nothing.

## 0.9.0

### A view has an address

`#x,z,px-per-block` in the address bar, the same three numbers the corner shows,
so a link says what it points at. It follows panning and zooming, and opening one
lands where it says rather than fitting the world first.

Built to make the zoom levels testable from outside the page, which they were not
before; that it is also the feature every map has is a happy coincidence.

## 0.8.2

### Fixed

Zooming past eight pixels per block went black. A tile layer defaults to a maximum
zoom of eighteen and tests the map's own zoom against it *before* clamping to the
finest level, so past that every tile was dropped — at exactly the magnification
the level system exists to provide. The layer now states the range it is used at.

## 0.8.1

### Fixed

The map tore itself down whenever the world grew. One pixel per block sat at
whatever zoom the world happened to need, so a player exploring far enough added a
level and everything shifted underneath: the tile layer was rebuilt and the view
snapped back to fit, on nearly every export. One pixel per block is now pinned to
a fixed zoom, and growth only changes how far out the view may go.

## 0.8.0

**Clears the map on upgrade.** Regions changed size and there is no reader for the
old ones. Deploy with mod 0.8.0; neither half reads what the other minor wrote.

### The map has zoom levels

Tiles were drawn at one block per pixel at every zoom and scaled in the browser,
so the number on screen grew as the square of how far out you were: about fifty to
fit a small world, and **seventy-nine thousand** at the zoom-out limit, which is
minutes of rendering and a browser holding seventy-nine thousand images.

Level 0 is one block per pixel; each level above covers twice as much world, so a
view holds roughly twenty tiles however far out it is. Levels are numbered from
the finest, which is the numbering a world can grow under: level 0 stays one block
per pixel whatever anyone explores, so a bigger world adds numbers rather than
renaming every tile already written.

Each level is built by averaging the level below two by two. Averaging `2^L`
blocks straight from the world gives the same answer and costs `4^L` times as
much — a thousand times, at level 5. Averaged rather than sampled, because
sampling erases everything narrower than the step: paths, walls, rivers.

Level 0 is not stored; it is rendered from the world and cached. Everything above
is written when a region changes, through a queue that drains every two seconds,
so a region changing repeatedly costs one rebuild rather than one per change. A
restart rebuilds only what moved while it was away — a region whose level is
already newer than it is left alone.

### A region is a tile is a game region

Regions went from eight chunks square to sixteen: 512 blocks, one tile at the
finest level, and the same square the game itself calls a map region. Three
concepts became one.

### The viewer is Leaflet

Vendored, not fetched — the service is still one binary that works offline. It
brings the zoom handling, tile eviction, touch and keyboard, and the marker and
popup machinery: players draw with their names, markers in their owner's colour,
both with coordinates on click.

Tiles the service says have changed are still replaced one at a time rather than
through `redraw()`, which would discard every tile on screen to update one.

### Also

- A map with nothing in it starts and serves, rather than refusing to. It is the
  state every server is in for a moment after a format change clears it, and the
  service being down then looks like the upgrade failed.
- A tile nobody has built answers `404` rather than `500`. It is missing, not
  broken, and a viewer can draw around one and not the other.
- Responses carry one `Cache-Control` and one only. Two is not a stronger
  instruction but an ambiguous one, and the browser takes the first — so the
  vendored library was being fetched again on every page load while appearing to
  be cached for a year.

## 0.7.2

### The viewer stops asking for tiles that are not there

`draw` walked every tile coordinate in the viewport with no regard for where the
world actually is. Zoomed out that is overwhelmingly space no chunk has ever
occupied, and each one was a request, a render and an image kept forever. It is
now clamped to the world's own bounds.

On the 289-chunk test world the difference is the whole problem: **9 tiles at
every zoom, against 79,125 at the zoom-out limit** and 7.9 million at the floor.

### Zoom-out stops where the budget does

A world too large to fit inside the tile budget whatever the scale now has a
floor, computed from the viewport so that no view asks for more than four hundred
tiles. A world small enough to fit entirely has no floor at all — the clamp
already bounds it, so zooming out simply makes a fixed handful of tiles smaller.

This is what a floor looks like without zoom levels, and it is the same answer
JourneyMap's web map reaches by capping Leaflet at `minZoom: -2`. It goes away
once there are levels to draw from: the coarsest level becomes the floor instead,
and it is far further out.

The heads-up display now shows how many tiles the view is drawing.

### The viewer has tests

`cargo test` now runs `tests/viewer.mjs`, which lifts the tile arithmetic out of
`src/viewer.html` by name and exercises it — small worlds, worlds larger than the
budget, worlds away from the origin and across it, and four window sizes.

It lifts rather than copies. The first version reimplemented the clamp in the test
instead of calling the viewer's, and removing that clamp from `draw` altogether
left every check passing.

## 0.7.1

### Tiles are rendered on more than one thread

Requests were answered one at a time, so several people opening a cold map queued
behind each other and a cold view arrived a tile at a time. `tiny_http` supports a
shared server across threads and always did; nothing needed replacing.

Measured on 576 cold tiles over 12 connections: **696 tiles a second on one
thread, 2,150 on eight** — a little over three times. Sixteen threads was slower
than eight, which is why the automatic choice is capped there.

`threads` in the configuration, or `-t`. Zero decides from the machine, capped at
eight: this usually shares a box with the game server, which has the better claim
on its cores.

### Looking for a new export moved off the request path

Every request used to check the filesystem for a newer export. With one thread
that was merely wasteful; with several it was two threads racing to reload the
same regions and bumping the generation twice for one export, which made every
viewer repaint twice. A single watcher now does it on its own clock, once a
second, and holds its gate across the reload.

### Whole-map renders use every core

`witchlight render` draws a row at a time in parallel — every pixel is decided from
the world and the palette alone, so rows are independent. A 6,144 by 6,144 world
went from 3.79s to 1.54s, and the output is byte for byte what one core produces.

## 0.7.0

The map became a directory of regions, and live data stopped going through a file.

### Terrain is stored per region

One file per 8×8 chunks — 256 blocks, exactly one tile — instead of one file for
the whole map. A chunk that changes now costs the square it sits in rather than
everything anyone has ever explored, and the tile cache drops one entry rather
than all of them.

Each region is a gzip stream, which measured between five and eight times smaller
on real exports: a 1,828-chunk map is 1,570 KiB where the records alone are 11 MB.

### Only what changed is re-read and repainted

`/info.json?since=N` answers with the tiles that have actually changed since
generation `N`, or `all` when the palette moved or the caller is further behind
than the 128 generations kept. The viewer repaints those and leaves the rest.

Tiles are also swapped only once their replacement has decoded, so the map no
longer blinks through the background colour on every export.

### Players and markers arrive over a socket

The mod posts to an **API socket** — by default `/tmp/witchlight-{hash of the
export path}.sock`, which both halves derive for themselves — instead of writing
`live.json` every two seconds. Players are held in memory and **expire after 30
seconds**, so a game server that stops leaves no dots behind. Markers are written
to `markers.json` when they arrive and differ.

The socket takes writes, which is why it is not on the map port. `api_socket` in
the configuration moves it, and accepts a `host:port` for a mod on another machine.

A socket that will not bind is now a warning rather than a failure to start: the
map is the product and live data is a garnish.

### Removed

- Reading `live.json`. It briefly stayed as a fallback and disguised a mod posting
  no markers as a map that merely had none.
- Reading the single-file `columns.msqc`. While Witchlight is alpha a format change
  clears the map rather than upgrading it, and the mod does the clearing.
