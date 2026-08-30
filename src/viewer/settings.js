// What the viewer is showing, and what it remembers between visits.
//
// One table of switches with an `apply` each, so adding a thing to show is an
// entry rather than a checkbox, a handler and a line in the writer. Sizes live
// beside them: how large the page draws its panels is about the screen in front
// of somebody, which is why it stays in the browser rather than following a uid.

const hud = document.getElementById('hud');
const waiting = document.getElementById('waiting');
const hudWhat = document.getElementById('what');
/** The seven readouts, by the name each answers to in `say`. */
const readout = {
  x: document.getElementById('at-x'),
  y: document.getElementById('at-y'),
  z: document.getElementById('at-z'),
  scale: document.getElementById('at-scale'),
  level: document.getElementById('at-level'),
  chunks: document.getElementById('at-chunks'),
  online: document.getElementById('at-online'),
};

/**
 * How the map is coloured, over the top of what the service drew.
 *
 * The service now paints the ground the colours the game would, which is right
 * and is harder to read: real terrain is subtle, and a map is looked at to tell
 * one piece of ground from another. Measured over one world, the older build's
 * ground varied half again as much in brightness as the corrected one does —
 * which is the whole of why it read more easily and none of why it was right.
 *
 * These sit on the tiles as a filter rather than changing what is drawn, so
 * choosing one costs nothing, is nobody else's business, and leaves the markers,
 * the grid and the player dots the colours somebody actually chose.
 *
 * None of them is the older build. That map's advantage was that its ground
 * changed *hue* from region to region — a rust-brown north against a green
 * middle — because it multiplied two tints per pixel, and a filter shifts every
 * pixel the same way. What a filter can do is put the contrast back, which is
 * what these do.
 */
const filters = {
  none: { label: 'Default', css: 'none' },
  vivid: { label: 'Vivid', css: 'saturate(1.35) brightness(0.93) contrast(1.12)' },
  strong: { label: 'Strong', css: 'saturate(1.6) brightness(0.92) contrast(1.25)' },
  muted: { label: 'Muted', css: 'saturate(0.6) brightness(1.06)' },
  grey: { label: 'Greyscale', css: 'grayscale(1) contrast(1.15)' },
};

/**
 * Repainting the map for an eye that cannot separate two of its colours.
 *
 * Not a simulation of colour blindness — that shows somebody what they already
 * see. This is daltonisation: what the deficiency loses is worked out and put
 * back into the channels the eye still has, so that ground and markers which
 * arrive as one colour arrive as two.
 *
 * It covers the whole map rather than only the terrain, unlike the presets
 * above, and that is where it earns itself. Measured against the marker colours
 * the game hands out, every pair a red-green reader loses came back apart —
 * about two and a half times further apart. The terrain gains far less, being
 * mostly olive and tan to begin with, which little of this can confuse.
 */
const visions = {
  none: { label: 'Off', css: 'none' },
  protan: { label: 'Red-green (protan)', css: 'url(#cvd-protan)' },
  deutan: { label: 'Red-green (deutan)', css: 'url(#cvd-deutan)' },
  tritan: { label: 'Blue-yellow (tritan)', css: 'url(#cvd-tritan)' },
};

/** Which of them is on. Kept in the browser: it is one person's eyes. */
let filterName = 'vivid';
let visionName = 'none';

function applyVision() {
  const chosen = visions[visionName] || visions.none;
  document.documentElement.style.setProperty('--cvd-filter', chosen.css);
  for (const button of document.querySelectorAll('#access-panel button[data-vision]')) {
    button.setAttribute('aria-pressed', String(button.dataset.vision === visionName));
  }
}

function chooseVision(name) {
  if (!visions[name]) return;
  visionName = name;
  applyVision();
  remember();
}

function applyFilter() {
  const chosen = filters[filterName] || filters.none;
  document.documentElement.style.setProperty('--terrain-filter', chosen.css);
  for (const button of document.querySelectorAll('#access-panel button[data-filter]')) {
    button.setAttribute('aria-pressed', String(button.dataset.filter === filterName));
  }
}

function chooseFilter(name) {
  if (!filters[name]) return;
  filterName = name;
  applyFilter();
  remember();
}
const who = document.getElementById('who');

/**
 * What the reader has chosen to see.
 *
 * Kept in the browser rather than on the server: these are one person's
 * preferences about one screen, and everybody looking at the same map is
 * entitled to a different answer.
 */
const settings = {
  // Drawn again on the way back, because Leaflet builds every marker a fresh
  // element when its layer returns and the way a player is looking lives on that
  // element. Without this they all point north until the next post, which is a
  // claim about eight people rather than a missing one.
  players: { label: 'Players', on: true, apply: on => { layer(people, on); if (on) drawPlayers(); } },
  markers: { label: 'Markers', on: true, apply: on => layer(places, on) },
  grid: { label: 'Chunk grid', on: false, apply: on => layer(grid, on) },
  panel: { label: 'Player list', on: true, apply: on => { who.style.display = on ? '' : 'none'; } },
  // The corner, the marker form and the list of every marker all say a position,
  // so all three are rewritten rather than only the two that are on the screen.
  absolute: {
    label: 'Absolute coordinates',
    on: false,
    apply: () => { say(); reframe(); drawDirectory(); },
  },
  names: { label: 'Player names', on: true, apply: on => {
    document.body.classList.toggle('no-names', !on);
  } },
  markerNames: { label: 'Marker names', on: false, apply: on => {
    document.body.classList.toggle('show-marker-names', on);
  } },
  hover: { label: 'Marker info on hover', on: false, apply: () => {} },
  // Not about what is on the map but about what a reader can do with it, so it
  // is switched in the other panel. Still an entry here, because what a reader
  // has chosen is one table however many panels show it.
  deepZoom: {
    label: 'Deeper zoom',
    on: false,
    panel: 'access',
    apply: on => {
      zoomBeyond = on ? ZOOM_IN_DEEPER : ZOOM_IN_BEYOND_NATIVE;
      applyZoomCeiling();
    },
  },
};

/**
 * How large the page draws itself, in three parts.
 *
 * Kept beside the other view settings and for the same reason: this is one
 * person's answer about one screen, and the same person on a phone wants a
 * different one. What follows them between machines is what they set about
 * markers, which lives on the service against their uid.
 */
const scales = { people: 1, panel: 1, tools: 1 };

function applyScales() {
  for (const [part, size] of Object.entries(scales)) {
    document.documentElement.style.setProperty(`--scale-${part}`, String(size));
  }
}

function layer(group, on) {
  // A layer the map has not built yet — the grid, before anything is exported —
  // is nothing to show and nothing to hide.
  if (!group) return;
  if (on) {
    if (!map.hasLayer(group)) map.addLayer(group);
  } else if (map.hasLayer(group)) {
    map.removeLayer(group);
  }
}

/** A switch, wherever it is shown. */
function switchFor(setting) {
  const label = document.createElement('label');
  const box = document.createElement('input');
  box.type = 'checkbox';
  box.checked = setting.on;
  box.addEventListener('change', () => {
    setting.on = box.checked;
    remember();
    setting.apply(box.checked);
  });
  label.append(box, document.createTextNode(setting.label));
  return label;
}

function buildSettings() {
  const panel = document.getElementById('settings');
  const access = document.createElement('div');
  access.id = 'access-panel';

  for (const setting of Object.values(settings)) {
    (setting.panel === 'access' ? access : panel).append(switchFor(setting));
  }

  // Hung on the button that opens it rather than placed near it. It used to be
  // pinned 92 pixels down the page, which is a guess at where the cog is — one
  // that a scaled toolbar or a second button in the row makes wrong, and which
  // left it floating well below the control it belongs to.
  cogBar.append(panel);

  // What the map can be asked to do differently for one pair of eyes: the
  // switches above, and then the colours, under a heading of their own so the
  // two read as different kinds of answer to the same question.
  const heading = document.createElement('h2');
  heading.textContent = 'Map colours';
  access.append(heading);
  for (const [name, filter] of Object.entries(filters)) {
    const button = document.createElement('button');
    button.type = 'button';
    button.dataset.filter = name;
    button.textContent = filter.label;
    button.setAttribute('aria-pressed', 'false');
    button.addEventListener('click', () => chooseFilter(name));
    access.append(button);
  }
  const seeing = document.createElement('h2');
  seeing.textContent = 'Colour vision';
  access.append(seeing);
  for (const [name, vision] of Object.entries(visions)) {
    const button = document.createElement('button');
    button.type = 'button';
    button.dataset.vision = name;
    button.textContent = vision.label;
    button.setAttribute('aria-pressed', 'false');
    button.addEventListener('click', () => chooseVision(name));
    access.append(button);
  }

  accessBar.append(access);
  applyFilter();
  applyVision();

  accessBar.querySelector('a').addEventListener('click', event => {
    event.stopPropagation();
    access.classList.toggle('open');
    panel.classList.remove('open');
  });
  addEventListener('click', () => access.classList.remove('open'));
  access.addEventListener('click', event => event.stopPropagation());

  cogBar.querySelector('a').addEventListener('click', event => {
    event.stopPropagation();
    panel.classList.toggle('open');
    access.classList.remove('open');
  });
  // Anywhere else closes it, including the map underneath.
  addEventListener('click', () => panel.classList.remove('open'));
  panel.addEventListener('click', event => event.stopPropagation());
}

/** Choices survive a reload; a browser that refuses to remember is not an error. */
function remember() {
  try {
    // What order the marker list is in belongs here for the reason the sizes do:
    // it is one person's answer about one screen, and having to set it again on
    // every reload is what makes a preference not worth having.
    const state = { scales, filter: filterName, vision: visionName, sorting };
    for (const [key, setting] of Object.entries(settings)) state[key] = setting.on;
    localStorage.setItem('witchlight.settings', JSON.stringify(state));
  } catch (error) {
    /* private windows and cleared storage are allowed to say no */
  }
}

function recall() {
  try {
    const state = JSON.parse(localStorage.getItem('witchlight.settings') || '{}');
    for (const [key, setting] of Object.entries(settings)) {
      if (typeof state[key] === 'boolean') setting.on = state[key];
    }
    for (const part of Object.keys(scales)) {
      const size = Number(state.scales && state.scales[part]);
      // Bounded on the way in as well as on the slider: what is stored here came
      // from a browser and a page drawn at a hundredth of size cannot be reached
      // to put right.
      if (Number.isFinite(size) && size >= 0.7 && size <= 1.8) scales[part] = size;
    }
    if (filters[state.filter]) filterName = state.filter;
    if (visions[state.vision]) visionName = state.vision;
    // Checked against what this build can sort by rather than taken as read: the
    // name came out of a browser, and a column this build does not have would be
    // a list that throws on the first marker it is handed.
    if (state.sorting && sorts[state.sorting.by]) {
      sorting = { by: state.sorting.by, down: state.sorting.down === true };
    }
  } catch (error) {
    /* whatever was stored is not usable, so the defaults stand */
  }
}

/**
 * Blocks rather than degrees, and north stays up.
 *
 * `CRS.Simple` halves the scale for every zoom level down from zero; this shifts
 * that so the scale is one to one at the finest level instead, which is where one
 * pixel is one block. The transformation drops the vertical flip `Simple` applies,
 * because world Z already grows downward in a tile.
 */
function blockCrs(native) {
  return L.extend({}, L.CRS.Simple, {
    transformation: new L.Transformation(1, 0, 1, 0),
    scale: zoom => scaleAt(zoom, native),
    zoom: scale => Math.log(scale) / Math.LN2 + native,
  });
}
