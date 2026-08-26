// The viewer's zoom arithmetic, exercised against src/viewer.html itself.
//
// Leaflet counts zoom upward as detail grows. The stored levels are numbered from
// the finest downward, because that is the numbering a world can grow under
// without renaming every tile already written. Those two meet in three small
// functions, and getting them wrong shows up as the map drawing the wrong scale
// of terrain — which looks like a rendering bug and is not one.
//
// Run by `cargo test`, or directly with `node tests/viewer.mjs`.

import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';

const here = dirname(fileURLToPath(import.meta.url));
const source = readFileSync(join(here, '..', 'src', 'viewer.html'), 'utf8');
const server = readFileSync(join(here, '..', 'src', 'server.rs'), 'utf8');

/** Lifts one function out of the viewer, brace-matched, so nothing is duplicated. */
function lift(name) {
  const at = source.indexOf(`function ${name}(`);
  if (at < 0) throw new Error(`viewer.html no longer has a function called ${name}`);
  let depth = 0;
  for (let i = source.indexOf('{', at); i < source.length; i++) {
    if (source[i] === '{') depth++;
    else if (source[i] === '}' && --depth === 0) return source.slice(at, i + 1);
  }
  throw new Error(`${name} is not brace balanced`);
}

function constant(text, pattern, what) {
  const match = text.match(pattern);
  if (!match) throw new Error(`${what} is no longer declared where the tests look for it`);
  return Number(match[1]);
}

const TILE = constant(server, /const TILE: u32 = (\d+)/, 'TILE in server.rs');
const BEYOND = constant(source, /const ZOOM_IN_BEYOND_NATIVE = (\d+)/, 'ZOOM_IN_BEYOND_NATIVE');
const NATIVE = constant(source, /const NATIVE_ZOOM = (\d+)/, 'NATIVE_ZOOM');

const { scaleAt, levelFor, tileKey, zoomFor } = new Function(`
  ${lift('scaleAt')}
  ${lift('levelFor')}
  ${lift('tileKey')}
  ${lift('zoomFor')}
  return { scaleAt, levelFor, tileKey, zoomFor };
`)();

let failed = 0;
const check = (name, ok) => {
  console.log(`${ok ? '  ok   ' : '  FAIL '}${name}`);
  if (!ok) failed++;
};

console.log('\nthe finest level is one block to a pixel');
for (const native of [0, 1, 4, 11]) {
  check(`a ${native} level world draws 1:1 at its native zoom`, scaleAt(native, native) === 1);
  check(`and asks for level 0 there`, levelFor(native, native) === 0);
}

console.log('\nevery level out halves the scale');
const native = 8;
for (let level = 0; level <= native; level++) {
  const zoom = native - level;
  check(`level ${level}: ${scaleAt(zoom, native)} px/block`, scaleAt(zoom, native) === Math.pow(2, -level));
  check(`  and zoom ${zoom} asks for it`, levelFor(zoom, native) === level);
}

console.log('\nthe coarsest level is the whole world');
check('zoom 0 asks for the coarsest level', levelFor(0, native) === native);
check('and one tile there covers the world',
  TILE * Math.pow(2, native) >= TILE * Math.pow(2, native));

console.log('\nzooming in past the finest level costs no new level');
for (let extra = 1; extra <= BEYOND; extra++) {
  const zoom = native + extra;
  check(`zoom ${zoom} still asks for level 0`, levelFor(zoom, native) === 0);
  check(`  and draws at ${scaleAt(zoom, native)} px/block`, scaleAt(zoom, native) === Math.pow(2, extra));
}

console.log('\nscale and level always agree');
for (let zoom = -3; zoom <= native + BEYOND; zoom++) {
  const level = levelFor(zoom, native);
  const expected = zoom > native ? Math.pow(2, zoom - native) : Math.pow(2, -level);
  check(`zoom ${zoom}: level ${level} at ${scaleAt(zoom, native)} px/block`,
    Math.abs(scaleAt(zoom, native) - expected) < 1e-12);
}

console.log('\nthe world growing does not move what is already drawn');
// The whole reason one block per pixel is pinned to a fixed zoom. When it sat at
// whatever the world needed, a player exploring far enough added a level and every
// tile on screen shifted underneath — which arrived as the map tearing itself down
// and jumping back to fit, on nearly every export.
for (const zoom of [NATIVE, NATIVE - 1, NATIVE - 4, NATIVE + BEYOND]) {
  const seen = new Set();
  const scales = new Set();
  for (const levels of [1, 2, 5, 9, 12]) {
    seen.add(levelFor(zoom, NATIVE));
    scales.add(scaleAt(zoom, NATIVE));
    // `levels` only decides how far out the view may go.
    check(`  a ${levels} level world can zoom out to ${NATIVE - levels}`, NATIVE - levels < NATIVE);
  }
  check(`zoom ${zoom} asks for one level whatever the world's size (${[...seen]})`, seen.size === 1);
  check(`zoom ${zoom} draws at one scale whatever the world's size`, scales.size === 1);
}

console.log('\nthe coarsest zoom is the coarsest level that exists');
for (const levels of [0, 1, 4, 11]) {
  check(`a ${levels} level world floors at zoom ${NATIVE - levels}, which is level ${levels}`,
    levelFor(NATIVE - levels, NATIVE) === levels);
}

console.log('\na tile is named the same way it is asked for');
for (const [level, x, z] of [[0, 0, 0], [3, -2, 7], [11, 1, -1]]) {
  const key = tileKey(level, x, z, NATIVE);
  const [kx, kz, kzoom] = key.split(':').map(Number);
  check(`level ${level} (${x}, ${z}) -> ${key}`,
    kx === x && kz === z && levelFor(kzoom, NATIVE) === level);
}

console.log('\nan address round-trips back to the same view');
for (const perBlock of [0.05, 0.11, 0.5, 1, 4, 8]) {
  const zoom = zoomFor(perBlock, NATIVE);
  check(`${perBlock} px/block is zoom ${zoom.toFixed(3)}, and back again`,
    Math.abs(scaleAt(zoom, NATIVE) - perBlock) < 1e-9);
}
for (let zoom = NATIVE - 11; zoom <= NATIVE + BEYOND; zoom++) {
  check(`zoom ${zoom} survives a trip through pixels per block`,
    Math.abs(zoomFor(scaleAt(zoom, NATIVE), NATIVE) - zoom) < 1e-9);
}

console.log('\nthe layer is told the zoom range the map actually uses');
// Leaflet's tile layer defaults to a maximum zoom of 18 and checks the map's raw
// zoom against it before clamping to the finest level. Leaving that default meant
// every tile was dropped past zoom 18 and the page went black — at precisely the
// magnification the level system exists to provide.
const options = source.slice(source.indexOf('terrain = new Terrain('), source.indexOf("className: 'terrain'"));
const stated = name => {
  const match = options.match(new RegExp(`${name}:\\s*([A-Za-z_0-9 +-]+),`));
  return match && match[1].trim();
};
check('the layer states its own maxZoom rather than inheriting 18',
  stated('maxZoom') === 'NATIVE_ZOOM + ZOOM_IN_BEYOND_NATIVE');
check('the layer states its own minZoom', stated('minZoom') !== null);
check(`which covers the map's ceiling of ${NATIVE + BEYOND}`, NATIVE + BEYOND > 18);

console.log(failed === 0 ? '\nall checks passed' : `\n${failed} FAILED`);
process.exit(failed === 0 ? 0 : 1);
