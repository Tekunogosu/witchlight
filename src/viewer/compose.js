// The marker form: opening it, filling it in, and putting it away.
//
// One window for four jobs — a new marker, a change to one, a new preset and a
// change to one — because they are the same fields and a second window would be
// a second place for them to drift apart. `mode` is what says which.

/**
 * Making and changing a marker.
 *
 * The game is the only thing that can do either: a waypoint lives on the game
 * server, belongs to a player the game knows, and appears on that player's own
 * map. So nothing here makes anything. The form asks, the service holds the ask
 * until the mod collects it, and the marker arrives on the next poll like any
 * other. What the page can honestly say is "asked for", and then whether it
 * turned up.
 *
 * Three ways in, one form. A right click on the map names the spot before the
 * form opens, which is what somebody who can already see where they mean wants.
 * The flag button opens it with the spot unnamed, to be typed or picked. A right
 * click on a marker opens it on that marker, to be changed.
 */

/** The colours the game offers, in the order its own picker shows them. */
let palette = [];

/** Which colour and picture the form is on. */
let chosenColour = '#ffffff';
let chosenPicture = 'circle';

/** Whether the next click on the map names the spot rather than doing nothing. */
let placing = false;

/** The marker this page asked for and has not seen arrive, and when it asked. */
let awaiting = null;
let askedAt = 0;

/**
 * What the form is for: a new marker, a marker that exists, or a preset.
 *
 * One window rather than three, because every choice on it — a name, a colour, a
 * picture, who sees it — is the same choice in all three cases. What changes is
 * where the answer goes and what the button that sends it is called.
 */
let mode = 'new';

/** The marker being changed, when there is one. */
let editing = null;

/** Which preset is being changed, by its place in the list. */
let editingPreset = -1;

/** What was under the pointer when the form was opened: the block code a preset
 *  is remembered against, and what the game calls it. */
let clicked = null;

/** Who the service says is looking. Kept because the form needs it after load. */
let viewer = null;

/** What this person has set for themselves — their presets, and where their new
 *  markers start. Read at login and again whenever they change it. */
let mine = { Presets: [], PresetsByDefault: false };

/** How long a marker may take to appear before the page stops expecting it.
 *  The mod collects every two seconds, so this is a game server that has gone. */
const MARKER_PATIENCE = 20000;

/**
 * What a marker with no name is called.
 *
 * The game server names an unnamed marker this, and the page has to ask for the
 * same word or it cannot recognise its own marker coming back — an edit is known
 * to have landed by the marker reading as what was asked for. Three programs
 * agree on it; this is the copy the person filling in the form sees.
 */
const UNNAMED = 'Marker';

const composer = document.getElementById('compose');
const composeTitle = document.getElementById('compose-title');
const markerName = document.getElementById('marker-name');
const markerX = document.getElementById('marker-x');
const markerY = document.getElementById('marker-y');
const markerZ = document.getElementById('marker-z');
const markerPrivate = document.getElementById('marker-private');
const markerRemember = document.getElementById('marker-remember');
const rememberLine = document.getElementById('remember-line');
const rememberWhat = document.getElementById('remember-what');
const markerPattern = document.getElementById('marker-pattern');
const markerPick = document.getElementById('marker-pick');
const markerSave = document.getElementById('marker-save');
const saidHere = document.getElementById('said');

/** A position the reader typed, in the numbers the world itself uses. The
 *  inverse of `said`, and it has to stay so or a marker lands a spawn away. */
const meant = (x, z) => settings.absolute.on
  ? [Math.round(x), Math.round(z)]
  : [Math.round(x + spawn.x), Math.round(z + spawn.z)];

/** What the form is saying, and whether it is a complaint. */
function sayHere(what, wrong) {
  saidHere.textContent = what || '';
  saidHere.classList.toggle('wrong', Boolean(wrong));
}

/**
 * Whether a preset's pattern names this block.
 *
 * `*` stands for any run of characters and everything else is itself, which is
 * the whole grammar: a pattern is read by whoever typed it, and one that needed
 * escaping rules would be a pattern nobody could check by eye. Matched here
 * rather than by the service, because this is the side holding both the code
 * under the pointer and the presets to try against it.
 */
function fits(pattern, code) {
  if (!pattern || !code) return false;
  const parts = String(pattern).toLowerCase().split('*');
  const named = String(code).toLowerCase();
  let reached = 0;

  for (let i = 0; i < parts.length; i++) {
    const part = parts[i];
    if (part === '') continue;
    const found = i === 0 ? (named.startsWith(part) ? 0 : -1) : named.indexOf(part, reached);
    if (found < 0) return false;
    reached = found + part.length;
  }
  // A pattern not ending in `*` has to reach the end of the code, or `rock-*`
  // and `rock` would both answer for every rock there is.
  const last = parts[parts.length - 1];
  return last === '' || named.endsWith(last);
}

/** The first preset that names this block, or nothing. */
function presetFor(code) {
  return (mine.Presets || []).find(preset => fits(preset.Pattern, code)) || null;
}

/** Where a new marker starts when nobody has said otherwise: what this person
 *  chose, and failing that what the operator set. */
function privateByDefault() {
  if (mine.PrivateByDefault === true || mine.PrivateByDefault === false) {
    return mine.PrivateByDefault;
  }
  return !(viewer && viewer.MarkersPublic);
}

function buildCompose() {
  for (const panel of [composer, presetPanel, profile]) dragBy(panel);

  markerPick.addEventListener('click', () => setPlacing(!placing));
  markerSave.addEventListener('click', () => started(askForMarker(), 'asking for the marker'));
  document.getElementById('marker-cancel').addEventListener('click', closeCompose);
  markerName.addEventListener('keydown', event => {
    if (event.key === 'Enter') started(askForMarker(), 'asking for the marker');
  });
  // The box decides whether there is a pattern to fill in, so it redraws the
  // form rather than only remembering its own state.
  markerRemember.addEventListener('change', showFields);
  buildBlockSearch();

  // A right click names the spot, and what is on it names the marker. Both come
  // from the one lookup the block picker already uses.
  map.on('contextmenu', event => {
    const block = { x: Math.floor(event.latlng.lng), z: Math.floor(event.latlng.lat) };
    openCompose(block, null);
    // What the form opened with, so that what the lookup does next can tell a
    // field nobody has touched from one somebody is halfway through.
    const opened = markerName.value;
    started(lookUp(block.x, block.z).then(ground => {
      if (!ground || !composer.classList.contains('open') || editing) return;
      // Opened again rather than patched: the block decides which preset applies,
      // and a preset is a colour, a picture and who sees it as well as a name.
      // Anything typed in the meantime is put back over the top of it, because
      // the form is only slower than the person filling it in, not righter.
      const typed = markerName.value === opened ? null : markerName.value;
      if (ground.code) openCompose(block, { code: ground.code, name: ground.name });
      if (typed !== null) markerName.value = typed;
      if (typeof ground.y === 'number' && markerY.value === '') markerY.value = ground.y;
    }), 'naming the block that was right-clicked');
  });

  map.on('click', event => {
    if (placing) started(settle(event.latlng), 'taking the spot that was clicked');
  });

  markerButton.addEventListener('click', () => {
    if (composer.classList.contains('open')) closeCompose();
    else openCompose(null, null);
  });
}

/**
 * Opens the form on a spot, filling in whatever is known about what is there.
 *
 * A spot is in the world's own numbers; the fields show them the way the rest of
 * the page does, so what is typed here and what the corner reads are the same two
 * numbers. `ground` is what the map knows about that block — its code and the
 * name the game gives it — which is what a preset is matched against and what
 * the marker is called when no preset names it.
 */
function openCompose(spot, ground) {
  mode = 'new';
  editing = null;
  editingPreset = -1;
  clicked = ground || null;
  composeTitle.textContent = 'New marker';
  markerSave.textContent = 'Save';

  if (spot) {
    const [x, z] = said(spot.x, spot.z);
    markerX.value = x;
    markerZ.value = z;
    if (spot.y !== undefined && spot.y !== null) markerY.value = spot.y;
  }

  const preset = clicked ? presetFor(clicked.code) : null;
  if (preset) {
    markerName.value = preset.Title || clicked.name || shortCode(clicked.code);
    if (preset.Color) chosenColour = preset.Color;
    if (preset.Icon) chosenPicture = preset.Icon;
    markerPrivate.checked =
      preset.Private === true || preset.Private === false ? preset.Private : privateByDefault();
  } else {
    markerName.value = clicked ? (clicked.name || shortCode(clicked.code) || '') : '';
    markerPrivate.checked = privateByDefault();
  }

  markerRemember.checked = Boolean(mine.PresetsByDefault) && Boolean(clicked && clicked.code);
  // What it would be remembered against, ready to be widened into a pattern.
  markerPattern.value = clicked && clicked.code ? clicked.code : '';
  showFields();
  showCompose();
}

/** Opens the form on a marker that already exists, to change it. */
function editCompose(place) {
  mode = 'marker';
  editing = place;
  editingPreset = -1;
  clicked = null;
  composeTitle.textContent = 'Edit marker';
  markerSave.textContent = 'Save';

  const [x, z] = said(place.X, place.Z);
  markerName.value = place.Title || '';
  markerX.value = x;
  markerY.value = place.Y;
  markerZ.value = z;
  chosenColour = place.Color || '#ffffff';
  chosenPicture = place.Icon || 'circle';
  markerPrivate.checked = Boolean(place.Private);
  markerRemember.checked = false;
  showFields();
  showCompose();

  // A marker somebody else owns is theirs to be seen by whoever they let; taking
  // it private would be taking it off their own map.
  const ours = viewer && place.OwnerUid === viewer.Uid;
  markerPrivate.disabled = !ours;
  document.querySelector('.deed .keep').title =
    ours ? '' : "Only a marker's owner decides who sees it";
}

/**
 * Opens the form on a preset, beside the list it came from.
 *
 * The same window, because a preset is a marker with the place left out: it says
 * what a thing is called, what colour it is, which picture it takes and who sees
 * it, and those are the questions this window already asks. What it does not ask
 * is where — so the coordinates give way to the pattern, which is a preset's own
 * version of the same question.
 */
function editPreset(which) {
  const preset = (mine.Presets || [])[which];
  if (!preset) return;

  mode = 'preset';
  editing = null;
  editingPreset = which;
  clicked = null;
  composeTitle.textContent = 'Edit preset';
  // Said on the button, so it is plain that this changes the preset that was
  // picked rather than adding another one beside it.
  markerSave.textContent = 'Update';

  markerName.value = preset.Title || '';
  markerPattern.value = preset.Pattern || '';
  chosenColour = preset.Color || '#ffffff';
  chosenPicture = preset.Icon || 'circle';
  markerPrivate.checked = preset.Private === true;
  markerPrivate.disabled = false;
  markerRemember.checked = false;
  showFields();
  showCompose();
  besideThePresets();
  drawPresets();
}

/** Opens the form on a preset that does not exist yet. */
function newPreset() {
  mode = 'preset';
  editing = null;
  editingPreset = -1;
  clicked = null;
  composeTitle.textContent = 'New preset';
  markerSave.textContent = 'Create';

  markerName.value = '';
  markerPattern.value = '';
  chosenColour = palette[0] || '#ffffff';
  chosenPicture = 'circle';
  markerPrivate.checked = privateByDefault();
  markerPrivate.disabled = false;
  markerRemember.checked = false;
  showFields();
  showCompose();
  besideThePresets();
  drawPresets();
  markerPattern.focus();
}

/** Puts the form beside the list it was opened from, where there is room. */
function besideThePresets() {
  const list = presetPanel.getBoundingClientRect();
  if (list.width === 0) return;
  settleWindow(composer, list.right + 10, list.top);
}

/** Everything both ways in have in common: draw it, and say whether it can act. */
function showCompose() {
  openWindow(composer);
  showFields();
  drawColours();
  drawPictures();
  frame();

  if (viewer && viewer.Name) {
    sayHere('');
    markerSave.disabled = false;
    markerName.focus();
    markerName.select();
  } else {
    sayHere('Run /witchlight login in the game to make a marker.', true);
    markerSave.disabled = true;
  }


  // A page that loaded before the mod had posted anything has no colours to
  // offer. Asked again here rather than only at start, which is the same rule
  // the marker pictures follow.
  if (palette.length === 0) started(pollColours().then(drawColours), 'reading the colours');
}

/**
 * Which of the form's questions this mode can answer.
 *
 * A preset has no place, and a marker has no pattern — except when it is also
 * being kept as a preset, which is the one case where both are asked at once.
 * The box that decides that is offered whenever a new marker is being made,
 * whether or not a block was clicked: what a preset is remembered against can be
 * typed as readily as it can be pointed at.
 */
function showFields() {
  const preset = mode === 'preset';
  rememberLine.style.display = mode === 'new' ? '' : 'none';
  rememberWhat.textContent = clicked ? (clicked.name || shortCode(clicked.code)) : 'a block';
  document.getElementById('pattern-field').style.display =
    preset || (mode === 'new' && markerRemember.checked) ? '' : 'none';
  document.getElementById('place-field').style.display = preset ? 'none' : '';
}

function forgetCompose() {
  setPlacing(false);
  awaiting = null;
  mode = 'new';
  editing = null;
  editingPreset = -1;
  clicked = null;
  markerPattern.value = '';
  closeFound();
  drawPresets();
  markerPrivate.disabled = false;
  markerName.value = '';
  markerX.value = '';
  markerY.value = '';
  markerZ.value = '';
  sayHere('');
}

function closeCompose() {
  shutWindow(composer);
}

/** Which numbers the coordinate fields are in, said where they are typed. */
function frame() {
  document.getElementById('place-said').textContent =
    settings.absolute.on ? 'Coords — absolute' : 'Coords — relative';
}

/**
 * The reader changed which numbers the page counts in, so what is already typed
 * is rewritten to mean the same place.
 *
 * Leaving them alone would move a half-filled marker by a spawn without touching
 * it. Called after the setting has flipped, so what is in the fields is in the
 * frame that is no longer current.
 */
function reframe() {
  frame();
  const absolute = settings.absolute.on;
  for (const [field, origin] of [[markerX, spawn.x], [markerZ, spawn.z]]) {
    const held = Number(field.value);
    if (field.value.trim() === '' || !Number.isFinite(held)) continue;
    field.value = Math.round(absolute ? held + origin : held - origin);
  }
}
