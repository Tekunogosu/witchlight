// Who you are, what you have told the map about yourself, and the size of the
// furniture.
//
// Your presets and defaults follow a uid and are kept by the service; sizes are
// about the screen in front of somebody and stay in this browser. Two stores
// because they answer to two different questions, not because they arrived
// separately. The presets themselves are a window of their own — this file owns
// the reading and writing that window and the marker form both go through.

/**
 * Who you are, and what you have told the map about yourself.
 *
 * Two kinds of thing sit in one window because there is one question behind
 * them — how this person wants the map to behave — but they are kept in
 * different places. Where a new marker starts, and every preset, are about the
 * person: they go to the service against their uid and follow them to any
 * browser. How large the page draws itself is about the screen in front of them,
 * and stays here.
 */

const profile = document.getElementById('profile');
const wantPresets = document.getElementById('want-presets');
const wantPrivate = document.getElementById('want-private');
const wantSaid = document.getElementById('want-said');

/** Reads back what this person has set. Nobody signed in has set nothing. */
async function pollMine() {
  if (!(viewer && viewer.Name)) {
    mine = { Presets: [], PresetsByDefault: false };
    return;
  }
  try {
    const held = await (await fetch('/me/preferences.json', { cache: 'no-store' })).json();
    mine = held && typeof held === 'object' ? held : mine;
  } catch (error) {
    /* the service may be restarting; what is held stands until it answers */
  }
}

/**
 * Keeps what this person has set, and takes back what the service made of it.
 *
 * The whole document goes each time. It is a handful of presets and two
 * switches, so sending all of it costs nothing and there is no merge to get
 * wrong; what comes back is what was actually stored, which is how a pattern
 * trimmed or a preset dropped shows up here rather than only on the next load.
 */
async function keepMine(asked) {
  const before = mine;
  mine = asked;
  try {
    const answer = await fetch('/me/preferences', {
      method: 'PUT',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(asked),
    });
    if (!answer.ok) throw new Error(answer.status);
    mine = await answer.json();
    return true;
  } catch (error) {
    mine = before;
    return false;
  }
}

/** Draws the window on whoever is looking. */
function drawProfile() {
  const named = viewer && viewer.Name;
  document.getElementById('profile-name').textContent = named || 'Not signed in';
  document.getElementById('sign-out').style.display = named ? '' : 'none';

  const face = document.getElementById('profile-face');
  face.textContent = '';
  // The picture the game drew of them, where they are online and have sent one.
  // Nothing else here knows how a portrait is named for somebody who is not —
  // the mod decides that, and it says so only about players who are on.
  const them = named && players.find(player => player.Uid === viewer.Uid);
  const source = them ? portraitSrc(them) : '';
  if (source) {
    const picture = document.createElement('img');
    picture.src = source;
    picture.alt = '';
    face.append(picture);
  } else {
    // The same mark the account button wears, for the same reason: nobody has
    // sent a picture, and a letter or an emoji in its place reads as a different
    // kind of thing rather than as an absent one.
    face.append(chromeMark('user'));
  }

  wantPresets.checked = Boolean(mine.PresetsByDefault);
  wantPrivate.checked = privateByDefault();
  wantPresets.disabled = !named;
  wantPrivate.disabled = !named;
  wantSaid.textContent = named
    ? (mine.PrivateByDefault === true || mine.PrivateByDefault === false
      ? 'Your own choice, over the server default.'
      : `Following the server default, which is ${viewer && viewer.MarkersPublic ? 'public' : 'private'}.`)
    : 'Run /witchlight login in the game to keep settings.';

  for (const part of Object.keys(scales)) {
    document.getElementById(`scale-${part}`).value = String(scales[part]);
  }
  draftProfile();
}

/**
 * What the profile window is holding but has not kept.
 *
 * The switches wait for Save. The sliders do not — a size you cannot see is a
 * size you cannot choose — but they land on release rather than as they move: a
 * slider that applies on every step rescales the window the slider is in, which
 * walks it out from under the pointer and makes the thing impossible to set.
 */
let draft = null;

/** Reads the switches into a draft, so what is shown is what Save will keep. */
function draftProfile() {
  draft = { presets: wantPresets.checked, private: wantPrivate.checked };
}

/**
 * Takes a size, once the hand has let go of the slider.
 *
 * Kept as it is set rather than waiting for Save, because the whole of what a
 * size slider is for is seeing the answer. Where the window sits is worked out
 * again straight after: it just changed size, and a window grown against the
 * edge of the screen has to be pulled back to stay reachable.
 */
function takeScale(part, size) {
  scales[part] = size;
  applyScales();
  remember();
  const held = windowsAt.get(profile);
  if (held) settleWindow(profile, held.x, held.y);
}

function sayProfile(what, wrong) {
  const note = document.getElementById('profile-said');
  note.textContent = what || '';
  note.classList.toggle('wrong', Boolean(wrong));
}

/** Keeps everything the window is holding: yours on the service, the sizes here. */
async function keepProfile() {
  if (!draft) return;

  if (!(viewer && viewer.Name)) {
    sayProfile('Sign in to keep these.', true);
    return;
  }

  sayProfile('Keeping…');
  const kept = await keepMine({
    ...mine,
    PresetsByDefault: draft.presets,
    PrivateByDefault: draft.private,
  });
  drawProfile();
  sayProfile(kept ? 'Kept.' : 'The map service is not answering.', !kept);
  if (kept) shutWindow(profile);
}

/** Puts the switches back and shuts the window: the mark in the bar, further out
 *  where a hand is already reaching for Save. */
function revertProfile() {
  drawProfile();
  sayProfile('');
  shutWindow(profile);
}

function buildProfile() {
  accountBar.querySelector('a').addEventListener('click', () => {
    if (profile.classList.contains('open')) shutWindow(profile);
    else {
      drawProfile();
      sayProfile('');
      openWindow(profile, true);
    }
  });

  document.getElementById('profile-save')
    .addEventListener('click', () => started(keepProfile(), 'keeping your settings'));
  document.getElementById('profile-revert').addEventListener('click', revertProfile);

  for (const box of [wantPresets, wantPrivate]) {
    box.addEventListener('change', () => {
      draftProfile();
      sayProfile('Not kept yet.');
    });
  }

  for (const part of Object.keys(scales)) {
    const slider = document.getElementById(`scale-${part}`);
    slider.value = String(scales[part]);
    // `change` rather than `input`: a range fires `change` when the hand lets
    // go, which is exactly the moment the window may safely resize under it.
    slider.addEventListener('change', () => takeScale(part, Number(slider.value)));
  }
}
