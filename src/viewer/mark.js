// How a marker is shown: the mark itself, and the row a list shows one in.
//
// Its own file because four things draw a mark — the map, the picker in the
// marker form, the preset list and the marker list — and every one of them wants
// the same picture. It was written out at each of them until a mask that failed
// to load turned out to draw nothing at all, in three of the four.
//
// The row is here for the same reason one step up. The presets and the markers
// are the same shape on the screen, and the stylesheet already says so in one
// rule; two functions building it was the half of that agreement that could
// drift.

/**
 * A colour the page is willing to draw a marker in.
 *
 * What arrives is whatever a game client wrote into a waypoint, and a value the
 * browser cannot parse is not a refusal — it is a marker drawn in whatever the
 * last rule happened to set, which on this page is nothing at all. White is the
 * honest stand-in: still a marker, visibly not a colour anybody chose.
 */
function colourOf(asked) {
  return /^#[0-9a-f]{6}$/i.test(asked || '') ? asked : '#ffffff';
}

/**
 * A marker's mark: the picture it takes, filled with the colour it is.
 *
 * The one place that draws one. The map, the picker in the form, the preset list
 * and the marker list all show the same thing — a marker as it will look — and
 * each of them used to build it from the mask rules again, which is four places
 * for the same picture to come out differently.
 *
 * A silhouette used as a mask and filled from behind, which is how the game
 * itself draws them. Where the service has no picture under that name, a plain
 * diamond stands in: a mask whose image never loads draws *nothing*, so a name
 * the map has not got left a hole rather than a marker, and a preset saved
 * against an icon from a mod since removed vanished out of its own list.
 *
 * How large it is belongs to whatever holds it, so the map, the picker and the
 * two lists each say that for their own copy rather than sharing a number none
 * of them wants.
 */
function markFor(picture, colour) {
  const mark = document.createElement('i');
  const name = String(picture || '');
  mark.style.background = colour;

  if (!icons.has(name)) {
    mark.className = 'diamond';
    return mark;
  }

  mark.className = 'masked';
  const url = `url(/icons/${encodeURIComponent(name)}.svg)`;
  mark.style.webkitMaskImage = url;
  mark.style.maskImage = url;
  return mark;
}

/**
 * One row in a list of things to pick from: a mark, a name, and a quieter line
 * under it.
 *
 * The row and the button come back apart because what a row offers besides being
 * picked is not the same in both lists — a preset has a way to be deleted and a
 * marker has a lock — and that is the only thing they do differently.
 *
 * `shaded` bands every other row, counted over what is drawn rather than over
 * what is held: a search that hides three rows must not take the banding with it
 * and leave two shaded rows together.
 */
function listedRow(picture, colour, name, under, shaded) {
  const line = document.createElement('div');
  line.className = 'listed' + (shaded ? ' alt' : '');

  const mark = document.createElement('span');
  mark.className = 'mark';
  mark.append(markFor(picture, colourOf(colour)));

  const open = document.createElement('button');
  open.type = 'button';
  open.className = 'said';
  const named = document.createElement('b');
  named.textContent = name;
  const quiet = document.createElement('span');
  quiet.textContent = under;
  open.append(named, quiet);

  line.append(mark, open);
  return { line, open };
}
