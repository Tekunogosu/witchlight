// Changing many markers at once.
//
// The marker list already does one thing to a screenful: who may see them. That
// works without choosing anything, because "every marker on this screen" is a
// set the reader can already see and the button says how many it is.
//
// Deleting and re-dressing are not like that. There is no way back from a
// deletion and a preset rewrites a marker's name, so those act on markers that
// were picked one at a time — which is what bulk edit is: a column of boxes down
// the left of the list, and a row of things to do to whatever is ticked.

/**
 * Whether the list is in bulk edit.
 *
 * The list's own mode rather than a window of its own: what is being chosen from
 * is the list, and a second window showing the same rows would be a second place
 * for the search, the tabs and the order to be answered.
 */
let bulking = false;

/**
 * Which markers are ticked, by key.
 *
 * Keys rather than the markers themselves, because the list is redrawn from a
 * fresh set every time the game says anything and a held object would be the
 * marker as it was two seconds ago. A key that stops arriving is dropped when
 * the list is next drawn — see `pruneTicked`.
 */
const ticked = new Set();

const applyPanel = document.getElementById('apply');
const applyFind = document.getElementById('apply-find');
const applyList = document.getElementById('apply-list');
const applyWhat = document.getElementById('apply-what');
const applySaid = document.getElementById('apply-said');
const applySave = document.getElementById('apply-save');
const bulkButton = document.getElementById('marker-bulk');
const bulkDropButton = document.getElementById('marker-bulk-drop');
const fromPresetButton = document.getElementById('marker-from-preset');
const tickAll = document.getElementById('marker-tick-all');

/**
 * The markers on the screen right now.
 *
 * What the tabs and the search have left, which is what every button under the
 * list promises to act on. Written once because four things ask it and a button
 * that touched a different set from the one it counted would be a button nobody
 * could trust.
 */
function listedNow() {
  return listed.filter(place =>
    listings[listing].holds(place)
    && looksLike(markerFind.value, place.Title, place.Owner));
}

/**
 * The markers a bulk action acts on.
 *
 * What is ticked while the list is in bulk edit, and what is listed the rest of
 * the time. One rule, because the row of buttons under the list is one row and a
 * reader must not have to remember which of them means which set. What is ticked
 * is still narrowed to what is on the screen: a tick survives a change of tab,
 * and a button under a list of three that quietly changes forty is exactly what
 * the count on it exists to prevent.
 */
function chosenMarkers() {
  const shown = listedNow();
  return bulking ? shown.filter(place => ticked.has(place.Key)) : shown;
}

/** Forgets ticks on markers that are no longer on the map at all. A tick is
 *  about a marker, and a marker somebody deleted has none to be about. */
function pruneTicked() {
  const alive = new Set(listed.map(place => place.Key));
  for (const key of [...ticked]) {
    if (!alive.has(key)) ticked.delete(key);
  }
}

/**
 * One row's box, or nothing while the list is not in bulk edit.
 *
 * Nothing rather than a hidden box: the heading and the rows are one grid and
 * agree on their columns by sharing a template, so a cell that is present but
 * not drawn would leave every row a column out of step with its own heading.
 */
function tickCell(place) {
  if (!bulking) return null;

  const column = document.createElement('span');
  column.className = 'tick';
  const box = document.createElement('input');
  box.type = 'checkbox';
  box.checked = ticked.has(place.Key);
  // One of a column of identical boxes, so the marker it stands for is the only
  // thing telling a reader — or a test — which one this is.
  box.setAttribute('aria-label', `Choose ${place.Title || 'marker'}`);
  box.addEventListener('click', event => event.stopPropagation());
  box.addEventListener('change', () => {
    if (box.checked) ticked.add(place.Key);
    else ticked.delete(place.Key);
    showFooter();
  });
  column.append(box);
  return column;
}

/**
 * The heading box, which says what the column under it adds up to.
 *
 * Three states rather than two: all of them, none of them, and some — because a
 * box that showed "some" as "none" would untick nothing on its first press and
 * read as broken.
 */
function showTickAll() {
  const shown = listedNow();
  const many = shown.filter(place => ticked.has(place.Key)).length;
  tickAll.disabled = shown.length === 0;
  tickAll.checked = many > 0 && many === shown.length;
  tickAll.indeterminate = many > 0 && many < shown.length;
}

/** Ticks or unticks every marker on the screen, which is what a heading over a
 *  column of boxes means. */
function tickEverything(on) {
  for (const place of listedNow()) {
    if (on) ticked.add(place.Key);
    else ticked.delete(place.Key);
  }
  drawDirectory();
}

/** Turns bulk edit on or off. Turning it off forgets what was ticked: a set
 *  chosen for something that is no longer being done is a set nobody meant. */
function setBulking(on) {
  bulking = on;
  if (!on) ticked.clear();
  armed = null;
  sayDirectory('');
  directory.classList.toggle('bulk', on);
  bulkButton.setAttribute('aria-pressed', String(on));
  drawDirectory();
}

/**
 * Says what everything under the list would do.
 *
 * Both rows, because in bulk edit both read the same set: a tick changes what
 * "make all private" would touch exactly as much as it changes what the bin
 * would. Said in one place, or a tick moves half the buttons and the other half
 * go on promising a number that is no longer true.
 */
function showFooter() {
  showBulk();
  showBulkRow();
}

/**
 * Says what the bulk row would do, or takes each button out of use.
 *
 * The count is on the button for the reason the two above it carry one: what is
 * about to happen is a promise about a number, and the number is the part
 * somebody checks before pressing.
 */
function showBulkRow() {
  const shown = bulking;
  fromPresetButton.style.display = shown ? '' : 'none';
  bulkDropButton.style.display = shown ? '' : 'none';
  showTickAll();
  if (!shown) return;

  const many = chosenMarkers().length;
  const own = deletable().length;
  fromPresetButton.disabled = many === 0;
  fromPresetButton.textContent = many === 0 ? 'From preset' : `From preset (${many})`;

  bulkDropButton.disabled = own === 0;
  const words = own === 0
    ? 'Nothing ticked that is yours to delete'
    : armed === 'delete' ? `Press again to delete ${own}` : `Delete ${own}`;
  bulkDropButton.classList.toggle('armed', armed === 'delete' && own > 0);
  bulkDropButton.title = words;
  bulkDropButton.setAttribute('aria-label', words);
  bulkDropButton.setAttribute('aria-pressed', String(armed === 'delete'));
}

/**
 * The ticked markers this person may take away.
 *
 * Only their own, whatever the operator has said about correcting public
 * markers: that setting lets somebody fix a marker they can see, which is not
 * the same permission as taking it off its owner's map. The mod decides it again
 * against the waypoint, and this decides what to offer.
 */
function deletable() {
  return chosenMarkers().filter(place =>
    viewer && viewer.Uid && place.OwnerUid === viewer.Uid);
}

/**
 * Deletes what is ticked, once it has been asked twice.
 *
 * The second press is on the same mark, which is the rule the bulk privacy
 * buttons and the marker form's own bin both follow. Each marker is asked for on
 * its own because that is what the service offers, and they go together rather
 * than one after the next.
 */
async function dropTicked() {
  const going = deletable();
  if (going.length === 0) return;

  if (armed !== 'delete') {
    armed = 'delete';
    showBulkRow();
    sayDirectory(`Press the bin again to delete ${going.length}.`);
    return;
  }

  armed = null;
  showBulkRow();
  sayDirectory(`Asking the game server to delete ${going.length}…`);
  const took = await Promise.all(going.map(place =>
    started(askDelete(place), 'deleting a marker')));
  const failed = took.filter(ok => !ok).length;
  sayDirectory(failed === 0
    ? `Asked for ${took.length} to be deleted.`
    : `${took.length - failed} asked for; ${failed} refused.`, failed > 0);
}

/**
 * Asks the game server to take one marker away.
 *
 * Nothing waits here. The form waits because somebody is sitting in front of a
 * half-filled window; a bin under a list is one press over a set, and the
 * markers ceasing to arrive is the answer. What comes back is whether the
 * service took the ask, which is the part that can fail on this side.
 */
async function askDelete(place) {
  try {
    const answer = await fetch(`/markers/${encodeURIComponent(place.Key)}`, {
      method: 'DELETE',
    });
    return answer.ok;
  } catch (error) {
    return false;
  }
}

/** Which preset the window is on, by its place in what the service holds. */
let applying = -1;

/**
 * Opens the window that says what a screenful of markers should look like.
 *
 * A window of its own rather than the presets list. That one is where a preset
 * is made, changed and deleted, and every row in it does something the moment it
 * is pressed; this one picks exactly one, holds it, and is answered by Update or
 * Cancel — which is the shape a change to forty markers has to have.
 */
function openApply() {
  const many = chosenMarkers().length;
  if (many === 0) return;

  applying = -1;
  applyFind.value = '';
  applyWhat.textContent =
    `Make ${many} marker${many === 1 ? '' : 's'} look like one of your presets.`;
  sayApply('');
  drawApplyList();
  openWindow(applyPanel, true);
  besideWindow(applyPanel);
  applyFind.focus();
}

/** The presets, as a list to pick exactly one from. */
function drawApplyList() {
  applyList.textContent = '';
  const held = mine.Presets || [];
  let drawn = 0;

  held.forEach((preset, which) => {
    if (!looksLike(applyFind.value, preset.Title, preset.Pattern)) return;

    const { line, open } = listedRow(
      preset.Icon, preset.Color, preset.Title || '(unnamed)', preset.Pattern || '',
      drawn % 2 === 1);
    drawn += 1;
    line.classList.toggle('chosen', which === applying);
    open.setAttribute('role', 'option');
    open.setAttribute('aria-selected', String(which === applying));
    open.setAttribute('aria-label', `Use preset ${preset.Title || preset.Pattern}`);
    open.addEventListener('click', () => {
      applying = which;
      drawApplyList();
      showApply();
    });
    applyList.append(line);
  });

  showApply();
  if (drawn > 0) return;
  nothingFound(applyList, held.length === 0
    ? 'No presets yet. Tick "set as preset" when you save a marker.'
    : `None of your ${held.length} presets matches that.`);
}

/** Nothing to apply until one is picked, and the button says so rather than
 *  looking ready and doing nothing. */
function showApply() {
  applySave.disabled = applying < 0 || !(mine.Presets || [])[applying];
}

function sayApply(what, wrong) {
  applySaid.textContent = what || '';
  applySaid.classList.toggle('wrong', Boolean(wrong));
}

/**
 * Makes every ticked marker look like the preset that was picked.
 *
 * The name exactly, the picture, the colour, and the block the marker is about —
 * which is the preset's pattern, since what a preset says is "markers of this
 * kind belong to this block". Who may see each one is left alone: that is a
 * choice about one marker rather than a property of the kind of thing it is, and
 * a preset applied to forty markers must not quietly publish somebody's base.
 *
 * Each is asked for on its own, because that is what the service offers and what
 * the mod enforces per marker. They go together rather than one after the next.
 */
async function applyToTicked() {
  const preset = (mine.Presets || [])[applying];
  const changing = chosenMarkers().filter(mayEdit);
  if (!preset) return;
  if (changing.length === 0) {
    sayApply('None of those is yours to change.', true);
    return;
  }

  applySave.disabled = true;
  sayApply(`Asking the game server about ${changing.length}…`);
  const took = await Promise.all(changing.map(place =>
    started(askShape(place, preset), 'making a marker look like a preset')));
  const failed = took.filter(ok => !ok).length;
  applySave.disabled = false;

  if (failed === 0) {
    shutWindow(applyPanel);
    sayDirectory(`Asked for ${took.length} to be made like ${preset.Title || preset.Pattern}.`);
    return;
  }
  sayApply(`${took.length - failed} asked for; ${failed} refused.`, true);
}

/** Asks for one marker to be made what the preset says it is. */
async function askShape(place, preset) {
  try {
    const answer = await fetch(`/markers/${encodeURIComponent(place.Key)}`, {
      method: 'PUT',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(markerFrom(place, {
        Title: preset.Title || UNNAMED,
        Icon: preset.Icon || place.Icon,
        Color: colourOf(preset.Color || place.Color),
        Block: preset.Pattern || '',
      })),
    });
    return answer.ok;
  } catch (error) {
    return false;
  }
}

function buildBulk() {
  bulkButton.addEventListener('click', () => setBulking(!bulking));
  tickAll.addEventListener('change', () => tickEverything(tickAll.checked));
  bulkDropButton.addEventListener('click', () =>
    started(dropTicked(), 'deleting the ticked markers'));
  fromPresetButton.addEventListener('click', openApply);
  document.getElementById('apply-cancel').addEventListener('click', () =>
    shutWindow(applyPanel));
  applySave.addEventListener('click', () =>
    started(applyToTicked(), 'making the ticked markers look like a preset'));
  findingIn(applyFind, drawApplyList);
}
