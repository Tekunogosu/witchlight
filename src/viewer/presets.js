// What a marker starts as: which block each one names, and the list of them.
//
// A preset is this page's own record and lives on the service against a uid, so
// nothing here waits on a game. What it holds is edited in the marker window,
// which already asks every one of those questions — this file is the matching,
// the list, the search over it, and the two things a row can do.

const presetPanel = document.getElementById('presets');
const presetFind = document.getElementById('preset-find');

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

/**
 * The presets, as a list to pick from.
 *
 * Names and patterns and nothing else. What a preset holds is edited in the
 * marker window, which already asks every one of those questions — two sets of
 * pickers for one choice is two places for it to end up looking different.
 *
 * A row keeps the place it has in what the service holds, not the place it has
 * on the screen: the search hides rows and the window that edits one is opened
 * on a number, so filtering by anything but the real index would edit whichever
 * preset happened to be sitting where the one that was clicked is drawn.
 */
function drawPresets() {
  const list = document.getElementById('preset-list');
  list.textContent = '';

  const held = mine.Presets || [];
  let drawn = 0;

  held.forEach((preset, which) => {
    if (!looksLike(presetFind.value, preset.Title, preset.Pattern)) return;

    // Drawn as the marker it makes, so the list reads as what it produces.
    const { line, open } = listedRow(
      preset.Icon, preset.Color, preset.Title || '(unnamed)', preset.Pattern || '',
      drawn % 2 === 1);
    if (mode === 'preset' && which === editingPreset) line.classList.add('chosen');
    drawn += 1;

    // One of a list of identical rows, so the name it stands for is the only
    // thing telling a reader — or a test — which one this is.
    open.setAttribute('aria-label', `Edit preset ${preset.Title || preset.Pattern}`);
    open.addEventListener('click', () => editPreset(which));

    const drop = document.createElement('button');
    drop.type = 'button';
    drop.className = 'shut';
    drop.append(chromeMark('x'));
    drop.title = 'Delete this preset';
    drop.setAttribute('aria-label', `Delete preset ${preset.Title || preset.Pattern}`);
    drop.addEventListener('click', () => started(dropPreset(which), 'deleting the preset'));

    line.append(drop);
    list.append(line);
  });

  // Two different absences, said apart. A list with nothing in it needs telling
  // how to start one; a search that found nothing needs telling that the presets
  // are still there.
  if (drawn > 0) return;
  nothingFound(list, held.length === 0
    ? 'No presets yet. Tick "set as preset" when you save a marker.'
    : `None of your ${held.length} presets matches that.`);
}

/** Takes one preset away, and closes the form if it was the one open. */
async function dropPreset(which) {
  const presets = (mine.Presets || []).slice();
  const gone = presets.splice(which, 1)[0];
  if (!gone) return;

  if (mode === 'preset' && editingPreset === which) closeCompose();
  else if (mode === 'preset' && editingPreset > which) editingPreset--;

  sayPresets('Deleting…');
  if (await keepMine({ ...mine, Presets: presets })) {
    drawPresets();
    sayPresets(`Deleted ${gone.Title || gone.Pattern}.`);
  } else {
    sayPresets('The map service is not answering.', true);
  }
}

function sayPresets(what, wrong) {
  const note = document.getElementById('preset-said');
  note.textContent = what || '';
  note.classList.toggle('wrong', Boolean(wrong));
}

function buildPresets() {
  presetButton.addEventListener('click', () => {
    if (presetPanel.classList.contains('open')) shutWindow(presetPanel);
    else {
      drawPresets();
      sayPresets('');
      openWindow(presetPanel, true);
    }
  });

  document.getElementById('preset-new').addEventListener('click', newPreset);
  findingIn(presetFind, drawPresets);
}
