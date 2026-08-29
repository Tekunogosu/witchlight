// The panel of who is online, down the right hand side.
//
// The map answers where somebody is. This answers who there is, which is the
// question somebody opens a map of a server to ask — and the two are not the same
// list: an operator can decide that where a player is standing is their own
// group's business, and then the map draws fewer people than are on.
//
// Nothing here is rebuilt. Cards are made once and written into, so the panel
// keeping up with the world never disturbs what is on the screen.

/** Cards on the panel, by the player they stand for. */
const cards = new Map();

/** Where the cards go. The panel around it holds the tabs and the note, so a
 *  list with nothing in it is still a panel that says why. */
const whoList = document.getElementById('who-list');

/**
 * Which of the two lists of players is showing.
 *
 * All to begin with, because a map is opened to see what is going on. Somebody
 * playing with two other people switches once and it stays switched.
 */
let whoTab = 'all';

/** Whether this player is one of the ones the Group tab keeps. */
function inGroup(player) {
  return grouped.has(String(player.Uid || ''));
}

/** The players the panel is currently showing. */
function listedPlayers() {
  return whoTab === 'group' ? players.filter(inGroup) : players;
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
  const shown = listedPlayers();

  for (const player of shown) {
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

  for (const tab of who.querySelectorAll('.tab')) {
    tab.querySelector('.tally').textContent =
      tab.dataset.who === 'group' ? players.filter(inGroup).length : players.length;
  }
  sayWho(shown.length);
}

/**
 * What the panel says when it is showing fewer people than are on.
 *
 * There is more than one way for a list of players to be short and they are not
 * the same thing, so they are not said the same way: a server that does not share
 * positions has decided something, and a group tab with nobody in it is a
 * question of who you are playing with. Said only when the list cannot answer for
 * itself, because a line that is always there is a line nobody reads.
 *
 * Nobody on at all is neither of those. It is a quiet server, the corner readout
 * already says so, and a panel explaining that there is nothing to show is
 * furniture doing nothing — so the whole thing takes itself off the map.
 */
function sayWho(shown) {
  const note = document.getElementById('who-said');
  const hidden = viewer && viewer.PlayersPublic === false;

  who.classList.toggle('quiet', shown === 0 && online === 0);
  if (shown > 0 || online === 0) {
    note.textContent = '';
    return;
  }

  if (whoTab === 'group') {
    note.textContent = viewer && viewer.Name
      ? `Nobody from your group is on. ${online} on the server.`
      : 'Sign in to see who is on with you.';
    return;
  }
  note.textContent = hidden
    ? `${online} online. This server shares where somebody is standing with their own group only.`
    : `${online} online.`;
}

/** Marks the card of whoever the map is keeping itself on, if anyone. */
function showFollowed() {
  for (const [uid, card] of cards) {
    card.element.classList.toggle('followed', uid === following);
  }
}

/** Shows one of the two lists of players, and says which. */
function chooseWho(which) {
  whoTab = which === 'group' ? 'group' : 'all';
  for (const tab of who.querySelectorAll('.tab')) {
    const on = tab.dataset.who === whoTab;
    tab.classList.toggle('on', on);
    tab.setAttribute('aria-selected', String(on));
  }
  // The cards are made from whichever list is showing, so the ones that no
  // longer belong have to go before the panel is read again. The dots on the map
  // are not touched: which players the map draws is what the Players switch is
  // for, and two controls over one thing is one of them being ignored.
  drawWho();
}

function buildWho() {
  for (const tab of who.querySelectorAll('.tab')) {
    tab.addEventListener('click', () => chooseWho(tab.dataset.who));
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
  whoList.append(element);

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

