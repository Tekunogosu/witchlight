// Every marker there is, as a list.
//
// The map answers where a marker is. It cannot answer what there is, because the
// answer is spread over a million blocks and half of it is off the screen — so a
// marker somebody made last week is found by remembering roughly where they were
// standing, which is not finding it at all.
//
// The same set, read down instead of looked at: split by who can see it, searched
// by name, and every row a way into the window that changes it.

const directory = document.getElementById('directory');
const markerFind = document.getElementById('marker-find');

/**
 * The markers the service last sent.
 *
 * Kept rather than asked for again. `/live.json` already carries them every two
 * seconds and has already decided which of them this person may see, so a window
 * with a question of its own would be a second answer to a question the page has
 * asked and had answered.
 */
let listed = [];

/**
 * The three lists this window can show, and what each of them holds.
 *
 * Written as a table rather than as a condition per tab, so a tab is an entry
 * and the counts, the words a tab says when it is empty, and what it draws all
 * come from the one place. `all` is not the other two added together — it is the
 * same set unsorted, which is what somebody looking for a marker they cannot
 * remember the sharing of wants.
 */
const listings = {
  all: { label: 'all', holds: () => true },
  public: { label: 'public', holds: place => !place.Private },
  private: { label: 'private', holds: place => Boolean(place.Private) },
};

/**
 * Which tab is showing.
 *
 * All to begin with, because the first question anybody brings to a list of
 * markers is which markers there are; who else can see one is the second.
 */
let listing = 'all';

/**
 * What the list can be put in order by.
 *
 * It was already in an order and did not say so, which is the worst of both:
 * the service sends the markers everybody can see and then the ones only you
 * can, so making a marker private moved its row to the bottom of the list. That
 * reads as the list rearranging itself for no reason, because from the outside
 * that is exactly what it did.
 *
 * So the order is this window's own, it is written at the top of the list, and
 * it is somebody's to change. Nothing about a marker changing can move a row now
 * except the thing the list is actually sorted by.
 *
 * Distance is measured from spawn rather than from the middle of the view. Spawn
 * is what the coordinates on every row are counted from, and an order that
 * changed every time the map was dragged would be the same complaint again.
 */
const sorts = {
  name: { of: place => (place.Title || '').toLowerCase() },
  away: { of: place => Math.hypot(place.X - spawn.x, place.Z - spawn.z) },
  owner: { of: place => (place.Owner || '').toLowerCase() },
  private: { of: place => (place.Private ? 1 : 0) },
};

/** Which of them the list is in, and which way round. */
let sorting = { by: 'name', down: false };

/** Two values in the order they belong, whichever kind they are. */
function ranks(one, two) {
  if (one < two) return -1;
  if (one > two) return 1;
  return 0;
}

/**
 * The markers in the order the list is set to.
 *
 * Broken by name and then by key, so the order is total: two markers the chosen
 * column cannot tell apart still have one answer between them, and a row cannot
 * swap places with its neighbour on a poll that changed nothing.
 */
function inOrder(rows) {
  const of = sorts[sorting.by].of;
  const way = sorting.down ? -1 : 1;
  return rows.slice().sort((one, two) =>
    way * ranks(of(one), of(two))
    || ranks(sorts.name.of(one), sorts.name.of(two))
    || ranks(one.Key, two.Key));
}

/** Puts the list in a different order, or the same one the other way round. */
function chooseSort(by) {
  if (!sorts[by]) return;
  sorting = by === sorting.by ? { by, down: !sorting.down } : { by, down: false };
  remember();
  drawDirectory();
}

/** Says which column the list is in the order of, for the eye and the reader. */
function showSort() {
  for (const button of directory.querySelectorAll('.sort')) {
    const on = button.dataset.sort === sorting.by;
    button.classList.toggle('on', on);
    button.classList.toggle('down', on && sorting.down);
    button.setAttribute('aria-sort', on ? (sorting.down ? 'descending' : 'ascending') : 'none');
  }
}

/**
 * Every marker there is, on the tab it belongs to.
 *
 * Sorted into tabs rather than badged row by row: who else can see a marker is
 * the thing somebody opens this window to check, and a column that has to be
 * read down does not answer that. Each tab counts what it holds, so the split is
 * legible without opening all three.
 */
function drawDirectory() {
  const list = document.getElementById('marker-list');
  list.textContent = '';

  for (const tab of directory.querySelectorAll('.tab')) {
    const holds = listings[tab.dataset.tab].holds;
    tab.querySelector('.tally').textContent = listed.filter(holds).length;
  }

  const kind = listings[listing];
  const held = listed.filter(kind.holds);
  let drawn = 0;

  for (const place of inOrder(held)) {
    if (!looksLike(markerFind.value, place.Title, place.Owner)) continue;
    list.append(markerRow(place, drawn % 2 === 1));
    drawn += 1;
  }

  showSort();
  showBulk();

  if (drawn > 0) return;
  nothingFound(list, held.length === 0
    ? `No ${kind.label} markers on the map.`
    : `None of the ${held.length} ${kind.label} markers matches that.`);
}

/**
 * One marker as a row.
 *
 * Clicking it always puts the map on the marker, because a list of places whose
 * rows do not go anywhere is a list that has to be cross-referenced by hand. Where
 * this person may change the marker it opens the form on it as well, beside the
 * list, so the row and what it edits are both on the screen at once.
 *
 * Somebody else's marker is still worth having a row: it is on the map, it is in
 * the way, and knowing whose it is answers most of what anybody wants from it.
 * What the row will not do is offer an edit the mod would refuse.
 */
function markerRow(place, shaded) {
  const ours = mayEdit(place);
  const title = place.Title || 'marker';
  const [x, z] = said(place.X, place.Z);

  const where = place.Owner
    ? `${x}, ${place.Y}, ${z} · ${place.Owner}`
    : `${x}, ${place.Y}, ${z}`;
  const { line, open } = listedRow(place.Icon, place.Color, title, where, shaded);
  if (mode === 'marker' && editing && editing.Key === place.Key) line.classList.add('chosen');
  const lock = lockFor(place, ours);
  // One of a list of identical rows, so the marker it stands for is the only
  // thing telling a reader — or a test — which one this is. What the row will
  // do is in the name too, because it is not the same for every row.
  open.setAttribute('aria-label', ours ? `Edit marker ${title}` : `Show ${title} on the map`);
  open.title = ours ? '' : "Only this marker's owner may change it";
  open.addEventListener('click', () => {
    showOnMap(place);
    if (!ours) return;
    editCompose(place);
    besideWindow(directory);
    drawDirectory();
  });

  line.append(lock);
  return line;
}

/**
 * Who may see this marker, as a switch where it is this person's to decide.
 *
 * A shut lock is a marker its owner keeps and an open one is a marker the server
 * can see, which is one picture saying the state and the same picture saying what
 * a click will undo. Somebody else's marker gets the same mark without the button
 * around it: knowing who else can see a thing on the map is worth having whether
 * or not it is yours to change.
 */
function lockFor(place, ours) {
  const hidden = Boolean(place.Private);
  const mark = chromeMark(hidden ? 'lock-simple' : 'lock-simple-open');
  const title = place.Title || 'marker';

  if (!ours) {
    const shown = document.createElement('span');
    shown.className = 'lock';
    shown.title = hidden ? 'Private to its owner' : 'Everyone can see this';
    shown.append(mark);
    return shown;
  }

  const flip = document.createElement('button');
  flip.type = 'button';
  flip.className = 'lock' + (hidden ? ' shut' : '');
  flip.title = hidden ? 'Make this public' : 'Make this private';
  // One of a list of identical marks, so the marker it stands for is the only
  // thing telling a reader — or a test — which one this is.
  flip.setAttribute('aria-label', `${hidden ? 'Make public' : 'Make private'}: ${title}`);
  flip.setAttribute('aria-pressed', String(hidden));
  flip.append(mark);
  flip.addEventListener('click', () => started(onePrivacy(place, !hidden), 'changing who sees a marker'));
  return flip;
}

/** Flips one marker, and says what the game server made of it. */
async function onePrivacy(place, hidden) {
  sayDirectory('Asking the game server…');
  const took = await askPrivacy(place, hidden);
  sayDirectory(took
    ? `Asked for ${place.Title || 'the marker'} to be ${hidden ? 'private' : 'public'}.`
    : 'That was refused.', !took);
}

function sayDirectory(what, wrong) {
  const note = document.getElementById('marker-said');
  note.textContent = what || '';
  note.classList.toggle('wrong', Boolean(wrong));
}

/**
 * Puts the map on a marker without changing how close it is looking.
 *
 * Following a player is a standing instruction to keep the view on them, so it
 * has to end here or the map walks straight back off the marker that was asked
 * for — which is the same rule dragging the map already follows.
 */
function showOnMap(place) {
  if (following !== null) follow(following);
  map.panTo(at(place.X, place.Z), { animate: true, duration: 0.4 });
}

/**
 * The markers a bulk change would touch: the ones on the screen that this person
 * may change and that are not already the way they would be put.
 *
 * What is on the screen rather than what is on the tab. Somebody who has typed
 * into the search box has narrowed what they are looking at, and a button under a
 * list of three that quietly changes forty is a button nobody can trust.
 */
function wouldFlip(hidden) {
  return listed.filter(place =>
    listings[listing].holds(place)
    && looksLike(markerFind.value, place.Title, place.Owner)
    && mayEdit(place)
    && Boolean(place.Private) !== hidden);
}

/**
 * Says what each bulk button would do, or takes it out of use.
 *
 * The count is on the button because "make all private" over a filtered list is
 * a promise about a number, and the number is the part somebody checks. Nothing
 * to do is a button that says so rather than one that looks ready and does
 * nothing when pressed.
 */
function showBulk() {
  for (const [button, hidden, verb] of bulkButtons()) {
    const many = wouldFlip(hidden).length;
    button.disabled = many === 0;
    button.textContent = many === 0 ? `No markers to make ${verb}` : `Make ${many} ${verb}`;
    button.classList.toggle('armed', armed === verb && many > 0);
    if (armed === verb && many > 0) button.textContent = `Really — ${many} ${verb}?`;
  }
}

/** The two bulk buttons, each with the state it puts markers into. */
function bulkButtons() {
  return [
    [document.getElementById('marker-hide'), true, 'private'],
    [document.getElementById('marker-show'), false, 'public'],
  ];
}

/**
 * Which bulk change has been asked for once and not yet confirmed.
 *
 * Two presses rather than one. Making a marker public shows somebody's base to
 * the server, which is not a thing to do because a pointer was in the wrong place
 * — and making a screenful private at once is the same size of surprise from the
 * other direction. The second press is on the same button, so the confirming is
 * where the asking was.
 */
let armed = null;

/** Puts the bulk buttons back to asking rather than confirming. */
function disarm() {
  armed = null;
  showBulk();
}

/**
 * Changes who may see every marker on the screen, once it has been asked twice.
 *
 * Each marker is asked for on its own, because that is what the service offers
 * and what the mod enforces per marker. They go together rather than one after
 * the next: forty markers at a round trip each is forty round trips, and they do
 * not depend on one another.
 */
async function allPrivacy(hidden, verb) {
  const flipping = wouldFlip(hidden);
  if (flipping.length === 0) return;

  if (armed !== verb) {
    armed = verb;
    showBulk();
    sayDirectory(`Press again to make ${flipping.length} ${verb}.`);
    return;
  }

  disarm();
  sayDirectory(`Asking the game server about ${flipping.length}…`);
  const took = await Promise.all(flipping.map(place =>
    started(askPrivacy(place, hidden), 'changing who sees a marker')));
  const failed = took.filter(ok => !ok).length;
  sayDirectory(failed === 0
    ? `Asked for ${took.length} to be made ${verb}.`
    : `${took.length - failed} asked for; ${failed} refused.`, failed > 0);
}

/** Shows one of the lists, and says which for the eye and for the reader. */
function chooseTab(which) {
  if (!listings[which]) return;
  listing = which;
  // A confirmation is about the markers that were on the screen when it was
  // asked for, and those are no longer the markers on the screen.
  armed = null;
  sayDirectory('');
  for (const tab of directory.querySelectorAll('.tab')) {
    const on = tab.dataset.tab === listing;
    tab.classList.toggle('on', on);
    tab.setAttribute('aria-selected', String(on));
  }
  drawDirectory();
}

function buildDirectory() {
  directoryButton.addEventListener('click', () => {
    if (directory.classList.contains('open')) shutWindow(directory);
    else {
      drawDirectory();
      openWindow(directory, true);
    }
  });

  for (const tab of directory.querySelectorAll('.tab')) {
    tab.addEventListener('click', () => chooseTab(tab.dataset.tab));
  }
  for (const [button, hidden, verb] of bulkButtons()) {
    button.addEventListener('click', () =>
      started(allPrivacy(hidden, verb), `making the listed markers ${verb}`));
  }
  for (const button of directory.querySelectorAll('.sort')) {
    button.addEventListener('click', () => chooseSort(button.dataset.sort));
  }
  // Typing narrows what a bulk button would touch, so a confirmation given for
  // one list must not be spent on another.
  findingIn(markerFind, () => { armed = null; drawDirectory(); });
  chooseTab(listing);
}
