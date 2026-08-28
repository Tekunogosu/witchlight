// What the viewer is showing, and what it remembers between visits.
//
// One table of switches with an `apply` each, so adding a thing to show is an
// entry rather than a checkbox, a handler and a line in the writer. Sizes live
// beside them: how large the page draws its panels is about the screen in front
// of somebody, which is why it stays in the browser rather than following a uid.

const hudWhere = document.getElementById('where');
const hudWhat = document.getElementById('what');
const who = document.getElementById('who');

/**
 * What the reader has chosen to see.
 *
 * Kept in the browser rather than on the server: these are one person's
 * preferences about one screen, and everybody looking at the same map is
 * entitled to a different answer.
 */
const settings = {
  players: { label: 'Players', on: true, apply: on => layer(people, on) },
  markers: { label: 'Markers', on: true, apply: on => layer(places, on) },
  grid: { label: 'Chunk grid', on: false, apply: on => layer(grid, on) },
  panel: { label: 'Player list', on: true, apply: on => { who.style.display = on ? '' : 'none'; } },
  absolute: { label: 'Absolute coordinates', on: false, apply: () => { say(); reframe(); } },
  names: { label: 'Player names', on: true, apply: on => {
    document.body.classList.toggle('no-names', !on);
  } },
  markerNames: { label: 'Marker names', on: false, apply: on => {
    document.body.classList.toggle('show-marker-names', on);
  } },
  hover: { label: 'Marker info on hover', on: false, apply: () => {} },
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

function buildSettings() {
  const panel = document.getElementById('settings');
  for (const setting of Object.values(settings)) {
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
    panel.append(label);
  }

  cogBar.querySelector('a').addEventListener('click', event => {
    event.stopPropagation();
    panel.classList.toggle('open');
  });
  // Anywhere else closes it, including the map underneath.
  addEventListener('click', () => panel.classList.remove('open'));
  panel.addEventListener('click', event => event.stopPropagation());
}

/** Choices survive a reload; a browser that refuses to remember is not an error. */
function remember() {
  try {
    const state = { scales };
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
