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
const wantFollow = document.getElementById('want-follow');
const wantSaid = document.getElementById('want-said');

/** What was last read, so a beat that says nothing new redraws nothing. */
let mineShape = '';

/**
 * Reads what this person has set, and draws again where it has changed.
 *
 * Read on a beat rather than once at login, because this is not only the page's
 * to change: a preset can be made from in game, and the mod keeps it against the
 * same person the map's own form writes to. A page that read it once had a
 * preset list as old as the session, and a preset made in front of somebody
 * standing on the block it names did not appear on the map they had open.
 *
 * Slower than the live poll. Markers change a few times an hour and these change
 * a few times a day, so this is a few hundred bytes every fifteen seconds
 * against a marker list every two.
 */
async function watchMine() {
  await pollMine();
  const shape = JSON.stringify(mine);
  if (shape === mineShape) return;
  mineShape = shape;

  // Everything drawn from what this person has set. The windows that are shut
  // are drawn again when they open, so only the ones on the screen matter — and
  // each of these draws nothing where its own window is not up.
  drawProfile();
  drawPresets();
  if (applyPanel.classList.contains('open')) drawApplyList();
}

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
  wantFollow.checked = Boolean(mine.FollowSelf);
  wantPresets.disabled = !named;
  wantPrivate.disabled = !named;
  wantFollow.disabled = !named;
  wantSaid.textContent = named
    ? (mine.PrivateByDefault === true || mine.PrivateByDefault === false
      ? 'Your own choice, over the server default.'
      : `Following the server default, which is ${viewer && viewer.MarkersPublic ? 'public' : 'private'}.`)
    : 'Run /witchlight login in the game to keep settings.';

  for (const [part, scale] of Object.entries(scales)) {
    const slider = document.getElementById(`scale-${part}`);
    if (slider) slider.value = String(scale.at);
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
  draft = {
    presets: wantPresets.checked,
    private: wantPrivate.checked,
    follow: wantFollow.checked,
  };
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
    FollowSelf: draft.follow,
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

  for (const box of [wantPresets, wantPrivate, wantFollow]) {
    box.addEventListener('change', () => {
      draftProfile();
      sayProfile('Not kept yet.');
    });
  }

}

/**
 * Takes up following this person's own player, where they asked the map to.
 *
 * Once, when the map first learns who is looking, and never again: following is
 * a standing instruction that dragging the map ends, and a page that took it up
 * again on the next poll would be a map somebody could not look away from.
 *
 * Their player need not be online. Nothing is panned until one turns up — see
 * `keepUp`, which is what actually moves the view — so somebody who opens the map
 * before joining the server has the map land on them when they do.
 */
function followSelf() {
  if (!(viewer && viewer.Uid) || !mine.FollowSelf) return;
  // Lights their card as well as moving the map, which is the whole of what
  // clicking that card does — a map following somebody with nothing saying so is
  // a map that has quietly stopped answering the pointer. `drawWho` applies it
  // to a card that does not exist yet, which is the usual case here.
  keeping(String(viewer.Uid));
  keepUp();
}
