// Who is online: dots on the map, and a card each beside it.
//
// Nothing here is redrawn for being sent again. A card is built once and
// patched afterwards, and the markers are compared as a set — the service posts
// every player every two seconds and almost none of them has changed.

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
      // Only the parts that can change: the popup carries their position.
      drawn.marker.setPopupContent(popupFor(player));
      continue;
    }

    const marker = L.marker(at(player.X, player.Z), {
      icon: L.divIcon({
        className: 'player',
        html: `<i class="pin"></i><span class="tag">${escaped(player.Name)}</span>`,
      }),
      // Above markers: a player is the thing that is moving.
      zIndexOffset: 1000,
    })
      .bindPopup(popupFor(player))
      .addTo(people);
    drawnPlayers.set(uid, { marker });
  }

  for (const [uid, drawn] of drawnPlayers) {
    if (!seen.has(uid)) {
      people.removeLayer(drawn.marker);
      drawnPlayers.delete(uid);
    }
  }
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

  // Every marker is about to be replaced, including whichever one a hover had
  // opened; a wait to close a marker that no longer exists closes nothing.
  forgetHovered();
  places.clearLayers();
  for (const place of waypoints) {
    const colour = /^#[0-9a-f]{6}$/i.test(place.Color || '') ? place.Color : '#ffffff';
    const name = String(place.Icon || '');
    // A picture where the service has one, and a plain shape where it does not —
    // a hole is what a missing icon would otherwise leave.
    const picture = icons.has(name)
      ? `<i class="icon masked" style="background:${colour};` +
        `-webkit-mask-image:url(/icons/${encodeURIComponent(name)}.svg);` +
        `mask-image:url(/icons/${encodeURIComponent(name)}.svg)"></i>`
      : `<i class="diamond" style="background:${colour}"></i>`;
    // Every death marker is titled "You died here", so the owner is the only
    // thing that says whose it is. A marker only its owner is sent says so as
    // well: somebody who ticked the box should be able to see that it took.
    const owner = place.Owner ? `<span class="said-who">${escaped(place.Owner)}</span>` : '';
    const kept = place.Private ? '<span class="said-keep">private</span>' : '';
    const title = escaped(place.Title || 'marker');
    const [x, z] = said(place.X, place.Z);
    const drawn = L.marker(at(place.X, place.Z), {
      icon: L.divIcon({
        className: 'marker',
        html: `${picture}<span class="tag">${title}</span>`,
      }),
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

/** Cards on the panel, by the player they stand for. */
const cards = new Map();

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
  for (const [id, card] of cards) {
    card.element.classList.toggle('followed', id === following);
  }
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
 * Who is online, down the right hand side.
 *
 * Health and food come from the server, which already knows them, so nothing is
 * asked of anyone to show them. Clicking a card follows that player until the map
 * is dragged.
 *
 * Nothing here is rebuilt. Cards are made once and then written into, so the
 * panel keeping up with the world never disturbs what is on the screen.
 */
function drawWho() {
  const seen = new Set();

  for (const player of players) {
    const uid = String(player.Uid || player.Name);
    seen.add(uid);
    patchCard(cards.get(uid) || newCard(uid), player);
  }

  for (const [uid, card] of cards) {
    if (!seen.has(uid)) {
      card.element.remove();
      cards.delete(uid);
      if (following === uid) following = null;
    }
  }
}

/** A card, built once. What changes on it afterwards is set, not rebuilt. */
function newCard(uid) {
  const element = document.createElement('div');
  element.className = 'card';

  const face = document.createElement('div');
  face.className = 'face';
  const details = document.createElement('div');
  const name = document.createElement('div');
  name.className = 'name';
  const hp = bar('hp');
  const food = bar('food');
  details.append(name, hp.wrap, food.wrap);
  element.append(face, details);
  element.addEventListener('click', () => follow(uid));
  who.append(element);

  const card = { element, face, name, hp, food, look: null, named: null };
  cards.set(uid, card);
  return card;
}

/**
 * Brings one card up to date.
 *
 * Every field is compared before it is written. The panel shares the two second
 * beat the map data arrives on, and a player who is walking changes their
 * position and their food on every one of them — so rebuilding a card whenever
 * anything about its player moved meant rebuilding all of them, always. Setting a
 * width does not disturb the screen; replacing an element does.
 */
function patchCard(card, player) {
  if (card.named !== player.Name) {
    card.named = player.Name;
    card.name.textContent = player.Name;
  }

  const look = `${portraitSrc(player)}|${player.Name}`;
  if (card.look !== look) {
    card.look = look;
    card.face.textContent = '';
    card.face.append(portrait(player));
  }

  fill(card.hp, 'health', player.Health, player.MaxHealth);
  fill(card.food, 'food', player.Saturation, player.MaxSaturation);

  const [x, z] = said(player.X, player.Z);
  const title = `${player.Name} — ${x}, ${player.Y}, ${z}`;
  if (card.element.title !== title) {
    card.element.title = title;
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

function popupFor(player) {
  const [x, z] = said(player.X, player.Z);
  return `<b>${escaped(player.Name)}</b><br>${x}, ${player.Y}, ${z}`;
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
