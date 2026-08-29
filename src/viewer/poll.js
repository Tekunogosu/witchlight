// Asking the service what has changed, and starting the page.
//
// Three clocks: markers, players and terrain every two seconds, and the things
// that only change when a mod set does asked once at load.
//
// Terrain used to be asked for every five. Nothing about the question is
// expensive — `?since=` answers with the tiles that moved and usually with none
// — and five seconds of it sat on top of every other wait between a player
// walking into new ground and seeing it drawn.

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
  try {
    const live = await (await fetch('/live.json')).json();
    players = live.Players || [];
    // How many are on, and who of them is in a group with whoever is asking.
    // Both are worked out by the mod and passed through per viewer, because a
    // browser cannot be asked to hide what it has already been handed.
    online = Number.isFinite(live.Online) ? live.Online : players.length;
    grouped = new Set(live.Grouped || []);
    showWhen(live.World);
    const waypoints = live.Waypoints || [];

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
    drawWho();
    keepUp();
    drawPlaces(waypoints);
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
  try {
    const query = generation === 0 ? '' : `?since=${generation}`;
    const info = await (await fetch(`/info.json${query}`, { cache: 'no-store' })).json();
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
    /* the service may be restarting; try again next time */
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
started(pollMe(), 'reading who is signed in');
for (const setting of Object.values(settings)) setting.apply(setting.on);

// A beat rather than an interval: each answer is waited for before the next
// question is counted, so a service slower than the gap is not asked twice over.
beat(pollLive, 2000, 'the live poll');
beat(pollWorld, 2000, 'the terrain poll');
started(pollWorld().then(pollIcons).then(pollColours).then(pollLive), 'the first poll');
