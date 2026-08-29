// Finding one row in a list of them.
//
// Two panels list things somebody may have a great many of — every preset they
// have kept, and every marker on the map — and a list that cannot be searched is
// a list that gets scrolled. Both ask the same question of the same shape of
// row, so the answer is here rather than once in each of them.

/**
 * Whether a row answers to what somebody has typed.
 *
 * A plain substring, case folded, over each of the words the row is known by.
 * This is a filter over a list already on the screen rather than a query against
 * a store, so `cop` finds copper without anybody having learnt a syntax first —
 * and deliberately not the `*` grammar a preset's own pattern uses, which is a
 * different question asked in a different box.
 *
 * Nothing typed matches everything, which is what makes an empty box the whole
 * list rather than none of it.
 */
function looksLike(typed, ...words) {
  const wanted = String(typed ?? '').trim().toLowerCase();
  if (wanted === '') return true;
  return words.some(word => String(word ?? '').toLowerCase().includes(wanted));
}

/**
 * Wires a search box to the list it filters.
 *
 * Filtered as it is typed rather than on Enter: the rows are already here, so
 * making somebody press a key to see the answer would only be slower than not.
 * Escape empties the box, which is the way back to the whole list without
 * holding backspace down — and it is stopped there, because a browser clears its
 * own search field on the same key and the list would be drawn twice for one
 * press.
 */
function findingIn(box, redraw) {
  box.addEventListener('input', redraw);
  box.addEventListener('keydown', event => {
    if (event.key !== 'Escape' || box.value === '') return;
    event.stopPropagation();
    box.value = '';
    redraw();
  });
}

/** What a list says when it has nothing to show, in the words of why. */
function nothingFound(list, what) {
  const note = document.createElement('p');
  note.className = 'nothing';
  note.textContent = what;
  list.append(note);
}
