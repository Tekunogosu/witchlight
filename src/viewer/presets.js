// What a marker starts as: which block each one names, and the list of them.
//
// A preset is this page's own record and lives on the service against a uid, so
// nothing here waits on a game. What it holds is edited in the marker window,
// which already asks every one of those questions — this file is the matching,
// the list, the search over it, and the two things a row can do.

const presetPanel = document.getElementById('presets');
const presetFind = document.getElementById('preset-find');
const presetPick = document.getElementById('preset-pick');
const presetPickButton = document.getElementById('marker-preset-pick');

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

/**
 * The pattern a block code is remembered as, unless somebody says otherwise.
 *
 * Block codes carry their variant as a number — `game:leaves-grown7-oak`,
 * `game:water-still-7`, `game:tallgrass-3` — so a preset kept against one of
 * them answers for exactly one of the eight, or the seven. Keeping a preset for
 * grass meant keeping it again for every stage of grass, which is one preset
 * written down eight times and eight rows to delete when it changes.
 *
 * So the number is where the wildcard goes by default, and `game:leaves-grown*-oak`
 * covers the lot. It is a character in a text field either way: move it, add
 * another, or take it out to name one block exactly. A code with no number in it
 * is its own pattern — there is nothing to widen, and widening it further would
 * be guessing at what somebody meant.
 *
 * The mod does the same thing to the pattern the in-game window opens on, so a
 * preset made from a key press and one made from a right click start out the
 * same — see `BlockPattern.Widened`.
 */
function widened(code) {
  return code ? String(code).replace(/\d+/g, '*') : '';
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

    // Two things a row can do, in the order they read: use this one, or lose it.
    const use = document.createElement('button');
    use.type = 'button';
    use.className = 'use';
    use.append(chromeMark('eyedropper'));
    use.title = 'Put a marker on the map with this preset';
    use.setAttribute(
      'aria-label', `Place a marker with preset ${preset.Title || preset.Pattern}`);
    use.addEventListener('click', () => placeWithPreset(which));

    const drop = document.createElement('button');
    drop.type = 'button';
    drop.className = 'shut';
    drop.append(chromeMark('x'));
    drop.title = 'Delete this preset';
    drop.setAttribute('aria-label', `Delete preset ${preset.Title || preset.Pattern}`);
    drop.addEventListener('click', () => started(dropPreset(which), 'deleting the preset'));

    line.append(use, drop);
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

/**
 * Fills the marker form in from a preset.
 *
 * Everything a preset holds, not only its name: a preset is what a marker starts
 * as, and half of one applied is a marker somebody has to finish by hand anyway.
 * What it says nothing about — where the marker goes — is left exactly as it was.
 *
 * The pattern comes with it so that saving the marker as a preset again writes
 * back to the same one rather than adding a second under the block's own code.
 */
function fillFromPreset(preset) {
  markerName.value = preset.Title || '';
  chosenColour = preset.Color || chosenColour;
  chosenPicture = preset.Icon || chosenPicture;
  privately = preset.Private === true || preset.Private === false
    ? preset.Private
    : privateByDefault();
  if (preset.Pattern) markerPattern.value = preset.Pattern;
  showFields();
  drawColours();
  drawPictures();
}

/**
 * Opens the marker form on a preset and arms the map for where it goes.
 *
 * The way round the presets window was missing. It could make a preset and
 * change one, and the only way to use one was to right-click the block it names
 * and hope — which is no use at all for a preset whose pattern names a block you
 * are not standing on, or for putting a second marker somewhere you already know.
 *
 * Armed rather than placed, because a preset says what a marker is and nothing
 * about where: the one question left is the one the map answers with a click.
 */
function placeWithPreset(which) {
  const preset = (mine.Presets || [])[which];
  if (!preset) return;

  openCompose(null, null);
  fillFromPreset(preset);
  setPlacing(true);
  sayHere('Click the map to put it somewhere.');
}

/**
 * The presets, offered inside the marker form rather than listed beside it.
 *
 * A flyout and not a window: this is a choice made in the middle of filling one
 * form in, handed straight back to it and gone. A window would have to be found,
 * moved out of the way of the form it is feeding, and closed again.
 *
 * Drawn on every open rather than kept, because a preset made a minute ago in
 * the window behind has to be in it.
 *
 * `looking` narrows it to what somebody has typed in the name box, which is the
 * other way in — see `presetSearch`. Empty is everything, which is what the
 * button beside the box asks for.
 */
function drawPresetPick(looking) {
  presetPick.textContent = '';
  const all = mine.Presets || [];
  const held = looking
    ? all.filter(preset => looksLike(looking, preset.Title, preset.Pattern))
    : all;

  if (all.length === 0) {
    nothingFound(presetPick, 'No presets yet. Tick "set as preset" when you save a marker.');
    return;
  }
  if (held.length === 0) {
    nothingFound(presetPick, `None of your ${all.length} presets matches that.`);
    return;
  }

  pickable = held;
  pickedRow = -1;

  held.forEach((preset, which) => {
    const { line, open } = listedRow(
      preset.Icon, preset.Color, preset.Title || '(unnamed)', preset.Pattern || '',
      which % 2 === 1);
    line.id = `preset-pick-${which}`;
    open.setAttribute('role', 'option');
    open.setAttribute('aria-label', `Fill in from ${preset.Title || preset.Pattern}`);
    open.addEventListener('click', () => takePreset(preset));
    presetPick.append(line);
  });
}

/** The presets the list is showing, in the order it shows them. */
let pickable = [];

/** Which of them the keyboard is on, or -1 for none. */
let pickedRow = -1;

/**
 * The preset the keyboard is on, or nothing.
 *
 * What decides whether a press in the name box belongs to this list or to the
 * form behind it. Asked rather than each of them reading `pickedRow` for itself:
 * the two listeners on that box both have to agree about whether a list is open
 * on a row, and a press that means two things at once is the bug this answers.
 */
function pickedPreset() {
  return presetPick.classList.contains('open') && pickedRow >= 0
    ? pickable[pickedRow]
    : null;
}

/**
 * Takes one, and puts the list away.
 *
 * The window stays open. A preset is what a marker starts as rather than what it
 * is, and somebody who picked one may still want to move the place, change the
 * colour, or take the name it gave them and add to it — so this fills the form
 * in and stops there.
 *
 * The focus goes to Save. Not back into the name box, or the search opens again
 * on the name the preset just put there; and not to the button that opened the
 * list, where the next press would open it a second time. On Save, the press
 * after this one does the thing the two presses were leading to.
 */
function takePreset(preset) {
  fillFromPreset(preset);
  showPresetPick(false);
  markerSave.focus();
}

/**
 * Says which row the keyboard is on, to the eye and to a screen reader.
 *
 * The same mark a chosen row in the presets window wears, because it means the
 * same thing in both — this is the one the next press acts on.
 */
function showPickedRow() {
  const rows = [...presetPick.querySelectorAll('.listed')];
  rows.forEach((line, which) => {
    line.classList.toggle('chosen', which === pickedRow);
    line.setAttribute('aria-selected', String(which === pickedRow));
  });
  if (pickedRow < 0) {
    markerName.removeAttribute('aria-activedescendant');
    return;
  }
  markerName.setAttribute('aria-activedescendant', rows[pickedRow].id);
  rows[pickedRow].scrollIntoView({ block: 'nearest' });
}

/** Shows or hides the flyout, and says which on the button that opens it. */
function showPresetPick(open, looking) {
  if (open) drawPresetPick(looking);
  presetPick.classList.toggle('open', open);
  presetPickButton.setAttribute('aria-expanded', String(open));
  if (open) placePresetPick();
}

/**
 * Puts the flyout beside the window it was opened from.
 *
 * Measured rather than written in the style sheet, because it is not inside that
 * window: a window scrolls its own contents, and a list hung off the side of one
 * from the cascade is a list clipped at its edge.
 *
 * To the right where there is room and to the left where there is not, and never
 * off the bottom — a list somebody has to scroll the page to reach is a list they
 * cannot reach, since the map does not scroll.
 */
function placePresetPick() {
  const window_ = composer.getBoundingClientRect();
  const box = presetPick.getBoundingClientRect();
  const gap = 8;

  const right = window_.right + gap;
  const left = right + box.width <= innerWidth - gap
    ? right
    : Math.max(gap, window_.left - gap - box.width);

  const top = Math.min(
    Math.max(gap, window_.top),
    Math.max(gap, innerHeight - gap - box.height));

  presetPick.style.left = `${Math.round(left)}px`;
  presetPick.style.top = `${Math.round(top)}px`;
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

  presetPickButton.addEventListener('click', () => {
    showPresetPick(!presetPick.classList.contains('open'));
  });

  // The name box doubles as a way into the presets, because typing a name is
  // faster than reaching for a button and the box is already under the pointer.
  // Only while making a marker: a preset being edited *is* the thing in the box,
  // and offering to fill it in from itself is a loop rather than a shortcut.
  markerName.addEventListener('input', () => {
    if (!settings.presetSearch.on || mode === 'preset') return;
    const looking = markerName.value.trim();
    showPresetPick(looking.length > 0, looking);
  });

  // Walking the list from the box, without leaving it. Tab because that is what
  // a hand already on the keyboard reaches for over a list that has just opened
  // under what it typed, and the arrows because that is what a list means
  // everywhere else on this page — the block search under this same form
  // included. Both wrap, through `nextRow`, which is the one place that decides
  // what the end of a list does.
  markerName.addEventListener('keydown', event => {
    if (!presetPick.classList.contains('open') || pickable.length === 0) return;

    if (event.key === 'Tab' || event.key === 'ArrowDown' || event.key === 'ArrowUp') {
      // Or Tab takes the focus out of the form and an arrow puts the caret at
      // one end of the box, neither of which is what was meant over an open list.
      event.preventDefault();
      const back = event.key === 'ArrowUp' || (event.key === 'Tab' && event.shiftKey);
      pickedRow = nextRow(pickedRow, back ? -1 : 1, pickable.length);
      showPickedRow();
    } else if (event.key === 'Enter' && pickedPreset()) {
      // Only with a row under the keyboard. Typing a name and pressing Enter is
      // the other half of what this box is for, and it must not turn into
      // whichever preset happened to be listed first — which is the same
      // question the form behind asks, through the same function.
      event.preventDefault();
      takePreset(pickedPreset());
    }
  });

  // A name typed to the end is a name, not a search. Escape is already the way
  // out of the list, and picking one is the way through it.
  markerName.addEventListener('blur', () => {
    // Late enough that a click on a row is still a click on a row: a blur fires
    // before the press that caused it lands.
    //
    // The button that opens the list counts as being in it. Pressing that button
    // takes the focus out of this box, and closing on that took two presses to
    // open the list at all: the first opened it and this shut it again a
    // sixth of a second later, and the second worked only because the focus had
    // already left the box by then.
    setTimeout(() => {
      if (holdingPresetPick()) return;
      showPresetPick(false);
    }, 150);
  });
  // Escape shuts the list before it shuts the window the list was opened from.
  // On the document rather than on either of them, because the list is no longer
  // inside the window and a press may land in either.
  document.addEventListener('keydown', event => {
    if (event.key !== 'Escape' || !presetPick.classList.contains('open')) return;
    event.stopPropagation();
    showPresetPick(false);
    presetPickButton.focus();
  });

  // A press anywhere but the list and the button that opens it is somebody
  // having moved on from choosing.
  document.addEventListener('pointerdown', event => {
    if (!presetPick.classList.contains('open')) return;
    if (!presetPick.contains(event.target) && !presetPickButton.contains(event.target)) {
      showPresetPick(false);
    }
  }, { capture: true });
}

/**
 * Whether the list is still somebody's to be looking at.
 *
 * The list itself and the button that opens it are one control between them, so
 * the focus being on either is the focus being in the list. Its own function
 * because two things ask it — what closes the list when the focus leaves, and
 * what closes it on a press elsewhere — and a list that answered differently to
 * the two would close under whichever of them ran second.
 */
function holdingPresetPick() {
  return presetPick.contains(document.activeElement)
    || presetPickButton.contains(document.activeElement);

  // The window it is placed against can be dragged out from under it, and the
  // screen can be resized under both.
  addEventListener('resize', () => {
    if (presetPick.classList.contains('open')) placePresetPick();
  });
}
