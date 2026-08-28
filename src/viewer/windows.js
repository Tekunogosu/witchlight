// The panels that can be dragged about.
//
// Positioned rather than laid out, and clamped so that a bar dragged past an
// edge can still be grabbed: the only way back from one that cannot is
// reloading the page.

/**
 * Windows over the map.
 *
 * Three of them — the marker form, the presets, and who you are — and one set of
 * manners between them: shown by a class, moved by their bar, closed by the mark
 * in their corner, and kept where they were put for as long as the page is open.
 *
 * They float rather than sitting in the layout because everything else here is
 * anchored to a corner of the map and would have to move aside for them. A
 * window that covers the zoom is a window somebody drags; a page that reshuffles
 * itself is a page where the button moved out from under the pointer.
 */

/** Where each window has been put, by the element it is. */
const windowsAt = new Map();

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
  windowsAt.set(panel, { x, y });
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
 * Shows a window, where it was left or where it belongs.
 *
 * `middle` opens it centred, which is where something about you rather than
 * about a place on the map wants to be. Measured only once it is shown: a window
 * still `display:none` has no size, and both keeping it on screen and centring
 * it are sums over its width.
 */
function openWindow(panel, middle) {
  panel.classList.add('open');
  const held = windowsAt.get(panel);
  if (held) {
    settleWindow(panel, held.x, held.y);
  } else if (middle) {
    const box = panel.getBoundingClientRect();
    settleWindow(panel, (innerWidth - box.width) / 2, (innerHeight - box.height) / 2);
  }
}

function shutWindow(panel) {
  panel.classList.remove('open');
  if (panel === composer) forgetCompose();
}

// A browser narrowed under a window that was near the right edge leaves it off
// the screen, which is the same unreachable bar arrived at from the other
// direction — so the clamp is asked again whenever the screen changes size.
addEventListener('resize', () => {
  for (const [panel, where] of windowsAt) settleWindow(panel, where.x, where.y);
});
