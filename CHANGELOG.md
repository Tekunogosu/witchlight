# mapstique

The map service. It pairs with the [server mod](../../mapstique-csharp), and the
two **must match on minor version**: minor is the compatibility generation, moved
whenever the file format or the socket protocol changes. Patch is free to differ,
and covers anything that changes only one half.

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

`mapstique render` draws a row at a time in parallel — every pixel is decided from
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

The mod posts to an **API socket** — by default `/tmp/mapstique-{hash of the
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
- Reading the single-file `columns.msqc`. While Mapstique is alpha a format change
  clears the map rather than upgrading it, and the mod does the clearing.
