// One key per thing the corner offers.
//
// A press does what the button does, through the button: every one of these
// already knows whether it is offered — signed in, allowed to take land — and
// whether a press opens or shuts, so a key that reached past it would have to
// know both again. The one exception is the marker, which the key puts under the
// pointer rather than wherever the form last was, the way a right click does.
//
// Which key is which follows the account. A keyboard's question is about the
// hand on it rather than the screen in front of it, and the same hand reaches for
// the same key on every machine — so the bindings live in the person's
// preferences beside the switches, and the reminder under the map is the one
// part of this kept in the browser, being a thing about one screen.

/**
 * What a key can do, in the order the reminder lists them.
 *
 * `offered` is the control that gates the action: the key is silent, and the
 * reminder leaves it out, while that control is not on the page. `act` is what a
 * press does. Both are looked up at the press rather than held, because two of
 * the controls are built after this table is read.
 */
const hotkeys = {
  marker: {
    label: 'Marker at pointer',
    key: 'c',
    offered: () => markerButton,
    // Under the pointer where there is one, and wherever the form was where the
    // hand is off the map — the same answer the button gives.
    act: () => { if (pointer) composeAt(pointer); else markerButton.click(); },
  },
  markers: { label: 'Markers', key: 'm', offered: () => directoryButton, act: () => directoryButton.click() },
  drawClaim: { label: 'Draw claim', key: 'k', offered: () => claimDraw, act: () => claimDraw.click() },
  claims: { label: 'Claims', key: 'l', offered: () => claimList, act: () => claimList.click() },
  presets: { label: 'Presets', key: 'p', offered: () => presetButton, act: () => presetButton.click() },
  // Offered with the presets rather than with the Create button, which is in a
  // window that is usually shut: what gates making one is being signed in.
  newPreset: { label: 'New preset', key: 'P', offered: () => presetButton, act: () => newPreset() },
  accessibility: {
    label: 'Accessibility',
    key: 'A',
    offered: () => accessBar.querySelector('a'),
    act: () => accessBar.querySelector('a').click(),
  },
  inspect: { label: 'Inspect', key: 'i', offered: () => picker._button, act: () => setPicking(!picking) },
  settings: {
    label: 'Account',
    key: 'o',
    offered: () => accountBar.querySelector('a'),
    act: () => accountBar.querySelector('a').click(),
  },
};

/** Whether a control is on the page: hidden by a class is as absent as missing. */
function onThePage(control) {
  return Boolean(control) && control.getClientRects().length > 0;
}

/**
 * The key an action answers to, as the person has it.
 *
 * Their own choice where they made one — an empty string being the choice to
 * have none — and the default otherwise.
 */
function keyFor(name, bindings) {
  const chosen = bindings && typeof bindings === 'object' ? bindings[name] : undefined;
  return typeof chosen === 'string' ? chosen : hotkeys[name].key;
}

/** What the person has kept, which is what a press answers to. */
function keptHotkeys() {
  return mine && mine.Hotkeys && typeof mine.Hotkeys === 'object' ? mine.Hotkeys : {};
}

/** A key as the reminder writes it: a bare letter, or a word for a key that is one. */
function keyName(key) {
  if (key === '') return '—';
  if (key === ' ') return 'Space';
  return key;
}

/** Whether a press landed where letters are being typed, which no key here may steal. */
function typingAt(target) {
  if (!(target instanceof Element)) return false;
  if (target.isContentEditable) return true;
  return Boolean(target.closest('input, textarea, select'));
}

const keysLine = document.getElementById('keys');

/**
 * Writes the reminder under the map, or takes it away.
 *
 * Only what is offered right now, so a reader who is not signed in is not shown
 * keys that do nothing, and only what has a key. Rebuilt whole: it is nine words.
 */
function showHotkeys() {
  if (!keysLine) return;
  keysLine.textContent = '';
  if (!settings.hotkeys.on) return;
  const bindings = keptHotkeys();
  for (const [name, action] of Object.entries(hotkeys)) {
    const key = keyFor(name, bindings);
    if (key === '' || !onThePage(action.offered())) continue;
    const word = document.createElement('span');
    const shown = document.createElement('b');
    shown.textContent = keyName(key);
    word.append(shown, document.createTextNode(` ${action.label}`));
    keysLine.append(word);
  }
}

/**
 * Answers a press anywhere on the page that nothing smaller wanted.
 *
 * A chord with Control, Alt or Meta is the browser's, and a press while typing
 * is the field's. Shift on its own is part of the key — `P` is a binding of its
 * own — which is what `event.key` already says.
 */
function pressHotkey(event) {
  if (event.defaultPrevented || event.ctrlKey || event.altKey || event.metaKey) return;
  if (listening || typingAt(event.target)) return;
  const bindings = keptHotkeys();
  for (const [name, action] of Object.entries(hotkeys)) {
    if (keyFor(name, bindings) !== event.key) continue;
    if (!onThePage(action.offered())) return;
    event.preventDefault();
    action.act();
    return;
  }
}

/* ---- the rows in the account window ---- */

/**
 * What the account window is holding for the keys but has not kept.
 *
 * Only the actions the person has moved off the default, so a row put back to its
 * default is a row that stops being kept. Read into the window's draft with the
 * rest, and sent on Save.
 */
let hotkeyDraft = {};

/** The row whose button is waiting for a key, or null. */
let listening = null;

/** What the profile draft carries for the keys. */
function hotkeysDrafted() {
  return { ...hotkeyDraft };
}

/** The action already answering to a key in the draft, other than this one. */
function alreadyBound(key, except) {
  for (const name of Object.keys(hotkeys)) {
    if (name !== except && keyFor(name, hotkeyDraft) === key) return name;
  }
  return null;
}

/** Puts one row's key into the draft, and says so on the row. */
function bindHotkey(name, key) {
  if (key === hotkeys[name].key) delete hotkeyDraft[name];
  else hotkeyDraft[name] = key;
  draftProfile();
  sayProfile('Not kept yet.');
  drawHotkeyRows(Boolean(viewer && viewer.Name), true);
}

/** Stops waiting for a key, on whichever row was. */
function stopListening() {
  if (!listening) return;
  const button = listening;
  listening = null;
  button.classList.remove('listening');
  button.textContent = keyName(keyFor(button.dataset.hotkey, hotkeyDraft));
}

/**
 * Takes the next press for the row that asked for one.
 *
 * Captured ahead of everything else on the page, so a press meant as a binding
 * neither shuts a window nor opens one. Escape keeps what was there, Backspace
 * and Delete unbind it, a chord is refused, and a key another row already has is
 * refused by name — so no two rows ever answer to one press.
 */
function takeHotkey(event) {
  if (!listening) return;
  event.preventDefault();
  event.stopPropagation();
  const name = listening.dataset.hotkey;
  if (event.key === 'Escape') { stopListening(); return; }
  if (event.key === 'Backspace' || event.key === 'Delete') {
    stopListening();
    bindHotkey(name, '');
    return;
  }
  // A modifier on its own is not yet a press; a chord is the browser's.
  if (['Shift', 'Control', 'Alt', 'Meta'].includes(event.key)) return;
  if (event.ctrlKey || event.altKey || event.metaKey) {
    sayProfile('A key on its own, or with Shift — not a chord.', true);
    return;
  }
  const taken = alreadyBound(event.key, name);
  if (taken) {
    sayProfile(`${keyName(event.key)} already opens ${hotkeys[taken].label}.`, true);
    return;
  }
  stopListening();
  bindHotkey(name, event.key);
}

/**
 * Draws the rows from what is kept plus what has been changed since.
 *
 * Called with the rest of the window, which is when what is kept may have moved
 * under it — the draft is started again from what is kept then, the way the
 * switches are read back from it.
 */
function drawHotkeyRows(named, keepDraft) {
  if (!keepDraft) {
    hotkeyDraft = {};
    for (const [name, key] of Object.entries(keptHotkeys())) {
      if (hotkeys[name] && typeof key === 'string' && key !== hotkeys[name].key) hotkeyDraft[name] = key;
    }
    stopListening();
  }
  const rows = document.getElementById('hotkey-rows');
  rows.textContent = '';
  for (const [name, action] of Object.entries(hotkeys)) {
    const line = document.createElement('div');
    line.className = 'line';
    const label = document.createElement('span');
    label.textContent = action.label;
    const button = document.createElement('button');
    button.type = 'button';
    button.className = 'key';
    button.dataset.hotkey = name;
    button.disabled = !named;
    // One of nine identical buttons, so what it binds is the only thing that
    // tells a reader — or a test — which row was reached.
    button.setAttribute('aria-label', `Change the key for ${action.label}`);
    button.textContent = keyName(keyFor(name, hotkeyDraft));
    button.addEventListener('click', () => {
      if (listening === button) { stopListening(); return; }
      stopListening();
      listening = button;
      button.classList.add('listening');
      button.textContent = 'press a key';
      sayProfile('');
    });
    line.append(label, button);
    rows.append(line);
  }
  document.getElementById('hotkey-reset').disabled = !named;
}

function buildHotkeys() {
  addEventListener('keydown', takeHotkey, { capture: true });
  addEventListener('keydown', pressHotkey);
  document.getElementById('hotkey-reset').addEventListener('click', () => {
    stopListening();
    hotkeyDraft = {};
    draftProfile();
    sayProfile('Not kept yet.');
    drawHotkeyRows(Boolean(viewer && viewer.Name), true);
  });
  showHotkeys();
}
