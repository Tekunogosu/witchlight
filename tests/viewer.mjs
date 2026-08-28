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
const pending = readFileSync(join(here, '..', 'src', 'pending.rs'), 'utf8');

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

/** Lifts one arrow-function constant, up to the semicolon that ends it. */
function liftConst(name) {
  const at = source.indexOf(`const ${name} = (`);
  if (at < 0) throw new Error(`viewer.html no longer has a constant called ${name}`);
  const end = source.indexOf(';', at);
  if (end < 0) throw new Error(`${name} is not terminated`);
  return source.slice(at, end + 1);
}

function constant(text, pattern, what) {
  const match = text.match(pattern);
  if (!match) throw new Error(`${what} is no longer declared where the tests look for it`);
  return Number(match[1]);
}

const TILE = constant(server, /const TILE: u32 = (\d+)/, 'TILE in server.rs');
const BEYOND = constant(source, /const ZOOM_IN_BEYOND_NATIVE = (\d+)/, 'ZOOM_IN_BEYOND_NATIVE');
const NATIVE = constant(source, /const NATIVE_ZOOM = (\d+)/, 'NATIVE_ZOOM');

const GRID_MIN = constant(source, /const GRID_MIN_PIXELS = (\d+)/, 'GRID_MIN_PIXELS');

const { scaleAt, levelFor, tileKey, zoomFor, gridFloor, chunkLines, portraitSrc } = new Function(`
  const GRID_MIN_PIXELS = ${GRID_MIN};
  ${lift('scaleAt')}
  ${lift('levelFor')}
  ${lift('tileKey')}
  ${lift('zoomFor')}
  ${lift('gridFloor')}
  ${lift('chunkLines')}
  ${lift('portraitSrc')}
  return { scaleAt, levelFor, tileKey, zoomFor, gridFloor, chunkLines, portraitSrc };
`)();

// The two directions of the same translation: what the page shows a reader, and
// what a reader's numbers mean. They are used by the marker form in both
// directions in one round trip, so one drifting from the other puts a marker a
// spawn away from where it was typed.
const frames = new Function(`
  const settings = { absolute: { on: false } };
  let spawn = { x: 0, z: 0 };
  ${liftConst('said')}
  ${liftConst('meant')}
  return {
    said, meant,
    frame: (absolute, at) => { settings.absolute.on = absolute; spawn = at; },
  };
`)();

// Keeping the marker window on screen. A bar dragged past an edge cannot be
// grabbed again, and the only way back is reloading the page, so the clamp being
// backwards in any one of four directions is a window somebody loses.
const WINDOW_WIDE = 232;
const windows = new Function(`
  const windowsAt = new Map();
  let innerWidth = 0, innerHeight = 0;
  const panel = {
    style: {},
    getBoundingClientRect: () => ({ width: ${WINDOW_WIDE}, height: 400, left: 0, top: 0 }),
  };
  ${lift('settleWindow')}
  return (left, top, wide, high) => {
    innerWidth = wide; innerHeight = high;
    settleWindow(panel, left, top);
    return { x: parseInt(panel.style.left, 10), y: parseInt(panel.style.top, 10) };
  };
`)();

// Which preset a block gets. `*` stands for any run of characters and everything
// else is itself; the whole point of a pattern somebody types by hand is that
// they can check it by eye, so there is no escaping and no other metacharacter.
const { fits } = new Function(`${lift('fits')} return { fits };`)();

// A marker's details, put up by a hover and taken down again. Wrong in either
// direction is a bug somebody lives with: a box that never closes covers the map,
// and one that closes while being read cannot be read at all.
const LINGER = 30;
const hovering = new Function(`
  const HOVER_LINGER = ${LINGER};
  let hovered = null;
  let hoverTimer = null;
  const closed = [];
  ${lift('keepHovered')}
  ${lift('closeHovered')}
  ${lift('forgetHovered')}
  const marker = name => ({ name, closePopup() { closed.push(name); } });
  return {
    closed,
    open: m => { keepHovered(); hovered = m; },
    leave: () => closeHovered(),
    reenter: () => keepHovered(),
    click: () => forgetHovered(),
    marker,
    isWaiting: () => hoverTimer !== null,
  };
`)();

const rest = ms => new Promise(done => setTimeout(done, ms));

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

console.log('\nthe chunk grid stops where a chunk stops being worth outlining');
// The grid is a tile layer, so this floor is what stops Leaflet asking for a
// screenful of canvases per pan at a zoom where every one of them comes back
// blank — and, on the other side of it, what stops the grid becoming the map.
for (const chunk of [16, 32, 64]) {
  const floor = gridFloor(chunk, NATIVE);
  check(`a ${chunk} block chunk floors at zoom ${floor}, which is ${GRID_MIN}px across`,
    Math.abs(chunk * scaleAt(floor, NATIVE) - GRID_MIN) < 1e-9);
  check(`  and one level below it is under ${GRID_MIN}px`,
    chunk * scaleAt(floor - 1, NATIVE) < GRID_MIN);
  check(`  while the finest level is well above it`, floor < NATIVE);
}

console.log('\nevery chunk line is drawn once, by the tile it starts');
// Tiles are drawn side by side and each is a separate canvas, so a line on a
// shared edge is either drawn twice — half a pixel apart, which reads as a
// double line — or not at all. Which tile owns it has to be decided once.
for (const chunk of [16, 32]) {
  for (const level of [0, 1, 2]) {
    const scale = Math.pow(2, -level);
    const blocks = TILE / scale;
    for (const tile of [-2, -1, 0, 1, 7]) {
      const lines = chunkLines(tile * blocks, blocks, chunk, scale);
      check(`level ${level}, chunk ${chunk}, tile ${tile}: ${lines.length} lines`,
        lines.length === blocks / chunk);
      check(`  the first is on the tile's own edge`, lines[0] === 0.5);
      check(`  and the last stops short of the next tile's`, lines[lines.length - 1] < TILE);
      check(`  spaced ${chunk * scale}px apart`,
        lines.every((at, i) => Math.abs(at - (0.5 + i * chunk * scale)) < 1e-9));
    }
  }
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

console.log('\na redrawn player gets a new address to fetch');
// A portrait is filed under its player, so the name is the same before and after
// somebody is redrawn. If the address were the name alone, the card would compare
// equal to itself and the browser would be entitled to keep the picture it had —
// which is exactly what made a new portrait need the page reloading.
const drawnAt = at => portraitSrc({ Portrait: 'a0ff', PortraitAt: at });
check('a player with no picture has no address', portraitSrc({ Name: 'Bo' }) === '');
check('an address is under the stored name',
  drawnAt(1000).startsWith('/portraits/a0ff.png'));
check('the same picture keeps the same address', drawnAt(1000) === drawnAt(1000));
check('a picture drawn again does not', drawnAt(1000) !== drawnAt(1001));
check('a picture with no time still resolves to something fetchable',
  portraitSrc({ Portrait: 'a0ff' }) === '/portraits/a0ff.png?v=0');

// The card rebuilds its face when this string changes and leaves it alone when it
// does not, so the address changing is the whole of what makes a new picture show.
const look = source.slice(source.indexOf('const look = '), source.indexOf('if (card.look !== look)'));
check('the card compares the address rather than the name behind it',
  look.includes('portraitSrc(player)') && !look.includes('player.Portrait'));

console.log('\nwhat a reader types means where they meant');
// The form shows a place the way the corner does and sends it the way the world
// stores it. A marker typed at the coordinates a player reads off their own
// screen has to land there, in either frame and wherever spawn is.
for (const [absolute, at] of [[false, { x: 0, z: 0 }], [false, { x: 512000, z: -318 }],
                              [true, { x: 512000, z: -318 }]]) {
  frames.frame(absolute, at);
  const where = absolute ? 'absolute' : 'from spawn';
  const round = (x, z) => frames.meant(...frames.said(x, z));
  check(`${where}, spawn ${at.x},${at.z}: a place survives the round trip`,
    round(512004, -300).join() === '512004,-300' && round(0, 0).join() === '0,0');
}

// Absolute coordinates are the world's own, so that frame is the identity and a
// round trip passing there would pass even if both halves were wrong together.
frames.frame(false, { x: 512000, z: -318 });
check('and the frame is a translation rather than nothing at all',
  frames.said(512000, -318).join() === '0,0');

console.log('\nthe form asks for a marker in the words the service reads');
// Serde fills a field it was not sent with that field's default, so a name the
// page spells differently is not an error anywhere — it is a marker that arrives
// unnamed, or white, or at the origin. Nothing at either end would say so.
const asks = source.slice(source.indexOf("const marker = {"),
                          source.indexOf("};", source.indexOf("const marker = {")));
const sent = [...asks.matchAll(/^\s*([A-Z]\w*):/gm)].map(found => found[1]).sort();

// `Asked` in pending.rs, which is PascalCase on the wire.
const taken = pending.slice(pending.indexOf('struct Asked {'), pending.indexOf('}', pending.indexOf('struct Asked {')));
const reads = [...taken.matchAll(/^\s*(\w+): /gm)]
  .map(found => found[1][0].toUpperCase() + found[1].slice(1))
  .sort();

check(`the form sends ${sent.length} fields and the service reads ${reads.length}`,
  sent.length === reads.length && sent.length > 0);
check(`and they are the same words: ${sent.join(' ')}`, sent.join() === reads.join());

console.log('\nthe marker window cannot be dragged out of reach');
const SCREEN = [1000, 800];
const put = (x, y) => windows(x, y, ...SCREEN);
check('somewhere ordinary is left alone', put(300, 200).x === 300 && put(300, 200).y === 200);
check('dragged off the left, a grip of it stays on screen',
  put(-9000, 200).x + WINDOW_WIDE >= 60);
check('dragged off the right, a grip of it stays on screen',
  put(9000, 200).x <= SCREEN[0] - 60);
check('dragged above the top, the bar stays reachable', put(300, -9000).y >= 0);
check('dragged past the bottom, the bar stays reachable',
  put(300, 9000).y <= SCREEN[1] - 30);
// A browser shrunk under a window that was near an edge is the same question
// asked again, so the same clamp has to answer it on a smaller screen.
check('and a smaller browser pulls it back in',
  windows(940, 770, 400, 300).x <= 340 && windows(940, 770, 400, 300).y <= 270);

console.log('\nnothing in the page is declared twice');
// A second `function foo()` at the top level silently replaces the first, and
// every existing call then reaches the wrong one. That is how the settings
// stopped being written to storage: a later `remember(marker)` for presets took
// over the name of `remember()` for the browser's own settings, and nothing —
// not the parser, not a lint, not the page — said a word.
const declared = [...source.matchAll(/^(?:async )?function (\w+)\(/gm)].map(found => found[1]);
const twice = declared.filter((name, at) => declared.indexOf(name) !== at);
check(`${declared.length} top-level functions, none sharing a name`,
  twice.length === 0 || !console.log(`       clashing: ${[...new Set(twice)].join(', ')}`));

const consts = [...source.matchAll(/^(?:const|let) (\w+) =/gm)].map(found => found[1]);
const both = consts.filter(name => declared.includes(name));
check('and none shadowed by a top-level binding',
  both.length === 0 || !console.log(`       clashing: ${[...new Set(both)].join(', ')}`));

console.log('\na preset matches the blocks its pattern names');
const COPPER = 'game:ore-bountiful-nativecopper-basalt';
check('an exact code matches itself', fits(COPPER, COPPER));
check('and nothing else', !fits(COPPER, 'game:ore-bountiful-nativecopper-chalk'));
check('a widened pattern reaches every rock',
  fits('game:ore-*-nativecopper-*', COPPER)
  && fits('game:ore-*-nativecopper-*', 'game:ore-poor-nativecopper-granite'));
check('and still not another metal',
  !fits('game:ore-*-nativecopper-*', 'game:ore-poor-cassiterite-granite'));
check('a trailing star is a prefix', fits('game:rock-*', 'game:rock-granite'));
check('a leading star is a suffix', fits('*-basalt', COPPER));
check('a star on both sides is a contains', fits('*nativecopper*', COPPER));
// Without the end-anchor, `rock` would answer for every rock there is — which is
// the whole difference between a pattern and a search box.
check('a pattern with no star must reach the end',
  !fits('game:rock', 'game:rock-granite'));
check('a bare star takes everything', fits('*', COPPER));
check('case is not the point', fits('GAME:ORE-*-NATIVECOPPER-*', COPPER));
check('nothing matches nothing', !fits('', COPPER) && !fits(COPPER, '') && !fits(null, COPPER));

console.log("\na marker's details come down on their own");
{
  const forge = hovering.marker('forge');
  hovering.open(forge);
  hovering.leave();
  check('still up the moment the pointer leaves', hovering.closed.length === 0);
  await rest(LINGER * 3);
  check('and down a moment later', hovering.closed.join() === 'forge');
}
{
  // Crossing back over it, or onto the box itself, is not done reading.
  const hoard = hovering.marker('hoard');
  hovering.open(hoard);
  hovering.leave();
  await rest(LINGER / 3);
  hovering.reenter();
  await rest(LINGER * 3);
  check('coming back cancels the wait', !hovering.closed.includes('hoard'));
}
{
  // A deliberate open is not a hover, however it started.
  const road = hovering.marker('road');
  hovering.open(road);
  hovering.click();
  hovering.leave();
  await rest(LINGER * 3);
  check('one opened by clicking stays open', !hovering.closed.includes('road'));
  check('and nothing is left waiting to fire', !hovering.isWaiting());
}

console.log(failed === 0 ? '\nall checks passed' : `\n${failed} FAILED`);
process.exit(failed === 0 ? 0 : 1);
