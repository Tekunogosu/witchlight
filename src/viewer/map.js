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
    return `/tiles/${level}/${coords.x}/${coords.y}.png?v=${generation}`;
  },

  /**
   * Refetches only the tiles named, leaving the rest of the map alone.
   *
   * `redraw()` would throw away every tile and ask again for all of them, which
   * for one person building a wall is hundreds of tiles to redraw one. The
   * service says exactly which tiles changed, so only those are replaced — and
   * each is swapped once its replacement has decoded, so nothing blinks.
   */
  refresh(changed) {
    for (const [level, x, z] of changed) {
      const key = tileKey(level, x, z, this.options.maxNativeZoom);
      const held = this._tiles && this._tiles[key];
      if (!held || !held.el) continue;

      const next = new Image();
      next.onload = () => { held.el.src = next.src; };
      next.src = `/tiles/${level}/${x}/${z}.png?v=${generation}`;
    }
  },
});

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
    hudWhere.textContent = 'waiting for the server to export';
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
  map.setMaxZoom(NATIVE_ZOOM + ZOOM_IN_BEYOND_NATIVE);

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
      maxZoom: NATIVE_ZOOM + ZOOM_IN_BEYOND_NATIVE,
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
      maxZoom: NATIVE_ZOOM + ZOOM_IN_BEYOND_NATIVE,
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
