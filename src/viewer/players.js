// What the map itself draws: a dot per player, a mark per marker.
//
// Nothing here is redrawn for being sent again. A player who is still online
// keeps the marker they had and is moved, and the markers are compared as a set —
// the service posts every player every two seconds and almost none of them has
// changed. Who is online as a list is the panel, in `who.js`.

const people = L.layerGroup().addTo(map);
const places = L.layerGroup().addTo(map);

/** Players on the map, by the identity that survives a rename. */
const drawnPlayers = new Map();

/**
 * Moves the players who moved.
 *
 * Everything here used to be cleared and rebuilt on every poll, which at forty
 * markers and a picture each was forty elements destroyed and recreated every two
 * seconds — a map that flickered twice a minute for every minute it was open.
 * A player who is still online keeps the marker they had and is moved to where
 * they are now.
 */
function drawPlayers() {
  const seen = new Set();

  for (const player of players) {
    const uid = String(player.Uid || player.Name);
    seen.add(uid);

    const drawn = drawnPlayers.get(uid);
    if (drawn) {
      drawn.marker.setLatLng(at(player.X, player.Z));
      // Only the parts that can change: where they are, which way they are
      // looking, and what the popup says, which is written into the popup that
      // is already there rather than over the top of it.
      turn(drawn.marker, player);
      fillPopup(drawn.saying, player);
      continue;
    }

    const saying = popupFor(player);
    const marker = L.marker(at(player.X, player.Z), {
      icon: pinned('player', `${PLAYER_MARK}<span class="tag">${escaped(player.Name)}</span>`),
      // Above markers: a player is the thing that is moving.
      zIndexOffset: 1000,
    })
      .bindPopup(saying.box)
      .addTo(people);
    turn(marker, player);
    drawnPlayers.set(uid, { marker, saying });
  }

  for (const [uid, drawn] of drawnPlayers) {
    if (!seen.has(uid)) {
      people.removeLayer(drawn.marker);
      drawnPlayers.delete(uid);
    }
  }
}

/**
 * A picture hung on the map at a position, with nothing between the two.
 *
 * Leaflet's own answer for a picture it did not draw is a twelve pixel box
 * offset by half of itself, so everything inside one is measured from a corner
 * six pixels up and to the left of the place it is about — which is where every
 * mark on this map used to sit. A box of no size puts the position itself where
 * the stylesheet can find it, and each mark centres itself on it from there.
 */
function pinned(className, html) {
  return L.divIcon({ className, iconSize: [0, 0], html });
}

/**
 * The mark a player is drawn as: the game's own, so somebody who has read the
 * map in the client already knows what it means.
 *
 * A circle for the player with a cone over it for the way they are looking, and
 * the paths are the client's — transcribed from `IconUtil.DrawMapPlayer`, which
 * is what the game's own world map draws a player with. Each of the two shapes
 * is filled and outlined, and the two after them fill the insides again to hide
 * the halves of those outlines that fall where the circle and the cone cross.
 *
 * Drawn looking north, because north is up on this map and a bearing of zero is
 * north. Everything else is the turn `turn` gives it.
 */
const PLAYER_MARK = `<svg class="pin" viewBox="0 0 26 39">
  <path class="shape cone" d="M19.75 14.835938C19.75 7.820313 16.726563 2.132813 13 2.132813C9.273438 2.132813 6.25 7.820313 6.25 14.835938C6.25 21.851563 9.273438 27.539063 13 27.539063C16.726563 27.539063 19.75 21.851563 19.75 14.835938Z"/>
  <path class="shape" d="M24.015625 25.851563C24.015625 31.933594 19.082031 36.867188 13 36.867188C6.917969 36.867188 1.984375 31.933594 1.984375 25.851563C1.984375 19.769531 6.917969 14.835938 13 14.835938C19.082031 14.835938 24.015625 19.769531 24.015625 25.851563Z"/>
  <path class="inner" d="M11.558594 34.703125C9.667969 34.386719 7.675781 33.269531 6.339844 31.769531C2.125 27.050781 4 19.707031 10 17.4375C11.214844 16.980469 13.53125 16.816406 14.878906 17.097656C17.867188 17.71875 20.46875 20.09375 21.511719 23.164063C21.960938 24.472656 22.015625 26.71875 21.636719 28.167969C20.519531 32.460938 15.929688 35.433594 11.558594 34.703125Z"/>
  <path class="inner cone" d="M17.296875 13.511719C17.160156 13.433594 16.519531 13.253906 15.871094 13.105469C13.867188 12.65625 11.019531 12.789063 8.785156 13.433594L8.386719 13.550781L8.492188 12.335938C8.722656 9.628906 9.820313 6.734375 11.15625 5.308594C12.140625 4.25 12.890625 3.988281 13.753906 4.398438C15.496094 5.226563 17.199219 8.871094 17.5625 12.554688C17.621094 13.164063 17.640625 13.660156 17.605469 13.65625C17.570313 13.652344 17.433594 13.589844 17.296875 13.511719Z"/>
</svg>`;

/**
 * Which way a player is looking, in degrees clockwise from north, or null where
 * the mod on the other end does not say.
 *
 * The mod turns the game's yaw into a bearing, so this reads a number and does
 * no arithmetic on it. A mod older than the field says nothing rather than zero,
 * and zero is north — which is a claim about where somebody is looking that
 * nobody made.
 */
function facingOf(player) {
  return Number.isFinite(player.Facing) ? player.Facing : null;
}

/**
 * Turns a player's mark to the way they are looking.
 *
 * The element is asked for each time rather than held: the players are a layer
 * that can be switched off, and Leaflet builds every marker a fresh element when
 * it comes back on, which a held one would outlive.
 */
function turn(marker, player) {
  const mark = marker.getElement() && marker.getElement().querySelector('.pin');
  if (!mark) return;

  const facing = facingOf(player);
  // Nothing said, so the mark says nothing: the cone comes off and what is left
  // is the dot this map drew before it could point anywhere.
  mark.classList.toggle('blind', facing === null);
  mark.style.transform = facing === null ? '' : `rotate(${facing}deg)`;
}

/** What the markers looked like last time, so an unchanged set is left alone. */
let drawnPlaces = null;

/**
 * Draws the markers, when they are not the ones already drawn.
 *
 * Markers change a few times an hour and arrive every two seconds. Rebuilding
 * them on arrival meant tearing down every marker on the map to discover that
 * none of them had moved.
 */
function drawPlaces(waypoints) {
  const shape = JSON.stringify(waypoints);
  if (shape === drawnPlaces) {
    return;
  }
  drawnPlaces = shape;
  // The list of every marker there is shows this same set, so it is drawn from
  // the same moment they changed rather than from a clock of its own.
  listed = waypoints;
  drawDirectory();

  // Every marker is about to be replaced, including whichever one a hover had
  // opened; a wait to close a marker that no longer exists closes nothing.
  forgetHovered();
  places.clearLayers();
  for (const place of waypoints) {
    const picture = markFor(place.Icon, colourOf(place.Color)).outerHTML;
    // Every death marker is titled "You died here", so the owner is the only
    // thing that says whose it is. A marker only its owner is sent says so as
    // well: somebody who ticked the box should be able to see that it took.
    const owner = place.Owner ? `<span class="said-who">${escaped(place.Owner)}</span>` : '';
    const kept = place.Private ? '<span class="said-keep">private</span>' : '';
    const title = escaped(place.Title || 'marker');
    const [x, z] = said(place.X, place.Z);
    const drawn = L.marker(at(place.X, place.Z), {
      icon: pinned('marker', `${picture}<span class="tag">${title}</span>`),
    })
      .bindPopup(
        `<b class="said-name">${title}${kept}</b>` +
        `<span class="said-foot">` +
        `<span class="said-where">${x}, ${place.Y}, ${z}</span>${owner}</span>`)
      .addTo(places);

    // Reading a marker without clicking it, for somebody who has asked for that.
    // The setting is read at the moment of the hover rather than wired in when
    // the marker is drawn, so turning it on reaches the markers already on the
    // map instead of only the next set to arrive.
    drawn.on('mouseover', () => {
      if (!settings.hover.on) return;
      keepHovered();
      hovered = drawn;
      drawn.openPopup();
      linger(drawn);
    });
    drawn.on('mouseout', () => {
      if (hovered === drawn) closeHovered();
    });
    // A marker opened on purpose stays open. Only what a hover put up comes down
    // on its own, and a click is not a hover however it started.
    drawn.on('click', forgetHovered);

    // A right click on a marker opens it rather than opening a new one on top of
    // it. Offered only to somebody who may change it — see `mayEdit`, and the
    // mod, which decides it again for real.
    drawn.on('contextmenu', event => {
      L.DomEvent.preventDefault(event.originalEvent);
      L.DomEvent.stopPropagation(event);
      if (mayEdit(place)) editCompose(place);
    });
  }
}

/** How long a marker's details stay up once the pointer has left it. Long
 *  enough to be crossed on the way to reading them, short enough that a map
 *  swept over is not left covered in boxes. */
const HOVER_LINGER = 2500;

/** The marker a hover opened, and the wait that will close it again. */
let hovered = null;
let hoverTimer = null;

/** Something is still being read, so nothing is closing. */
function keepHovered() {
  clearTimeout(hoverTimer);
  hoverTimer = null;
}

/** Closes what a hover opened, after a moment. */
function closeHovered() {
  clearTimeout(hoverTimer);
  hoverTimer = setTimeout(() => {
    if (hovered) hovered.closePopup();
    hovered = null;
    hoverTimer = null;
  }, HOVER_LINGER);
}

/** Nothing is being hovered any more — the markers themselves have gone. */
function forgetHovered() {
  keepHovered();
  hovered = null;
}

/**
 * Lets the box a hover opened be read.
 *
 * Moving onto the box is not moving away from the marker, but as far as the
 * marker is concerned it is — so the box cancels the wait while a pointer is on
 * it and starts it again on the way out. Wired once per element: Leaflet builds
 * a fresh one on each open, and a box that is somehow reused must not collect a
 * listener every time it is looked at.
 */
function linger(marker) {
  const popup = marker.getPopup();
  const box = popup && popup.getElement && popup.getElement();
  if (!box || box.dataset.linger) return;
  box.dataset.linger = 'yes';
  box.addEventListener('mouseenter', keepHovered);
  box.addEventListener('mouseleave', closeHovered);
}

/** Whose view the map is keeping, if anyone's. */
let following = null;

/**
 * Keeps the map on one player until somebody drags it.
 *
 * Zooming does not stop it: changing how close you are looking is not the same as
 * choosing to look somewhere else. Dragging is, so that ends it — and so does
 * clicking the same player again.
 */
function follow(uid) {
  following = following === uid ? null : uid;
  showFollowed();
  keepUp();
}

function keepUp() {
  if (following === null) {
    return;
  }
  const player = players.find(p => String(p.Uid || p.Name) === following);
  if (player) {
    map.panTo(at(player.X, player.Z), { animate: true, duration: 0.4 });
  }
}

/**
 * Where a player's picture is, or the empty string where they have sent none.
 *
 * The address carries when the picture was drawn, because the name cannot: a
 * portrait is filed under its player, so a player who sends a new one keeps the
 * name they had. An address that never changes is one a browser is entitled to go
 * on using, and a card comparing names alone has no way to notice that the picture
 * behind one has been replaced — which is why a new portrait used to need the page
 * reloading before it showed.
 *
 * The whole address is the thing to compare, so this is what the card's own
 * comparison uses too rather than a second reading of the same two fields.
 */
function portraitSrc(player) {
  if (!player.Portrait) {
    return '';
  }
  return `/portraits/${encodeURIComponent(player.Portrait)}.png?v=${player.PortraitAt || 0}`;
}

/**
 * What goes in a player's card.
 *
 * A picture their own client drew of them where there is one, and their initial
 * where there is not. There is nothing in between worth drawing: what a seraph
 * looks like exists only on the machine rendering it, and everything the server
 * can read about an appearance amounts to less than a letter does.
 */
function portrait(player) {
  // A picture the player's own client drew of them, which is the only place a
  // seraph's real appearance exists: its skin parts are textures a dedicated
  // server does not ship, and its clothes and armour are a rendered scene rather
  // than a list of colours.
  const src = portraitSrc(player);
  if (src) {
    const sent = document.createElement('img');
    sent.src = src;
    sent.alt = '';
    // A picture that will not load leaves a broken image in the card, which reads
    // worse than the letter it stands in for.
    sent.addEventListener('error', () => {
      const box = sent.parentElement;
      if (!box) return;
      sent.remove();
      box.append(initial(player));
    }, { once: true });
    return sent;
  }

  return initial(player);
}

/**
 * A player's initial, for anyone who has not sent a picture of themselves.
 *
 * The map used to assemble a face here out of the three colours a server can read
 * off a player — skin, hair, eyes. It was a guess at a likeness and looked like
 * one. A letter says exactly as much as the map actually knows.
 */
function initial(player) {
  const letter = document.createElement('span');
  letter.textContent = (player.Name || '?').trim().charAt(0).toUpperCase() || '?';
  return letter;
}


/** An empty bar, to be filled in later. */
function bar(kind) {
  const wrap = document.createElement('div');
  wrap.className = `bar ${kind}`;
  const inner = document.createElement('i');
  wrap.append(inner);
  return { wrap, inner, at: null };
}

/** A reading, written only when it is not the reading already there. */
function fill(meter, what, value, most) {
  const share = most > 0 ? Math.max(0, Math.min(1, value / most)) : 0;
  const width = `${(share * 100).toFixed(1)}%`;
  if (meter.at === width) {
    return;
  }
  meter.at = width;
  meter.inner.style.width = width;
  meter.wrap.title = most > 0 ? `${what} ${Math.round(value)} of ${Math.round(most)}` : '';
}

/**
 * What a click on a player says: who they are, with their own face, and where
 * they are standing.
 *
 * The shape a click on a marker already answers in — a name for a heading and
 * what is true of it under a rule — because a dot and a mark are two things on
 * one map and being told about them in two different shapes is the reader's
 * problem, not the map's.
 *
 * Built once and kept. The poll that moves everybody arrives every two seconds,
 * and a popup written out again each time would throw the picture away and ask
 * for it back — which is the rule the cards in the panel already follow.
 */
function popupFor(player) {
  const box = document.createElement('div');
  const head = document.createElement('b');
  head.className = 'said-name said-head';
  const face = document.createElement('span');
  face.className = 'face said-face';
  const called = document.createElement('span');
  called.className = 'said-called';
  head.append(face, called);

  const foot = document.createElement('span');
  foot.className = 'said-foot';
  const where = document.createElement('span');
  where.className = 'said-where';
  foot.append(where);

  box.append(head, foot);
  const saying = { box, face, called, where, look: null };
  fillPopup(saying, player);
  return saying;
}

/**
 * Says who and where, and draws the face again only when it is a different one.
 *
 * A portrait is a request, so it is asked for when the picture or the name it
 * stands in for has changed and at no other time — the same reading the cards
 * are kept from flickering by.
 */
function fillPopup(saying, player) {
  const [x, z] = said(player.X, player.Z);
  saying.called.textContent = player.Name;
  saying.where.textContent = `${x}, ${player.Y}, ${z}`;

  const look = `${portraitSrc(player)}|${player.Name}`;
  if (saying.look === look) return;
  saying.look = look;
  saying.face.textContent = '';
  saying.face.append(portrait(player));
}

/**
 * Titles and names come from players, so they are text and never markup.
 *
 * Quotes are escaped along with the angle brackets. Every use of this today puts
 * its answer between tags, where a quote is harmless — but the marker popups are
 * built as strings and the first one written into an attribute would otherwise
 * be a hole nobody could see from the call site.
 */
function escaped(text) {
  return String(text ?? '').replace(/[&<>"']/g, mark => `&#${mark.charCodeAt(0)};`);
}
