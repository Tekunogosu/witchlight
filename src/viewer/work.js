// Things that answer later, with nobody waiting on them.
//
// A browser does not wait for a handler, so an async function wired to a click,
// a clock or a keystroke hands back a promise nobody is holding. Both functions
// here exist so there is one place that says what happens when one of those
// fails, rather than a rejection in a console nobody has open.

'use strict';

/**
 * Starts something that answers later and is not waited on.
 *
 * The page is full of these: a click that asks the service, a poll on a timer, a
 * search that fires after a pause in typing. None of them has a caller to return
 * to, so a throw inside one is an unhandled rejection — the page carries on as
 * though the work had happened, and the only sign is a line in a console.
 *
 * `what` names the work in the words the reader of that console needs, because
 * a stack through three promises does not say which of a dozen pollers it was.
 *
 * What comes back never rejects, so it can be awaited by something that wants to
 * know the work is over without wanting to know how it went.
 */
function started(work, what) {
  return Promise.resolve(work).catch(error => {
    console.error(`witchlight: ${what} failed`, error);
  });
}

/**
 * Runs something on a beat, waiting for each answer before counting the next.
 *
 * `setInterval` counts the beat whether or not the last one has been answered,
 * so a service slower than the gap is asked again while it is still answering,
 * and the asking only outruns it further. The gap here is between one answer and
 * the next question, which is what "every two seconds" meant to say.
 */
function beat(work, every, what) {
  const again = async () => {
    await started(work(), what);
    setTimeout(again, every);
  };
  setTimeout(again, every);
}
