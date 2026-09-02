// The viewer's zoom arithmetic, exercised against the scripts themselves.
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
const viewer = join(here, '..', 'src', 'viewer');
const read = name => readFileSync(join(here, '..', 'src', name), 'utf8');

// The scripts as the page runs them: one scope, in the order `viewer.rs` joins
// them. Read from that list rather than from a copy of it, so a file added to
// the page is a file these tests see without anyone remembering to say so.
const order = [...read('viewer.rs').matchAll(/include_str!\("viewer\/(\w+\.js)"\)/g)]
  .map(found => found[1]);
if (order.length === 0) throw new Error('viewer.rs no longer lists the page scripts');
const source = order.map(name => readFileSync(join(viewer, name), 'utf8')).join('\n');

const page = readFileSync(join(viewer, 'page.html'), 'utf8');
const style = readFileSync(join(viewer, 'style.css'), 'utf8');
const pyramid = read('pyramid.rs');
const pending = read('pending.rs');
const preferences = read('preferences.rs');

/** Lifts one function out of the viewer, brace-matched, so nothing is duplicated. */
function lift(name) {
  const at = source.indexOf(`function ${name}(`);
  if (at < 0) throw new Error(`the viewer no longer has a function called ${name}`);
  let depth = 0;
  for (let i = source.indexOf('{', at); i < source.length; i++) {
    if (source[i] === '{') depth++;
    else if (source[i] === '}' && --depth === 0) return source.slice(at, i + 1);
  }
  throw new Error(`${name} is not brace balanced`);
}

/** Lifts one object constant, brace-matched, so the table is the real one. */
function liftObject(name) {
  const at = source.indexOf(`const ${name} = {`);
  if (at < 0) throw new Error(`the viewer no longer has an object called ${name}`);
  let depth = 0;
  for (let i = source.indexOf('{', at); i < source.length; i++) {
    if (source[i] === '{') depth++;
    else if (source[i] === '}' && --depth === 0) return `${source.slice(at, i + 1)};`;
  }
  throw new Error(`${name} is not brace balanced`);
}

/** Lifts one single-statement constant, up to the semicolon that ends it. */
function liftConst(name) {
  const at = source.indexOf(`const ${name} = `);
  if (at < 0) throw new Error(`the viewer no longer has a constant called ${name}`);
  const end = source.indexOf(';', at);
  if (end < 0) throw new Error(`${name} is not terminated`);
  return source.slice(at, end + 1);
}

function constant(text, pattern, what) {
  const match = text.match(pattern);
  if (!match) throw new Error(`${what} is no longer declared where the tests look for it`);
  return Number(match[1]);
}

const TILE = constant(pyramid, /pub const TILE: u32 = (\d+)/, 'TILE in pyramid.rs');
const BEYOND = constant(source, /const ZOOM_IN_BEYOND_NATIVE = (\d+)/, 'ZOOM_IN_BEYOND_NATIVE');
const NATIVE = constant(source, /const NATIVE_ZOOM = (\d+)/, 'NATIVE_ZOOM');
const DEEPER = constant(source, /const ZOOM_IN_DEEPER = (\d+)/, 'ZOOM_IN_DEEPER');

const GRID_MIN = constant(source, /const GRID_MIN_PIXELS = (\d+)/, 'GRID_MIN_PIXELS');

const { scaleAt, levelFor, tileKey, zoomFor, gridFloor, chunkLines, portraitSrc, facingOf } = new Function(`
  const GRID_MIN_PIXELS = ${GRID_MIN};
  ${lift('scaleAt')}
  ${lift('levelFor')}
  ${lift('tileKey')}
  ${lift('zoomFor')}
  ${lift('gridFloor')}
  ${lift('chunkLines')}
  ${lift('portraitSrc')}
  ${lift('facingOf')}
  return { scaleAt, levelFor, tileKey, zoomFor, gridFloor, chunkLines, portraitSrc, facingOf };
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
const WINDOW_WIDE = 266;
const windows = new Function(`
  const windowsAt = new Map();
  let innerWidth = 0, innerHeight = 0;
  const panel = {
    style: {},
    getBoundingClientRect: () => ({ width: ${WINDOW_WIDE}, height: 400, left: 0, top: 0 }),
  };
  // Whatever a window carries with it, lifted rather than stubbed out: the point
  // of clamping a window on screen is that everything hung off it goes too, and
  // a settle that quietly stopped calling this would still pass.
  const composer = { other: true };
  const presetPick = { classList: { contains: () => false } };
  const placePresetPick = () => {};
  ${lift('followTheWindow')}
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
const { fits, widened } = new Function(
  `${lift('fits')} ${lift('widened')} return { fits, widened };`)();

// Walking the block list with the arrows. Both ends wrap, and from nowhere the
// two directions must not both land on the first row — which is what makes the
// up arrow useless on a list of a screenful when the wanted block is at the end.
const { nextRow } = new Function(`${lift('nextRow')} return { nextRow };`)();

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

// Following somebody, and what a wheel turn means while you are. Leaflet zooms
// about the pointer, so a map that is keeping up with a player walks off them a
// notch at a time unless it is told otherwise — and told back afterwards, which
// is the half a player logging out used to skip.
const followed = new Function(`
  // A touch gesture this browser does not have, on purpose: putting back what was
  // there is not the same as putting back \`true\`.
  const map = { options: { scrollWheelZoom: true, touchZoom: false, doubleClickZoom: true } };
  let following = null;
  const showFollowed = () => {};
  const keepUp = () => {};
  ${liftConst('ZOOMS_WITHOUT_CHOOSING')}
  ${liftConst('zoomsAboutThePointer')}
  ${lift('keeping')}
  ${lift('follow')}
  return { zoom: map.options, keeping, follow, whom: () => following };
`)();

// The bars a server shows for a player beyond health and food. They are made as
// they turn up and taken away when they stop, because which ones a player has is
// a fact about that player — and a row of empty bars is a worse answer than none.
const cards = new Function(`
  const made = [];
  const element = (name) => ({
    className: '', style: {}, title: '', kids: [],
    append(...more) { this.kids.push(...more); more.forEach(m => { m.parent = this; }); },
    remove() { if (this.parent) this.parent.kids = this.parent.kids.filter(k => k !== this); },
  });
  const document = { createElement: element };
  // The real noticing and the real filtering, because "a bar switched off is not
  // drawn but is still offered" is the whole of what the settings section rests
  // on — and a stub would agree with itself rather than with the page.
  const barsSeen = new Map();
  let barsHidden = {};
  const drawBarSwitches = () => {};
  ${lift('barWanted')}
  ${lift('noticeBar')}
  ${lift('bar')}
  ${lift('fill')}
  ${lift('fillExtra')}
  const card = { extra: element(), bars: new Map() };
  return {
    put: bars => fillExtra(card, bars),
    names: () => [...card.bars.keys()],
    drawn: () => card.extra.kids.length,
    widthOf: name => card.bars.get(name)?.inner.style.width,
    colourOf: name => card.bars.get(name)?.inner.style.background,
    offered: () => [...barsSeen.entries()].map(([n, g]) => g ? n + '/' + g : n),
    hide: name => { barsHidden[name] = true; },
    show: name => { delete barsHidden[name]; },
  };
`)();

const rest = ms => new Promise(done => setTimeout(done, ms));

let failed = 0;
const check = (name, ok, said) => {
  console.log(`${ok ? '  ok   ' : '  FAIL '}${name}`);
  // What a check found, where naming the offenders is the difference between a
  // failure somebody can act on and one they have to go looking for.
  if (!ok && said) console.log(`       ${said}`);
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
  const match = options.match(new RegExp(`${name}:\\s*([A-Za-z_0-9 +()-]+),`));
  return match && match[1].trim();
};
// Said as the one function that owns the ceiling rather than as the sum, because
// the ceiling moves now: a reader who wants to get closer to the blocks raises
// it, and a layer left on the old sum would drop every tile above the old top.
check('the layer states its own maxZoom rather than inheriting 18',
  stated('maxZoom') === 'zoomCeiling()');
check('the layer states its own minZoom', stated('minZoom') !== null);
check(`which covers the map's ceiling of ${NATIVE + BEYOND}`, NATIVE + BEYOND > 18);
check(`and the deeper ceiling of ${NATIVE + DEEPER}`, NATIVE + DEEPER > NATIVE + BEYOND);

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

console.log('\na player points where the mod says they are looking');
// Zero is north, which is a real answer and therefore not the one to give when
// nobody has answered. A mod older than the field says nothing, and a mark that
// says nothing is the one thing the map can draw that is not a claim.
check('a bearing is read as it arrives', facingOf({ Facing: 90 }) === 90);
check('north is a bearing like any other', facingOf({ Facing: 0 }) === 0);
check('a mod that does not say leaves it unsaid', facingOf({ Name: 'Bo' }) === null);
check('and so does one that says something that is not an angle',
  facingOf({ Facing: 'north' }) === null && facingOf({ Facing: null }) === null);

// The mark is the game's own, and the cone is the half of it that points. Turned
// by the bearing, since the page draws it looking north and north is up.
const turning = lift('turn');
check('the mark is turned by the bearing itself',
  turning.includes('rotate(${facing}deg)'));
check('and a player with no bearing loses the cone rather than pointing north',
  turning.includes("classList.toggle('blind'") && style.includes('.pin.blind .cone'));

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

// The form is not the only thing that asks for a change. A switch in the marker
// list builds one from a marker the page already has, and a field left out of
// that body is a marker that comes back white, or unnamed, or at the origin —
// silently, and only for the edits that do not go through the form.
const rebuilt = source.slice(source.indexOf('const asked = {'),
                             source.indexOf('};', source.indexOf('const asked = {')));
const again = [...rebuilt.matchAll(/^\s*([A-Z]\w*):/gm)].map(found => found[1]).sort();
check(`a marker rebuilt from one the page holds sends the same ${again.length} fields`,
  again.join() === reads.join(), `${again.join(' ')} against ${reads.join(' ')}`);

console.log('\nevery element the scripts reach for is on the page');
// The page is markup in one file and behaviour in another, so an element renamed
// in one and not the other is `null.textContent` on the first line that touches
// it — which on a page that builds itself is a blank map and one line in a
// console nobody has open.
const inMarkup = new Set([...page.matchAll(/id="([\w-]+)"/g)].map(found => found[1]));
const madeInJs = new Set([...source.matchAll(/\.id = '([\w-]+)'/g)].map(found => found[1]));
const asked = [...new Set([...source.matchAll(/getElementById\('([\w-]+)'\)/g)].map(f => f[1]))];

check(`the scripts name ${asked.length} elements`, asked.length > 0);
for (const id of asked) {
  check(`  #${id} exists`, inMarkup.has(id) || madeInJs.has(id));
}

console.log('\nthe page asks for the files that carry the rest of it');
check('the style', /href="\/viewer\.css\?v=/.test(page));
check('the scripts', /src="\/viewer\.js\?v=/.test(page));
check('leaflet, before them', page.indexOf('/leaflet.js') < page.indexOf('/viewer.js'));
check('and the values they open on', /window\.witchlight = \{/.test(page));

console.log('\nnothing shows a window except the class that shows a window');
// `.window` hides with `display: none` and `.window.open` shows with it, so a
// later rule that sets `display` on a window and does not say `.open` outranks
// both — and the window is on the screen whatever class it wears. That was two
// windows nobody could close: the mark removed the class and changed nothing.
{
  const windows = [...page.matchAll(/id="([\w-]+)" class="window"/g)].map(found => found[1]);
  check(`the page has ${windows.length} windows`, windows.length > 0);

  // Comments out first: one sitting above a rule lands in that rule's selector,
  // and a comment mentioning `.open` would then vouch for a rule that does not.
  const rules = style.replace(/\/\*[\s\S]*?\*\//g, ' ');
  const loose = [];
  for (const [, selector, body] of rules.matchAll(/([^{}]+)\{([^}]*)\}/g)) {
    if (!/(^|[;\s])display\s*:/.test(body)) continue;
    for (const one of selector.split(',').map(part => part.trim())) {
      for (const id of windows) {
        // The window itself, rather than something inside it: the id has to be
        // in the last compound of the selector.
        if (!new RegExp(`#${id}(?![\\w-])[^\\s>+~]*$`).test(one)) continue;
        if (one.includes('.open')) continue;
        loose.push(`${one} sets display on #${id}`);
      }
    }
  }
  check('and none of them is shown by a rule that ignores .open',
    loose.length === 0, loose.join('; '));
}

console.log('\nno colour in the stylesheet is defined as itself');
// Six were. A custom property that names itself is invalid at computed-value
// time, so every rule using it silently inherited instead — which cost this page
// its accent, its warning colour and three of its five surfaces, with nothing
// anywhere reporting a thing.
const defined = [...style.matchAll(/(--[\w-]+)\s*:\s*([^;]+);/g)];
const named = new Set(defined.map(found => found[1]));
for (const [, name, value] of defined) {
  check(`  ${name} is a colour and not a reference to itself`, !value.includes(`var(${name})`));
}
const used = new Set([...style.matchAll(/var\((--[\w-]+)/g)].map(found => found[1]));
for (const name of used) {
  check(`  ${name} is defined somewhere`, named.has(name));
}

console.log('\na preset is kept in the words the service reads');
// The same trap as the marker form, one route along: serde fills a field it was
// not sent with that field's default, so a preset the page spells differently is
// kept with no name, no colour and a pattern that matches nothing — and neither
// end says a word about it.
const keptFields = source.slice(source.indexOf('const kept = {'),
                                source.indexOf('};', source.indexOf('const kept = {')));
const keeps = [...keptFields.matchAll(/^\s*([A-Z]\w*):/gm)].map(found => found[1]).sort();

const preset = preferences.slice(preferences.indexOf('pub struct Preset {'),
                                 preferences.indexOf('}', preferences.indexOf('pub struct Preset {')));
const holds = [...preset.matchAll(/^\s*pub (\w+):/gm)]
  .map(found => found[1].split('_').map(part => part[0].toUpperCase() + part.slice(1)).join(''))
  .sort();

check(`the form keeps ${keeps.length} fields and the service holds ${holds.length}`,
  keeps.length === holds.length && keeps.length > 0);
check(`and they are the same words: ${keeps.join(' ')}`, keeps.join() === holds.join());

// What a person has set for themselves, read by the page and written by it.
const person = preferences.slice(preferences.indexOf('pub struct Person {'),
                                 preferences.indexOf('}', preferences.indexOf('pub struct Person {')));
for (const [field, of] of [...person.matchAll(/^\s*pub (\w+):/gm)].map(f => [f[1], 'Person'])) {
  const cased = field.split('_').map(part => part[0].toUpperCase() + part.slice(1)).join('');
  check(`the page reads ${of}.${cased} by that name`, source.includes(`mine.${cased}`));
}

console.log('\nan unnamed marker is asked for by the name it will come back under');
// An edit is known to have landed by the marker reading as what was asked for.
// A blank name asked for one the game renames on arrival, so the form waited out
// its whole patience and then reported a failure that had not happened.
check('the form names it rather than leaving it blank',
  /Title: markerName\.value\.trim\(\) \|\| UNNAMED/.test(source));
check("and the name is the one the game server gives", /const UNNAMED = 'Marker';/.test(source));

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

console.log('\na window is resized by the number a size is set to');
// The corner divides the distance a hand moved by the scale the window is drawn
// at. The table holds a row per size and the number is one field of it, so
// reading the row gives an object: the division yields NaN, `NaNpx` is written
// to the style, and a browser discards that without a word — a resize handle
// that answers to nothing at all, with nothing anywhere saying why. Nothing in
// the arithmetic is wrong, which is why only the answer catches it.
const scaleOf = new Function(`${liftObject('scales')}
${lift('scaleOf')}
return scaleOf;`)();

check('a size in the table reads as a number', Number.isFinite(scaleOf('panel')));
check('and so does every other one',
  Object.keys(new Function(`${liftObject('scales')} return scales;`)())
    .every(part => Number.isFinite(scaleOf(part))));
check('a part nobody has a size for still divides', scaleOf('no-such-part') === 1);

// The sum the grip does, with the real reader: a hand moved fifty pixels across
// a window drawn at its ordinary size asks for fifty more pixels of window.
const WINDOW_FLOOR = 240;
const grownTo = 340 + (150 - 100) / scaleOf('panel');
check('a drag of fifty pixels asks for fifty more', grownTo === 390);
check('so what reaches the style is a length rather than NaN',
  /^\d+px$/.test(`${Math.round(Math.max(grownTo, WINDOW_FLOOR))}px`));
check('and the grip asks through that one reader',
  /scale: scaleOf\('panel'\)/.test(source));

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

console.log('\na preset starts out wide enough to name a whole block family');
// The number in a block code is its variant, so a preset kept against the exact
// code answers for one stage of grass out of eight. What is offered instead is
// the same code with the number widened — and what it produces has to be a
// pattern `fits` reads the wanted way, which is why the two are checked together.
for (const [code, pattern, also] of [
  ['game:tallgrass-3', 'game:tallgrass-*', 'game:tallgrass-7'],
  ['game:leaves-grown7-oak', 'game:leaves-grown*-oak', 'game:leaves-grown1-oak'],
  ['game:water-still-7', 'game:water-still-*', 'game:water-still-1'],
]) {
  check(`${code} widens to ${pattern}`, widened(code) === pattern, widened(code));
  check(`  and names ${code}`, fits(widened(code), code));
  check(`  and ${also} with it`, fits(widened(code), also));
}
// A preset is still allowed to name exactly one thing, and a code with no
// variant in it is one — widening it further would be inventing a wish.
check('a code with no number is its own pattern',
  widened('game:soil-low-sparse') === 'game:soil-low-sparse');
check('  and names only itself',
  fits(widened('game:soil-low-sparse'), 'game:soil-low-sparse')
  && !fits(widened('game:soil-low-sparse'), 'game:soil-low-none'));
// The star is a character in a text field, so somebody may put it anywhere —
// that is the same grammar `fits` already reads, and the default is only where
// it starts.
check('a star moved by hand still matches what it names',
  fits('game:ore-*', COPPER) && fits('*-basalt', COPPER));
check('nothing widens to nothing', widened('') === '' && widened(null) === '');

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

console.log('\nthe arrows walk the blocks a search found');
check('down from nowhere takes the first', nextRow(-1, 1, 5) === 0);
check('and up from nowhere takes the last', nextRow(-1, -1, 5) === 4);
check('down walks forward', nextRow(0, 1, 5) === 1);
check('and off the end comes back to the first', nextRow(4, 1, 5) === 0);
check('up walks back', nextRow(3, -1, 5) === 2);
check('and off the front comes back to the last', nextRow(0, -1, 5) === 4);
// A list of one is every row at once, and an empty one has nowhere to be.
check('one row is where both arrows land', nextRow(-1, 1, 1) === 0 && nextRow(0, 1, 1) === 0);
check('and an empty list is on no row at all',
  nextRow(-1, 1, 0) === -1 && nextRow(2, -1, 0) === -1);

console.log('\nnothing the page starts is left to fail in silence');
// A browser does not wait for a handler, so an async function wired to a click
// or a clock hands back a promise nobody is holding: a throw inside one is a
// rejection in a console nobody has open, while the page carries on as though
// the work had happened. Every one of them goes through `started`, which is the
// one place that says what a failure looks like.
{
  const asyncs = [...source.matchAll(/^async function (\w+)\(/gm)].map(found => found[1]);
  const loose = [];
  for (const name of asyncs) {
    for (const call of source.matchAll(new RegExp(`(.{0,16})\\b${name}\\(`, 'g'))) {
      const before = call[1];
      if (/function $/.test(before)) continue;
      if (/(?:await |started\()$/.test(before)) continue;
      loose.push(`${name} after "${before.trim()}"`);
    }
  }
  check(`${asyncs.length} functions answer later, every call awaited or started`,
    loose.length === 0, loose.join('; '));
  // `setInterval` counts the beat whether or not the last one was answered, so a
  // service slower than the gap is asked again while it is still answering.
  check('and nothing polls on a bare interval', !/setInterval\(/.test(source));
}

console.log('\nthe page is one strict script');
// Without the directive a mistyped name is a new global rather than an error,
// which is the whole class of bug the shadowing checks above cannot see.
check('it opens with the directive', /^(?:\/\/[^\n]*\n|\s)*'use strict';/.test(source));
check('and the whole of it parses under one', (() => {
  try { new Function(source); return true; } catch { return false; }
})());

console.log('\nnothing hides a name the rest of the page uses');
// The check above catches two top-level bindings of one name. This catches the
// other half: a local or a parameter standing in front of one, which is how a
// `const said` in a helper turned the function that words a position into a
// string, and would have thrown the first time anyone called it there.
{
  const top = new Set([...source.matchAll(/^(?:async )?function (\w+)\(/gm)].map(f => f[1]));
  for (const found of source.matchAll(/^(?:const|let) (\w+)/gm)) top.add(found[1]);

  const hiding = new Map();
  const hides = (name, how) => {
    if (top.has(name)) hiding.set(name, `${name} (${how})`);
  };
  for (const found of source.matchAll(/\n[ \t]+(?:const|let|var) (\w+)/g)) hides(found[1], 'a local');
  for (const found of source.matchAll(/for \((?:const|let) (\w+) of/g)) hides(found[1], 'a loop name');
  for (const found of source.matchAll(/catch \((\w+)\)/g)) hides(found[1], 'a caught error');

  const lists = [
    ...[...source.matchAll(/function\s*\w*\s*\(([^)]*)\)/g)].map(f => f[1]),
    ...[...source.matchAll(/\(([^()]*)\)\s*=>/g)].map(f => f[1]),
    ...[...source.matchAll(/(?:^|[^\w.])(\w+)\s*=>/g)].map(f => f[1]),
  ];
  for (const list of lists) {
    for (const part of list.split(',')) {
      const name = part.trim().replace(/[=:][\s\S]*$/, '').replace(/^\.\.\./, '').trim();
      if (/^[A-Za-z_$][\w$]*$/.test(name)) hides(name, 'a parameter');
    }
  }
  check(`${top.size} names at the top level, none hidden underneath`,
    hiding.size === 0, [...hiding.values()].join(', '));
}

// A button in the row a window ends with is dressed by what it carries rather
// than by being a button: a word takes the padding and the border, and a mark
// takes the square the lists draw it in. A button in that row wearing neither is
// the browser's own button in the middle of the page — which is how the profile's
// Cancel and Save came out unstyled the day the rule was named.
console.log('\nevery button in a window\'s last row says what it is');
{
  const dressed = ['word', 'seen', 'keepsake', 'drop'];
  const bare = [];
  for (const row of page.matchAll(/<div class="deed">([\s\S]*?)<\/div>/g)) {
    for (const button of row[1].matchAll(/<button\b([^>]*)>/g)) {
      const wears = /class="([^"]*)"/.exec(button[1]);
      const worn = wears ? wears[1].split(/\s+/) : [];
      if (!worn.some(one => dressed.includes(one))) {
        bare.push(/id="([^"]*)"/.exec(button[1])?.[1] || button[0]);
      }
    }
  }
  check(`${dressed.join(', ')} — and nothing bare`, bare.length === 0, bare.join(', '));
}

console.log('\na zoom lands on whoever is being followed');
{
  const { zoom, keeping, follow, whom } = followed;
  const pointer = { ...zoom };

  follow('sam');
  check('following aims every zoom at the middle of the view, where they are',
    zoom.scrollWheelZoom === 'center'
    && zoom.touchZoom === 'center'
    && zoom.doubleClickZoom === 'center',
    JSON.stringify(zoom));
  check('  and the map is keeping sam', whom() === 'sam');

  follow('sam');
  check('clicking them again lets go, and the pointer has the wheel back',
    zoom.scrollWheelZoom === pointer.scrollWheelZoom
    && zoom.doubleClickZoom === pointer.doubleClickZoom,
    JSON.stringify(zoom));
  check('  including a gesture this browser never had',
    zoom.touchZoom === false, String(zoom.touchZoom));
  check('  and nobody is being kept', whom() === null);

  // The one that was wrong: a followed player logging out cleared the name and
  // left the map zooming about its middle for nobody.
  follow('sam');
  keeping(null);
  check('a followed player logging out gives the wheel back too',
    zoom.scrollWheelZoom === pointer.scrollWheelZoom && whom() === null,
    JSON.stringify(zoom));

  follow('sam');
  follow('robin');
  check('following somebody else keeps aiming at the middle',
    whom() === 'robin' && zoom.scrollWheelZoom === 'center');
}

console.log("\na player's card carries whatever else the server shows for them");
{
  cards.put([{ Name: 'Mana', Value: 42, Max: 120, Colour: '#7c5cff', Group: 'Rustbound Magic' }]);
  check('a bar arrives and is drawn', cards.names().join() === 'Mana' && cards.drawn() === 1);
  check('  filled to its share', cards.widthOf('Mana') === '35.0%', cards.widthOf('Mana'));
  check('  in the colour the server asked for',
    cards.colourOf('Mana') === '#7c5cff', cards.colourOf('Mana'));

  cards.put([
    { Name: 'Mana', Value: 60, Max: 120, Colour: '#7c5cff', Group: 'Rustbound Magic' },
    { Name: 'Magic', Value: 25, Max: 100, Colour: '#d8a24a', Group: 'Rustbound Magic' },
  ]);
  check('a second joins it', cards.names().join() === 'Mana,Magic' && cards.drawn() === 2);
  check('  and the first is written into rather than made again',
    cards.widthOf('Mana') === '50.0%', cards.widthOf('Mana'));

  // Spending the lot is not the same as not having it, and the two used to look
  // the same from here: an empty bar and no bar are different sentences.
  cards.put([{ Name: 'Mana', Value: 0, Max: 120, Colour: '#7c5cff', Group: 'Rustbound Magic' }]);
  check('a bar that stops arriving is taken away',
    cards.names().join() === 'Mana' && cards.drawn() === 1);
  check('  while one at empty stays', cards.widthOf('Mana') === '0.0%');

  cards.put([]);
  check('and a player with none has none', cards.names().length === 0 && cards.drawn() === 0);

  // What the settings section is built from, and what it does when it is used.
  check('every bar seen is offered, under what it came from',
    cards.offered().join() === 'Mana/Rustbound Magic,Magic/Rustbound Magic',
    cards.offered().join());

  cards.hide('Mana');
  cards.put([
    { Name: 'Mana', Value: 60, Max: 120, Colour: '#7c5cff', Group: 'Rustbound Magic' },
    { Name: 'Magic', Value: 25, Max: 100, Colour: '#d8a24a', Group: 'Rustbound Magic' },
  ]);
  check('a bar switched off is not drawn', cards.names().join() === 'Magic', cards.names().join());
  check('  and is still offered, or it could not be switched back on',
    cards.offered().length === 2);

  cards.show('Mana');
  cards.put([{ Name: 'Mana', Value: 60, Max: 120, Colour: '#7c5cff', Group: 'Rustbound Magic' }]);
  check('and switching it back on draws it again', cards.names().join() === 'Mana');
}

console.log('\nwhose land it is, is what colour it is drawn in');
// A hue off the owner's uid, which is the only way the same person's ground can
// be the same colour on two screens that have never spoken to each other. The
// map has no roster to hand colours out of and no moment to hand them out at:
// claims arrive every two seconds from a server that has never heard of this
// browser.
const claims = new Function(`
  let bounds = { minX: 0, minZ: 0, maxX: 0, maxZ: 0 };
  const at = (x, z) => [z, x];
  ${liftConst('CLAIM_UNOWNED')}
  ${lift('claimColour')}
  ${lift('claimOnTheMap')}
  return {
    CLAIM_UNOWNED, claimColour, claimOnTheMap,
    mapped: box => { bounds = box; },
  };
`)();

check('one owner is one colour, every time it is asked',
  claims.claimColour('abc') === claims.claimColour('abc'));
check('and two owners are not the same colour',
  claims.claimColour('abc') !== claims.claimColour('abd'),
  `${claims.claimColour('abc')} and ${claims.claimColour('abd')}`);
// Land nobody owns is the world's own: a trader camp is not a player and must
// not be dealt a player's colour, or the map would say somebody lives there.
check('land nobody owns wears the colour of land nobody owns',
  claims.claimColour('') === claims.CLAIM_UNOWNED
  && claims.claimColour(null) === claims.CLAIM_UNOWNED
  && claims.claimColour(undefined) === claims.CLAIM_UNOWNED);
// A palette with a dark end would deal some owner a boundary they cannot find.
// Fixed saturation and lightness are what stop that, so they are checked rather
// than trusted to stay written down.
check('every owner gets a colour at the same weight',
  ['a', 'bb', 'ccc', 'dddd', 'player-uid-1', 'player-uid-2']
    .every(uid => /^hsl\(\d{1,3}, 72%, 55%\)$/.test(claims.claimColour(uid))),
  claims.claimColour('player-uid-1'));

console.log('\na claim is drawn on ground the map has, and nowhere else');
// The world protects a trader camp the moment it generates one, hundreds of
// blocks past anywhere anybody has walked. Drawn whole, that boundary is a
// rectangle ruled across the black outside the map — which hands whoever is
// looking the location of somewhere they have not found.
claims.mapped({ minX: 0, minZ: 0, maxX: 100, maxZ: 100 });
// `at` is stubbed to [z, x], which is the order Leaflet takes a point in.
const drawn = area => claims.claimOnTheMap(area);
check('a claim inside the map is drawn whole',
  JSON.stringify(drawn({ X1: 10, Z1: 20, X2: 29, Z2: 39 })) === '[[20,10],[40,30]]',
  JSON.stringify(drawn({ X1: 10, Z1: 20, X2: 29, Z2: 39 })));
check('a claim outside it is not drawn at all',
  drawn({ X1: 200, Z1: 200, X2: 210, Z2: 210 }) === null
  && drawn({ X1: -50, Z1: -50, X2: -40, Z2: -40 }) === null);
// Touching is not overlapping: a claim whose far corner is the first block past
// the map covers no ground the map has.
check('and neither is one that only touches its edge',
  drawn({ X1: 100, Z1: 10, X2: 110, Z2: 20 }) === null,
  JSON.stringify(drawn({ X1: 100, Z1: 10, X2: 110, Z2: 20 })));
check('a claim reaching past the edge is drawn as far as the map goes',
  JSON.stringify(drawn({ X1: 90, Z1: 90, X2: 130, Z2: 130 })) === '[[90,90],[100,100]]',
  JSON.stringify(drawn({ X1: 90, Z1: 90, X2: 130, Z2: 130 })));
// A map with nothing exported has no ground at all, and the claims a server
// sends on the first beat must not be drawn over the waiting screen.
claims.mapped({ minX: 0, minZ: 0, maxX: 0, maxZ: 0 });
check('and a map with nothing on it draws no claims',
  drawn({ X1: 0, Z1: 0, X2: 10, Z2: 10 }) === null);

console.log(failed === 0 ? '\nall checks passed' : `\n${failed} FAILED`);
process.exit(failed === 0 ? 0 : 1);
