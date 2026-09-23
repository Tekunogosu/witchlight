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
    icons = new Set(await (await fetch('/icons')).json());
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
    const offered = await (await fetch('/colors')).json();
    if (Array.isArray(offered) && offered.length > 0) palette = offered;
  } catch (error) {
    /* the service may be restarting */
  }
}

/**
 * Reads what moves constantly: who is online and what time it is.
 *
 * The markers and the claims are not here. Each is read from its own address,
 * on the beat the service says it moved, and once at the start. See
 * `takeWhatMoved` and `firstReading`.
 */
async function pollLive() {
  if (pushed) return;
  try {
    const live = await (await fetch('/live')).json();
    await takeLive(live);
  } catch (error) {
    /* the service may be restarting */
  }
}

/**
 * Takes the markers the page now holds: draws them and answers what was asked.
 *
 * Shared by the whole reading from `/live` and by the markers' own address, so
 * what happens when markers arrive is written once however they arrived.
 */
async function markersArrived() {
  // A marker naming a picture nobody has heard of means the set has grown.
  if (waypointsHeld.some(place => place.Icon && !icons.has(String(place.Icon)))) {
    await pollIcons();
    // The pictures changed, so what is drawn no longer matches what was drawn,
    // and the form's picker is short of one.
    drawnPlaces = null;
    if (composer.classList.contains('open')) drawPictures();
  }

  // The one honest confirmation there is: the marker this page asked for is now
  // among the markers the service is sending, which means the game made it.
  if (awaiting) {
    if (arrived(waypointsHeld)) landed();
    else if (Date.now() - askedAt > MARKER_PATIENCE) await lost();
  }

  drawPlaces(waypointsHeld);
  // The form may be open on a marker whose pin was set from another browser, or
  // refused by the game since it was pressed. The mark is drawn from what
  // arrived rather than from what was asked for.
  if (composer.classList.contains('open')) showPin();
}

/**
 * Takes one reading of the live feed, however it arrived.
 *
 * The feed arrives in parts and a part that did not move is absent, so each is
 * read only where it is present. A whole reading from `/live` carries every
 * part and sets all of them. Reading an absent part as empty would clear the
 * markers every time the clock ticked.
 */
async function takeLive(live) {
  try {
    if ('Players' in live) {
      players = live.Players || [];
      // How many are on, and who of them is in a group with whoever is asking.
      // Both are worked out by the mod and passed through per viewer, because a
      // browser cannot be asked to hide what it has already been handed.
      online = Number.isFinite(live.Online) ? live.Online : players.length;
      grouped = new Set(live.Grouped || []);
      playerColours = (live.Colors && typeof live.Colors === 'object') ? live.Colors : {};
    }
    // Both from the same part: the claims this reader may be sent, and whether
    // the mod says they may draw one. The second rides the live feed rather than
    // `/me` because it is the mod's answer and arrives when the mod does —
    // a page opened before the game server was up learns it on the next beat
    // instead of needing a reload.
    if ('Claims' in live) {
      claims = live.Claims || [];
      allowance = live.Claiming || null;
      worldHeight = Number.isFinite(live.Height) ? live.Height : worldHeight;
    }
    if ('World' in live) showWhen(live.World);
    // The pins travel with the markers, so they are read together. Which of
    // them this reader keeps in sight in game is sent to whoever set them and to
    // nobody else, so what arrives is already this reader's own answer — except
    // where this page has just asked for one and the game has not answered yet,
    // which `takePins` is what holds.
    if ('Waypoints' in live) {
      waypointsHeld = live.Waypoints || [];
      takePins(live.Pins);
    }

    await markersArrived();

    drawPlayers();
    // How far away every listed marker is moves with the reader rather than with
    // the markers, so it is written on this beat rather than on a redraw.
    showDistances();
    drawWho();
    keepUp();
    drawClaims(claims);
    showClaims();
    watchClaim();
    // On the same beat the markers are, so a plugin hears that something moved
    // when the page does rather than on a clock of its own — and is handed what
    // arrived, so it does not fetch the same answer a second time.
    pluginsChanged(live);
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
    const info = await (await fetch(`/info${query}`, { cache: 'no-store' })).json();
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
    noteChanges(generation, info.tiles);
    if (info.tiles) terrain?.refresh(info.tiles);
    else if (!grew) terrain?.refreshAll();

    // The block under a resting pointer may be a different block now. What was
    // said about it was true of the map before this export, so it is asked again.
    told = null;
    started(ask(), 'looking up the block under the pointer');
    // The ground itself moved, which is a different thing from the live beat and
    // on a far slower clock. A plugin drawing anything worked out from the
    // terrain had no way to hear about this at all.
    pluginsRedrew({ generation, bounds });
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

/**
 * What this page holds of each part of the live feed, as the service numbers
 * them.
 *
 * One number per part rather than one for the feed, because the parts move at
 * unrelated rates: the world's clock ticks every second and the markers change a
 * few times an hour. Sent back on each wait, and the service answers with the
 * parts that have moved past them. See the service's `events.rs`.
 */
let liveSeqs = { players: 0, markers: 0, claims: 0, world: 0, plugins: 0 };

/**
 * How long to leave between reading the parts served on their own address.
 *
 * Only ever used where the wait on `/events` is not running, so this is the
 * fallback's clock rather than the ordinary one. Slower than the live beat
 * because markers and claims change a few times an hour.
 */
const HELD_BEAT = 15000;

/**
 * Reads the markers and the claims, for a page that is not being told.
 *
 * Does nothing while the service is telling this page of changes, because then
 * each is read on the beat it moved and reading again on a clock would ask for
 * what the page already holds.
 */
async function pollHeld() {
  if (pushed) return;
  await pollMarkers();
  await pollClaims();
}

/**
 * Reads everything once, at the start.
 *
 * The parts served on their own address are not sent again until they change,
 * and a page that has just opened holds none of them. Two reads at load are what
 * that costs, against sending the same markers on every beat forever.
 */
async function firstReading() {
  await pollLive();
  await pollMarkers();
  await pollClaims();
}

/**
 * Fetches the parts that are served on their own address, where they moved.
 *
 * Called with the sequences the page held before the wait answered. A part whose
 * number has moved since is read from its own address; one that has not is left
 * alone, which is the ordinary case for both of these.
 */
async function takeWhatMoved(was) {
  if (liveSeqs.markers > was.markers) await pollMarkers();
  if (liveSeqs.claims > was.claims) await pollClaims();
}

/** Reads the markers this person may see, and which of them they pin. */
async function pollMarkers() {
  try {
    const sent = await (await fetch('/markers')).json();
    waypointsHeld = sent.Waypoints || [];
    takePins(sent.Pins);
    await markersArrived();
  } catch (error) {
    /* the service may be restarting */
  }
}

/** Reads the claims this person may see, and what they are allowed. */
async function pollClaims() {
  try {
    const sent = await (await fetch('/claims')).json();
    claims = sent.Claims || [];
    allowance = sent.Claiming || null;
    worldHeight = Number.isFinite(sent.Height) ? sent.Height : worldHeight;
    drawClaims(claims);
    showClaims();
  } catch (error) {
    /* the service may be restarting */
  }
}

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
      const was = { ...liveSeqs };
      const held = new URLSearchParams({ since: String(generation) });
      for (const [part, seq] of Object.entries(liveSeqs)) held.set(part, String(seq));
      const answer = await fetch(`/events?${held}`, { cache: 'no-store' });
      if (!answer.ok) throw new Error(String(answer.status));
      const moved = await answer.json();
      pushed = true;
      if (moved.seqs && typeof moved.seqs === 'object') {
        for (const part of Object.keys(liveSeqs)) {
          if (Number.isFinite(moved.seqs[part])) liveSeqs[part] = moved.seqs[part];
        }
      }
      if (moved.info) takeInfo(moved.info);
      if (moved.live) await takeLive(moved.live);
      // The markers and the claims are served on their own addresses and are
      // fetched when the wait says they moved. They are the bulk of what there
      // is to send and change a few times an hour, so they are asked for on the
      // beat they changed rather than carried on every beat.
      await takeWhatMoved(was);
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
buildHotkeys();
// After every bar that hangs in the tool column exists, including the map's own
// zoom and the block picker — which are Leaflet's and are moved into it.
gatherCorner();
// After the column exists, because a plugin's button hangs in it, and after
// `pollMe` has answered rather than merely started: a plugin reads `wl.me()` in
// its own `start`, and one that ran first would be told there is nobody looking
// and would quietly draw nothing that depends on who that is.
started(pollMe().then(loadPlugins), 'reading who is signed in, then loading plugins');
for (const setting of Object.values(settings)) setting.apply(setting.on);

// A beat rather than an interval: each answer is waited for before the next
// question is counted, so a service slower than the gap is not asked twice over.
beat(pollLive, LIVE_BEAT, 'the live poll');
// The parts served on their own address are read on the wait's word, and the
// wait is what a page on this clock has lost. Read on a clock of their own
// instead, slower, because they change a few times an hour and this is the
// fallback rather than the ordinary way.
beat(pollHeld, HELD_BEAT, 'the markers and claims');
// What this person has set, which the game can change as well as this page — see
// `watchMine`. Slower, because presets change a few times a day.
beat(watchMine, 15000, 'what this person has set');
beat(pollWorld, 2000, 'the terrain poll');
started(pollWorld().then(pollIcons).then(pollColours).then(firstReading), 'the first poll');
started(pushLoop(), 'waiting on the service for changes');
