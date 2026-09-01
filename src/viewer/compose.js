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

/**
 * Who will see it, and whether it is being remembered as well as made.
 *
 * The form's own answers rather than the state of two boxes read back out of the
 * page: both are drawn as the mark that says them, and a mark has no `checked` to
 * be asked. They sit beside the colour and the picture because they are the same
 * kind of thing — a choice the form is holding until it is sent.
 */
let privately = false;
let alsoPreset = false;

/** Whether who sees this is this person's to decide. A marker somebody else owns
 *  is theirs to be seen by whoever they let. */
let mayKeep = true;

/** Whether the next click on the map names the spot rather than doing nothing. */
let placing = false;

/** The marker this page asked for and has not seen arrive, and when it asked. */
let awaiting = null;
let askedAt = 0;

/**
 * What was handed to the game and not yet seen back.
 *
 * Kept rather than read off the page, because the form is closed the moment the
 * game takes the ask and closing it empties every field. This is what a failure
 * is put back up from.
 */
let handedOver = null;

/** Whether what is being waited on is a marker going rather than one coming.
 *  A removal has arrived when the marker stops arriving, which is the same
 *  watch read the other way round. */
let removing = false;

/**
 * Whether the bin has been pressed once and not yet confirmed.
 *
 * Two presses rather than one, the way the bulk buttons in the marker list are.
 * A marker is somebody's note to themselves about a place they walked to, and
 * there is no way back from deleting one — so the second press is on the same
 * mark, where the first one was.
 */
let dropArmed = false;

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
const markerPattern = document.getElementById('marker-pattern');
const markerPick = document.getElementById('marker-pick');
const markerDrop = document.getElementById('marker-drop');
const markerSave = document.getElementById('marker-save');
const saidHere = document.getElementById('said');

/** What the form is saying, and whether it is a complaint. */
function sayHere(what, wrong) {
  saidHere.textContent = what || '';
  saidHere.classList.toggle('wrong', Boolean(wrong));
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
  markerPick.addEventListener('click', () => setPlacing(!placing));
  markerPrivate.addEventListener('click', () => {
    if (!mayKeep) return;
    disarmDrop();
    privately = !privately;
    showSeen();
  });
  // The mark decides whether there is a pattern to fill in, so it redraws the
  // form rather than only remembering its own state.
  markerRemember.addEventListener('click', () => {
    disarmDrop();
    alsoPreset = !alsoPreset;
    showFields();
  });
  markerDrop.addEventListener('click', armOrDelete);
  markerSave.addEventListener('click', () => {
    disarmDrop();
    started(askForMarker(), 'asking for the marker');
  });
  document.getElementById('marker-cancel').addEventListener('click', closeCompose);
  markerName.addEventListener('keydown', event => {
    if (event.key === 'Enter') started(askForMarker(), 'asking for the marker');
  });
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
    privately =
      preset.Private === true || preset.Private === false ? preset.Private : privateByDefault();
  } else {
    markerName.value = clicked ? (clicked.name || shortCode(clicked.code) || '') : '';
    privately = privateByDefault();
  }

  mayKeep = true;
  alsoPreset = Boolean(mine.PresetsByDefault) && Boolean(clicked && clicked.code);
  // What it would be remembered against, with the block's variant number already
  // widened into a wildcard — which is what makes one preset answer for a whole
  // family of blocks rather than for the one that happened to be underfoot.
  markerPattern.value = widened(clicked && clicked.code);
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
  privately = Boolean(place.Private);
  alsoPreset = false;
  // A marker somebody else owns is theirs to be seen by whoever they let; taking
  // it private would be taking it off their own map. Set before the form is drawn,
  // because it is what the mark that says so is drawn from.
  mayKeep = Boolean(viewer && place.OwnerUid === viewer.Uid);
  showFields();
  showCompose();
}

/**
 * Opens the form on a preset that exists, beside the list it came from.
 *
 * The same window, because a preset is a marker with the place left out: it says
 * what a thing is called, what colour it is, which picture it takes and who sees
 * it, and those are the questions this window already asks. What it does not ask
 * is where — so the coordinates give way to the pattern, which is a preset's own
 * version of the same question.
 */
function editPreset(which) {
  const preset = (mine.Presets || [])[which];
  if (preset) openPreset(preset, which);
}

/**
 * Opens the form on a preset that does not exist yet.
 *
 * An empty preset carrying the answers a new one starts with, rather than a
 * second copy of the form-filling below it: what "new" means here is one row that
 * is not there yet, and everything else about the two is the same.
 */
function newPreset() {
  openPreset({ Color: palette[0], Private: privateByDefault() }, -1);
  markerPattern.focus();
}

/**
 * Fills the form in from a preset, and says which row it will be written to.
 *
 * `which` is that row, or -1 for one that does not exist yet — which is the whole
 * of the difference between making a preset and changing one, and is why they are
 * not two functions setting the same eleven fields in the same order.
 */
function openPreset(preset, which) {
  const making = which < 0;
  mode = 'preset';
  editing = null;
  editingPreset = which;
  clicked = null;
  composeTitle.textContent = making ? 'New preset' : 'Edit preset';
  // Said on the button, so it is plain whether this changes the preset that was
  // picked or adds another one beside it.
  markerSave.textContent = making ? 'Create' : 'Update';

  markerName.value = preset.Title || '';
  markerPattern.value = preset.Pattern || '';
  chosenColour = preset.Color || '#ffffff';
  chosenPicture = preset.Icon || 'circle';
  privately = preset.Private === true;
  mayKeep = true;
  alsoPreset = false;
  showFields();
  showCompose();
  besideWindow(presetPanel);
  drawPresets();
}

/** How far the form sits from the list it was opened beside. */
const BESIDE_GAP = 10;

/**
 * Puts the form beside the window it was opened from.
 *
 * A row is picked in one window and answered in another, and a form that opens
 * over the list it was picked from hides the rest of it — so the two sit side by
 * side, and picking a second row does not mean moving a window first. A window
 * that is not on the screen has no side to be beside.
 *
 * Which side depends on where the room is. A list dragged to the right edge has
 * none on its right, and the clamp that keeps a window reachable would leave the
 * form hanging off the screen with a finger's width of it showing — so it goes
 * to the left instead, and only stays on the right when neither side fits.
 */
function besideWindow(panel) {
  const box = panel.getBoundingClientRect();
  if (box.width === 0) return;
  const wide = composer.getBoundingClientRect().width;
  const right = box.right + BESIDE_GAP;
  const left = box.left - BESIDE_GAP - wide;
  const roomRight = right + wide <= innerWidth;
  settleWindow(composer, roomRight || left < 0 ? right : left, box.top);
  // Opened on purpose and put where it was asked for, so it belongs in front of
  // whatever it was opened from.
  raiseWindow(composer);
}

/**
 * The bin, pressed.
 *
 * The first press arms it and says what a second would do; the second does it.
 * Nothing else on the form arms anything, so a mark that is armed is a mark
 * somebody deliberately pressed a moment ago.
 */
function armOrDelete() {
  // Nothing to take away is nothing to arm. The mark is off the screen then, so
  // a press can only have come from something other than a hand — and an armed
  // mark nobody can see is a press stored up against the next window.
  if (!droppable()) return;

  if (!dropArmed) {
    dropArmed = true;
    showDrop();
    sayHere(`Press the bin again to delete ${namedHere()}.`);
    return;
  }
  dropArmed = false;
  showDrop();
  started(askToDelete(), 'deleting what the form is open on');
}

/** Puts the bin back to asking rather than confirming. */
function disarmDrop() {
  if (!dropArmed) return;
  dropArmed = false;
  showDrop();
}

/** What the form is open on, in the words somebody would use for it. */
function namedHere() {
  const called = markerName.value.trim();
  if (mode === 'preset') return called ? `the preset ${called}` : 'this preset';
  return called || 'this marker';
}

/**
 * Whether the form is open on something this person could take away.
 *
 * A marker being made does not exist yet, and a marker somebody else owns is
 * theirs — the operator's setting lets other people *correct* a public marker,
 * which is not the same permission as taking it off its owner's map. A preset is
 * this person's own record and is theirs to drop whenever it exists.
 *
 * One owner for the question, because the mark being on the screen and the mark
 * doing anything when pressed must be the same answer.
 */
function droppable() {
  if (mode === 'marker') return Boolean(editing && viewer && editing.OwnerUid === viewer.Uid);
  if (mode === 'preset') return editingPreset >= 0;
  return false;
}

/** The mark itself: whether it is offered, and whether it has been asked once. */
function showDrop() {
  markerDrop.style.display = droppable() ? '' : 'none';
  markerDrop.classList.toggle('armed', dropArmed);
  const words = dropArmed
    ? `Press again to delete ${namedHere()}`
    : `Delete ${namedHere()}`;
  markerDrop.title = words;
  markerDrop.setAttribute('aria-label', words);
  markerDrop.setAttribute('aria-pressed', String(dropArmed));
}

/** Everything both ways in have in common: draw it, and say whether it can act. */
function showCompose() {
  // A confirmation belongs to the window it was given in, and this is a
  // different one however it was opened.
  dropArmed = false;
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
  // A preset is already the thing the mark would make, so it is the one mode
  // that is not offered it. A marker being changed is offered it like a new one:
  // deciding a block should start this way is a thing somebody works out from a
  // marker they already have as readily as from one they are making.
  markerRemember.style.display = preset ? 'none' : '';
  document.getElementById('pattern-field').style.display =
    preset || alsoPreset ? '' : 'none';
  document.getElementById('place-field').style.display = preset ? 'none' : '';
  showSeen();
  showKeepsake();
  showDrop();
}

/** Who will see it, in the marker list's own mark: the same picture in the same
 *  colour, so the answer on the form and the answer in the list are one answer. */
function showSeen() {
  dressSeen(markerPrivate, privately, markerName.value.trim() || 'this marker', mayKeep);
  markerPrivate.disabled = !mayKeep;
}

/** Whether this is being remembered as what a block starts as, and what block. */
function showKeepsake() {
  const what = clicked ? (clicked.name || shortCode(clicked.code)) : null;
  const words = alsoPreset
    ? `Kept as what ${what || 'a block'} starts as — click to stop keeping it`
    : `Set as what ${what || 'a block'} starts as`;
  markerRemember.classList.toggle('on', alsoPreset);
  markerRemember.title = words;
  markerRemember.setAttribute('aria-label', words);
  markerRemember.setAttribute('aria-pressed', String(alsoPreset));
}

function forgetCompose() {
  setPlacing(false);
  showPresetPick(false);
  awaiting = null;
  removing = false;
  dropArmed = false;
  mode = 'new';
  editing = null;
  editingPreset = -1;
  clicked = null;
  markerPattern.value = '';
  closeFound();
  // Both lists mark the row they have open, and this form no longer has one.
  drawPresets();
  drawDirectory();
  mayKeep = true;
  alsoPreset = false;
  markerName.value = '';
  markerX.value = '';
  markerY.value = '';
  markerZ.value = '';
  sayHere('');
}

function closeCompose() {
  shutWindow(composer);
}

/**
 * Closes the form because the marker is with the game now, rather than because
 * somebody shut it.
 *
 * The difference is the whole of why this exists. Shutting the form by hand is
 * somebody saying they have stopped waiting, and `forgetCompose` reads it that
 * way and drops the watch. A marker handed over is still worth watching — the
 * watch is the only thing that ever reports a failure — so it is lifted over the
 * close and put back.
 *
 * The button is freed here rather than when the marker lands, because the point
 * of closing early is to mark the next thing: a Save that stays greyed until the
 * game answers is the same wait in a different place.
 */
function handedToTheGame() {
  const watch = { key: awaiting, going: removing, shape: changedShape, at: askedAt };
  closeCompose();
  awaiting = watch.key;
  removing = watch.going;
  changedShape = watch.shape;
  askedAt = watch.at;
  markerSave.disabled = false;
}

/**
 * Puts the form back up on a marker the game never made.
 *
 * Left alone where the form is already open on something else: somebody who has
 * moved on to the next marker is not helped by an old one landing on top of it,
 * and what is said names the marker either way.
 */
function putTheFormBack() {
  const held = handedOver;
  handedOver = null;
  if (!held || composer.classList.contains('open')) return;

  if (held.editing) {
    editCompose(held.editing);
  } else {
    openCompose({ x: held.marker.X, y: held.marker.Y, z: held.marker.Z }, held.ground);
  }

  // Over the top of whatever opening it worked out for itself: a preset may name
  // the block this was made on, and what somebody actually typed is the thing
  // they are being handed back.
  markerName.value = held.marker.Title === UNNAMED ? '' : held.marker.Title;
  chosenColour = held.marker.Color;
  chosenPicture = held.marker.Icon;
  privately = held.marker.Private;
  alsoPreset = held.alsoPreset;
  markerPattern.value = held.pattern;
  showFields();
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
