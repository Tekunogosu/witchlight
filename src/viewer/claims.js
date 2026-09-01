// Where the land is spoken for, and the drawing of a new one.
//
// A claim is a rectangle rather than a point, which is the whole of why none of
// this is in `markers.js`: a marker is a place and is drawn as a mark, a claim is
// ground and is drawn as the ground it covers. What they share is the round trip
// — the page asks, the game decides, and the claim appearing among the ones the
// service sends is the only honest confirmation there is.
//
// Nothing here decides who may. The mod answers that against the game's own
// rules, twice: once when it says who may be sent the claims at all, and again
// when it is handed one somebody drew. What the page does with the answer is
// decide which buttons to offer, and a button offered to somebody the mod then
// refuses costs a sentence rather than a claim.

/** Every claim the service is willing to send this reader. */
let claims = [];

/**
 * What this reader is allowed to claim, or null where they may not claim at all.
 *
 * The mod's answer, arriving with the claims: how much land their role allows,
 * how much of it they have already used, how many separate claims they may hold,
 * and the smallest one they are allowed. It is what lets the form say what a
 * rectangle costs before asking for it — see the mod's `ClaimAllowance`.
 */
let allowance = null;

/** How tall this world is, as the mod says. Zero until it has said. */
let worldHeight = 0;

/**
 * The claims on the map, under everything else drawn on it.
 *
 * A pane of its own between the grid and Leaflet's overlays: a claim is ground
 * rather than a thing standing on it, so a marker inside one has to stay
 * clickable and a claim has to stay behind it.
 */
map.createPane('claims');
map.getPane('claims').style.zIndex = 350;

const claimed = L.layerGroup();

/** What was last drawn, so a post that says nothing new redraws nothing. */
let drawnClaims = null;

/**
 * How a claim is drawn: its own ground tinted, its boundary ruled.
 *
 * One colour for every claim rather than one per owner. Whose it is, is written
 * in the label and said in the popup; colouring by owner would need a palette
 * that stayed stable as people came and went, and would say something the map
 * cannot promise — that two patches of the same colour are the same person's.
 */
const CLAIM_COLOUR = '#f0b429';

/** Faint enough to read the terrain through, strong enough to see the edge of. */
const CLAIM_STYLE = {
  pane: 'claims',
  color: CLAIM_COLOUR,
  weight: 1,
  fillColor: CLAIM_COLOUR,
  fillOpacity: 0.12,
};

/**
 * Draws the claims, and only when they are not the ones already drawn.
 *
 * Compared as a set, like the markers and for the same reason: they arrive every
 * two seconds and change a few times a week, and rebuilding them on arrival would
 * tear down every rectangle on the map to discover that none of them had moved.
 */
function drawClaims(sent) {
  const shape = JSON.stringify(sent);
  if (shape === drawnClaims) return;
  drawnClaims = shape;

  claimed.clearLayers();
  for (const claim of sent) {
    for (const area of claim.Areas || []) {
      L.rectangle([at(area.X1, area.Z1), at(area.X2 + 1, area.Z2 + 1)], CLAIM_STYLE)
        .bindPopup(claimSays(claim, area))
        .addTo(claimed);
    }
  }
}

/**
 * Draws them again from scratch, whatever they last were.
 *
 * For a change to how a position is *said* rather than to what the positions are.
 * `drawClaims` leaves the map alone when the claims have not changed, which is
 * what stops it tearing down every rectangle twice a second — and is exactly
 * wrong when the numbers written on them are the thing that moved.
 */
function redrawClaims() {
  drawnClaims = null;
  drawClaims(claims);
}

/** What one claim says when it is opened. */
function claimSays(claim, area) {
  const owner = claim.Owner
    ? `<span class="said-who">${escaped(claim.Owner)}</span>`
    : '';
  const [x1, z1] = said(area.X1, area.Z1);
  const [x2, z2] = said(area.X2, area.Z2);
  const name = escaped(claim.Description || 'claimed land');
  return `<b class="said-name">${name}</b>`
    + `<span class="said-foot">`
    + `<span class="said-where">${x1}, ${z1} to ${x2}, ${z2}</span>${owner}</span>`;
}

// The window the rectangle is described in, and the two corners it was drawn
// between. The corners are held here rather than read back off the form: what is
// typed into the boxes is what the person means, and what was dragged is only
// where the boxes started.
const claimPanel = document.getElementById('claim');
const claimSaid = document.getElementById('claim-said');
const claimWhat = document.getElementById('claim-what');
const claimLeft = document.getElementById('claim-left');
const claimName = document.getElementById('claim-name');
const claimGuests = document.getElementById('claim-guests');
const claimEveryoneWalks = document.getElementById('claim-everyone-walks');
const claimEveryoneUses = document.getElementById('claim-everyone-uses');
const claimFields = {
  x1: document.getElementById('claim-x1'),
  z1: document.getElementById('claim-z1'),
  x2: document.getElementById('claim-x2'),
  z2: document.getElementById('claim-z2'),
  y1: document.getElementById('claim-y1'),
  y2: document.getElementById('claim-y2'),
};

/**
 * The claim the form is open on, or null when it is open on new ground.
 *
 * What the form *is* rather than a flag beside it: a window that can be two
 * things needs one place saying which, and every button in it reads this rather
 * than a boolean each of them could disagree about. The same shape the marker
 * form's `editing` has, for the same reason.
 */
let editingClaim = null;

/**
 * How far above and below the ground a claim reaches when nobody has said.
 *
 * Enough to cover a cellar and a roof, which is what somebody drawing a boundary
 * round a base means by it, and small enough that a survival player's allowance
 * buys a footprint worth having: at sixty-five deep a quarter of a million cubic
 * metres is a square sixty-three blocks across, against thirty-two at the full
 * height of the world.
 */
const CLAIM_BELOW = 32;
const CLAIM_ABOVE = 32;

/** Whether the map is waiting for a rectangle to be dragged out. */
let drawing = false;

/** Where the drag started, while one is under way. Null between drags. */
let firstCorner = null;

/**
 * The rectangle being dragged out, in world coordinates.
 *
 * A layer rather than a box drawn at the pointer, for the reason the block
 * outline is one: it is ground being marked, so it has to stay on that ground
 * while the map moves and grow with it as the view zooms in.
 */
const drawnRectangle = L.rectangle([at(0, 0), at(1, 1)], {
  pane: 'claims',
  color: CLAIM_COLOUR,
  weight: 2,
  dashArray: '4 3',
  fillColor: CLAIM_COLOUR,
  fillOpacity: 0.18,
  interactive: false,
});

/**
 * Arms or disarms drawing a claim.
 *
 * Two cursors claiming the same click is one of them being ignored, so arming
 * this disarms the marker's own two — the same rule they already follow between
 * themselves.
 */
function setDrawing(on) {
  drawing = on;
  if (on) {
    if (placing) setPlacing(false);
    if (picking) setPicking(false);
  }
  firstCorner = null;
  layer(drawnRectangle, false);
  claimDraw.classList.toggle('armed', on);
  claimDraw.setAttribute('aria-pressed', String(on));

  // A boundary is dragged out, so while this is armed a drag has to mean the
  // rectangle and not the map. Leaflet's own handler is switched off rather than
  // fought with — a drag answered by both is the map sliding under the corner
  // being placed, which is the thing that made this unusable.
  if (on) map.dragging.disable();
  else map.dragging.enable();
  // Its own cursor rather than the block picker's. Both say "this click means
  // something", and a crosshair is what every other tool that marks out ground
  // uses — the picker deliberately leaves an arrow, because it names the one
  // block underneath and a crosshair would cover it.
  map.getContainer().classList.toggle('drawing', on);
  map.getContainer().classList.toggle('picking', placing || picking);

  if (on) sayClaim('Drag out the ground to claim.');
}

/** Moves the outline as the drag stretches it. */
function stretchClaim(latlng) {
  if (!drawing || !firstCorner || !latlng) return;
  const far = blockUnder(latlng);
  drawnRectangle.setBounds([
    at(Math.min(firstCorner.x, far.x), Math.min(firstCorner.z, far.z)),
    at(Math.max(firstCorner.x, far.x) + 1, Math.max(firstCorner.z, far.z) + 1),
  ]);
  layer(drawnRectangle, true);
}

/** The block a point on the map is over, which is what a corner is placed on. */
function blockUnder(latlng) {
  return { x: Math.floor(latlng.lng), z: Math.floor(latlng.lat) };
}

/**
 * Starts the drag that marks out a claim.
 *
 * Press, move, release — one gesture, which is what marking out ground on a map
 * means everywhere else. It was two separate clicks with the pointer free in
 * between, which reads as the map having missed the first one.
 */
function beginClaim(latlng) {
  if (!drawing || !latlng) return;
  firstCorner = blockUnder(latlng);
  stretchClaim(latlng);
}

/**
 * Finishes the drag and opens the form on what was drawn.
 *
 * Nothing is asked of the game here: a rectangle dragged out is a question about
 * the ground, and taking land is not something to do on the release of a
 * gesture. A drag that never moved is somebody clicking to cancel rather than a
 * claim one block across, so it clears the outline and leaves the tool armed.
 */
function endClaim(latlng) {
  if (!drawing || !firstCorner || !latlng) return;
  const far = blockUnder(latlng);
  const from = firstCorner;
  firstCorner = null;

  if (from.x === far.x && from.z === far.z) {
    layer(drawnRectangle, false);
    sayClaim('Drag out the ground to claim.');
    return;
  }

  openClaim(
    Math.min(from.x, far.x),
    Math.min(from.z, far.z),
    Math.max(from.x, far.x),
    Math.max(from.z, far.z));
}

/**
 * Opens the form on a rectangle, in the numbers the reader reads coordinates in.
 *
 * The depth starts as a band around the ground in the middle of what was drawn,
 * which is the useful answer for somebody marking out a base and a far better one
 * than the whole height of the world. The ground is asked for after the window is
 * open rather than before: it is a round trip, and a form that appears when the
 * service answers is a form that appears late for no reason.
 */
function openClaim(x1, z1, x2, z2) {
  setDrawing(false);
  editingClaim = null;
  const [westX, northZ] = said(x1, z1);
  const [eastX, southZ] = said(x2, z2);
  claimFields.x1.value = westX;
  claimFields.z1.value = northZ;
  claimFields.x2.value = eastX;
  claimFields.z2.value = southZ;
  claimFields.y1.value = '';
  claimFields.y2.value = '';
  claimName.value = '';
  claimGuests.value = '';
  claimEveryoneWalks.checked = false;
  claimEveryoneUses.checked = false;
  dressClaimForm();
  sayClaim('');
  measureClaim();
  openWindow(claimPanel, true);
  started(settleDepth(Math.round((x1 + x2) / 2), Math.round((z1 + z2) / 2)),
    'reading the ground the claim is on');
}

/**
 * Opens the form on a claim that already exists, to rename or re-permission it.
 *
 * The ground is shown and cannot be typed over. Moving a boundary has to be
 * judged against every other claim and against an allowance, and a map cannot
 * show somebody what they would be giving up — so redrawing is making a new one,
 * which is a different act and has its own button.
 */
function editClaim(claim) {
  setDrawing(false);
  editingClaim = claim;

  const first = (claim.Areas || [])[0] || { X1: 0, Z1: 0, X2: 0, Z2: 0, Y1: 0, Y2: 0 };
  const [westX, northZ] = said(first.X1, first.Z1);
  const [eastX, southZ] = said(first.X2, first.Z2);
  claimFields.x1.value = westX;
  claimFields.z1.value = northZ;
  claimFields.x2.value = eastX;
  claimFields.z2.value = southZ;
  claimFields.y1.value = first.Y1;
  claimFields.y2.value = first.Y2;

  claimName.value = claim.Description || '';
  claimGuests.value = (claim.Guests || []).map(guest => guest.Name).join(', ');
  claimEveryoneWalks.checked = claim.EveryoneWalks === true;
  claimEveryoneUses.checked = claim.EveryoneUses === true;

  dressClaimForm();
  sayClaim((claim.Areas || []).length > 1
    ? 'This claim is several areas. The map can rename it and say who may use it; '
      + 'its shape is changed in game.'
    : '');
  measureClaim();
  openWindow(claimPanel, true);
}

/**
 * Dresses the form as whichever of the two things it currently is.
 *
 * One place deciding what the window says and which of its controls apply, read
 * off `editingClaim`. Written per button, this is four places that can disagree
 * about whether a window is making something or changing it.
 */
function dressClaimForm() {
  const known = editingClaim !== null;
  document.getElementById('claim-title').textContent =
    known ? 'Land claim' : 'New land claim';
  document.getElementById('claim-save').textContent = known ? 'Save' : 'Claim';
  document.getElementById('claim-drop').style.display = known ? '' : 'none';
  document.getElementById('claim-draw-again').style.display = known ? 'none' : '';

  // The ground is the claim's own once it exists. Shown, because it is what
  // tells one claim from another in a list, and not editable, because this form
  // cannot honestly ask the game to move a boundary.
  for (const field of Object.values(claimFields)) {
    field.readOnly = known;
  }
  document.getElementById('claim-full-height').disabled = known;
  claimPanel.classList.toggle('editing', known);
}

/**
 * Fills the depth in from the ground under the middle of the rectangle.
 *
 * Only where the person has not already typed one. The answer arrives a moment
 * after the form opens, and writing over what somebody typed in that moment would
 * be the map arguing with them.
 */
async function settleDepth(x, z) {
  const ground = await lookUp(x, z);
  if (!ground || typeof ground.y !== 'number') return;
  if (claimFields.y1.value !== '' || claimFields.y2.value !== '') return;

  const top = worldHeight > 0 ? worldHeight : ground.y + CLAIM_ABOVE;
  claimFields.y1.value = Math.max(0, ground.y - CLAIM_BELOW);
  claimFields.y2.value = Math.min(top, ground.y + CLAIM_ABOVE);
  measureClaim();
}

/**
 * The claim the form is currently holding, in the world's own numbers.
 *
 * Null where any of the six boxes is not a number, which is a form somebody is
 * still filling in rather than one that is wrong. Only the two horizontal pairs
 * go through `meant`: a height is a height, and the game and the map count it
 * from the same place.
 */
function claimAsked() {
  const numbers = ['x1', 'z1', 'x2', 'z2', 'y1', 'y2']
    .map(part => Number(claimFields[part].value));
  if (!numbers.every(Number.isFinite)
      || Object.values(claimFields).some(field => field.value.trim() === '')) {
    return null;
  }
  const [westX, northZ] = meant(numbers[0], numbers[1]);
  const [eastX, southZ] = meant(numbers[2], numbers[3]);
  return {
    X1: westX,
    Z1: northZ,
    X2: eastX,
    Z2: southZ,
    Y1: Math.min(numbers[4], numbers[5]),
    Y2: Math.max(numbers[4], numbers[5]),
  };
}

/** How much ground a claim covers, in the cubic metres an allowance is counted
 *  in. Every corner is inside it, which is what dragging a rectangle means. */
function claimVolume(asked) {
  return (Math.abs(asked.X2 - asked.X1) + 1)
    * (Math.abs(asked.Z2 - asked.Z1) + 1)
    * (Math.abs(asked.Y2 - asked.Y1) + 1);
}

/**
 * Says how much ground the form is asking for.
 *
 * The one number the game will judge this by that the page can work out for
 * itself. Whether it is within somebody's allowance is the game's answer, and
 * saying the size is what lets a person see why the answer was no.
 */
function measureClaim() {
  const asked = claimAsked();
  if (!asked) {
    counted(claimWhat, '');
    counted(claimLeft, '');
    return;
  }

  const across = Math.abs(asked.X2 - asked.X1) + 1;
  const down = Math.abs(asked.Z2 - asked.Z1) + 1;
  const deep = Math.abs(asked.Y2 - asked.Y1) + 1;
  const volume = claimVolume(asked);
  counted(claimWhat,
    `${across} × ${down} blocks, ${deep} deep — ${volume.toLocaleString()} m³`);

  // What it would cost is a question about ground being taken. A claim that
  // already exists has already spent it — its volume is inside `Used` — so
  // pricing it again would tell its owner they are over their allowance by the
  // size of the land they are standing on.
  if (!allowance || editingClaim) {
    counted(claimLeft, '');
    return;
  }

  // What is left after this one, which is the number somebody sizing a claim is
  // actually reading for. Said as a shortfall when it is one, because "you are
  // over by 40,000" is what tells them how much to give up.
  const spare = allowance.Allowance - allowance.Used - volume;
  counted(claimLeft, spare >= 0
    ? `${spare.toLocaleString()} m³ of your allowance left after this.`
    : `${(-spare).toLocaleString()} m³ over your allowance — make it smaller or shallower.`);
  claimLeft.classList.toggle('wrong', spare < 0);
}

/**
 * Writes a sentence the window worked out, with the numbers in it coloured.
 *
 * Every line this window says about a rectangle is mostly words with two or
 * three figures buried in it, and the figures are the whole reason somebody is
 * reading it. Written here rather than at each sentence: three lines each
 * splitting their own text is three places for "what counts as a number" to
 * differ, and the answer has to be the same in all of them or the form colours
 * some of its figures and not others.
 *
 * Split rather than composed from parts because a number is not always in the
 * same place — a size says three, a shortfall says one, and what the service
 * refused says whatever the mod wrote. Text either way, never markup: what these
 * lines carry comes from the far end.
 */
function counted(into, sentence) {
  into.textContent = '';
  // A run of digits with the separators that are inside a number rather than
  // after one: a figure at the end of a sentence must not take the full stop
  // into the colour with it.
  for (const piece of String(sentence).split(/(\d+(?:[,.]\d+)*)/)) {
    if (piece === '') continue;
    if (!/^\d/.test(piece)) {
      into.append(piece);
      continue;
    }
    const figure = document.createElement('b');
    figure.className = 'count';
    figure.textContent = piece;
    into.append(figure);
  }
}

function sayClaim(what, wrong) {
  counted(claimSaid, what || '');
  claimSaid.classList.toggle('wrong', Boolean(wrong));
}

/**
 * What this page has asked for and not yet seen the game do.
 *
 * Three, because there are three asks and each is confirmed by a different thing
 * being true: a new claim by ground of this reader's own appearing, a change by
 * the claim coming back saying what was asked, and a removal by the claim no
 * longer arriving. Only one is ever set — the form can only be doing one thing.
 */
let claiming = null;
let changingClaim = null;
let droppingClaim = null;
let claimAskedAt = 0;

/**
 * Asks for the claim the form is holding.
 *
 * Nothing is done here and the page does not pretend otherwise. Whether this
 * person may take this land is the game's to answer against its own rules — the
 * privilege, the allowance, how small a claim may be, and whether it lands on
 * anybody else's — so the page says what it asked for and watches the ground.
 */
/** Who the form says may build here, as the mod reads it. */
function guestsAsked() {
  return {
    Names: claimGuests.value.split(',').map(name => name.trim()).filter(Boolean),
    EveryoneUses: claimEveryoneUses.checked,
    EveryoneWalks: claimEveryoneWalks.checked,
  };
}

/**
 * Saves a change to a claim that already exists.
 *
 * Its name and who it lets in, which is all this form can honestly change. The
 * game decides whether this person may, exactly as it does for a new one.
 */
async function saveClaim() {
  const save = document.getElementById('claim-save');
  save.disabled = true;
  sayClaim('Asking the game server…');

  let answer;
  try {
    answer = await fetch(`/claims/${encodeURIComponent(editingClaim.Key)}`, {
      method: 'PUT',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        Description: claimName.value.trim(),
        Allowed: guestsAsked(),
      }),
    });
  } catch (error) {
    sayClaim('The map service is not answering.', true);
    save.disabled = false;
    return;
  }

  if (!answer.ok) {
    sayClaim(await answer.text().catch(() => 'That was refused.'), true);
    save.disabled = false;
    return;
  }

  save.disabled = false;
  // Watched for by what the claim becomes rather than by its appearing: a rename
  // keeps the ground, so the key does not change and the only honest sign it
  // took is the description coming back as what was asked for.
  changingClaim = { key: editingClaim.Key, description: claimName.value.trim() };
  claimAskedAt = Date.now();
  sayClaim('Waiting for the game server…');
}

/** Asks for the claim the form is open on to be given up. */
async function dropClaim() {
  if (!editingClaim) return;
  const drop = document.getElementById('claim-drop');

  // Asked twice, because land is not a thing to lose to one stray press. The
  // second press is the answer, and the button says so in between.
  if (!drop.classList.contains('armed')) {
    drop.classList.add('armed');
    sayClaim('Press again to give up this claim.', true);
    return;
  }
  drop.classList.remove('armed');

  sayClaim('Asking the game server…');
  let answer;
  try {
    answer = await fetch(`/claims/${encodeURIComponent(editingClaim.Key)}`, {
      method: 'DELETE',
    });
  } catch (error) {
    sayClaim('The map service is not answering.', true);
    return;
  }
  if (!answer.ok) {
    sayClaim(await answer.text().catch(() => 'That was refused.'), true);
    return;
  }

  droppingClaim = editingClaim.Key;
  claimAskedAt = Date.now();
  sayClaim('Waiting for the game server…');
}

async function askForClaim() {
  if (editingClaim) {
    await saveClaim();
    return;
  }

  const asked = claimAsked();
  if (!asked) {
    sayClaim('A claim needs two corners and a depth: type them, or draw one on the map.', true);
    return;
  }

  // Refused here where the answer is already known, rather than sent to be
  // refused. The game decides for real and decides again whatever this says; what
  // this saves is a round trip and twenty seconds of watching for a claim that
  // was never going to arrive.
  if (allowance) {
    const over = claimVolume(asked) - (allowance.Allowance - allowance.Used);
    if (over > 0) {
      sayClaim(`That is ${over.toLocaleString()} m³ past your allowance.`, true);
      return;
    }
    if (allowance.Areas >= allowance.MaxAreas) {
      sayClaim(
        `You already have ${allowance.Areas} claims, which is all your role allows.`, true);
      return;
    }
  }

  const save = document.getElementById('claim-save');
  save.disabled = true;
  sayClaim('Asking the game server…');

  let answer;
  try {
    answer = await fetch('/claims', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        ...asked,
        Description: claimName.value.trim(),
        Allowed: guestsAsked(),
      }),
    });
  } catch (error) {
    sayClaim('The map service is not answering.', true);
    save.disabled = false;
    return;
  }

  if (!answer.ok) {
    sayClaim(await answer.text().catch(() => 'That was refused.'), true);
    save.disabled = false;
    return;
  }

  // The middle of what was asked for. A claim carries no name this page could
  // have agreed on beforehand — the game mints nothing and neither does the
  // service — so what is watched for is a claim of this reader's own covering
  // the ground they drew on, which is the thing they asked to be true.
  claiming = { x: (asked.X1 + asked.X2) / 2, z: (asked.Z1 + asked.Z2) / 2 };
  claimAskedAt = Date.now();
  save.disabled = false;
  sayClaim('Waiting for the game server…');
}

/** Whether the claim this page asked for is among the ones it has been sent. */
function claimArrived() {
  if (!claiming) return false;
  return claims.some(claim =>
    viewer && claim.OwnerUid === viewer.Uid
    && (claim.Areas || []).some(area =>
      claiming.x >= area.X1 && claiming.x <= area.X2 + 1
      && claiming.z >= area.Z1 && claiming.z <= area.Z2 + 1));
}

/**
 * Watches for the claim to appear, and gives up on it after a while.
 *
 * The same patience a marker is given, for the same reason: the mod collects
 * every two seconds, so a claim that has not arrived by then is a game server
 * that has stopped rather than one that is busy. Called on the live poll.
 */
function watchClaim() {
  if (!(claiming || changingClaim || droppingClaim)) return;

  if (claimSettled()) {
    claiming = changingClaim = droppingClaim = null;
    sayClaim('');
    shutWindow(claimPanel);
    return;
  }

  if (Date.now() - claimAskedAt > MARKER_PATIENCE) {
    claiming = changingClaim = droppingClaim = null;
    // What the game refused and why is said in game, not here: the rules a claim
    // is judged by are the server's, and a page inventing a reason would be
    // guessing at one of five. So this says what is true — it did not happen —
    // and points at where the answer is.
    sayClaim(
      'The game server did not do it. Check `/land` in game for why — a new claim '
      + 'may overlap another or be past what you are allowed, and a change needs '
      + 'the claim to still be yours.', true);
  }
}

/** Whether whichever ask is outstanding has been answered by the claims sent. */
function claimSettled() {
  if (claiming) return claimArrived();
  if (droppingClaim) return !claims.some(claim => claim.Key === droppingClaim);
  if (changingClaim) {
    return claims.some(claim =>
      claim.Key === changingClaim.key && (claim.Description || '') === changingClaim.description);
  }
  return false;
}

/**
 * The claims button, and the drawing of a new one.
 *
 * Last of the reader's own bars in the tool column and above the map's own two,
 * because what it does is about the ground the map is showing rather than about
 * the reader — so it sits between the two subjects rather than inside either.
 *
 * The second button is offered only where the mod says this reader may draw one,
 * so a server that keeps its land to a role shows a toggle and nothing else.
 */
const claimBar = cornerButton('claims', 'polygon', 'Land claims');
const claimShow = claimBar.querySelector('a');
const claimDraw = cornerAnchor(claimBar, 'selection-plus', 'Draw a land claim');
const claimList = cornerAnchor(claimBar, 'list-bullets', 'View claim list');
// Offered only to somebody the mod says may take land. Said as a class on the
// bar rather than a style on the button, so what is hidden and what is shown is
// one rule in the stylesheet — see `#claims .claim-make`.
claimDraw.classList.add('claim-make');

/** Whether the claims layer is being shown, said on the button that toggles it. */
function showClaimsToggle() {
  claimShow.classList.toggle('armed', settings.claims.on);
  claimShow.setAttribute('aria-pressed', String(settings.claims.on));
}

/**
 * Says who may do what with the claims, on the buttons that offer it.
 *
 * Read from the live poll rather than from `/me.json`: whether somebody may draw
 * a claim is the mod's answer and arrives with the claims themselves, so a page
 * opened before the game server was up learns it on the next beat instead of
 * needing a reload.
 */
function showClaims() {
  const may = Boolean(allowance);
  claimBar.classList.toggle('may-claim', may);
  if (!may && drawing) setDrawing(false);
  drawClaimsList();
  // The window may be open on a rectangle while the numbers behind it change:
  // somebody else's claim landing changes nothing here, but their own does, and
  // so does an operator raising their allowance.
  if (claimPanel.classList.contains('open')) measureClaim();
}

// Every claim there is, as a list. The map says where each one is; this says
// what there is, and is the only way to reach one to rename, re-permission or
// give up — a boundary on a map is a thing to look at, not a thing to click
// through to a form.
const claimsPanel = document.getElementById('claims-list');
const claimRows = document.getElementById('claim-rows');
const claimFind = document.getElementById('claim-find');
const claimsSaid = document.getElementById('claims-said');

/** Which of the two lists is showing: this reader's claims, or the lot. */
let claimsTab = 'mine';

/** Whether a claim is one this reader may rename or give up. */
function mineToChange(claim) {
  return Boolean(viewer && viewer.Uid && claim.OwnerUid === viewer.Uid);
}

/** The claims the list is currently showing, in the order it shows them. */
function listedClaims() {
  const looking = claimFind.value.trim().toLowerCase();
  return claims
    .filter(claim => claimsTab === 'all' || mineToChange(claim))
    .filter(claim => looking === ''
      || (claim.Description || '').toLowerCase().includes(looking)
      || (claim.Owner || '').toLowerCase().includes(looking))
    .sort((one, two) => (one.Description || '').localeCompare(two.Description || ''));
}

/**
 * Draws the list.
 *
 * Rebuilt rather than written into, unlike the player cards: claims change a few
 * times a week and this window is open for seconds at a time, so the reason the
 * cards are patched — a redraw twice a second taking a row out from under a
 * pointer — does not apply. It is only redrawn while it is open.
 */
function drawClaimsList() {
  if (!claimsPanel.classList.contains('open')) return;

  for (const tab of claimsPanel.querySelectorAll('.tab')) {
    const on = tab.dataset.claims === claimsTab;
    tab.classList.toggle('on', on);
    tab.setAttribute('aria-selected', String(on));
    tab.querySelector('.tally').textContent =
      tab.dataset.claims === 'mine' ? claims.filter(mineToChange).length : claims.length;
  }

  claimRows.textContent = '';
  const shown = listedClaims();
  if (shown.length === 0) {
    claimsSaid.textContent = claims.length === 0
      ? 'No land is claimed on this server, or none you may see.'
      : 'Nothing here matches.';
    return;
  }
  claimsSaid.textContent = '';

  shown.forEach((claim, nth) => claimRows.append(claimRow(claim, nth % 2 === 1)));
}

/**
 * One claim as a row: what it is called, whose it is and where, with a way in.
 *
 * Built by the same function the marker and preset lists use, so a list of
 * claims looks like a list of anything else on this page rather than like a
 * third thing that nearly does.
 */
function claimRow(claim, shaded) {
  const first = (claim.Areas || [])[0];
  let where = '';
  if (first) {
    const [x, z] = said(Math.round((first.X1 + first.X2) / 2),
                        Math.round((first.Z1 + first.Z2) / 2));
    where = `${x}, ${z}`;
  }
  const owner = claim.Owner ? ` · ${claim.Owner}` : '';

  const { line, open } = listedRow(
    'polygon', CLAIM_COLOUR, claim.Description || 'unnamed claim', where + owner, shaded);

  // Opening a row is going to look at it, so it moves the map and makes sure the
  // claims are being drawn. Changing one is a different act with its own button:
  // a list somebody is reading is not a list they meant to edit.
  open.addEventListener('click', () => {
    if (first) map.panTo(at((first.X1 + first.X2) / 2, (first.Z1 + first.Z2) / 2));
    setSetting('claims', true);
  });

  if (mineToChange(claim)) {
    const change = document.createElement('button');
    change.type = 'button';
    change.className = 'use';
    // One of a column of identical buttons, so what it is about is the only
    // thing telling a reader — or a test — which one this is.
    change.setAttribute('aria-label', `Edit ${claim.Description || 'unnamed claim'}`);
    change.title = 'Rename it, or say who else may build here';
    change.append(chromeMark('eyedropper'));
    change.addEventListener('click', event => {
      event.stopPropagation();
      editClaim(claim);
    });
    line.append(change);
  }

  return line;
}

function buildClaimsList() {
  for (const tab of claimsPanel.querySelectorAll('.tab')) {
    tab.addEventListener('click', () => {
      claimsTab = tab.dataset.claims === 'all' ? 'all' : 'mine';
      drawClaimsList();
    });
  }
  claimFind.addEventListener('input', drawClaimsList);

  claimList.addEventListener('click', () => {
    if (claimsPanel.classList.contains('open')) {
      shutWindow(claimsPanel);
      return;
    }
    openWindow(claimsPanel, true);
    drawClaimsList();
  });
}

function buildClaims() {
  buildClaimsList();
  showClaimsToggle();
  showClaims();

  // Through the one owner of what turning a setting on means, so the checkbox in
  // the panel follows this button and the button follows the checkbox.
  L.DomEvent.on(claimShow, 'click', () => setSetting('claims', !settings.claims.on));
  L.DomEvent.on(claimDraw, 'click', () => setDrawing(!drawing));

  // The tool's own handlers rather than lines in the marker form's, so the whole
  // of what drawing a claim does is in one file. Nothing else answers these
  // while it is armed: arming this disarms the other two tools, and Leaflet's
  // own dragging is off for as long as it is on.
  map.on('mousedown', event => beginClaim(event.latlng));
  map.on('mouseup', event => endClaim(event.latlng));

  document.getElementById('claim-save')
    .addEventListener('click', () => started(askForClaim(), 'asking for a land claim'));
  document.getElementById('claim-cancel').addEventListener('click', () => {
    claiming = changingClaim = droppingClaim = null;
    sayClaim('');
    shutWindow(claimPanel);
  });
  document.getElementById('claim-drop')
    .addEventListener('click', () => started(dropClaim(), 'giving up a land claim'));
  // The bin disarms itself the moment anything else is touched, so a press left
  // armed and forgotten cannot be completed by a later, unrelated one.
  for (const field of [claimName, claimGuests, claimEveryoneUses, claimEveryoneWalks]) {
    field.addEventListener('input', () =>
      document.getElementById('claim-drop').classList.remove('armed'));
  }
  document.getElementById('claim-draw-again').addEventListener('click', () => {
    shutWindow(claimPanel);
    setDrawing(true);
  });
  // The whole height of the world, for a claim that is meant to be one. Offered
  // rather than assumed, which is the difference between a choice and a default
  // that costs a survival player most of their land.
  document.getElementById('claim-full-height').addEventListener('click', () => {
    claimFields.y1.value = 0;
    claimFields.y2.value = worldHeight > 0 ? worldHeight : 0;
    measureClaim();
  });
  // Both of the game's everybody-permissions at once. It ticks the boxes rather
  // than holding a state of its own, so what has been granted is always readable
  // off the boxes and there is nothing to keep in step with them.
  document.getElementById('claim-open-up').addEventListener('click', () => {
    claimEveryoneWalks.checked = true;
    claimEveryoneUses.checked = true;
    document.getElementById('claim-drop').classList.remove('armed');
  });
  for (const field of Object.values(claimFields)) {
    field.addEventListener('input', measureClaim);
  }
}
