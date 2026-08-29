// Reading a place, and turning a zoom into a level.
//
// Leaflet counts zoom upward as detail grows; the stored levels are numbered
// from the finest downward, because that is the numbering a world can grow
// under without renaming every tile already written. The two meet in the four
// functions here and nowhere else, which is why they are tested on their own.

/** What the server knew when it served the page, so the first paint is already
 *  in the right place. Everything after that comes from `/info.json`. */
const TILE = window.witchlight.tile;
const bounds = { ...window.witchlight.bounds };

/**
 * Where the game counts from.
 *
 * Vintage Story shows every coordinate a player sees relative to world spawn,
 * while the world itself is a million blocks across with spawn near the middle.
 * Showing absolute positions is not wrong, but it agrees with nothing the player
 * can compare it against, which comes to the same thing.
 */
let spawn = { x: 0, z: 0 };

/** A world position as the game would say it. */
const said = (x, z) => settings.absolute.on
  ? [Math.round(x), Math.round(z)]
  : [Math.round(x - spawn.x), Math.round(z - spawn.z)];

/** A position the reader typed, in the numbers the world itself uses.
 *
 *  The inverse of `said`, and beside it: the marker form uses both directions in
 *  one round trip, so one of them drifting from the other puts a marker a spawn
 *  away from where it was typed. They are one translation and they are checked as
 *  a pair, which is a poor thing to have in two files. */
const meant = (x, z) => settings.absolute.on
  ? [Math.round(x), Math.round(z)]
  : [Math.round(x + spawn.x), Math.round(z + spawn.z)];

/**
 * How far past one block per pixel the view may go, as powers of two.
 *
 * Costs no extra tiles either way: Leaflet stretches the finest level rather
 * than asking for one that does not exist, so this is only how far it is allowed
 * to stretch it. Three is eight pixels to the block, four is sixteen — the rungs
 * are powers of two because the zoom is, and there is no rung between them.
 */
const ZOOM_IN_BEYOND_NATIVE = 3;
const ZOOM_IN_DEEPER = 4;

/** Which of the two is in force. */
let zoomBeyond = ZOOM_IN_BEYOND_NATIVE;

/** The furthest in the map may currently be taken. */
function zoomCeiling() {
  return NATIVE_ZOOM + zoomBeyond;
}

/**
 * The Leaflet zoom at which one pixel is one block, fixed once and never moved.
 *
 * It would be tempting to put the finest level at whatever zoom the world happens
 * to need today, but the world grows: a player walking far enough adds a level,
 * and everything would shift underneath. Pinning it means growth only ever
 * changes how far out the view may go, which is one call and no teardown.
 *
 * Sixteen is 65,536 blocks to a pixel at the coarsest — past any world.
 */
const NATIVE_ZOOM = 16;

/** A world block position as Leaflet sees it. Latitude is Z, longitude is X. */
const at = (x, z) => L.latLng(z, x);

/**
 * Pixels per block at a Leaflet zoom, given which zoom is the finest level.
 *
 * One at the finest, halving for every level out. Leaflet counts zoom upward as
 * detail grows and the levels are numbered from the finest downward, so this and
 * `levelFor` are the whole of the translation between the two.
 */
function scaleAt(zoom, native) {
  return Math.pow(2, zoom - native);
}

/** Which stored level a Leaflet zoom asks for. Never past the finest there is. */
function levelFor(zoom, native) {
  return Math.max(0, native - zoom);
}

/** The zoom that draws this many pixels per block. The inverse of `scaleAt`. */
function zoomFor(perBlock, native) {
  return native + Math.log(perBlock) / Math.LN2;
}

/** How Leaflet names a tile it is holding, for replacing one in place. */
function tileKey(level, x, z, native) {
  return `${x}:${z}:${native - level}`;
}

/** How few pixels a chunk may be across before its grid is a wash, not a grid. */
const GRID_MIN_PIXELS = 8;

/**
 * The coarsest zoom at which the chunk grid is still worth drawing.
 *
 * Told to Leaflet as the grid layer's own minimum, so zooming out past it stops
 * the tiles being asked for rather than drawing sixteen empty canvases per
 * screen. A chunk edge of 32 puts the floor two levels above the finest.
 */
function gridFloor(chunk, native) {
  return native + Math.log(GRID_MIN_PIXELS / chunk) / Math.LN2;
}

let generation = 0;
let levels = 0;
let chunks = 0;
/** Blocks along a chunk's edge, which is what the grid is drawn on. */
let chunkEdge = 0;
let players = [];

/**
 * How many are on the server, which is not how many the map may draw.
 *
 * An operator can decide that where somebody is standing is their own group's
 * business. The count is not: it says how busy the server is, which is a fact
 * about the server rather than about anybody on it — and a map that would not say
 * it is a map nobody can tell from a dead one.
 */
let online = 0;

/** Whose cards the Group tab keeps: the uids the service says share a group with
 *  whoever is looking, including their own. */
let grouped = new Set();

/** Marker pictures the service has. A name not in here gets a plain shape. */
let icons = new Set();
