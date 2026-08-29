// Naming what is under the pointer, and the buttons in the corner.
//
// The map's inspector: one request at a time, answered by the same reading the
// renderer made for that pixel, so the page never names a block it did not draw.

/** Where the pointer is, when it is over the map. */
let pointer = null;


/**
 * The picker: a cursor for reading one block, rather than looking at all of them.
 *
 * At one pixel per block the pointer covers whatever it is over, so the point of
 * the tool is not the fetching but the outline — being told which block the map
 * thinks you mean before it tells you what is on it.
 */
let picking = false;
/** The block the outline is on. */
let picked = null;
/** What the service last said, and about which block. */
let told = null;
/** One request at a time; a pointer crossing a hundred blocks asks about the one
 *  it stopped on rather than all hundred. */
let asking = false;

/**
 * The mark a control wears.
 *
 * A silhouette the service compiled in, filled with whatever colour the button
 * is currently using, so a tool that is armed colours its own mark. Every one of
 * these was a Unicode character: two were colour emoji that ignore `color`
 * outright, and the rest were symbol-font characters each machine drew in
 * whatever face it had — or drew as a box, having none.
 *
 * The mark is for the eye alone. What a screen reader says is the button's
 * label, which is why nothing here carries a name.
 */
function chromeMark(name) {
  const mark = document.createElement('span');
  mark.className = `chrome masked mark-${name}`;
  mark.setAttribute('aria-hidden', 'true');
  return mark;
}

const Picker = L.Control.extend({
  options: { position: 'topleft' },

  onAdd() {
    const box = L.DomUtil.create('div', 'leaflet-bar');
    const button = L.DomUtil.create('a', 'tool', box);
    button.href = '#';
    button.title = 'Inspect a block';
    button.setAttribute('role', 'button');
    // Named, so that a screen reader and a test can both say which control this
    // is among the three identical squares in the corner.
    button.setAttribute('aria-label', 'Inspect a block');
    button.setAttribute('aria-pressed', 'false');
    button.append(chromeMark('scan'));

    // Wired the way Leaflet wires its own bar buttons. Without the first line a
    // double click on the tool zooms the map underneath it and a drag from it
    // pans; without the last, the keyboard is left on the button and the arrow
    // keys stop moving the map.
    L.DomEvent.disableClickPropagation(button);
    L.DomEvent.on(button, 'click', L.DomEvent.stop);
    L.DomEvent.on(button, 'click', () => setPicking(!picking));
    L.DomEvent.on(button, 'click', this._refocusOnMap, this);

    this._button = button;
    return box;
  },

  /** Says out loud which state the tool is in, for the eye and for the reader. */
  show(on) {
    if (!this._button) return;
    this._button.classList.toggle('armed', on);
    this._button.setAttribute('aria-pressed', String(on));
  },
});

const picker = new Picker();
map.addControl(picker);

/**
 * A bar of one button, in the shape Leaflet draws its own, in the corner.
 *
 * Not a Leaflet control: these say who is looking rather than doing anything to
 * the map, and Leaflet has one container per corner — putting them in it would
 * tie where they sit to where the zoom sits. They borrow the class and nothing
 * else.
 */
function cornerAnchor(into, mark, label) {
  const button = L.DomUtil.create('a', 'tool', into);
  button.href = '#';
  button.title = label;
  button.setAttribute('role', 'button');
  // Every one of these is a square with a mark in it, so the name is the only
  // thing telling a reader — or a test — which one it reached.
  button.setAttribute('aria-label', label);
  button.append(chromeMark(mark));
  L.DomEvent.on(button, 'click', L.DomEvent.stop);
  return button;
}

/**
 * A bar of one button, in the shape Leaflet draws its own.
 *
 * A second button may be added to the same bar with `cornerAnchor`, which is how
 * the zoom pair is built: Leaflet rules a line between stacked anchors and
 * rounds only the ends, so two things that belong together read as one control
 * rather than as two that happen to be near each other.
 */
function cornerButton(id, mark, label, into) {
  const box = L.DomUtil.create('div', 'leaflet-bar', into || corner);
  box.id = id;
  cornerAnchor(box, mark, label);
  // Over the map, so a click or a drag on one must not reach it.
  L.DomEvent.disableClickPropagation(box);
  L.DomEvent.disableScrollPropagation(box);
  return box;
}

// Not `chrome`: a browser already has one of those, and a `const` of that
// name throws before a line of this page runs.
const corner = document.getElementById('corner');

// Leaflet draws its bar buttons four pixels larger on a machine it believes has
// a touch screen, and it says so with a class on the container it owns. This
// column borrows the bar and sits outside that container, so the rule never
// reached it: the cog and the account came out 26 square against a zoom and a
// picker at 30, on exactly the machines where Leaflet decides in favour of the
// larger one. Told what the map was told, both sizes stay one size — rather than
// a number written here that would be wrong on whichever machine Leaflet
// disagreed with.
if (map.getContainer().classList.contains('leaflet-touch')) {
  corner.classList.add('leaflet-touch');
}
/** The settings and who you are, side by side. One is about the map and one is
 *  about you; putting them in a column would make one read as the other's. */
const row = L.DomUtil.create('div', '', corner);
row.id = 'row';
const cogBar = cornerButton('cog', 'gear-six', 'Settings', row);
const accountBar = cornerButton('account', 'user', 'Account', row);
/**
 * The one corner button that says a name as well as wearing a mark.
 *
 * The name is its own element rather than the button's text, because the button
 * already has a child: writing a name onto the button would take the mark with
 * it. It says something before `/me.json` has answered, so the button reads as a
 * control rather than as an empty box for the width of one request.
 */
const accountName = document.createElement('span');
accountName.className = 'who';
accountName.textContent = 'Unauthenticated';
accountBar.querySelector('a').append(accountName);
/**
 * What the world's clock says, beside who is looking at it.
 *
 * Two columns of two: the date over the year, the time over the season. The
 * lower line of each is the quieter one — a year and a season are what the date
 * and the time are *in*, and saying all four equally loudly makes a reader work
 * out which is which every time they glance at it.
 */
const whenBar = L.DomUtil.create('div', 'leaflet-bar', row);
whenBar.id = 'when';
/** The same box the account wears, so the two sit level and read as one row of
 *  furniture rather than as a control and a label that happen to be adjacent. */
const whenBox = L.DomUtil.create('div', 'tool clock', whenBar);
const when = {};
for (const [top, under] of [['date', 'year'], ['time', 'season']]) {
  const column = L.DomUtil.create('div', 'when-part', whenBox);
  when[top] = L.DomUtil.create('b', '', column);
  when[under] = L.DomUtil.create('span', '', column);
  when[top].textContent = '—';
  when[under].textContent = '';
}
L.DomEvent.disableClickPropagation(whenBar);

/**
 * Says what the clock says, or takes itself off the map.
 *
 * Shown only where there is something to show: a service running without a game
 * server behind it has no clock at all, and four dashes in the corner is a broken
 * widget rather than an honest absence.
 */
function showWhen(clock) {
  const has = Boolean(clock && (clock.Date || clock.Time));
  whenBar.style.display = has ? '' : 'none';
  if (!has) return;
  when.date.textContent = clock.Date || '—';
  when.year.textContent = clock.Year || '';
  when.time.textContent = clock.Time || '—';
  when.season.textContent = clock.Season || '';
}
showWhen(null);

/**
 * What the map can be asked to do differently, under the settings and above the
 * marker controls.
 *
 * Not gated on being signed in, unlike what is under it: nothing in here changes
 * the map, or anything anybody else sees. It changes what one pair of eyes is
 * shown, which is nobody's business but theirs.
 */
const accessBar = cornerButton('access', 'person-arms-spread', 'Accessibility');

/** Making a marker, and deciding what one starts as: one control, two buttons. */
const mineBar = cornerButton('mine', 'map-pin-simple', 'Add a marker');
const markerButton = mineBar.querySelector('a');
const presetButton = cornerAnchor(mineBar, 'bookmarks-simple', 'Presets');

/**
 * Says who is looking, and offers what only they can act on.
 *
 * The account button is always there and is greyed when nobody has followed a
 * login link: a control that appears on login moves everything under it, and a
 * page whose furniture jumps is a page somebody clicks the wrong thing on.
 *
 * What sits under it is offered to whoever can act on it. Making a marker means
 * owning one, and only somebody the game named can own anything, so the flag is
 * for people who have followed a link and for nobody else.
 */
function showAccount(me) {
  const button = accountBar.querySelector('a');
  const named = me && me.Name;
  accountName.textContent = named || 'Unauthenticated';
  button.classList.toggle('out', !named);
  button.title = named
    ? `Signed in as ${me.Name}`
    : 'Not signed in — run /witchlight login in the game';
  button.setAttribute('aria-label', named ? `Account: ${me.Name}` : 'Not signed in');
  mineBar.classList.toggle('on', Boolean(named));
  drawProfile();
}

/** Who the service says is looking. Asked at load, which is when it changes:
 *  following a login link lands back here as a fresh page. */
async function pollMe() {
  try {
    viewer = await (await fetch('/me.json', { cache: 'no-store' })).json();
  } catch (error) {
    viewer = null;
  }
  showAccount(viewer);
  await pollMine();
  drawProfile();
}

/**
 * The one block the picker is on, outlined so that "under the pointer" is exact.
 *
 * A rectangle in world coordinates rather than a box drawn at the pointer: it is
 * a block that is being named, so it must stay on that block while the map moves
 * and grow with it as the view zooms in.
 */
const outline = L.rectangle([at(0, 0), at(1, 1)], {
  // The one accent this page has, read from the stylesheet that owns it rather
  // than written down a second time here. Leaflet draws the stroke as an SVG
  // attribute and cannot take a `var()`, so it is resolved once at load.
  color: accent(),
  weight: 1,
  fill: false,
  interactive: false,
});

/** What the stylesheet calls the accent, or a stand-in if it cannot be read. */
function accent() {
  const written = getComputedStyle(document.documentElement).getPropertyValue('--accent');
  return written.trim() || '#4dd2ff';
}

function setPicking(on) {
  picking = on;
  // Two cursors claiming the same click is one of them being ignored, so arming
  // this disarms the other — which is the rule `setPlacing` already followed in
  // one direction and not the other, leaving both buttons lit and the click
  // going to whichever was asked for first.
  if (on && placing) setPlacing(false);
  picker.show(on);
  map.getContainer().classList.toggle('picking', on);
  if (!on) forget();
  say();
}

/** Nothing is being pointed at, so nothing is claimed about anything. */
function forget() {
  picked = null;
  told = null;
  layer(outline, false);
}

/** Whether what the service last said is about this block. */
function about(block) {
  return told !== null && block !== null && told.x === block.x && told.z === block.z;
}

/**
 * Moves the outline onto the block under the pointer, and asks about it.
 *
 * Only when the block changes: a pointer sliding across a tile crosses hundreds
 * of them at a coarse zoom, and every one of those is a request nobody reads.
 */
function pick(latlng) {
  if (!picking || !latlng) return;

  const block = { x: Math.floor(latlng.lng), z: Math.floor(latlng.lat) };
  if (picked && picked.x === block.x && picked.z === block.z) return;

  picked = block;
  outline.setBounds([at(block.x, block.z), at(block.x + 1, block.z + 1)]);
  layer(outline, true);
  say();
  started(ask(), 'looking up the block under the pointer');
}

/**
 * Asks the service about whichever block the pointer has come to rest on.
 *
 * One request in flight and the rest coalesced into the latest position, so
 * crossing the map costs one answer rather than a queue of stale ones.
 */
async function ask() {
  if (asking) return;
  asking = true;
  try {
    while (picking && picked && !about(picked)) {
      told = await lookUp(picked.x, picked.z);
      say();
    }
  } finally {
    asking = false;
  }
}

async function lookUp(x, z) {
  try {
    const answer = await fetch(`/block.json?x=${x}&z=${z}`, { cache: 'no-store' });
    if (!answer.ok) throw new Error(answer.status);
    return await answer.json();
  } catch (error) {
    // Still an answer about that block, or the loop above would ask again as
    // fast as the service could refuse. The service may simply be restarting.
    return { x, z, state: 'unreachable' };
  }
}

/**
 * What the picker is on, in words the map can honestly use.
 *
 * A column nobody has exported has nothing on it to name, and says that rather
 * than naming a block that is not there. Everything else is what the renderer
 * read for that same pixel, so the line and the colour under it agree.
 */
function describe() {
  if (!picked) return '';
  if (!about(picked)) return 'looking…';

  switch (told.state) {
    case 'unmapped':
      return 'nothing exported here';
    case 'unreachable':
      return 'the service is not answering';
    case 'unknown':
      return `block ${told.block} — not in the palette`;
    default:
      return `${shortCode(told.code)}   y ${told.y}   ` +
             `${Math.round(told.temperature)}°C   ${Math.round(told.rainfall * 100)}% rain`;
  }
}

/** A block code as a reader wants it: `game:rock-granite` is just rock-granite. */
function shortCode(code) {
  return String(code ?? '').replace(/^game:/, '');
}

function say() {
  if (!terrain) return;
  const seen = pointer || map.getCenter();
  const [x, z] = said(seen.lng, seen.lat);
  // Written field by field rather than as one line, so that a pointer moving
  // across the map rewrites two numbers instead of the whole readout.
  readout.x.textContent = x;
  readout.z.textContent = z;
  readout.scale.textContent = scaleAt(map.getZoom(), NATIVE_ZOOM).toFixed(2);
  readout.level.textContent = levelFor(Math.round(map.getZoom()), NATIVE_ZOOM);
  readout.chunks.textContent = chunks;
  readout.online.textContent = players.length;
  // A world whose mod is older than this says neither, and a dash is the honest
  // answer to a question nobody has answered.
  hud.classList.remove('waiting');
  hudWhat.textContent = picking ? describe() : '';
}

map.on('move zoom', say);
// Under the pointer when there is one, the middle of the view when there is not.
map.on('mousemove', event => {
  pointer = event.latlng;
  pick(event.latlng);
  hover(event.latlng);
  say();
});
map.on('mouseout', () => { pointer = null; forget(); say(); });

// Dragging is choosing to look somewhere else, so it stops following. Zooming is
// not, so it does not.
map.on('dragstart', () => {
  if (following !== null) follow(following);
});
map.on('moveend zoomend', writeAddress);
addEventListener('hashchange', () => {
  const asked = readAddress();
  if (asked) map.setView(at(asked.x, asked.z), asked.zoom);
});
