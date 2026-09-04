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
  // Reached from the map as well as from here — see `setSetting` — so the apply
  // is what tells the button on the map, rather than each way in telling the
  // other. Off to begin with: a map opened to find where somebody is is a map
  // with nothing over it, and the ground people have spoken for is a question
  // asked on purpose.
  claims: {
    label: 'Land claims',
    on: false,
    apply: on => { layer(claimed, on); showClaimsToggle(); },
  },
  grid: { label: 'Chunk grid', on: false, apply: on => layer(grid, on) },
  panel: { label: 'Player list', on: true, apply: on => { who.style.display = on ? '' : 'none'; } },
  // Everything that says a position is rewritten, not only what is on the
  // screen: the corner, the marker form, the list of every marker, and the
  // claims — whose boundaries say the ground they cover and were the one thing
  // left reading in the other set of numbers until somebody opened one.
  absolute: {
    label: 'Absolute coordinates',
    on: false,
    apply: () => { say(); reframe(); drawDirectory(); redrawClaims(); },
  },
  names: { label: 'Player names', on: true, apply: on => {
    document.body.classList.toggle('no-names', !on);
  } },
  markerNames: { label: 'Marker names', on: false, apply: on => {
    document.body.classList.toggle('show-marker-names', on);
  } },
  hover: { label: 'Marker info on hover', on: false, apply: () => {} },
  // Typing a name is the fastest way to reach a preset, and the box is already
  // under the pointer — so the list is offered there as well as behind the
  // button beside it. Somebody who names markers rather than picking them turns
  // it off and the box is only a box again.
  presetSearch: { label: 'Preset name search', on: true, apply: () => {} },
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
 * How large the page draws each part of itself.
 *
 * One table for every size somebody can set, because they are one kind of
 * answer: about the screen in front of them rather than about their account, and
 * kept in this browser for that reason. What follows them between machines is
 * what they set about markers themselves, which lives on the service against
 * their uid.
 *
 * Each entry says what it is called, which custom property carries it, and the
 * range it may be set to. The sliders are built from this rather than written out
 * in the markup, so adding a size is an entry here and nothing else — the three
 * that were written out in both places had to be kept in step by hand.
 *
 * All of them are offered in one place, which is the accessibility panel. Three
 * of them used to be in the profile window instead, on the reasoning that how
 * large the interface is drawn is a preference and how large a marker is drawn is
 * a question about eyes. It is the same question either way, and splitting it
 * meant somebody making the page bigger found half the answer and had to know
 * there was another panel holding the rest.
 *
 * The order is the order they are shown in: what is on the map first, then what
 * is drawn over it. The marker icons in lists sit with the markers rather than
 * with the windows they are in, being the same picture at a second size.
 *
 * The marker pair are multipliers rather than second sets of sizes, so where a
 * mark sits on its block and where its name sits above the mark are answered once
 * and scale with them. A mark is eighteen pixels of silhouette, which is the size
 * the game itself draws and is small for anybody reading a screen at arm's
 * length; its name is eleven, which is smaller still.
 */
const scales = {
  mark: { label: 'Markers', css: '--mark-scale', at: 1, least: 0.6, most: 3 },
  markName: { label: 'Marker names', css: '--mark-name-scale', at: 1, least: 0.6, most: 3 },
  // Its own number rather than the map's: a marker the size of a house on the
  // map is not a marker the size of a house in a list beside it.
  listMark: { label: 'Marker icons in lists', css: '--list-mark-scale', at: 1, least: 0.6, most: 3 },
  playerMark: { label: 'Player icons', css: '--player-scale', at: 1, least: 0.6, most: 3 },
  people: { label: 'Players and clock', css: '--scale-people', at: 1, least: 0.7, most: 1.8 },
  panel: { label: 'Windows', css: '--scale-panel', at: 1, least: 0.7, most: 1.8 },
  tools: { label: 'Map buttons', css: '--scale-tools', at: 1, least: 0.7, most: 1.8 },
};

/**
 * The multiplier one part of the page is currently drawn at.
 *
 * The table above holds a description of each size — its label, the CSS variable
 * it writes, the range a reader may set it to — and `at` is the number in force.
 * Reading the row where the number was meant is a mistake nothing reports: the
 * arithmetic divides by an object, gets NaN, and writes `NaNpx`, which a browser
 * discards without a word. That is exactly what stopped a window being resizable
 * by its corner, so asking for a multiplier is one function with one answer and
 * every caller is a thin call against it.
 */
function scaleOf(part) {
  const scale = scales[part];
  return scale && Number.isFinite(scale.at) ? scale.at : 1;
}

function applyScales() {
  for (const scale of Object.values(scales)) {
    document.documentElement.style.setProperty(scale.css, String(scale.at));
  }
}

/**
 * One size slider, wherever it is shown.
 *
 * Built rather than written out, so the range a size may be set to is stated
 * once — in the table — rather than in the markup as well, where the two used to
 * be able to disagree about what a slider was allowed to reach.
 */
function sliderFor(part) {
  const scale = scales[part];
  const label = document.createElement('label');
  label.className = 'slide';
  label.append(document.createTextNode(scale.label));

  const slider = document.createElement('input');
  slider.id = `scale-${part}`;
  slider.type = 'range';
  slider.min = String(scale.least);
  slider.max = String(scale.most);
  slider.step = '0.05';
  slider.value = String(scale.at);
  // One of a row of identical sliders, so what it sizes is the only thing that
  // tells a reader — or a test — which one this is.
  slider.setAttribute('aria-label', `${scale.label} size`);
  // `change` rather than `input`: a range fires `change` when the hand lets go,
  // which is exactly the moment a window may safely resize under it.
  slider.addEventListener('change', () => takeScale(part, Number(slider.value)));

  label.append(slider);
  return label;
}

/** Every size the page can be set to, in the order the table gives them. */
function sizeSliders() {
  return Object.keys(scales).map(sliderFor);
}

/**
 * Takes a size, once the hand has let go of the slider.
 *
 * Kept as it is set rather than waiting for a Save, because the whole of what a
 * size slider is for is seeing the answer. Where every window sits is worked
 * out again straight after: each may have just changed size, and a window grown
 * against the edge of the screen has to be pulled back to stay reachable — the
 * one holding the slider first among them.
 */
function takeScale(part, size) {
  scales[part].at = size;
  applyScales();
  remember();
  for (const [panel, held] of windowsAt) settleWindow(panel, held.x, held.y);
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
/**
 * The bars this map has seen a server send, and what each is filed under.
 *
 * Learnt from the live data rather than declared, because which of them exist is
 * the server's answer and this page never sees the settings file that decides
 * it. A bar nobody on this server has is a switch nobody needs.
 *
 * Kept by name, since that is what a card's bars are keyed by and what a reader
 * chose about. The group is only a heading.
 */
const barsSeen = new Map();

/** Which bars a reader has switched off, by name. */
let barsHidden = {};

/** Whether a bar is one this reader wants drawn. */
function barWanted(name) {
  return barsHidden[name] !== true;
}

/**
 * Takes note of a bar the server sent, and offers a switch for it.
 *
 * The panel is redrawn only when the set of them changes, which is on the first
 * post that carries a new one and never again — a rebuild on every post would
 * take the switch out from under a pointer twice a second.
 */
function noticeBar(reading) {
  const name = String(reading.Name || '');
  if (!name || barsSeen.has(name)) return;
  barsSeen.set(name, String(reading.Group || ''));
  drawBarSwitches();
}

/**
 * The Bar Display section, rebuilt from what has been seen.
 *
 * Grouped by what the server said the bar belongs to, with everything it could
 * not place under a heading that says so rather than under a guess. The whole
 * section is absent until a bar turns up, because a heading over nothing is a
 * feature this server does not have described as one it does.
 */
function drawBarSwitches() {
  const panel = document.getElementById('bar-display');
  if (!panel) return;

  panel.textContent = '';
  panel.style.display = barsSeen.size === 0 ? 'none' : '';
  if (barsSeen.size === 0) return;

  const heading = document.createElement('h2');
  heading.textContent = 'Bar display';
  panel.append(heading);

  const byGroup = new Map();
  for (const [name, group] of barsSeen) {
    if (!byGroup.has(group)) byGroup.set(group, []);
    byGroup.get(group).push(name);
  }

  // Named groups first and in the order they were met; the unplaced last, since
  // "everything else" is only meaningful after the things it is not.
  const groups = [...byGroup.keys()].filter(group => group !== '');
  if (byGroup.has('')) groups.push('');

  for (const group of groups) {
    const under = document.createElement('p');
    under.className = 'note';
    under.textContent = group || 'Not from a mod this map could name';
    panel.append(under);

    for (const name of byGroup.get(group)) {
      panel.append(switchFor('', {
        label: name,
        on: barWanted(name),
        apply: on => {
          if (on) delete barsHidden[name];
          else barsHidden[name] = true;
          remember();
          drawWho();
        },
      }));
    }
  }
}

/**
 * The checkbox standing for each named setting, so that one turned on somewhere
 * else can say so.
 *
 * Most settings are reached only through their own checkbox, and for those this
 * is a map nobody reads. One is not: the claims are toggled from a button on the
 * map as well as from the panel, and a switch with two ways in and one way of
 * showing its state is a panel that lies about the map beside it.
 *
 * Keyed by the name in `settings`, which is what a caller has. The bars build
 * switches of their own and are not in here, because nothing else toggles one.
 */
const switchBoxes = new Map();

/**
 * Turns a setting on or off, wherever it was asked from.
 *
 * The one owner of what that means: the table is written, the choice is kept, the
 * page is told, and whatever shows the state is brought into line with it. Each
 * of those used to be spelled out at the checkbox, which was fine while a
 * checkbox was the only way to reach one.
 */
function setSetting(name, on) {
  const setting = settings[name];
  if (!setting || setting.on === on) return;
  setting.on = on;
  const box = switchBoxes.get(name);
  if (box) box.checked = on;
  remember();
  setting.apply(on);
}

function switchFor(name, setting) {
  const label = document.createElement('label');
  const box = document.createElement('input');
  box.type = 'checkbox';
  box.checked = setting.on;
  box.addEventListener('change', () => {
    setting.on = box.checked;
    remember();
    setting.apply(box.checked);
  });
  if (name) switchBoxes.set(name, box);
  label.append(box, document.createTextNode(setting.label));
  return label;
}

/** One titled part of the accessibility window. */
function accessSection(title) {
  const box = document.createElement('section');
  const heading = document.createElement('h3');
  heading.textContent = title;
  box.append(heading);
  return box;
}

/** One of a short list of answers, of which one is pressed at a time. */
function choiceButton(kind, name, label, choose) {
  const button = document.createElement('button');
  button.type = 'button';
  button.dataset[kind] = name;
  button.textContent = label;
  button.setAttribute('aria-pressed', 'false');
  button.addEventListener('click', choose);
  return button;
}

function buildSettings() {
  const panel = document.getElementById('settings');
  // Inside a window of its own — see the markup — rather than hung off the
  // button that opens it: what is in here sizes that button.
  const access = document.getElementById('access-panel');

  for (const [name, setting] of Object.entries(settings)) {
    (setting.panel === 'access' ? access : panel).append(switchFor(name, setting));
  }

  // Hung on the button that opens it rather than placed near it. It used to be
  // pinned 92 pixels down the page, which is a guess at where the cog is — one
  // that a scaled toolbar or a second button in the row makes wrong, and which
  // left it floating well below the control it belongs to.
  // What a server shows for a player beyond health and food, which is nothing
  // until one turns up — so the section is built empty and fills itself in.
  const bars = document.createElement('div');
  bars.id = 'bar-display';
  bars.style.display = 'none';
  panel.append(bars);

  cogBar.append(panel);

  // What the map can be asked to do differently for one pair of eyes: the
  // switches above, then the two questions about colour side by side — they
  // are the same length and answered the same way, one choice from a short
  // list — and every size under the pair, since six sliders are a block of
  // their own whatever is above them.
  const colours = accessSection('Map colours');
  for (const [name, filter] of Object.entries(filters)) {
    colours.append(choiceButton('filter', name, filter.label, () => chooseFilter(name)));
  }
  const seeing = accessSection('Colour vision');
  for (const [name, vision] of Object.entries(visions)) {
    seeing.append(choiceButton('vision', name, vision.label, () => chooseVision(name)));
  }
  const pair = document.createElement('div');
  pair.className = 'pair';
  pair.append(colours, seeing);
  access.append(pair);

  // Every size in one place. What is large enough is a judgement about one
  // person's eyes and one person's screen, and it is the same judgement about a
  // marker as about the panel beside it — so the markers come first, being what
  // is on the map, and the interface follows.
  const sized = accessSection('Size');
  sized.classList.add('sizes');
  sized.append(...sizeSliders());
  access.append(sized);

  applyFilter();
  applyVision();

  // The button opens the window, and shuts it if it is already in front: one
  // control that answers "show me" and "enough" both, the way the settings
  // button beside it does for its own panel.
  accessBar.querySelector('a').addEventListener('click', event => {
    event.stopPropagation();
    panel.classList.remove('open');
    if (accessibility.classList.contains('open')) shutWindow(accessibility);
    else openWindow(accessibility);
  });

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
    // What order the marker list is in belongs here for the reason the sizes do:
    // it is one person's answer about one screen, and having to set it again on
    // every reload is what makes a preference not worth having.
    const sizes = {};
    for (const [part, scale] of Object.entries(scales)) sizes[part] = scale.at;
    const state = {
      scales: sizes, filter: filterName, vision: visionName, sorting, bars: barsHidden,
    };
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
    // The three named marker sizes this replaced, so somebody who had chosen one
    // keeps what they chose. Read before the sizes below, so a size set on the
    // slider since wins over the name it grew out of.
    const named = { normal: 1, large: 1.4, largest: 1.9 };
    if (named[state.marks]) scales.mark.at = named[state.marks];

    for (const [part, scale] of Object.entries(scales)) {
      const size = Number(state.scales && state.scales[part]);
      // Bounded on the way in as well as on the slider, and by the same two
      // numbers: what is stored here came from a browser, and a page drawn at a
      // hundredth of size cannot be reached to put right.
      if (Number.isFinite(size) && size >= scale.least && size <= scale.most) scale.at = size;
    }
    // Only the ones switched off are kept, so a bar this server gains tomorrow
    // starts on rather than starting hidden because nobody had heard of it.
    if (state.bars && typeof state.bars === 'object') barsHidden = { ...state.bars };
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
