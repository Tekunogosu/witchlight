// The map itself: the terrain layer, the chunk grid, and the view.
//
// Both layers are Leaflet's own, extended rather than replaced, so panning,
// pinching and the tile lifecycle stay the browser's problem and only what a
// tile *is* belongs to this page.

/**
 * The terrain, at whichever level suits the zoom.
 *
 * Leaflet counts zoom upward as detail grows; the levels are numbered from the
 * finest, so that a world getting bigger adds numbers rather than renaming every
 * tile already written. The two meet here and nowhere else.
 */
const Terrain = L.TileLayer.extend({
  getTileUrl(coords) {
    const level = levelFor(coords.z, this.options.maxNativeZoom);
    return tileUrl(level, coords.x, coords.y);
  },

  /**
   * Puts a tile's new pixels in place without it ever being empty, and without
   * it being faded in over the top of what it replaces.
   *
   * Two things would otherwise show. Assigning `src` on the tile that is on
   * screen blanks it at once and leaves it blank until the bytes arrive, so the
   * bytes are fetched into an image nobody can see and only put in place once
   * they have decoded.
   *
   * The second is Leaflet's. It listens for `load` on every tile it made, and
   * answers one by setting the tile to nothing and fading it back in over
   * `FADE_MS` — right for a tile arriving into an empty square, wrong for one
   * being replaced in place, and it fires again because the listener is still
   * on the element when the new picture lands. So the fade is undone as it is
   * set: Leaflet's handler runs first and this one second, both before the
   * browser has painted either, which is why nothing is ever drawn faded.
   * Backdating `loaded` is what settles it, since that is the only thing
   * Leaflet's own fade reads.
   */
  _swap(held, level, x, z) {
    if (!held || !held.el) return;

    const next = new Image();
    next.onload = () => {
      const tile = held.el;
      if (!tile) return;
      tile.addEventListener('load', () => {
        held.loaded = Date.now() - FADE_MS;
        tile.style.opacity = 1;
      }, { once: true });
      tile.src = next.src;
    };
    next.src = tileUrl(level, x, z);
  },

  /**
   * Refetches only the tiles named, leaving the rest of the map alone.
   *
   * `redraw()` would throw away every tile and ask again for all of them, which
   * for one person building a wall is hundreds of tiles to redraw one. The
   * service says exactly which tiles changed, so only those are replaced.
   */
  refresh(changed) {
    for (const [level, x, z] of changed) {
      const key = tileKey(level, x, z, this.options.maxNativeZoom);
      this._swap(this._tiles && this._tiles[key], level, x, z);
    }
  },

  /**
   * Repaints everything on screen, still without blanking any of it.
   *
   * For the two cases where the service cannot say which tiles changed: a new
   * palette recolours the whole map, and a viewer that has been away longer than
   * the service remembers is told to assume the worst. Leaflet's own `redraw()`
   * answers both by dropping every tile and asking again, which empties the map
   * for as long as the refetch takes — the one moment the map is most obviously
   * working is the one moment it looked broken.
   *
   * Only what is held is swapped, which is what is on screen and its buffer.
   * Anything fetched after this asks for the current generation anyway, because
   * that is what `getTileUrl` writes.
   */
  refreshAll() {
    for (const held of Object.values(this._tiles || {})) {
      if (!held || !held.coords) continue;
      const { x, y, z } = held.coords;
      this._swap(held, levelFor(z, this.options.maxNativeZoom), x, y);
    }
  },
});

/**
 * The address of one tile, versioned by that tile's own last change.
 *
 * A tile's bytes are cached by the browser for as long as its address stands,
 * so the address must change exactly when the picture does. Versioning every
 * address by the map's one generation changed all of them whenever anything
 * anywhere moved — and on a server with forty people exploring that is several
 * times a second, so a tile panned back to a moment later was fetched again
 * unchanged. A tile is versioned instead by the generation it last changed at,
 * as the service reported it, or by `epoch` — the generation this page last
 * drew everything at — where it has not changed since.
 *
 * `changedAt` holds one entry per tile the service has named since the epoch;
 * an entry is a few bytes, and past a ceiling the epoch moves on and the map
 * empties, which costs one refetch of the screen rather than a page that grows.
 */
function tileUrl(level, x, z) {
  const version = Math.max(epoch, changedAt.get(tileName(level, x, z)) || 0);
  // The encoding rides the address only so that a change of it is a change of
  // address; the service reads it from the session, never from here.
  return `/tiles/${level}/${x}/${z}.png?v=${version}&f=${tileFormat()}`;
}

/** The name a tile's last change is filed under. */
function tileName(level, x, z) {
  return `${level}/${x}/${z}`;
}

/** The generation the page last drew everything at; see `tileUrl`. */
let epoch = 0;

/** Which tiles changed since the epoch, and at which generation. */
const changedAt = new Map();

/** Past this many remembered changes the epoch moves on instead. */
const MOST_CHANGES_KEPT = 20000;

/**
 * Takes what the service said moved at generation `reached`: which tiles, or that
 * everything did. Called before the tiles are refetched, since it decides
 * which address they are refetched at.
 */
function noteChanges(reached, tiles) {
  if (!tiles || changedAt.size + tiles.length > MOST_CHANGES_KEPT) {
    epoch = reached;
    changedAt.clear();
    return;
  }
  for (const [level, x, z] of tiles) changedAt.set(tileName(level, x, z), reached);
}

/**
 * How long Leaflet takes to fade a tile in, in milliseconds.
 *
 * Its own number, not a choice made here: a tile's opacity is the share of this
 * that has passed since it loaded, so a tile whose load is this far in the past
 * is one the fade has already finished with. Written down because a swap has to
 * say so, and read from nowhere else.
 */
const FADE_MS = 200;

/** Soft enough to read the terrain through, strong enough to count squares by. */
const GRID_COLOUR = '#ffffff26';

/**
 * Where a tile's chunk lines fall, in pixels across it.
 *
 * The tile begins at world block `start` and covers `blocks` of them, so its
 * lines are the multiples of a chunk edge inside that. A tile draws the line on
 * its own left edge and not the one on its right, which is what keeps the seam
 * between two tiles one line rather than two laid on each other.
 */
function chunkLines(start, blocks, chunk, scale) {
  const lines = [];
  for (let block = Math.ceil(start / chunk) * chunk; block < start + blocks; block += chunk) {
    // Half a pixel, so a one pixel line lands on a pixel rather than across two.
    lines.push(Math.round((block - start) * scale) + 0.5);
  }
  return lines;
}

/**
 * The chunk grid, drawn the way a minimap draws one: faint, square, and only
 * while a chunk is still large enough on screen to be worth outlining.
 *
 * A tile layer rather than an overlay, so it pans and zooms with the terrain by
 * construction and its lines land on the same pixels the tiles do — an overlay
 * would have to be repositioned by hand on every frame and would drift on
 * fractional zoom, which is exactly where a grid being one pixel out shows.
 */
const Grid = L.GridLayer.extend({
  createTile(coords) {
    const canvas = document.createElement('canvas');
    const size = this.getTileSize();
    // Leaflet sizes the element; the canvas is given the device's own pixels so
    // that a one pixel line is one pixel rather than a smear on a dense screen.
    const ratio = window.devicePixelRatio || 1;
    canvas.width = size.x * ratio;
    canvas.height = size.y * ratio;

    const scale = scaleAt(coords.z, NATIVE_ZOOM);
    const context = canvas.getContext('2d');
    context.scale(ratio, ratio);
    context.strokeStyle = GRID_COLOUR;
    context.lineWidth = 1;
    context.beginPath();

    // A tile at this zoom covers this many blocks, so its own left and top edges
    // are these world coordinates.
    const blocks = size.x / scale;

    for (const x of chunkLines(coords.x * blocks, blocks, chunkEdge, scale)) {
      context.moveTo(x, 0);
      context.lineTo(x, size.y);
    }
    for (const z of chunkLines(coords.y * blocks, blocks, chunkEdge, scale)) {
      context.moveTo(0, z);
      context.lineTo(size.x, z);
    }
    context.stroke();
    return canvas;
  },
});

const map = L.map('map', {
  crs: blockCrs(NATIVE_ZOOM),
  attributionControl: false,
  zoomControl: true,
  zoomSnap: 0,
  // Slack enough to move past what has been drawn, and a soft edge rather than
  // a wall at the far end of it.
  maxBoundsViscosity: 0.2,
});

let terrain = null;
let grid = null;

// Between the terrain and everything drawn on top of it. Leaflet puts tiles at
// 200 and overlays at 400, and a grid under the terrain is a grid nobody sees.
map.createPane('grid');
map.getPane('grid').style.zIndex = 250;
map.getPane('grid').style.pointerEvents = 'none';

/**
 * Follows the world as it grows.
 *
 * Called on every export that adds anything, which while somebody is exploring is
 * most of them, so it must not disturb what is on screen. Nothing is torn down and
 * the view is never moved: the layer keeps its tiles, and only the edges of what
 * may be asked for move outward.
 */
function resize() {
  // Nothing exported yet. Every server is here for a moment after a format change
  // clears the map, and a viewer that threw would look like the upgrade failed.
  if (bounds.maxX <= bounds.minX || bounds.maxZ <= bounds.minZ) {
    // A sentence in place of the readout, not written into it: the numbers are
    // six elements now, and a world with nothing in it has no value for any of
    // them rather than a zero for each.
    hud.classList.add('waiting');
    // Two reasons a map has nothing in it, and only one of them is the server's:
    // under a private map a page with nobody signed in is shown nothing until
    // somebody is, and saying "waiting for the server" would send them to the
    // wrong fix.
    waiting.textContent = viewer && viewer.PrivateMap && !(viewer && viewer.Name)
      ? 'sign in to see your map — run /witchlight login in the game'
      : (viewer && viewer.PrivateMap
        ? 'your map fills in as you explore'
        : 'waiting for the server to export');
    return;
  }

  const world = L.latLngBounds(at(bounds.minX, bounds.minZ), at(bounds.maxX, bounds.maxZ));
  const first = terrain === null;

  // A world's own size of slack on every side. What has been drawn used to be
  // pinned against the edges of the screen, which at a distance is the one time
  // you want to push it aside and look at where it sits.
  map.setMaxBounds(world.pad(1));
  // Only how far out may change. One pixel per block stays at NATIVE_ZOOM
  // whatever the world does, so nothing already drawn moves.
  map.setMinZoom(NATIVE_ZOOM - levels);
  map.setMaxZoom(zoomCeiling());

  if (first) {
    terrain = new Terrain('', {
      tileSize: TILE,
      minNativeZoom: NATIVE_ZOOM - levels,
      maxNativeZoom: NATIVE_ZOOM,
      // Without this Leaflet asks for tiles across an infinite plane: `CRS.Simple`
      // has no edges of its own, and most of a zoomed out view is world nobody
      // has ever been to.
      bounds: world,
      // Stated, not inherited. A tile layer defaults to a maximum zoom of 18,
      // and that limit is tested against the map's own zoom before it is clamped
      // to the finest level — so zooming past 18 dropped every tile and left the
      // page black, at exactly the magnification `maxNativeZoom` exists to serve.
      minZoom: 0,
      maxZoom: zoomCeiling(),
      // A tile the service has not built answers 404. Without something to put
      // there the browser draws a broken image, which on a world explored in an
      // awkward shape is a grid of them.
      errorTileUrl: 'data:image/gif;base64,R0lGODlhAQABAIAAAAAAAP///yH5BAEAAAAALAAAAAABAAEAAAIBRAA7',
      noWrap: true,
      keepBuffer: 2,
      updateWhenZooming: false,
      className: 'terrain',
    }).addTo(map);

    // Deliberately unbounded, unlike the terrain: a chunk nobody has been to is
    // still a chunk, and seeing where the explored area stops squarely is half
    // of what a grid is for.
    //
    // Its floor is worked out once, here, and the layer is never rebuilt — so a
    // chunk edge of zero would make the floor infinite and the grid would be gone
    // for as long as the page stayed open. It cannot be zero this far in: the
    // service reports an edge the moment it has a region, and this is past the
    // guard above on bounds nothing has been exported into. Pinned by
    // `a_world_worth_drawing_always_knows_its_chunk_edge` in `columns.rs`.
    grid = new Grid({
      tileSize: TILE,
      minZoom: gridFloor(chunkEdge, NATIVE_ZOOM),
      maxZoom: zoomCeiling(),
      pane: 'grid',
      className: 'grid',
      noWrap: true,
      updateWhenZooming: false,
    });
    settings.grid.apply(settings.grid.on);

    // A link to a particular view wins over fitting the world, so that opening
    // one lands where it says rather than somewhere else for a moment first.
    const asked = readAddress();
    if (asked) map.setView(at(asked.x, asked.z), asked.zoom);
    else map.fitBounds(world);
    return;
  }

  // The layer reads both of these when it decides what to ask for next, so a
  // world that has grown is picked up without touching a tile already drawn.
  terrain.options.bounds = world;
  terrain.options.minNativeZoom = NATIVE_ZOOM - levels;
}

/**
 * Takes the ceiling again, after somebody has moved it.
 *
 * Three things hold it: the map, which decides how far a wheel may go, and each
 * of the two layers, which decide how far they may be stretched. All three are
 * told, because a map allowed past what its layers allow shows nothing at the
 * top of the range — which looks like the map breaking rather than like a limit.
 */
function applyZoomCeiling() {
  const top = zoomCeiling();
  if (terrain) terrain.options.maxZoom = top;
  if (grid) grid.options.maxZoom = top;
  if (terrain) map.setMaxZoom(top);
  // And brings the view back under it. Leaflet lowers the limit without moving a
  // map that is already past it, which leaves the view somewhere it is no longer
  // allowed to be — the tiles stop at the ceiling and the map goes blank at
  // exactly the magnification somebody just turned off.
  if (terrain && map.getZoom() > top) map.setZoom(top, { animate: false });
}

/**
 * Where the map is, in the address bar.
 *
 * `#x,z,px-per-block` — the same three numbers the corner shows, so a link says
 * what it points at and someone can read one without following it. It is also the
 * only way to ask for a particular view from outside the page, which is what makes
 * the zoom levels testable at all.
 */
function readAddress() {
  const found = location.hash.match(/^#(-?\d+),(-?\d+),([\d.]+)$/);
  if (!found) return null;
  const perBlock = Number(found[3]);
  if (!(perBlock > 0)) return null;
  return { x: Number(found[1]), z: Number(found[2]), zoom: zoomFor(perBlock, NATIVE_ZOOM) };
}

function writeAddress() {
  if (!terrain) return;
  const centre = map.getCenter();
  const perBlock = scaleAt(map.getZoom(), NATIVE_ZOOM).toFixed(2);
  // Replace rather than push: panning a map should not fill the back button.
  history.replaceState(null, '', `#${Math.round(centre.lng)},${Math.round(centre.lat)},${perBlock}`);
}
