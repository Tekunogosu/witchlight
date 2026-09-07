// Naming what is under the pointer.
//
// The map's inspector: one request at a time, answered by the same reading the
// renderer made for that pixel, so the page never names a block it did not draw.
// The buttons it is armed from are the corner's, in `corner.js`.

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

/**
 * The tools a plugin has armed, by id, each with the way to put it down.
 *
 * Held here because this is where the map's own two already disarm each other.
 * That was done pairwise and by hand — a rule each new tool had to be written
 * into twice — and a plugin arming a cursor of its own could not be written into
 * it at all: two cursors ended up lit and one click went to whichever wired
 * first.
 */
const pluginTools = new Map();

/** Whether any plugin has a tool armed, which the map's own right click yields to. */
function pluginToolArmed() {
  return pluginTools.size > 0;
}

/**
 * Whether an element belongs to something a plugin drew.
 *
 * A plugin's shapes and marks live in panes of its own, all named for the
 * plugin — so what drew a thing is a question about which pane it is in rather
 * than about the thing itself.
 */
function drawnByAPlugin(target) {
  if (!(target instanceof Element)) return false;
  return Boolean(target.closest('.leaflet-pane[class*="leaflet-plugin-"]'));
}

/** Puts down every plugin tool but the one named, which may be none of them. */
function dropPluginTools(except) {
  for (const [id, drop] of pluginTools) {
    if (id === except) continue;
    try {
      drop();
    } catch (error) {
      console.error(`witchlight: ${id} would not put its tool down`, error);
    }
  }
}

/**
 * Arms a plugin's own click-mode, putting down whatever else was armed.
 *
 * The one place that knows what is armed, so a tool added later is a tool the
 * others already disarm rather than one that has to be written into each of
 * them.
 */
function armPluginTool(id, on, drop) {
  if (on) {
    pluginTools.set(id, drop);
    if (picking) setPicking(false);
    if (placing) setPlacing(false);
    dropPluginTools(id);
  } else {
    pluginTools.delete(id);
  }
  map.getContainer().classList.toggle('picking', on || picking || placing);
}

function setPicking(on) {
  picking = on;
  // Two cursors claiming the same click is one of them being ignored, so arming
  // this disarms the other — which is the rule `setPlacing` already followed in
  // one direction and not the other, leaving both buttons lit and the click
  // going to whichever was asked for first.
  if (on && placing) setPlacing(false);
  // And the same for anything a plugin has armed, which is why they are held in
  // one place rather than disarmed pairwise.
  if (on) dropPluginTools(null);
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
    const answer = await fetch(`/block?x=${x}&z=${z}`, { cache: 'no-store' });
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
 *
 * The height is not in here. It is a number of the same kind as the pointer's own
 * two, so it is read with them in the readout rather than said twice in two
 * places that could disagree.
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
    case 'uncoloured':
      return `${shortCode(told.code)} — no colour yet, drawn as bare ground`;
    default:
      return `${shortCode(told.code)}   ` +
             `${Math.round(told.temperature)}°C   ${Math.round(told.rainfall * 100)}% rain`;
  }
}

/** A block code as a reader wants it: `game:rock-granite` is just rock-granite. */
function shortCode(code) {
  return String(code ?? '').replace(/^game:/, '');
}

/**
 * How high the ground is under the pointer, as the picker was told it.
 *
 * The one number in the readout the page cannot work out for itself: x and z are
 * where the pointer is and arrive with the event, while y is what the column
 * under it stands at, and only the service's reading of that column knows it. So
 * it is said while the picker is on and is a dash the rest of the time, rather
 * than a height left over from wherever the pointer was when the tool was last
 * armed.
 */
function groundY() {
  return about(picked) && told.y !== undefined ? String(told.y) : '—';
}

function say() {
  if (!terrain) return;
  const seen = pointer || map.getCenter();
  const [x, z] = said(seen.lng, seen.lat);
  // Written field by field rather than as one line, so that a pointer moving
  // across the map rewrites three numbers instead of the whole readout.
  readout.x.textContent = x;
  readout.y.textContent = groundY();
  readout.z.textContent = z;
  readout.scale.textContent = scaleAt(map.getZoom(), NATIVE_ZOOM).toFixed(2);
  readout.level.textContent = levelFor(Math.round(map.getZoom()), NATIVE_ZOOM);
  readout.chunks.textContent = chunks;
  // What the server says, not what the map was sent: a server that shares
  // positions with a group only still says how many are on.
  readout.online.textContent = online;
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
  stretchClaim(event.latlng);
  say();
});
map.on('mouseout', () => { pointer = null; forget(); say(); });

// Dragging is choosing to look somewhere else, so it stops following. Zooming is
// not, so it does not — it lands on whoever is being followed instead. Where the
// zoom is aimed is settled before the gesture, in `keeping`; this is what puts the
// map back on them once it has settled, since they walk on between polls and the
// middle of the view is only where they were when it was last centred.
map.on('dragstart', () => {
  if (following !== null) follow(following);
});
map.on('zoomend', keepUp);
map.on('moveend zoomend', writeAddress);
addEventListener('hashchange', () => {
  // What somebody pasted may carry more than a place — a plugin's own state
  // rides here too — and taking it now is what stops the next pan overwriting
  // it with what this page happened to be holding.
  addressExtras = readAddressExtras();
  const asked = readAddress();
  if (asked) map.setView(at(asked.x, asked.z), asked.zoom);
  pluginsRelinked();
});
