/**
 * Finding a block by what it is called.
 *
 * A preset is keyed on a block code and nobody knows
 * `game:smallplants-fern-normal` from memory. The whole table is eleven thousand
 * entries, which is not a thing to hand a page on the chance somebody opens a
 * form, so the service is asked as the box is typed into and answers with a
 * screenful.
 *
 * What is typed stays what is kept. Picking from the list writes a code into the
 * box, and the box is still a box: a pattern with a `*` in it is typed straight
 * in and no search will have offered it.
 *
 * The box keeps the focus the whole time and says which row it is on with
 * `aria-activedescendant`, which is how a listbox under a field is driven. Moving
 * the focus into the list instead would take it out of the box being typed in,
 * and the next keystroke would go to a row rather than to the search.
 */
const blockFound = document.getElementById('block-found');

/** The search this page is waiting on, so a fast typist asks once. */
let searching = null;

/** What the list on screen is an answer to. A search the service was slow with
 *  must not land on top of a later one that has already been answered. */
let searchedFor = '';

/** The blocks the list is showing, in the order it shows them. */
let showing = [];

/** Which row the keyboard is on, or -1 for none of them. */
let onRow = -1;

function buildBlockSearch() {
  markerPattern.addEventListener('input', () => {
    clearTimeout(searching);
    searching = setTimeout(() => started(findBlocks(), 'the block search'), 160);
  });
  markerPattern.addEventListener('focus', () => {
    if (markerPattern.value.trim() !== '') started(findBlocks(), 'the block search');
  });
  // A click elsewhere is done looking. Not `blur`: that fires before the click
  // on a result lands, which would close the list out from under the choice.
  addEventListener('pointerdown', event => {
    if (!event.target.closest('#pattern-field')) closeFound();
  });
  markerPattern.addEventListener('keydown', event => {
    if (event.key === 'Escape') {
      closeFound();
      return;
    }
    if (!blockFound.classList.contains('open')) return;

    // The arrows would otherwise put the caret at one end of the box, which is
    // the one thing somebody pressing them over an open list did not mean.
    if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
      event.preventDefault();
      onRow = nextRow(onRow, event.key === 'ArrowDown' ? 1 : -1, showing.length);
      showRow();
    } else if (event.key === 'Enter' && onRow >= 0) {
      // Only with a row under the keyboard. A pattern typed by hand is the other
      // half of what this box is for, and Enter must not replace it with whatever
      // the service happened to offer first.
      event.preventDefault();
      takeBlock(showing[onRow]);
    }
  });
}

/**
 * The row an arrow key moves to, wrapping at both ends.
 *
 * Both ends wrap, so a list longer than the box is tall can be walked in either
 * direction without hunting for which end it stopped at. From nowhere, down
 * takes the first row and up takes the last — which is what an arrow pressed
 * over a list nobody has moved through yet means in either direction.
 */
function nextRow(from, step, many) {
  if (many <= 0) return -1;
  if (from < 0) return step > 0 ? 0 : many - 1;
  return (from + step + many) % many;
}

/** Says which row the keyboard is on, to the eye and to a screen reader. */
function showRow() {
  const rows = [...blockFound.children];
  rows.forEach((option, which) => {
    option.classList.toggle('on', which === onRow);
    option.setAttribute('aria-selected', String(which === onRow));
  });
  if (onRow < 0) {
    markerPattern.removeAttribute('aria-activedescendant');
    return;
  }
  markerPattern.setAttribute('aria-activedescendant', rows[onRow].id);
  rows[onRow].scrollIntoView({ block: 'nearest' });
}

/**
 * Takes one block out of the list, however it was picked.
 *
 * One function for the click and for the Enter key, because they are one choice:
 * two copies of this is where the keyboard and the pointer start disagreeing
 * about what picking a block does to the name beside it.
 */
function takeBlock(block) {
  markerPattern.value = block.Code;
  // A preset with no name of its own takes the block's, which is what somebody
  // picking "Eagle fern" out of a list meant by picking it.
  if (markerName.value.trim() === '' && block.Name) markerName.value = block.Name;
  closeFound();
  markerPattern.focus();
}

function closeFound() {
  blockFound.classList.remove('open');
  markerPattern.setAttribute('aria-expanded', 'false');
  markerPattern.removeAttribute('aria-activedescendant');
  onRow = -1;
}

async function findBlocks() {
  const asked = markerPattern.value.trim();
  searchedFor = asked;
  if (asked === '') {
    closeFound();
    return;
  }

  let found = [];
  try {
    found = await (await fetch(`/blocks.json?q=${encodeURIComponent(asked)}`)).json();
  } catch (error) {
    /* the service may be restarting; an empty list says so quietly enough */
  }

  // Somebody kept typing while this was in flight, and the box is asking a
  // different question now. Drawing this answer under it would offer the blocks
  // for a word that is no longer there.
  if (searchedFor !== asked) return;
5
  blockFound.textContent = '';
  if (!Array.isArray(found) || found.length === 0) {
    showing = [];
    closeFound();
    return;
  }

  showing = found;
  onRow = -1;
  found.forEach((block, which) => {
    const choice = document.createElement('button');
    choice.type = 'button';
    // Named so the box above can point at one of them. Out of the tab order:
    // the focus stays in the box, and a row that could take it would put the
    // next keystroke somewhere other than the search being typed.
    choice.id = `block-found-${which}`;
    choice.tabIndex = -1;
    choice.setAttribute('role', 'option');
    choice.setAttribute('aria-selected', 'false');
    const name = document.createElement('b');
    name.textContent = block.Name || shortCode(block.Code);
    const code = document.createElement('span');
    code.textContent = block.Code;
    choice.append(name, code);
    // Named by the block it stands for, so a reader hears which of a list of
    // near-identical rows this is rather than "button".
    choice.setAttribute('aria-label', `${block.Name || ''} ${block.Code}`.trim());
    choice.addEventListener('click', () => takeBlock(block));
    blockFound.append(choice);
  });

  markerPattern.removeAttribute('aria-activedescendant');
  blockFound.classList.add('open');
  markerPattern.setAttribute('aria-expanded', 'true');
}
