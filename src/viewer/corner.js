// The buttons down the left edge, and what they say.
//
// Everything anchored to a corner of the map rather than drawn on it: the marks
// a control wears, the bars they sit in, the world's clock, and who is looking.
// Leaflet owns one container per corner and these are not its controls, so they
// borrow the shape of its bar and nothing else — see `cornerButton`.

/**
 * The mark a control wears.
 *
 * A silhouette the service compiled in, filled with whatever colour the button
 * is currently using, so a tool that is armed colours its own mark. Every one of
 * these was a Unicode character: two were colour emoji that ignore `color`
 * outright, and the rest were symbol-font characters each machine drew in
 * whatever face it had — or drew as a box, having none.
 *
 * The mark is for the eye alone. What a screen reader says is the button's
 * label, which is why nothing here carries a name.
 */
function chromeMark(name) {
  const mark = document.createElement('span');
  mark.className = `chrome masked mark-${name}`;
  mark.setAttribute('aria-hidden', 'true');
  return mark;
}

/**
 * A bar of one button, in the shape Leaflet draws its own, in the corner.
 *
 * Not a Leaflet control: these say who is looking rather than doing anything to
 * the map, and Leaflet has one container per corner — putting them in it would
 * tie where they sit to where the zoom sits. They borrow the class and nothing
 * else.
 */
function cornerAnchor(into, mark, label) {
  const button = L.DomUtil.create('a', 'tool', into);
  button.href = '#';
  button.title = label;
  button.setAttribute('role', 'button');
  // Every one of these is a square with a mark in it, so the name is the only
  // thing telling a reader — or a test — which one it reached.
  button.setAttribute('aria-label', label);
  button.append(chromeMark(mark));
  L.DomEvent.on(button, 'click', L.DomEvent.stop);
  return button;
}

/**
 * A bar of one button, in the shape Leaflet draws its own.
 *
 * A second button may be added to the same bar with `cornerAnchor`, which is how
 * the zoom pair is built: Leaflet rules a line between stacked anchors and
 * rounds only the ends, so two things that belong together read as one control
 * rather than as two that happen to be near each other.
 */
function cornerButton(id, mark, label, into) {
  const box = L.DomUtil.create('div', 'leaflet-bar', into || corner);
  box.id = id;
  cornerAnchor(box, mark, label);
  // Over the map, so a click or a drag on one must not reach it.
  L.DomEvent.disableClickPropagation(box);
  L.DomEvent.disableScrollPropagation(box);
  return box;
}

// Not `chrome`: a browser already has one of those, and a `const` of that
// name throws before a line of this page runs.
const corner = document.getElementById('corner');

// Leaflet draws its bar buttons four pixels larger on a machine it believes has
// a touch screen, and it says so with a class on the container it owns. This
// column borrows the bar and sits outside that container, so the rule never
// reached it: the cog and the account came out 26 square against a zoom and a
// picker at 30, on exactly the machines where Leaflet decides in favour of the
// larger one. Told what the map was told, both sizes stay one size — rather than
// a number written here that would be wrong on whichever machine Leaflet
// disagreed with.
if (map.getContainer().classList.contains('leaflet-touch')) {
  corner.classList.add('leaflet-touch');
}
/** The settings and who you are, side by side. One is about the map and one is
 *  about you; putting them in a column would make one read as the other's. */
const row = L.DomUtil.create('div', '', corner);
row.id = 'row';
const cogBar = cornerButton('cog', 'gear-six', 'Settings', row);
const accountBar = cornerButton('account', 'user', 'Account', row);
/**
 * The one corner button that says a name as well as wearing a mark.
 *
 * The name is its own element rather than the button's text, because the button
 * already has a child: writing a name onto the button would take the mark with
 * it. It says something before `/me.json` has answered, so the button reads as a
 * control rather than as an empty box for the width of one request.
 */
const accountName = document.createElement('span');
accountName.className = 'who';
accountName.textContent = 'Unauthenticated';
accountBar.querySelector('a').append(accountName);
/**
 * What the world's clock says, beside who is looking at it.
 *
 * Two columns of two: the year over the date, the season over the time. The
 * upper line of each is the quieter one — a year and a season are what the date
 * and the time are *in*, and saying all four equally loudly makes a reader work
 * out which is which every time they glance at it. The quiet line goes above the
 * loud one the way a heading goes above what it heads: the eye lands on the date
 * and has already passed the year it is in.
 */
const whenBar = L.DomUtil.create('div', 'leaflet-bar', row);
whenBar.id = 'when';
/** The same box the account wears, so the two sit level and read as one row of
 *  furniture rather than as a control and a label that happen to be adjacent. */
const whenBox = L.DomUtil.create('div', 'tool clock', whenBar);
const when = {};
for (const [over, said] of [['year', 'date'], ['season', 'time']]) {
  const column = L.DomUtil.create('div', 'when-part', whenBox);
  when[over] = L.DomUtil.create('span', '', column);
  when[said] = L.DomUtil.create('b', '', column);
  when[over].textContent = '';
  when[said].textContent = '—';
}
L.DomEvent.disableClickPropagation(whenBar);

/**
 * Says what the clock says, or takes itself off the map.
 *
 * Shown only where there is something to show: a service running without a game
 * server behind it has no clock at all, and four dashes in the corner is a broken
 * widget rather than an honest absence.
 */
function showWhen(clock) {
  const has = Boolean(clock && (clock.Date || clock.Time));
  whenBar.style.display = has ? '' : 'none';
  if (!has) return;
  when.date.textContent = clock.Date || '—';
  when.year.textContent = clock.Year || '';
  when.time.textContent = clock.Time || '—';
  when.season.textContent = clock.Season || '';
}
showWhen(null);

/**
 * What the map can be asked to do differently, under the settings and above the
 * marker controls.
 *
 * Not gated on being signed in, unlike what is under it: nothing in here changes
 * the map, or anything anybody else sees. It changes what one pair of eyes is
 * shown, which is nobody's business but theirs.
 */
const accessBar = cornerButton('access', 'person-arms-spread', 'Accessibility');

/**
 * Everything about markers: one control, three buttons.
 *
 * Making one, deciding what a new one starts as, and reading down every one
 * there is. Stacked into a single bar because they are three answers to the same
 * subject, and Leaflet rules a line between stacked anchors — which is what makes
 * three buttons read as one control rather than as three that happen to be near
 * each other.
 */
const mineBar = cornerButton('mine', 'map-pin-simple', 'Add a marker');
const markerButton = mineBar.querySelector('a');
const presetButton = cornerAnchor(mineBar, 'bookmarks-simple', 'Presets');
const directoryButton = cornerAnchor(mineBar, 'list-bullets', 'All markers');

/**
 * Says who is looking, and offers what only they can act on.
 *
 * The account button is always there and is greyed when nobody has followed a
 * login link: a control that appears on login moves everything under it, and a
 * page whose furniture jumps is a page somebody clicks the wrong thing on.
 *
 * What sits under it is offered to whoever can act on it. Making a marker means
 * owning one, and only somebody the game named can own anything, so the flag is
 * for people who have followed a link and for nobody else.
 */
function showAccount(me) {
  const button = accountBar.querySelector('a');
  const named = me && me.Name;
  accountName.textContent = named || 'Unauthenticated';
  button.classList.toggle('out', !named);
  button.title = named
    ? `Signed in as ${me.Name}`
    : 'Not signed in — run /witchlight login in the game';
  button.setAttribute('aria-label', named ? `Account: ${me.Name}` : 'Not signed in');
  mineBar.classList.toggle('on', Boolean(named));
  drawProfile();
}

/** Who the service says is looking. Asked at load, which is when it changes:
 *  following a login link lands back here as a fresh page. */
async function pollMe() {
  try {
    viewer = await (await fetch('/me.json', { cache: 'no-store' })).json();
  } catch (error) {
    viewer = null;
  }
  showAccount(viewer);
  await pollMine();
  drawProfile();
}
