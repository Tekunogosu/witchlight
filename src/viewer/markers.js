// What a marker is made of, and the asking for it.
//
// The colours and pictures the game offers, where a marker goes, and the round
// trip that ends with it appearing among everybody else's — which is the only
// honest confirmation there is that the game made it.

/**
 * The colours the game offers.
 *
 * Asked of the service rather than written down here, so a mod that adds a
 * colour to the game's picker adds it to this one. A service that has not heard
 * from a mod yet has none to offer, and the form falls back to plain white so
 * that it is still a form.
 */
function drawColours() {
  const box = document.getElementById('colours');
  box.textContent = '';
  const offered = palette.length > 0 ? palette : ['#ffffff'];
  if (!offered.includes(chosenColour)) offered.unshift(chosenColour);

  for (const colour of offered) {
    const swatch = document.createElement('button');
    swatch.type = 'button';
    swatch.className = 'swatch' + (colour === chosenColour ? ' chosen' : '');
    swatch.style.background = colour;
    swatch.title = colour;
    // One of thirty-odd identical squares, so the colour is the only thing that
    // tells a reader — or a test — which one this is.
    swatch.setAttribute('aria-label', `Colour ${colour}`);
    swatch.setAttribute('aria-pressed', String(colour === chosenColour));
    swatch.addEventListener('click', () => {
      chosenColour = colour;
      drawColours();
      // The pictures are drawn in it, so they are drawn again.
      drawPictures();
    });
    box.append(swatch);
  }
}

/**
 * How bright a colour is, on the scale a screen is measured by.
 *
 * The standard's own weighting rather than an average of the three channels:
 * green carries most of what an eye reads as brightness and blue almost none, so
 * a plain mean calls a saturated blue as light as a mid grey.
 */
function brightness(colour) {
  const parts = [1, 3, 5].map(from => parseInt(colour.slice(from, from + 2), 16) / 255);
  const [red, green, blue] = parts.map(one =>
    one <= 0.03928 ? one / 12.92 : ((one + 0.055) / 1.055) ** 2.4);
  return 0.2126 * red + 0.7152 * green + 0.0722 * blue;
}

/** Where the ground the pictures are drawn on stops being dark enough to draw a
 *  dark marker against. Black on the panel's own near-black is a row of holes. */
const TOO_DARK_TO_READ = 0.06;

/** The pictures the service has, drawn as the markers they will become. */
function drawPictures() {
  const box = document.getElementById('pictures');
  box.textContent = '';
  // The pictures are the chosen colour, so a colour near black leaves the picker
  // showing nothing to pick from. Lifted rather than outlined: what is being
  // chosen is a silhouette, and a silhouette is read against its ground.
  box.classList.toggle('lit', brightness(colourOf(chosenColour)) < TOO_DARK_TO_READ);
  const offered = [...icons].sort();
  if (offered.length === 0) offered.push('circle');
  if (!offered.includes(chosenPicture)) chosenPicture = offered[0];

  for (const name of offered) {
    box.append(pictureButton(name, chosenColour, name === chosenPicture, () => {
      chosenPicture = name;
      drawPictures();
    }));
  }
}

/** One marker picture as a button, drawn the way the marker itself will be. */
function pictureButton(name, colour, chosen, chose) {
  const swatch = document.createElement('button');
  swatch.type = 'button';
  swatch.className = 'swatch' + (chosen ? ' chosen' : '');
  swatch.title = name;
  swatch.setAttribute('aria-label', `Picture ${name}`);
  swatch.setAttribute('aria-pressed', String(chosen));
  swatch.append(markFor(name, colour));
  swatch.addEventListener('click', chose);
  return swatch;
}

/**
 * Naming the spot by pointing at it.
 *
 * The block picker is a different tool answering a different question, and two
 * cursors claiming the same click is one of them being ignored, so arming this
 * disarms that.
 */
function setPlacing(on) {
  placing = on;
  if (on && picking) setPicking(false);
  markerPick.classList.toggle('armed', on);
  markerPick.setAttribute('aria-pressed', String(on));
  map.getContainer().classList.toggle('picking', on || picking);
  if (!on && !picking) forget();
  if (on) sayHere('Click the map where the marker goes.');
}

/** Moves the outline while a spot is being pointed at. */
function hover(latlng) {
  if (!placing || !latlng) return;
  const block = { x: Math.floor(latlng.lng), z: Math.floor(latlng.lat) };
  outline.setBounds([at(block.x, block.z), at(block.x + 1, block.z + 1)]);
  layer(outline, true);
}

/**
 * Takes the spot that was clicked, and the height of the ground on it.
 *
 * The map is looked at from above, so a click names two of the three numbers a
 * waypoint needs. The third is what the renderer drew for that same pixel, which
 * is the surface — and the surface is where somebody pointing at the map means.
 */
async function settle(latlng) {
  const block = { x: Math.floor(latlng.lng), z: Math.floor(latlng.lat) };
  const [x, z] = said(block.x, block.z);
  markerX.value = x;
  markerZ.value = z;
  setPlacing(false);
  sayHere('');

  const ground = await lookUp(block.x, block.z);
  if (ground && typeof ground.y === 'number') markerY.value = ground.y;
}

/**
 * Asks for the marker, whether it is a new one or a change to one that exists.
 *
 * Nothing is done here and the page does not pretend otherwise: it says what it
 * asked for, and waits to see the marker arrive among the rest. A marker that
 * never arrives is a game server that stopped collecting, which the service can
 * tell apart from a slow one and this asks it to.
 */
async function askForMarker() {
  if (mode === 'preset') {
    await updatePreset();
    return;
  }

  const x = Number(markerX.value);
  const y = Number(markerY.value);
  const z = Number(markerZ.value);
  if (![markerX, markerY, markerZ].every(field => field.value.trim() !== '')
      || ![x, y, z].every(Number.isFinite)) {
    sayHere('A marker needs a place: type one, or pick it on the map.', true);
    return;
  }

  // Asked for and impossible is worth saying out loud. A marker made from a
  // right click carries the block it was made on, but one being changed carries
  // nothing to key a preset to — so the pattern has to be typed, and a mark
  // pressed to no effect is worse than a mark that says why.
  if (alsoPreset && markerPattern.value.trim() === '' && !(clicked && clicked.code)) {
    sayHere('A preset needs a block to start from: type one above.', true);
    return;
  }

  const [worldX, worldZ] = meant(x, z);
  const marker = {
    // Named here rather than left blank for the game to name. A marker with no
    // name is called "Marker" by the time it comes back, and an edit is
    // recognised as having landed by the marker reading as what was asked for —
    // so a blank name asked for a marker that could never match, and the form sat
    // waiting twenty seconds before reporting a failure that had not happened.
    Title: markerName.value.trim() || UNNAMED,
    Icon: chosenPicture,
    Color: chosenColour,
    X: worldX,
    Y: Math.round(y),
    Z: worldZ,
    Private: privately,
  };

  markerSave.disabled = true;
  sayHere('Asking the game server…');

  // A preset is this page's own record and lands whatever the game says, so it
  // is kept first: a marker that fails to reach a stopped game server should not
  // also lose the choice to remember what it was for.
  if (alsoPreset) await rememberPreset(marker);

  let answer;
  try {
    answer = editing
      ? await fetch(`/markers/${encodeURIComponent(editing.Key)}`, {
        method: 'PUT',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(marker),
      })
      : await fetch('/markers', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(marker),
      });
  } catch (error) {
    sayHere('The map service is not answering.', true);
    markerSave.disabled = false;
    return;
  }

  if (!answer.ok) {
    // The service says which field was wrong, in words meant to be read.
    sayHere(await answer.text().catch(() => 'That was refused.'), true);
    markerSave.disabled = false;
    return;
  }

  const { Key } = await answer.json().catch(() => ({}));
  awaiting = Key || null;
  askedAt = Date.now();
  // A change keeps the marker's own name, so the page has to watch for the
  // marker's shape rather than for a name it has not seen before.
  changedShape = editing ? JSON.stringify(marker) : null;

  // Everything the form would have to be rebuilt from, taken before it closes.
  handedOver = { marker, editing, ground: clicked, alsoPreset, pattern: markerPattern.value };

  // And closed. Waiting for the game to make it is the map's business rather
  // than the person's: they marked a place, and the next thing they want is to
  // mark another one — not a window sitting over the map for the second or two
  // the round trip takes. It comes back only if the marker never arrives.
  handedToTheGame();
}

/**
 * Asks for whatever the form is open on to be taken away.
 *
 * A preset is this page's own record, so it goes at once and the list behind is
 * drawn again. A marker is the game's, so this asks and then waits — for the
 * marker to stop arriving, which is the same watch an edit uses read the other
 * way round and the only honest confirmation either of them has.
 */
async function askToDelete() {
  if (mode === 'preset') {
    if (editingPreset >= 0) await dropPreset(editingPreset);
    return;
  }
  if (!editing) return;

  markerSave.disabled = true;
  markerDrop.disabled = true;
  sayHere('Asking the game server to delete it…');

  let answer;
  try {
    answer = await fetch(`/markers/${encodeURIComponent(editing.Key)}`, { method: 'DELETE' });
  } catch (error) {
    return refused('The map service is not answering.');
  }
  if (!answer.ok) {
    return refused(await answer.text().catch(() => 'That was refused.'));
  }

  awaiting = editing.Key;
  removing = true;
  changedShape = null;
  askedAt = Date.now();
  sayHere('Waiting for the game server to delete it…');
}

/** The ask did not even reach the game, so the form is somebody's again. */
function refused(why) {
  sayHere(why, true);
  markerSave.disabled = false;
  markerDrop.disabled = false;
}

/**
 * Writes back the preset this form was opened on.
 *
 * Nothing waits on the game here: a preset is this page's own record of what a
 * marker should start as, so it is kept and done. The list behind is drawn again
 * from what came back, which is how a pattern the service trimmed shows up here
 * rather than only on the next load.
 */
async function updatePreset() {
  const pattern = markerPattern.value.trim();
  if (pattern === '') {
    sayHere('A preset needs something to match. `*` stands for any run of characters.', true);
    return;
  }

  const presets = (mine.Presets || []).slice();
  const making = editingPreset < 0;
  if (!making && !presets[editingPreset]) {
    sayHere('That preset is no longer there.', true);
    return;
  }

  const kept = {
    Pattern: pattern,
    Title: markerName.value.trim(),
    Icon: chosenPicture,
    Color: chosenColour,
    Private: privately,
  };
  if (making) presets.unshift(kept);
  else presets[editingPreset] = kept;

  markerSave.disabled = true;
  sayHere('Keeping…');
  if (await keepMine({ ...mine, Presets: presets })) {
    // A new one is now first in the list, and this window is what is editing it,
    // so the two agree about which row is open rather than the form quietly
    // being about nothing.
    if (making) {
      editingPreset = 0;
      composeTitle.textContent = 'Edit preset';
      markerSave.textContent = 'Update';
    }
    drawPresets();
    sayHere(making ? 'Created.' : 'Updated.');
  } else {
    sayHere('The map service is not answering.', true);
  }
  markerSave.disabled = false;
}

/** What an edit asked for, so its arrival can be told from the old marker. */
let changedShape = null;

/**
 * A marker the page already has, in the words the service reads, with whatever
 * is being changed about it written over the top.
 *
 * The form builds its own from what was typed into it; this builds one from a
 * marker that already exists, for the changes that are made without opening the
 * form. Both send every field: serde fills a field it was not sent with that
 * field's default, so a marker edited by a body that left out its colour comes
 * back white, and nothing at either end says a word about it.
 */
function markerFrom(place, changes) {
  const asked = {
    Title: place.Title || UNNAMED,
    Icon: place.Icon || 'circle',
    Color: colourOf(place.Color),
    X: place.X,
    Y: place.Y,
    Z: place.Z,
    Private: Boolean(place.Private),
  };
  return { ...asked, ...changes };
}

/**
 * Asks the game server to change who may see a marker.
 *
 * Nothing waits here. The form waits because somebody is sitting in front of it
 * with a half-filled window open; a switch in a list is one click among several,
 * and the marker arriving changed on the next poll is the answer. What comes back
 * is whether the service took the ask, which is the part that can fail on this
 * side of the game.
 */
async function askPrivacy(place, hidden) {
  try {
    const answer = await fetch(`/markers/${encodeURIComponent(place.Key)}`, {
      method: 'PUT',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(markerFrom(place, { Private: hidden })),
    });
    return answer.ok;
  } catch (error) {
    return false;
  }
}

/** Keeps what this marker is, against whatever was clicked to place it. */
async function rememberPreset(marker) {
  // What was typed, or failing that what was clicked. Nothing to key it on is
  // not worth stopping a marker over — the marker is the point and the preset is
  // the extra.
  const pattern = markerPattern.value.trim() || widened(clicked && clicked.code);
  if (pattern === '') return;

  const kept = {
    Pattern: pattern,
    Title: marker.Title,
    Icon: marker.Icon,
    Color: marker.Color,
    Private: marker.Private,
  };

  const presets = (mine.Presets || []).filter(preset => preset.Pattern !== pattern);
  presets.unshift(kept);
  await keepMine({ ...mine, Presets: presets });
}

/** The marker asked for has arrived, so the form has nothing left to do. */
function landed() {
  // A marker that was handed over closed the form when it was handed over, and
  // by now somebody may have opened it again on the next one. Only a form still
  // standing on what it asked for — which is what a deletion leaves — is a form
  // this has any business closing.
  const handed = handedOver !== null;
  awaiting = null;
  removing = false;
  changedShape = null;
  handedOver = null;
  markerSave.disabled = false;
  markerDrop.disabled = false;
  if (!handed) closeCompose();
}

/** It has not arrived, and the service says whether anything is collecting. */
async function lost() {
  const going = removing;
  // Named, because the form it was asked from is closed by now and may have been
  // opened again on the next marker — "the marker" is only unambiguous while the
  // one it means is the one on screen.
  const named = handedOver && handedOver.marker.Title !== UNNAMED
    ? `“${handedOver.marker.Title}”`
    : 'The marker';
  awaiting = null;
  removing = false;
  changedShape = null;
  markerSave.disabled = false;
  markerDrop.disabled = false;
  putTheFormBack();
  await pollMe();
  if (viewer && viewer.Waiting > 0) {
    sayHere('The game server has not collected it. Is it running?', true);
    return;
  }
  sayHere(going
    ? `${named} was taken but is still there. Try again.`
    : `${named} was taken but has not appeared. Try again.`, true);
}

/** Whether what this page is waiting on has happened yet. */
function arrived(waypoints) {
  const found = waypoints.find(place => place.Key === awaiting);
  // A marker asked to be taken away has arrived when it stops arriving. The mod
  // decides for itself whether the person asking may, so a marker that is still
  // here after the wait is a refusal rather than a slow game server.
  if (removing) return !found;
  if (!found) return false;
  if (!changedShape) return true;

  // An edit comes back under the name it already had, so its arrival is the
  // moment the marker reads as what was asked for rather than as what it was.
  return JSON.stringify({
    Title: found.Title,
    Icon: found.Icon,
    Color: found.Color,
    X: found.X,
    Y: found.Y,
    Z: found.Z,
    Private: found.Private,
  }) === changedShape;
}

/**
 * Whether this person may change this marker.
 *
 * Its owner always may; anybody else only where the operator has said public
 * markers are everyone's to correct, and only for one that is in fact public.
 * The mod decides this again against the waypoint itself before anything moves —
 * what is worked out here only decides whether an edit is offered, because what
 * this page knows about who owns what came from a post that may be seconds old.
 */
function mayEdit(place) {
  if (!viewer || !viewer.Uid) return false;
  if (place.OwnerUid === viewer.Uid) return true;
  return Boolean(viewer.PublicMarkersEditable) && !place.Private;
}
