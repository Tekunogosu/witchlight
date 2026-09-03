// Asking the service what has changed, and starting the page.
//
// The service is asked once and then told: `/events` is a request the service
// holds until the map or the live feed has moved past what this page last saw,
// and answers with exactly what a poll of each would have — see the service's
// `events.rs`. The page asks again the moment it is answered, so a change
// reaches the screen within a round trip of arriving at the service.
//
// Two clocks stay as the fallback: markers and players on the beat the operator
// set, terrain every two seconds. They run only while the waiting request is
// not, which is a proxy that will not hold a request open, or a service that has
// too many browsers waiting already. The things that only change when a mod set
// does are asked once at load.

/**
 * How long to leave between asking where everybody is.
 *
 * The operator's `live_refresh_ms`, served with the page and already held to a
 * gap a browser can keep up with, so nothing here has a second opinion about
 * what a sensible number is.
 */
const LIVE_BEAT = window.witchlight.refresh;

/**
 * Which marker pictures exist.
 *
 * Asked once at start and again whenever markers arrive naming one that is not
 * known yet — which is what happens when a mod adding markers is installed while
 * the map is open.
 */
async function pollIcons() {
  try {
    icons = new Set(await (await fetch('/icons.json')).json());
  } catch (error) {
    /* the service may be restarting */
  }
}

/**
 * Which colours the game offers.
 *
 * Asked once at start and again whenever the form is opened without them, which
 * is what happens when the page loaded before the mod had posted anything.
 */
async function pollColours() {
  try {
    const offered = await (await fetch('/colors.json')).json();
    if (Array.isArray(offered) && offered.length > 0) palette = offered;
  } catch (error) {
    /* the service may be restarting */
  }
}

/** Players move constantly; markers rarely. Both are cheap to fetch. */
async function pollLive() {
  if (pushed) return;
  try {
    const live = await (await fetch('/live.json')).json();
    await takeLive(live);
  } catch (error) {
    /* the service may be restarting */
  }
}

/** Takes one reading of the live feed, however it arrived. */
async function takeLive(live) {
  try {
    players = live.Players || [];
    // How many are on, and who of them is in a group with whoever is asking.
    // Both are worked out by the mod and passed through per viewer, because a
    // browser cannot be asked to hide what it has already been handed.
    online = Number.isFinite(live.Online) ? live.Online : players.length;
    grouped = new Set(live.Grouped || []);
    // Both from the same post: the claims this reader may be sent, and whether
    // the mod says they may draw one. The second rides the live poll rather than
    // `/me.json` because it is the mod's answer and arrives when the mod does —
    // a page opened before the game server was up learns it on the next beat
    // instead of needing a reload.
    claims = live.Claims || [];
    allowance = live.Claiming || null;
    worldHeight = Number.isFinite(live.Height) ? live.Height : worldHeight;
    showWhen(live.World);
    const waypoints = live.Waypoints || [];
    // Which of them this reader keeps in sight in game. Sent to whoever set them
    // and to nobody else, so what arrives is already this reader's own answer —
    // except where this page has just asked for one and the game has not
    // answered yet, which `takePins` is what holds.
    takePins(live.Pins);

    // A marker naming a picture nobody has heard of means the set has grown.
    if (waypoints.some(place => place.Icon && !icons.has(String(place.Icon)))) {
      await pollIcons();
      // The pictures changed, so what is drawn no longer matches what was drawn,
      // and the form's picker is short of one.
      drawnPlaces = null;
      if (composer.classList.contains('open')) drawPictures();
    }

    // The one honest confirmation there is: the marker this page asked for is
    // now among the markers the service is sending, which means the game made it.
    if (awaiting) {
      if (arrived(waypoints)) landed();
      else if (Date.now() - askedAt > MARKER_PATIENCE) await lost();
    }

    drawPlayers();
    // How far away every listed marker is moves with the reader rather than with
    // the markers, so it is written on this beat rather than on a redraw.
    showDistances();
    drawWho();
    keepUp();
    drawPlaces(waypoints);
    // The form may be open on a marker whose pin was set from another browser,
    // or refused by the game since it was pressed. The mark is drawn from what
    // arrived rather than from what was asked for.
    if (composer.classList.contains('open')) showPin();
    drawClaims(claims);
    showClaims();
    watchClaim();
    say();
  } catch (error) {
    /* the service may be restarting */
  }
}

/**
 * Watches for terrain that has changed.
 *
 * Asking with `since` gets back the tiles that actually changed rather than a
 * bare "something did", so a server where one person is building repaints one
 * square instead of the map.
 */
async function pollWorld() {
  if (pushed && generation !== 0) return;
  try {
    const query = generation === 0 ? '' : `?since=${generation}`;
    const info = await (await fetch(`/info.json${query}`, { cache: 'no-store' })).json();
    takeInfo(info);
  } catch (error) {
    /* the service may be restarting; try again next time */
  }
}

/** Takes one reading of where the map stands, however it arrived. */
function takeInfo(info) {
  try {
    if (info.generation === generation) return;

    const grew = terrain === null
      || info.levels !== levels
      || info.minX < bounds.minX || info.maxX > bounds.maxX
      || info.minZ < bounds.minZ || info.maxZ > bounds.maxZ;

    generation = info.generation;
    chunks = info.chunks;
    levels = info.levels ?? 0;
    chunkEdge = info.chunk ?? 0;
    spawn = { x: info.spawnX ?? 0, z: info.spawnZ ?? 0 };

    // The four edges by name, rather than every field the service happened to
    // send. Copying the lot left `bounds` holding a generation, a tile list and
    // a chunk count — none of which is an edge, and any of which a later field
    // named like one would have silently become.
    bounds.minX = info.minX;
    bounds.minZ = info.minZ;
    bounds.maxX = info.maxX;
    bounds.maxZ = info.maxZ;

    // Growing and changing are not the same thing. A world that has grown needs
    // its edges moved, and the tiles that changed still need replacing — usually
    // both at once, since the export that added a region also drew it.
    if (grew) resize();
    if (info.tiles) terrain?.refresh(info.tiles);
    else if (!grew) terrain?.refreshAll();

    // The block under a resting pointer may be a different block now. What was
    // said about it was true of the map before this export, so it is asked again.
    told = null;
    started(ask(), 'looking up the block under the pointer');
    say();
  } catch (error) {
    /* a reading the page could not take is one the next will replace */
  }
}

/**
 * Whether the service is telling this page of changes as they happen, which is
 * what lets the two clocks below stand down.
 */
let pushed = false;

/** The live feed's own clock, as the service numbers it. */
let liveSeq = 0;

/**
 * Waits on the service for the next change, takes it, and waits again.
 *
 * Nothing is asked while the map's first reading is still on its way: the wait
 * says what changed since a generation, and until there is one there is
 * nothing to be since. A refusal — too many browsers waiting, or a proxy that
 * would not hold the request — leaves the clocks running and tries again
 * later, so a page never goes quiet for want of this.
 */
async function pushLoop() {
  for (;;) {
    if (generation === 0) {
      await new Promise(resolve => setTimeout(resolve, 500));
      continue;
    }
    try {
      const answer = await fetch(`/events?since=${generation}&live=${liveSeq}`, { cache: 'no-store' });
      if (!answer.ok) throw new Error(String(answer.status));
      const moved = await answer.json();
      pushed = true;
      liveSeq = Number.isFinite(moved.liveSeq) ? moved.liveSeq : liveSeq;
      if (moved.info) takeInfo(moved.info);
      if (moved.live) await takeLive(moved.live);
    } catch (error) {
      pushed = false;
      await new Promise(resolve => setTimeout(resolve, 5000));
    }
  }
}

recall();
applyScales();
buildSettings();
buildWho();
buildWindows();
buildCompose();
buildPresets();
buildDirectory();
buildProfile();
buildClaims();
// After every bar that hangs in the tool column exists, including the map's own
// zoom and the block picker — which are Leaflet's and are moved into it.
gatherCorner();
started(pollMe(), 'reading who is signed in');
for (const setting of Object.values(settings)) setting.apply(setting.on);

// A beat rather than an interval: each answer is waited for before the next
// question is counted, so a service slower than the gap is not asked twice over.
beat(pollLive, LIVE_BEAT, 'the live poll');
// What this person has set, which the game can change as well as this page — see
// `watchMine`. Slower, because presets change a few times a day.
beat(watchMine, 15000, 'what this person has set');
beat(pollWorld, 2000, 'the terrain poll');
started(pollWorld().then(pollIcons).then(pollColours).then(pollLive), 'the first poll');
started(pushLoop(), 'waiting on the service for changes');
