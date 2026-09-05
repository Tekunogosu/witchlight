// The panels that can be dragged about.
//
// Positioned rather than laid out, and clamped so that a bar dragged past an
// edge can still be grabbed: the only way back from one that cannot is
// reloading the page.

/**
 * Windows over the map.
 *
 * Four of them — the marker form, the presets, every marker there is, and who
 * you are — and one set of manners between them: shown by a class, moved by
 * their bar, closed by the mark in their corner, and kept where they were put
 * for as long as the page is open. The two that list things a reader may have a
 * great many of are resized by their corner as well.
 *
 * They float rather than sitting in the layout because everything else here is
 * anchored to a corner of the map and would have to move aside for them. A
 * window that covers the zoom is a window somebody drags; a page that reshuffles
 * itself is a page where the button moved out from under the pointer.
 */

/** Where each window has been put, by the element it is. */
const windowsAt = new Map();

/** What one pair of eyes is shown — colours, sizes, what the map may do. */
const accessibility = document.getElementById('accessibility');

/** How small a window may be dragged before it stops being one. */
const WINDOW_LEAST = { wide: 240, high: 140 };

/** How far one press of an arrow key resizes a window. */
const WINDOW_STEP = 24;

/**
 * The stacking order windows are dealt from.
 *
 * They were all at one z-index, which leaves the markup deciding which of two
 * overlapping windows is in front — permanently, and in an order nobody chose.
 * The one written last in the page won every time, so a window opened on top of
 * it was behind it, and the mark that shuts the window underneath did nothing at
 * all: the click never reached it.
 *
 * Counted up rather than shuffled down, so raising one window is one write and
 * touches no other. The number only ever grows, which a browser has no trouble
 * with and nobody can reach the end of by clicking.
 */
let stacked = 1100;

/** Puts a window in front of the others. */
function raiseWindow(panel) {
  if (panel.style.zIndex === String(stacked)) return;
  stacked += 1;
  panel.style.zIndex = String(stacked);
}

/**
 * Puts a window somewhere it can still be reached.
 *
 * A bar dragged off the top or past a side cannot be grabbed again, and the only
 * way back would be reloading the page — so a margin of it is kept on screen
 * whatever is asked for, including when the browser is resized under it.
 */
function settleWindow(panel, left, top) {
  const held = 60;
  const box = panel.getBoundingClientRect();
  const x = Math.min(Math.max(left, held - box.width), innerWidth - held);
  const y = Math.min(Math.max(top, 0), innerHeight - 30);
  panel.style.left = `${Math.round(x)}px`;
  panel.style.top = `${Math.round(y)}px`;
  panel.style.maxHeight = `${Math.round(heightLeftUnder(y))}px`;
  windowsAt.set(panel, { x, y });
  followTheWindow(panel);
}

/**
 * How tall a window may be from a given top before it runs off the screen, in
 * its own pixels.
 *
 * A window scrolls its own contents, which keeps every control in it
 * reachable — but only while the window's foot is on the screen. The stylesheet
 * caps a window at the height of the page, which a window opened partway down
 * runs past by however far down it is, and a window drawn through
 * `--scale-panel` runs past by that factor again: the largest setting put the
 * slider that shrinks the windows back below the bottom of the screen, with no
 * way to scroll to it. So the cap is worked out where the position is, from
 * the position, and divided by the scale because the stylesheet sizes the
 * window before the transform draws it larger.
 */
function heightLeftUnder(top) {
  const margin = 12;
  return Math.max(WINDOW_LEAST.high, (innerHeight - top - margin) / scaleOf('panel'));
}

/**
 * Makes a window's bar move it.
 *
 * Pointer events rather than mouse ones, so a finger drags it too, and the
 * pointer is captured so a hand that outruns the window keeps hold of it. The
 * map underneath never hears any of this: the window already stops clicks, and
 * the bar takes the pointer for the length of the drag.
 */
function dragBy(panel) {
  const handle = panel.querySelector('.bar');
  handle.addEventListener('pointerdown', event => {
    // A right click on a window is not a drag, and it must not open a marker
    // form on the map underneath either.
    if (event.button !== 0) return;

    // Nor is a press on something in the bar that does its own job. Capturing
    // the pointer sends the release to the bar, and a click is delivered to the
    // nearest ancestor of where it went down and where it came up — so a bar
    // that captures on every press eats the press on its own close button, and
    // the mark does nothing at all.
    if (event.target.closest('button, input, a, select')) return;

    const box = panel.getBoundingClientRect();
    const grip = { x: event.clientX - box.left, y: event.clientY - box.top };

    handle.setPointerCapture(event.pointerId);
    handle.classList.add('held');
    event.preventDefault();

    const moved = to => settleWindow(panel, to.clientX - grip.x, to.clientY - grip.y);
    const done = () => {
      handle.classList.remove('held');
      handle.removeEventListener('pointermove', moved);
      handle.removeEventListener('pointerup', done);
      handle.removeEventListener('pointercancel', done);
    };
    handle.addEventListener('pointermove', moved);
    handle.addEventListener('pointerup', done);
    handle.addEventListener('pointercancel', done);
  });

  // Touching a window is asking for it, so it comes to the front. Captured
  // rather than bubbled: this has to happen for a press anywhere in the window,
  // including on a control that stops the event getting any further.
  panel.addEventListener('pointerdown', () => raiseWindow(panel), { capture: true });

  // Only the bar's own mark. `.shut` is the shape of a small × and a preset row
  // has one to delete itself with, which is not a window closing.
  for (const shut of panel.querySelectorAll('.bar .shut')) {
    shut.addEventListener('click', () => shutWindow(panel));
  }
  // Over the map, so a click, a drag or a scroll on a window must not reach it.
  L.DomEvent.disableClickPropagation(panel);
  L.DomEvent.disableScrollPropagation(panel);
}

/**
 * Gives a window a corner to be resized by.
 *
 * A list of what somebody has kept is as long as they have made it, and there is
 * no width or height that is right for everybody's: the presets window at three
 * hundred and forty pixels is generous for four presets and a slot for eighty.
 * So the reader says, and what they say holds for as long as the page is open —
 * the same rule the window's position follows, and for the same reason: this is
 * about the screen in front of them and this session on it.
 *
 * The sums are in the window's own pixels rather than the pointer's. A window is
 * drawn through `--scale-panel`, so a hand that moves a hundred pixels across a
 * panel drawn half again as large has asked for sixty-odd more pixels of window,
 * not a hundred — and taking the pointer's number would run the corner out from
 * under the hand at every size but one.
 *
 * A button rather than a bare corner, because a resize nobody can reach without
 * a pointer is furniture a keyboard cannot move: the arrows do the same job, and
 * a name says which window they will do it to.
 */
function growBy(panel) {
  const grip = document.createElement('button');
  grip.type = 'button';
  grip.className = 'grip';
  const named = panel.querySelector('h2').textContent.trim();
  grip.title = `Resize ${named}`;
  grip.setAttribute('aria-label', `Resize the ${named} window — drag, or use the arrow keys`);
  panel.append(grip);

  grip.addEventListener('pointerdown', event => {
    if (event.button !== 0) return;
    const from = {
      wide: panel.offsetWidth,
      high: panel.offsetHeight,
      x: event.clientX,
      y: event.clientY,
      scale: scaleOf('panel'),
    };

    grip.setPointerCapture(event.pointerId);
    grip.classList.add('held');
    event.preventDefault();

    const moved = to => sizeWindow(
      panel,
      from.wide + (to.clientX - from.x) / from.scale,
      from.high + (to.clientY - from.y) / from.scale,
    );
    const done = () => {
      grip.classList.remove('held');
      grip.removeEventListener('pointermove', moved);
      grip.removeEventListener('pointerup', done);
      grip.removeEventListener('pointercancel', done);
    };
    grip.addEventListener('pointermove', moved);
    grip.addEventListener('pointerup', done);
    grip.addEventListener('pointercancel', done);
  });

  grip.addEventListener('keydown', event => {
    const step = {
      ArrowLeft: [-WINDOW_STEP, 0],
      ArrowRight: [WINDOW_STEP, 0],
      ArrowUp: [0, -WINDOW_STEP],
      ArrowDown: [0, WINDOW_STEP],
    }[event.key];
    if (!step) return;
    event.preventDefault();
    sizeWindow(panel, panel.offsetWidth + step[0], panel.offsetHeight + step[1]);
  });
}

/**
 * Takes a size for a window, and keeps it reachable at it.
 *
 * Floored rather than free: a window dragged to nothing is a window that cannot
 * be dragged back, which is the same trap the position clamp exists to close.
 * The ceiling is the stylesheet's, which already holds every window inside the
 * viewport. Settled afterwards because a window that just grew against the right
 * edge has to be pulled back to stay grabbable.
 */
function sizeWindow(panel, wide, high) {
  panel.classList.add('sized');
  panel.style.width = `${Math.round(Math.max(wide, WINDOW_LEAST.wide))}px`;
  panel.style.height = `${Math.round(Math.max(high, WINDOW_LEAST.high))}px`;
  const held = windowsAt.get(panel);
  if (held) settleWindow(panel, held.x, held.y);
}

/**
 * Shows a window, where it was left or where it belongs.
 *
 * `middle` opens it centred, which is where something about you rather than
 * about a place on the map wants to be. Measured only once it is shown: a window
 * still `display:none` has no size, and both keeping it on screen and centring
 * it are sums over its width.
 */
function openWindow(panel, middle) {
  panel.classList.add('open');
  raiseWindow(panel);
  const held = windowsAt.get(panel);
  if (held) {
    settleWindow(panel, held.x, held.y);
    return;
  }
  // Settled on its first opening too, and not only when it is moved: where
  // the stylesheet put it is a position like any other, and the height a
  // window may have follows from its position — see `settleWindow`.
  const box = panel.getBoundingClientRect();
  if (middle) {
    settleWindow(panel, (innerWidth - box.width) / 2, (innerHeight - box.height) / 2);
  } else {
    settleWindow(panel, box.left, box.top);
  }
}

/**
 * Anything placed against a window rather than inside it, moved with it.
 *
 * Called wherever a window's position is written, which is one place — see
 * `settleWindow`. A list beside a window that stayed put while the window was
 * dragged would be a list beside nothing.
 */
function followTheWindow(panel) {
  if (panel === composer && presetPick.classList.contains('open')) placePresetPick();
}

function shutWindow(panel) {
  panel.classList.remove('open');
  if (panel === composer) forgetCompose();
  if (panel === claimPanel) forgetClaimOutline();
  // A window shut with a row still waiting for a key is a page whose next
  // press vanishes into it.
  if (panel === profile) stopListening();
}

/**
 * Gives every window its manners, in one place.
 *
 * Each of these used to be wired where its own panel was built, which meant the
 * list of what is a window was spread over three files and a fourth one could be
 * added without ever being made draggable. What a window is, is decided here.
 */
function buildWindows() {
  for (const panel of [composer, presetPanel, directory, profile, claimPanel, claimsPanel, claimView, accessibility])
    dragBy(panel);
  // Only the two that list things. A form is as big as its fields and a window
  // with a size nobody can use is a corner that does nothing when pulled.
  for (const panel of [presetPanel, directory, claimsPanel]) growBy(panel);
}

/**
 * Escape shuts the window in front, one press at a time.
 *
 * Down the stack rather than all at once: three windows open are three things
 * somebody put there, and a key that swept the lot would take two of them away
 * from a reader who wanted one gone. The one in front is the one being looked
 * at, so it is the one that goes, and the next press finds the next.
 *
 * Which is in front is the number `raiseWindow` deals out, so this reads the
 * same order the eye does rather than the order the page happens to list them.
 *
 * A press already answered by something smaller is left alone: a search box
 * empties itself, a block list closes, the preset flyout shuts. Each of those
 * stops the press where it happens, so what reaches here is a press nothing
 * else wanted.
 */
addEventListener('keydown', event => {
  if (event.key !== 'Escape' || event.defaultPrevented) return;

  let front = null;
  for (const panel of document.querySelectorAll('.window.open')) {
    if (!front || Number(panel.style.zIndex || 0) >= Number(front.style.zIndex || 0)) {
      front = panel;
    }
  }
  if (!front) return;

  event.preventDefault();
  shutWindow(front);
});

// A browser narrowed under a window that was near the right edge leaves it off
// the screen, which is the same unreachable bar arrived at from the other
// direction — so the clamp is asked again whenever the screen changes size.
addEventListener('resize', () => {
  for (const [panel, where] of windowsAt) settleWindow(panel, where.x, where.y);
});
