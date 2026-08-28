//! Serving the map.
//!
//! Tiles are rendered when asked for and kept, so starting up costs nothing and
//! only the part of the world someone actually looks at is ever drawn.

use std::collections::{HashMap, HashSet, VecDeque};
use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, SystemTime};

use image::RgbImage;
use tiny_http::{Header, Method, Request, Response, Server};

use crate::api::Api;
use crate::auth::{Sessions, Who};
use crate::columns::{Region, World, columns_dir, region_coords, region_files};
use crate::live::Live;
use crate::pending::{Edit, Pending, Wanted};
use crate::preferences::{Person, Preferences};
use crate::pyramid;
use crate::render::UNMAPPED;
use crate::error::{Error, Result};
use crate::config::MarkerRules;
use crate::palette::Palette;
use crate::render::{Renderer, Surface};

/// Blocks per tile, and pixels per tile at the finest level: one pixel is one
/// block. Equal to a region, so a region that changes is exactly one tile.
const TILE: u32 = 512;

/// Which tiles one generation changed, as level and coordinates. `None` means
/// every tile: a new palette recolours the lot, and so does a gap in the history.
type Changed = Option<Vec<(u32, i32, i32)>>;

/// How often the levels above zero are rebuilt from what has changed.
///
/// Slower than the watcher on purpose. A region that changes twice in this window
/// costs one rebuild of everything above it rather than two, and the levels are
/// what someone zoomed out is looking at — a second late is not noticeable, and
/// rebuilding eleven levels per change would be.
const BUILD_EVERY: Duration = Duration::from_secs(2);

/// How many generations of tile changes to remember. A viewer polls every few
/// seconds and the mod exports every thirty, so this is minutes of slack; past it
/// a viewer is told to repaint everything rather than lied to.
const HISTORY: usize = 128;

/// The most a single post on the API channel may carry. Positions for a full
/// server are a couple of kilobytes and markers are tens; this only stops a
/// broken poster from being read into memory without limit.
const POST_LIMIT: u64 = 8 * 1024 * 1024;

/// How many blocks a search answers with. A list somebody reads down, not a
/// result set: past a screenful the answer is to type more, not to scroll.
const MOST_BLOCKS_FOUND: usize = 24;

pub fn serve(
    bind: &str,
    data: &Path,
    palette: Palette,
    api: &Api,
    threads: usize,
    cache_mb: usize,
    rules: MarkerRules,
) -> Result<()> {
    let cache_bytes = cache_mb.max(1) * 1024 * 1024;
    let columns = columns_dir(data);
    let state = Arc::new(State {
        world: RwLock::new(World::load(data)?),
        palette: RwLock::new(palette),
        seen: Mutex::new(modified(&columns)),
        regions: Mutex::new(region_times(&columns)),
        painted: Mutex::new(modified(&data.join("palette.json"))),
        live: Arc::new(Live::load(data)),
        sessions: Arc::new(Sessions::new()),
        pending: Arc::new(Pending::new()),
        preferences: Arc::new(Preferences::load(data)),
        names: RwLock::new(block_names(data)),
        named: Mutex::new(modified(&data.join("blocknames.json"))),
        rules,
        generation: AtomicU64::new(1),
        history: Mutex::new(VecDeque::new()),
        stale: Mutex::new(HashSet::new()),
        cache: Mutex::new(Cache::new(cache_bytes)),
        data: data.to_path_buf(),
        columns,
    });

    // The map is the product and live data is a garnish, so an API channel that
    // will not bind is said out loud and stepped over rather than taken as fatal.
    if let Err(error) = serve_api(
        api,
        Arc::clone(&state.live),
        Arc::clone(&state.sessions),
        Arc::clone(&state.pending),
        data,
    ) {
        eprintln!("witchlight: {error}");
        eprintln!(
            "witchlight: nobody will show on the map. Set `api_bind` to an address \
             this machine has free."
        );
    }

    let server = Server::http(bind).map(Arc::new).map_err(|error| {
        Error::io(
            format!("listening on {bind}"),
            std::io::Error::other(error.to_string()),
        )
    })?;
    let addresses = reachable_at(bind);
    for address in &addresses {
        let note = if only_here(address) { "  (this machine only)" } else { "" };
        println!("witchlight: serving on {address}{note}");
    }
    publish_addresses(data, bind, &addresses);

    let threads = workers(threads);
    println!("witchlight: rendering on {threads} threads");

    // Said out loud, because the alternative is a map whose coordinates quietly
    // disagree with every number the player can read off their own screen — and
    // nothing on either side would look wrong.
    if !data.join("world.json").exists() {
        println!(
            "witchlight: no world.json — coordinates will be absolute rather than \
             counted from spawn, which means the server mod is older than this build"
        );
    }

    // Levels built from a region format this build no longer reads would show
    // terrain that has since been cleared, so they go.
    if pyramid::reset_unless_built_from(data, crate::columns::VERSION) {
        println!("witchlight: the stored levels were built from an older format and have been cleared");
    }

    // What is left is kept. Only regions with a level above them missing or older
    // than the region itself get rebuilt, so a run whose levels are already
    // current starts with nothing to do rather than redrawing a world that has
    // not moved.
    // A palette with no colours in it draws bare ground everywhere. Said before
    // anything is served, because the map that follows is not broken — its
    // colours are missing, and those are two different things to go and fix.
    let blank = state.palette.read().is_ok_and(|palette| palette.paints_nothing());
    if blank {
        println!(
            "witchlight: the palette has no colours at all — the finest zoom will not draw \
             and the stored levels are whatever the last usable palette left behind. \
             An admin joining the game supplies one."
        );
    }

    // Levels drawn with a different palette than the one in use disagree with the
    // level below them, which is a map that changes as it is zoomed. Redrawing
    // them settles it — but only when there is something to redraw them with.
    let drawn_with = pyramid::palette_built_from(data);
    let painting = state.palette.read().ok().map(|palette| palette.fingerprint.clone());
    let repaint = !blank
        && matches!((&drawn_with, &painting), (Some(was), Some(now)) if was != now);
    if repaint {
        println!(
            "witchlight: the stored levels were drawn with a different palette — redrawing them"
        );
    }

    let levels = state.levels();
    if let (Ok(mut stale), Ok(regions)) = (state.stale.lock(), state.regions.lock()) {
        let behind = pyramid::behind(data, &regions, levels);
        println!(
            "witchlight: {} of {} regions need their levels built",
            behind.len(),
            regions.len()
        );
        stale.extend(behind);
        if repaint {
            stale.extend(regions.keys().copied());
        }
    }

    // One watcher, so noticing a new export is not something every request pays
    // for and not something two of them can race each other doing.
    watch(Arc::clone(&state));
    build(Arc::clone(&state));

    let mut others = Vec::with_capacity(threads - 1);
    for _ in 1..threads {
        let server = Arc::clone(&server);
        let state = Arc::clone(&state);
        others.push(std::thread::spawn(move || answer(&server, &state)));
    }

    // This thread works too, rather than standing over the others.
    answer(&server, &state);
    for thread in others {
        let _ = thread.join();
    }

    Ok(())
}

/// Takes requests until the server stops. Every thread runs this, and `recv`
/// hands each request to whichever is free — the whole reason a cold map no
/// longer arrives one tile at a time.
fn answer(server: &Server, state: &State) {
    while let Ok(mut request) = server.recv() {
        let response = route(&mut request, state);
        if let Err(error) = request.respond(response) {
            eprintln!("witchlight: response failed: {error}");
        }
    }
}

/// One request, decided by its path alone: tile URLs carry a `?v=` so that a new
/// export is a new URL, and the query says nothing about what to serve.
fn route(request: &mut Request, state: &State) -> Response<Cursor<Vec<u8>>> {
    let url = request.url().to_owned();
    let path = url.split('?').next().unwrap_or(&url);

    if path == "/leaflet.js" {
        asset(include_str!("vendor/leaflet.js"), "application/javascript")
    } else if path == "/leaflet.css" {
        asset(include_str!("vendor/leaflet.css"), "text/css")
    } else if path == "/" {
        let (min_x, min_z, max_x, max_z) = state.bounds();
        html(&viewer(min_x, min_z, max_x, max_z))
    } else if path == "/login" {
        // The one address that turns a word into a browser somebody knows. It
        // answers with a redirect so the word leaves the address bar at once:
        // what stays in history, in a bookmark and in a pasted link is `/`.
        match link_asked(&url).and_then(|link| state.sessions.redeem(link)) {
            Some(session) => seated(&session),
            None => redirect("/?login=expired", None),
        }
    } else if path == "/logout" {
        state.sessions.forget(&cookies(request));
        redirect("/", Some(format!("{}=; Path=/; Max-Age=0; SameSite=Lax", crate::auth::COOKIE)))
    } else if path == "/me.json" {
        json(&state.me(&cookies(request)))
    } else if path == "/live.json" {
        // Whose markers these are depends on who is asking, and the answer is
        // worked out here rather than sent and filtered on the page: a browser
        // cannot be asked to hide what it has already been handed.
        let who = state.sessions.who(&cookies(request));
        json(&state.live.body(who.as_ref().map(|who| who.uid.as_str())))
    } else if path == "/markers" {
        made(request, state)
    } else if let Some(key) = marker_key(path) {
        changed(request, state, key)
    } else if path == "/me/preferences.json" || path == "/me/preferences" {
        preferences(request, state)
    } else if path == "/blocks.json" {
        json(&state.blocks_like(&decoded(param(&url, "q").unwrap_or_default())))
    } else if path == "/icons.json" {
        json(&state.icons())
    } else if path == "/colors.json" {
        json(&state.live.colors())
    } else if let Some(name) = icon_name(path) {
        match std::fs::read(state.data.join("icons").join(format!("{name}.svg"))) {
            Ok(bytes) => svg(bytes),
            Err(_) => text(404, "no icon by that name"),
        }
    } else if let Some(name) = portrait_name(path) {
        match std::fs::read(state.data.join("portraits").join(format!("{name}.png"))) {
            Ok(bytes) => png(bytes),
            Err(_) => text(404, "nobody by that name has sent a picture"),
        }
    } else if path == "/info.json" {
        json(&state.info(since_of(&url)))
    } else if path == "/block.json" {
        match block_asked(&url).map(|(x, z)| state.block(x, z)) {
            Some(Some(body)) => json(&body),
            Some(None) => text(503, "the map is being reloaded"),
            None => text(400, "name the block with ?x= and ?z="),
        }
    } else if let Some((level, tx, tz)) = tile_coords(path) {
        match state.tile(level, tx, tz) {
            Ok(bytes) => tile_response(bytes),
            // A tile nobody has built is missing, not broken. Saying so lets a
            // viewer draw around it rather than treat the map as failing, and
            // keeps a real failure worth noticing.
            Err(Error::Empty(why)) => text(404, &why),
            Err(error) => text(500, &format!("render failed: {error}")),
        }
    } else {
        text(404, "not found")
    }
}

/// What the game calls each block, as the mod last exported it.
///
/// Absent or unreadable is no names rather than a failure: everything that asks
/// falls back to the block's code, which is what the page showed before there
/// were names at all.
fn block_names(data: &Path) -> HashMap<String, String> {
    std::fs::read_to_string(data.join("blocknames.json"))
        .ok()
        .and_then(|body| serde_json::from_str(&body).ok())
        .unwrap_or_default()
}

/// A marker somebody asked for on the map's own form.
///
/// The one thing the public port accepts rather than serves. It is a write, so it
/// needs to know who is asking, and the session cookie is the whole of that — the
/// same proof that decides whose private markers a page is sent.
///
/// Answers with the name the marker will be made under. Nothing has been made
/// yet: the game has not heard of it, and will not until the mod next collects.
/// The page watches its markers for that name to appear, which is the only honest
/// confirmation there is — a service that said "done" here would be reporting on
/// something it does not do.
fn made(request: &mut Request, state: &State) -> Response<Cursor<Vec<u8>>> {
    if *request.method() != Method::Post {
        return text(405, "markers are made with a post");
    }

    let Some(who) = state.sessions.who(&cookies(request)) else {
        return text(401, "run /witchlight login in the game to make a marker");
    };

    let mut body = String::new();
    if request.as_reader().take(POST_LIMIT).read_to_string(&mut body).is_err() {
        return text(400, "unreadable body");
    }

    let wanted = match Wanted::asked(&who.uid, &body) {
        Ok(wanted) => wanted,
        Err(why) => return text(400, why),
    };

    let key = wanted.key.clone();
    if !state.pending.want(wanted) {
        return text(503, "the game server is not collecting markers");
    }

    // Accepted, not done. The status says so, and so does the page.
    json(&format!(r#"{{"Key":"{key}"}}"#)).with_status_code(202)
}

/// A change to a marker that already exists.
///
/// Whether this person may is asked twice and answered by the mod. What is
/// decided here is only that they are somebody: the service knows who owns what
/// from a post that is seconds old, so the gate is the half holding the waypoint.
///
/// Answers the same way making one does — accepted, not done — because it is the
/// same queue and the same collection that carries it out.
fn changed(request: &mut Request, state: &State, key: &str) -> Response<Cursor<Vec<u8>>> {
    if *request.method() != Method::Put {
        return text(405, "a marker is changed with a put");
    }

    let Some(who) = state.sessions.who(&cookies(request)) else {
        return text(401, "run /witchlight login in the game to change a marker");
    };

    let mut body = String::new();
    if request.as_reader().take(POST_LIMIT).read_to_string(&mut body).is_err() {
        return text(400, "unreadable body");
    }

    let edit = match Edit::asked(&who.uid, key, &body) {
        Ok(edit) => edit,
        Err(why) => return text(400, why),
    };

    if !state.pending.change(edit) {
        return text(503, "the game server is not collecting markers");
    }

    json(&format!(r#"{{"Key":"{key}"}}"#)).with_status_code(202)
}

/// What one person has set for themselves, and the setting of it.
///
/// A whole document either way rather than a field at a time. It is a handful of
/// presets and two switches — small enough that sending all of it costs nothing,
/// and a page that holds the lot and puts the lot back needs no merge rules and
/// no route per field.
fn preferences(request: &mut Request, state: &State) -> Response<Cursor<Vec<u8>>> {
    let Some(who) = state.sessions.who(&cookies(request)) else {
        return text(401, "run /witchlight login in the game to keep settings");
    };

    match *request.method() {
        Method::Get => {
            json(&serde_json::to_string(&state.preferences.of(&who.uid)).unwrap_or_default())
        }
        Method::Put => {
            let mut body = String::new();
            if request.as_reader().take(POST_LIMIT).read_to_string(&mut body).is_err() {
                return text(400, "unreadable body");
            }
            let Ok(person) = serde_json::from_str::<Person>(&body) else {
                return text(400, "expected presets and defaults");
            };
            if state.preferences.set(&who.uid, person) {
                json(&serde_json::to_string(&state.preferences.of(&who.uid)).unwrap_or_default())
            } else {
                text(500, "those could not be kept")
            }
        }
        _ => text(405, "settings are read with a get and kept with a put"),
    }
}

/// The marker a `/markers/{key}` path names.
///
/// Only the shape of the path here; whether the key is a name this map ever
/// handed out is decided where the change is read, beside everything else that
/// arrived with it.
fn marker_key(path: &str) -> Option<&str> {
    let key = path.strip_prefix("/markers/")?;
    (!key.is_empty() && !key.contains('/') && key != "pending").then_some(key)
}

/// Watches for a newer export, on its own thread and on its own clock.
///
/// This used to run on every request. That was one filesystem check per tile,
/// and with more than one thread taking requests it was also two threads racing
/// to reload the same regions and bumping the generation twice for one export,
/// which made every viewer repaint twice.
fn watch(state: Arc<State>) {
    std::thread::spawn(move || {
        loop {
            state.refresh();
            std::thread::sleep(WATCH_EVERY);
        }
    });
}

/// Rebuilds the levels above zero, on its own thread and its own slower clock.
fn build(state: Arc<State>) {
    std::thread::spawn(move || {
        loop {
            state.build_levels();
            std::thread::sleep(BUILD_EVERY);
        }
    });
}

/// How often to look for a newer export. The mod writes at most every thirty
/// seconds, so this is far more attentive than it needs to be and still costs
/// two stat calls when nothing has changed.
const WATCH_EVERY: Duration = Duration::from_secs(1);

/// How many threads take requests.
///
/// Zero means decide here. The cap is deliberate: this shares a machine with the
/// game server, which has the better claim on its cores, and past a handful of
/// threads a cold map is bound by the tile cache rather than by rendering.
fn workers(setting: usize) -> usize {
    if setting > 0 {
        return setting.min(MAX_WORKERS);
    }

    std::thread::available_parallelism().map_or(4, |cores| cores.get().clamp(1, 8))
}

const MAX_WORKERS: usize = 64;

/// The map as it currently stands, reloaded when the server exports again.
struct State {
    data: PathBuf,
    columns: PathBuf,
    world: RwLock<World>,
    /// Reloaded like the world is. A palette can arrive long after start-up —
    /// an admin's client sends one when the server cannot build its own — and
    /// waiting for a restart to notice would make the map look broken.
    palette: RwLock<Palette>,
    /// Who is online and every marker, posted by the mod rather than read from a
    /// file it rewrote every couple of seconds.
    live: Arc<Live>,
    /// Who has followed a login link. Memory only — see [`crate::auth`].
    sessions: Arc<Sessions>,
    /// Markers asked for on the map and waiting for the mod to collect them.
    pending: Arc<Pending>,
    /// What each person has set for themselves — their presets, and where their
    /// new markers start. Kept against a uid and written to a file of its own.
    preferences: Arc<Preferences>,
    /// What the game calls each block, so that marking something can start from
    /// its name. Empty until the mod has exported one, and reloaded when it does.
    names: RwLock<HashMap<String, String>>,
    /// The names file's own timestamp, which is the whole signal that it moved.
    named: Mutex<Option<SystemTime>>,
    /// What the operator has said about who markers belong to. Read here only to
    /// tell the page which controls to offer; the mod is what enforces either.
    rules: MarkerRules,
    painted: Mutex<Option<SystemTime>>,
    /// The regions directory's own timestamp, which is the cheap gate. The mod
    /// writes a region beside itself and renames it into place — it must, or a
    /// reader would see half a file — and both of those touch the directory.
    seen: Mutex<Option<SystemTime>>,
    /// When each region was last written. The mod only writes a region that
    /// changed, so a timestamp is the whole signal; there is nothing to hash.
    regions: Mutex<HashMap<(i32, i32), SystemTime>>,
    /// Bumped whenever the world actually changes. The viewer watches this and
    /// it versions tile URLs, which is what gets a new map past the browser cache.
    generation: AtomicU64,
    /// Which tiles each generation changed, so a viewer that has fallen a few
    /// generations behind can repaint those and leave the rest of the map alone.
    /// `None` means every tile — a new palette recolours all of them.
    history: Mutex<VecDeque<(u64, Changed)>>,
    /// Level 0 tiles whose levels above are out of date. Drained by the builder,
    /// so many changes in one window cost one rebuild rather than many.
    stale: Mutex<HashSet<(i32, i32)>>,
    cache: Mutex<Cache>,
}

/// Encoded tiles, kept until the room runs out.
///
/// This used to be a map that only ever grew: nothing left it but a tile that
/// changed, so a service left running while people explored held every tile
/// anyone had ever looked at, at every level, at about a hundred kilobytes each.
/// That is a leak with a slow fuse rather than a tuning problem.
struct Cache {
    held: HashMap<(u32, i32, i32), (Vec<u8>, u64)>,
    bytes: usize,
    budget: usize,
    clock: u64,
}

impl Cache {
    fn new(budget: usize) -> Self {
        Self { held: HashMap::new(), bytes: 0, budget, clock: 0 }
    }

    fn get(&mut self, at: &(u32, i32, i32)) -> Option<Vec<u8>> {
        self.clock += 1;
        let clock = self.clock;
        let (bytes, used) = self.held.get_mut(at)?;
        *used = clock;
        Some(bytes.clone())
    }

    fn insert(&mut self, at: (u32, i32, i32), bytes: Vec<u8>) {
        self.clock += 1;
        self.bytes += bytes.len();
        if let Some((old, _)) = self.held.insert(at, (bytes, self.clock)) {
            self.bytes -= old.len();
        }
        self.evict();
    }

    /// Forgets everything, for a palette that has recoloured every tile there is.
    fn clear(&mut self) {
        self.held.clear();
        self.bytes = 0;
    }

    fn remove(&mut self, at: &(u32, i32, i32)) {
        if let Some((old, _)) = self.held.remove(at) {
            self.bytes -= old.len();
        }
    }

    /// Drops the tiles nobody has asked for in longest, until there is room.
    ///
    /// Least recently used rather than oldest: a map has a few squares everyone
    /// looks at and a long tail nobody returns to, and evicting by age alone
    /// would throw away the ones being used.
    fn evict(&mut self) {
        if self.bytes <= self.budget {
            return;
        }

        let mut by_age: Vec<((u32, i32, i32), u64)> =
            self.held.iter().map(|(at, (_, used))| (*at, *used)).collect();
        by_age.sort_unstable_by_key(|(_, used)| *used);

        for (at, _) in by_age {
            if self.bytes <= self.budget {
                break;
            }
            self.remove(&at);
        }
    }
}

impl State {
    /// Picks up whatever has changed on disk. The common case is two stat calls
    /// and nothing else.
    fn refresh(&self) {
        self.refresh_palette();
        self.refresh_names();
        self.refresh_world();
    }

    /// Rereads the block names when the mod has written them again.
    ///
    /// Its own timestamp, the way the palette and the regions have theirs. A mod
    /// set changing is the only thing that moves this file, so it is checked on
    /// the same clock as everything else and costs a stat when nothing has.
    fn refresh_names(&self) {
        let current = modified(&self.data.join("blocknames.json"));
        let Ok(mut named) = self.named.lock() else {
            return;
        };
        if current == *named {
            return;
        }
        *named = current;

        let names = block_names(&self.data);
        let count = names.len();
        if let Ok(mut held) = self.names.write() {
            *held = names;
        }
        println!("witchlight: block names reloaded from disk — {count} named");
    }

    /// Takes a new palette when one appears. Colours change for every tile, so
    /// this drops the cache exactly as a world reload does.
    fn refresh_palette(&self) {
        let path = self.data.join("palette.json");
        let current = modified(&path);
        let Ok(mut painted) = self.painted.lock() else {
            return;
        };
        if current == *painted {
            return;
        }
        *painted = current;

        // A palette being written as it is read is not worth complaining about.
        let Ok(palette) = Palette::load(&self.data) else {
            return;
        };

        // A file written again with the same colours in it is not a new palette.
        // Reloading one costs every tile in the cache and a redraw of every stored
        // level, which is seconds of blank map — so the timestamp moving is what
        // prompts a look, and the colours themselves are what decides.
        if self.palette.read().is_ok_and(|held| held.same_as(&palette)) {
            return;
        }

        let (named, source, blank) = (palette.named, palette.source.clone(), palette.paints_nothing());
        if let Ok(mut held) = self.palette.write() {
            *held = palette;
        }
        if let Ok(mut cache) = self.cache.lock() {
            cache.clear();
        }
        // Every stored level is drawn from level 0, so redrawing them against a
        // palette with no colours replaces a map that works with a blank one —
        // and the old pictures are the only thing left to look at until a real
        // palette arrives. The pyramid is left exactly as it is.
        if blank {
            let generation = self.bump(None);
            eprintln!(
                "witchlight: the palette that just arrived has no colours at all \
                 (source {source}). The stored zoom levels are being kept as they are \
                 and the finest level will not draw until a usable palette arrives — \
                 an admin joining the game supplies one."
            );
            println!("witchlight: generation {generation}, tiles dropped");
            return;
        }

        if let Ok(mut stale) = self.stale.lock()
            && let Ok(world) = self.world.read()
        {
            stale.extend(world.regions.iter().copied());
        }

        let generation = self.bump(None);
        println!(
            "witchlight: palette reloaded from disk — {named} blocks, source {source} \
             (generation {generation}, tiles dropped)"
        );
        self.report_coverage();
    }

    fn refresh_world(&self) {
        let current = modified(&self.columns);
        let Ok(mut seen) = self.seen.lock() else {
            return;
        };
        if current == *seen {
            return;
        }

        let now = region_times(&self.columns);
        let Ok(mut held) = self.regions.lock() else {
            return;
        };

        let mut touched = Vec::new();
        let mut incomplete = false;

        for (at, time) in &now {
            if held.get(at) == Some(time) {
                continue;
            }

            // A region being written as it is read is not worth complaining
            // about, but it must be tried again rather than remembered as done.
            match Region::read(&self.columns.join(format!("r.{}.{}.msqr", at.0, at.1))) {
                Ok(region) => {
                    if let Ok(mut world) = self.world.write() {
                        world.apply(region);
                    }
                    held.insert(*at, *time);
                    touched.push(*at);
                }
                Err(_) => incomplete = true,
            }
        }

        for at in held.keys().copied().collect::<Vec<_>>() {
            if !now.contains_key(&at) {
                if let Ok(mut world) = self.world.write() {
                    world.forget(at);
                }
                held.remove(&at);
                touched.push(at);
            }
        }
        drop(held);

        // Leaving the directory unseen sends the next look back for whatever was
        // half-written this time. The lock is held across the reload, so this is
        // the only thread that can be doing any of it.
        *seen = if incomplete { None } else { current };

        if touched.is_empty() {
            return;
        }

        // Slope shading reads the column to the west and the one to the north, so
        // a region also changes the western edge of the tile east of it and the
        // northern edge of the tile below. A region is a level 0 tile, so these
        // are tile coordinates already.
        let mut repaint = Vec::with_capacity(touched.len() * 3);
        for (rx, rz) in &touched {
            repaint.push((0, *rx, *rz));
            repaint.push((0, *rx + 1, *rz));
            repaint.push((0, *rx, *rz + 1));
        }
        repaint.sort_unstable();
        repaint.dedup();

        if let Ok(mut cache) = self.cache.lock() {
            for at in &repaint {
                cache.remove(at);
            }
        }

        // Handed to the builder, which announces the change once it has rebuilt
        // the levels above as well.
        //
        // Announcing it here too would announce it twice: the builder follows two
        // seconds later and bumps the generation again, and since the generation
        // versions every tile URL, that is the same pixels fetched under two
        // different names. On a dense world a tile is a third of a megabyte, so
        // the second fetch is not free and the swap is visible.
        if let Ok(mut stale) = self.stale.lock() {
            for (_, x, z) in &repaint {
                stale.insert((*x, *z));
            }
        }

        // Not announced here. The builder follows within seconds and announces the
        // whole export at once, including these tiles — and since the generation
        // versions every tile URL, announcing twice means the same pixels fetched
        // under two names. On a dense world a tile is a third of a megabyte.
        //
        // Coverage is a pass over every column in the world, which is worth it
        // when the palette changes because that changes every tile. A region
        // arriving changes one square, so it is reported by count alone.
        println!(
            "witchlight: {} regions reloaded — {} chunks",
            touched.len(),
            self.chunks()
        );
    }

    /// Says how the terrain resolves against whatever palette is loaded now.
    fn report_coverage(&self) {
        let (Ok(world), Ok(palette)) = (self.world.read(), self.palette.read()) else {
            return;
        };
        let coverage = Renderer::new(&world, &palette).coverage();
        println!("witchlight: surface {}", coverage.summary());
    }

    /// Records a generation and what it changed, then returns the new number.
    fn bump(&self, tiles: Changed) -> u64 {
        let generation = self.generation.fetch_add(1, Ordering::Relaxed) + 1;
        if let Ok(mut history) = self.history.lock() {
            history.push_back((generation, tiles));
            while history.len() > HISTORY {
                history.pop_front();
            }
        }
        generation
    }

    /// What a viewer last at `since` needs to repaint.
    ///
    /// `None` means everything: either the palette changed, or the viewer has
    /// fallen further behind than the history goes and there is no honest way to
    /// tell it which tiles it missed.
    fn changes_since(&self, since: u64) -> Changed {
        let generation = self.generation.load(Ordering::Relaxed);
        if since >= generation {
            return Some(Vec::new());
        }

        let Ok(history) = self.history.lock() else {
            return None;
        };

        // A gap between what the viewer saw and what is still remembered.
        match history.front() {
            Some((oldest, _)) if *oldest <= since + 1 => {}
            _ => return None,
        }

        let mut tiles = Vec::new();
        for (_, changed) in history.iter().filter(|(at, _)| *at > since) {
            tiles.extend(changed.as_ref()?.iter().copied());
        }
        tiles.sort_unstable();
        tiles.dedup();
        Some(tiles)
    }

    /// Where the world counts from, as the mod last wrote it.
    ///
    /// Read on demand rather than held: it is asked for every few seconds by one
    /// page, the file is a line long, and holding it would mean noticing when it
    /// changed for the sake of a number that changes when a world does.
    fn spawn(&self) -> (i32, i32) {
        let Ok(text) = std::fs::read_to_string(self.data.join("world.json")) else {
            return (0, 0);
        };

        let number = |key: &str| -> i32 {
            text.split(&format!("\"{key}\":"))
                .nth(1)
                .and_then(|rest| {
                    let digits: String = rest
                        .trim_start()
                        .chars()
                        .take_while(|c| c.is_ascii_digit() || *c == '-')
                        .collect();
                    digits.parse().ok()
                })
                .unwrap_or(0)
        };

        (number("SpawnX"), number("SpawnZ"))
    }

    /// Which marker icons exist, so the viewer draws a marker it can and a plain
    /// shape for one it cannot rather than a hole where a picture should be.
    fn icons(&self) -> String {
        let Ok(entries) = std::fs::read_dir(self.data.join("icons")) else {
            return "[]".to_owned();
        };

        let mut names: Vec<String> = entries
            .flatten()
            .filter_map(|entry| {
                let path = entry.path();
                if path.extension()? != "svg" {
                    return None;
                }
                Some(path.file_stem()?.to_str()?.to_owned())
            })
            .filter(|name| is_stored_name(name))
            .collect();
        names.sort();

        let quoted: Vec<String> = names.iter().map(|name| format!("\"{name}\"")).collect();
        format!("[{}]", quoted.join(","))
    }

    /// The state of the map, and — when the caller says which generation it last
    /// drew — which tiles it needs to fetch again.
    fn info(&self, since: Option<u64>) -> String {
        let (min_x, min_z, max_x, max_z) = self.bounds();
        // Where the game counts from, written by the mod. Absent until it has.
        let (spawn_x, spawn_z) = self.spawn();

        let mut body = format!(
            r#"{{"minX":{min_x},"minZ":{min_z},"maxX":{max_x},"maxZ":{max_z},"tile":{TILE},"spawnX":{spawn_x},"spawnZ":{spawn_z},"chunk":{},"levels":{},"chunks":{},"generation":{}"#,
            self.chunk_edge(),
            self.levels(),
            self.chunks(),
            self.generation.load(Ordering::Relaxed)
        );

        // Without a `since` there is nothing to be behind on, so nothing is said
        // about tiles and a first-time viewer draws whatever it needs.
        if let Some(since) = since {
            match self.changes_since(since) {
                Some(tiles) => {
                    body.push_str(r#","tiles":["#);
                    for (index, (level, x, z)) in tiles.iter().enumerate() {
                        if index > 0 {
                            body.push(',');
                        }
                        body.push_str(&format!("[{level},{x},{z}]"));
                    }
                    body.push(']');
                }
                None => body.push_str(r#","all":true"#),
            }
        }

        body.push('}');
        body
    }

    /// What is at one block, for the viewer's inspector.
    ///
    /// The same reading the renderer made for that pixel, so the map never names
    /// a block it did not draw. `None` while the map is between hands.
    fn block(&self, x: i32, z: i32) -> Option<String> {
        let (Ok(world), Ok(palette)) = (self.world.read(), self.palette.read()) else {
            return None;
        };

        let Ok(names) = self.names.read() else {
            return None;
        };

        let surface = Renderer::new(&world, &palette).surface_at(x, z);
        let body = Block::read(x, z, surface, &palette, &names);
        // Every field is a number, a fixed word, or a block code out of the
        // palette, so there is nothing here that can refuse to be JSON.
        serde_json::to_string(&body).ok()
    }

    fn bounds(&self) -> (i32, i32, i32, i32) {
        self.world
            .read()
            .map_or((0, 0, 0, 0), |world| world.bounds())
    }

    /// Blocks along a chunk's edge, which is what the viewer draws its grid on.
    /// Zero until something has been exported.
    fn chunk_edge(&self) -> usize {
        self.world.read().map_or(0, |world| world.edge)
    }

    /// What the page needs to know about whoever is looking at it.
    ///
    /// Always answers, and answers the same shape logged in or not: a page that
    /// has to tell an error from a stranger has two ways to draw one state.
    ///
    /// `Waiting` is how many markers the game server has not collected. A form
    /// whose marker has not appeared cannot otherwise tell a game server that has
    /// stopped from one that is merely slow, and those are not the same problem.
    fn me(&self, cookies: &str) -> String {
        let who = self.sessions.who(cookies);
        serde_json::json!({
            "Name": who.as_ref().map(|who| who.name.clone()),
            "Uid": who.as_ref().map(|who| who.uid.clone()),
            "MarkersPublic": self.rules.public,
            "PublicMarkersEditable": self.rules.public_editable,
            "Waiting": self.pending.waiting(),
        })
        .to_string()
    }

    /// Blocks whose code or name reads like what somebody is typing.
    ///
    /// A preset is keyed on a block code, and nobody knows what
    /// `game:smallplants-fern-normal` is called from memory. The whole table is
    /// eleven thousand entries and several hundred kilobytes, which is not a
    /// thing to hand a map page on the chance it opens a form — so the page asks
    /// as it types and this answers with a screenful.
    ///
    /// Matched against both the code and the name, because somebody typing
    /// "fern" and somebody typing "smallplants" are both looking for the same
    /// block and neither is wrong.
    fn blocks_like(&self, asked: &str) -> String {
        let asked = asked.trim().to_ascii_lowercase();
        if asked.is_empty() {
            return "[]".to_owned();
        }

        let Ok(names) = self.names.read() else {
            return "[]".to_owned();
        };

        let mut found: Vec<(&str, &str)> = names
            .iter()
            .filter(|(code, name)| {
                code.to_ascii_lowercase().contains(&asked)
                    || name.to_ascii_lowercase().contains(&asked)
            })
            .map(|(code, name)| (code.as_str(), name.as_str()))
            .collect();

        // What somebody typed, first. A search for "fern" that opens on
        // `bamboo-fern-shoot` because it sorts earlier is a search that has to be
        // read through rather than glanced at.
        found.sort_by_key(|(code, name)| {
            let short = code.split_once(':').map_or(*code, |(_, rest)| rest);
            (
                !short.to_ascii_lowercase().starts_with(&asked),
                !name.to_ascii_lowercase().starts_with(&asked),
                name.len(),
                *code,
            )
        });
        found.truncate(MOST_BLOCKS_FOUND);

        let listed: Vec<_> = found
            .into_iter()
            .map(|(code, name)| serde_json::json!({ "Code": code, "Name": name }))
            .collect();
        serde_json::to_string(&listed).unwrap_or_else(|_| "[]".to_owned())
    }

    fn chunks(&self) -> usize {
        self.world.read().map_or(0, |world| world.chunks.len())
    }

    fn tile(&self, level: u32, tx: i32, tz: i32) -> Result<Vec<u8>> {
        if let Ok(mut cache) = self.cache.lock()
            && let Some(bytes) = cache.get(&(level, tx, tz))
        {
            return Ok(bytes);
        }

        let bytes = if level == 0 {
            let (Ok(world), Ok(palette)) = (self.world.read(), self.palette.read()) else {
                return Err(Error::Empty("the map is being reloaded".to_owned()));
            };
            // Level 0 is drawn on demand and every level above it is a stored
            // picture, so a palette with no colours in it blanks the finest level
            // while the rest of the pyramid goes on showing the world. That reads
            // as a map that breaks when you zoom in, which is what it was taken
            // for three times. Refusing is what makes the viewer fall back to the
            // level above — a coarse map rather than an empty one.
            if palette.paints_nothing() {
                let Some(grown) = pyramid::from_above(&self.data, 0, tx, tz, TILE, self.levels())
                else {
                    return Err(Error::Empty(
                        "the palette has no colours and no level above has this ground"
                            .to_owned(),
                    ));
                };
                pyramid::encode(&grown)?
            } else {
                render_tile(&Renderer::new(&world, &palette), tx, tz)?
            }
        } else {
            // Built by the builder, not here. A coarse tile is made of four of the
            // level below, so making one on demand would make every tile beneath
            // it — a thousand renders for a level five, while somebody waits.
            let Some(image) = pyramid::read(&self.data, level, tx, tz) else {
                return Err(Error::Empty(format!("level {level} tile ({tx}, {tz}) is not built yet")));
            };
            pyramid::encode(&image)?
        };

        if let Ok(mut cache) = self.cache.lock() {
            cache.insert((level, tx, tz), bytes.clone());
        }
        Ok(bytes)
    }

    /// How many levels this world is wide enough to need.
    fn levels(&self) -> u32 {
        let (min_x, min_z, max_x, max_z) = self.bounds();
        let tile = i64::from(TILE);
        let across = (i64::from(max_x) - i64::from(min_x)).div_euclid(tile);
        let down = (i64::from(max_z) - i64::from(min_z)).div_euclid(tile);
        pyramid::levels_for(across, down)
    }

    /// One level 0 tile as an image, or nothing where the world has no chunks.
    fn level_zero(&self, tx: i32, tz: i32, mapped: &HashSet<(i32, i32)>) -> Option<RgbImage> {
        if !mapped.contains(&(tx, tz)) {
            return None;
        }
        let (Ok(world), Ok(palette)) = (self.world.read(), self.palette.read()) else {
            return None;
        };
        Some(Renderer::new(&world, &palette).render(tx * TILE as i32, tz * TILE as i32, TILE))
    }

    /// Rebuilds every level above zero for whatever has changed since last time.
    ///
    /// Bottom up, one level at a time: the tiles that changed at a level decide
    /// which tiles change at the level above, and four of the former make one of
    /// the latter. A region changing therefore costs one tile per level, not one
    /// tile per level per region.
    fn build_levels(&self) {
        let Ok(mut stale) = self.stale.lock() else {
            return;
        };
        if stale.is_empty() {
            return;
        }
        let mut changed: HashSet<(i32, i32)> = std::mem::take(&mut stale);
        drop(stale);

        let levels = self.levels();

        // A world that has grown past a power of two gains a coarsest level that
        // has never been built. Walking up from what changed builds exactly one
        // tile there and leaves the rest of the level missing — and that level is
        // the one a viewer opens on, so the map reads as empty until something
        // else marks every region stale. The whole pyramid is measured against the
        // world whenever it is shorter than the world needs, which is once per
        // doubling and never in the steady state.
        if pyramid::levels_built(&self.data) < levels
            && let Ok(regions) = self.regions.lock()
        {
            let behind = pyramid::behind(&self.data, &regions, levels);
            println!(
                "witchlight: the world now needs {levels} levels — {} regions to rebuild",
                behind.len()
            );
            changed.extend(behind);
        }

        let mapped: HashSet<(i32, i32)> = match self.world.read() {
            Ok(world) => world.regions.iter().copied().collect(),
            Err(_) => return,
        };

        let mut repainted: Vec<(u32, i32, i32)> = changed.iter().map(|&(x, z)| (0, x, z)).collect();

        for level in 1..=levels {
            let parents: HashSet<(i32, i32)> = changed
                .iter()
                .map(|&(x, z)| pyramid::ancestor(1, x, z))
                .collect();

            for &(px, pz) in &parents {
                let below = pyramid::children(px, pz).map(|(cx, cz)| {
                    if level == 1 {
                        self.level_zero(cx, cz, &mapped)
                    } else {
                        pyramid::read(&self.data, level - 1, cx, cz)
                    }
                });

                if below.iter().all(Option::is_none) {
                    continue;
                }

                let parent = pyramid::downsample(&below, TILE, UNMAPPED);
                if let Err(error) = pyramid::write(&self.data, level, px, pz, &parent) {
                    eprintln!("witchlight: {error}");
                }
                repainted.push((level, px, pz));
            }

            changed = parents;
        }

        if let Ok(mut cache) = self.cache.lock() {
            for at in &repainted {
                cache.remove(at);
            }
        }

        // One announcement for the whole export: the level 0 tiles the watcher
        // reloaded are in this list too, so a viewer fetches each changed tile
        // once rather than once per level of the pyramid that touched it.
        if let Ok(palette) = self.palette.read() {
            pyramid::record_palette(&self.data, &palette.fingerprint);
        }

        let generation = self.bump(Some(repainted.clone()));
        println!(
            "witchlight: {} tiles rebuilt across {levels} levels (generation {generation})",
            repainted.len()
        );
    }
}

/// Listens on the API socket for what the mod posts, on its own thread.
///
/// Separate from the map's own port on purpose: that one is meant to be reachable
/// and this one accepts writes, and anything that could reach a public write
/// endpoint could put people on the map who are not there.
fn serve_api(
    api: &Api,
    live: Arc<Live>,
    sessions: Arc<Sessions>,
    pending: Arc<Pending>,
    exports: &Path,
) -> Result<()> {
    // Before the bind rather than after the failure: a file naming a listener
    // that does not exist sends the mod's posts at whatever holds that port now,
    // and the window where that is true should not include this function.
    Api::unpublish(exports);

    let server = Server::http(&api.bind).map_err(|error| {
        Error::io(
            format!("listening for live data on {}", api.bind),
            std::io::Error::other(error.to_string()),
        )
    })?;

    // Asked of the listener rather than read back from the setting, because the
    // setting is usually a request for whatever port is free and says nothing
    // about which one that turned out to be.
    let Some(address) = server.server_addr().to_ip() else {
        return Err(Error::io(
            format!("listening for live data on {}", api.bind),
            std::io::Error::other("the listener has no address"),
        ));
    };

    api.publish(exports, address.port());
    println!("witchlight: taking live data on {address}");

    let token = Api { bind: api.bind.clone(), token: api.token.clone() };
    std::thread::spawn(move || {
        for mut request in server.incoming_requests() {
            let response = posted(&mut request, &live, &sessions, &pending, &token);
            if let Err(error) = request.respond(response) {
                eprintln!("witchlight: API response failed: {error}");
            }
        }
    });

    Ok(())
}

/// One post from the mod.
fn posted(
    request: &mut Request,
    live: &Live,
    sessions: &Sessions,
    pending: &Pending,
    api: &Api,
) -> Response<Cursor<Vec<u8>>> {
    if *request.method() != Method::Post {
        return text(405, "the API channel takes posts only");
    }

    // Loopback is not a trust boundary on a machine other people have accounts
    // on, so reaching the port is not the same as being the mod.
    if !api.authorized(request) {
        return text(401, "the API channel needs the token from api.json");
    }

    let url = request.url().to_owned();
    let path = url.split('?').next().unwrap_or(&url);

    let mut body = String::new();
    if request
        .as_reader()
        .take(POST_LIMIT)
        .read_to_string(&mut body)
        .is_err()
    {
        return text(400, "unreadable body");
    }

    // The one thing on this channel that answers with something rather than
    // merely accepting it. Minting lives here because this is the only listener
    // the mod can reach and the only party that knows which uid is which player
    // is the mod — so the trust this needs is the trust that is already here.
    if path == "/auth/mint" {
        let Some(who) = asked_for(&body) else {
            return text(400, "expected {\"Uid\":…, \"Name\":…}");
        };
        return json(&format!(r#"{{"Token":"{}"}}"#, sessions.mint(who)));
    }

    // The markers people asked for on the web, which the mod cannot be sent and
    // so comes to collect. Emptied by the asking; see `Pending::take`.
    if path == "/markers/pending" {
        return json(
            &serde_json::to_string(&pending.take())
                .unwrap_or_else(|_| r#"{"Make":[],"Change":[]}"#.to_owned()),
        );
    }

    let taken = match path {
        "/live/players" => live.set_players(body),
        "/live/markers" => live.set_markers(body),
        _ => return text(404, "not found"),
    };

    if taken {
        text(204, "")
    } else {
        text(400, "expected what this build posts: an array of players, or markers sorted by who may see them")
    }
}

/// Who the mod is asking a login word for.
///
/// The uid is the whole of the identity; the name only decides what the page
/// says. Both come from the game and neither is checked here — the mod is the
/// only thing that can reach this channel, and the only thing that knows.
fn asked_for(body: &str) -> Option<Who> {
    // PascalCase, because everything the mod posts is written by a C# serializer
    // and this is the same wire as the rest of it.
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "PascalCase")]
    struct Asked {
        uid: String,
        #[serde(default)]
        name: String,
    }

    let asked: Asked = serde_json::from_str(body).ok()?;
    (!asked.uid.is_empty()).then_some(Who { uid: asked.uid, name: asked.name })
}

/// The addresses worth telling the operator about.
///
/// `0.0.0.0` is not something anyone can type into a browser, so a bind to every
/// interface also reports this machine's address on the network.
fn reachable_at(bind: &str) -> Vec<String> {
    let Some((host, port)) = bind.rsplit_once(':') else {
        return vec![format!("http://{bind}")];
    };

    if host != "0.0.0.0" && host != "[::]" && host != "*" {
        return vec![format!("http://{bind}")];
    }

    // The one on the network first: it is the address worth giving somebody
    // else, and loopback only ever works for whoever is sitting at the machine.
    // The order is the whole of what says which is which, since the mod hands
    // players the first of them.
    let mut addresses = Vec::new();
    if let Some(local) = local_address() {
        addresses.push(format!("http://{local}:{port}"));
    }
    addresses.push(format!("http://127.0.0.1:{port}"));
    addresses
}

/// Whether an address only works for whoever is sitting at this machine.
fn only_here(address: &str) -> bool {
    address.contains("//127.0.0.1:") || address.contains("//[::1]:") || address.contains("//localhost:")
}

/// Publishes where this can be reached, for the half that can tell people.
///
/// Written rather than answered over the socket because the mod is not the only
/// thing that wants it and it is not always the one that started this: a file
/// beside the map is readable by whoever is looking, and is how the two halves
/// already talk about everything else.
///
/// Beside itself and then into place, so a reader never sees half of it.
fn publish_addresses(data: &Path, bind: &str, addresses: &[String]) {
    let body = serde_json::json!({
        "Urls": addresses,
        "Bind": bind,
        "Version": env!("CARGO_PKG_VERSION"),
    });

    let path = data.join("service.json");
    let temporary = path.with_extension("part");
    if std::fs::write(&temporary, body.to_string())
        .and_then(|()| std::fs::rename(&temporary, &path))
        .is_err()
    {
        eprintln!("witchlight: could not write {}", path.display());
    }
}

/// This machine's address on the network it routes through.
///
/// A connected UDP socket sends nothing — it only asks the routing table which
/// local address would be used — and the address it asks about is the reserved
/// documentation range, which goes nowhere.
fn local_address() -> Option<std::net::IpAddr> {
    let socket = std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect("192.0.2.1:80").ok()?;
    Some(socket.local_addr().ok()?.ip())
}

fn modified(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).ok().and_then(|meta| meta.modified().ok())
}

/// When each region on disk was last written.
fn region_times(dir: &Path) -> HashMap<(i32, i32), SystemTime> {
    let Ok(paths) = region_files(dir) else {
        return HashMap::new();
    };

    paths
        .into_iter()
        .filter_map(|path| {
            let at = region_coords(&path)?;
            Some((at, modified(&path)?))
        })
        .collect()
}

/// What the map knows about one block, as the viewer's inspector asks for it.
///
/// A struct rather than a hand-built string like the other feeds: a block code
/// comes out of a file this program did not write, and the one place a quote in
/// it could break the page is not worth a second escaper to guard.
#[derive(serde::Serialize)]
struct Block {
    x: i32,
    z: i32,
    /// How the column read against the palette: `painted`, `blank`, `unknown` or
    /// `unmapped`. The viewer speaks for the first three and stays quiet for the
    /// last, since there is nothing drawn there to be looking at.
    state: &'static str,
    /// The block id this world gave it. Absent where nothing was exported.
    #[serde(skip_serializing_if = "Option::is_none")]
    block: Option<u16>,
    /// Its code — `game:rock-granite`. Absent for a block the palette has never
    /// heard of, which is the whole of what `unknown` means.
    #[serde(skip_serializing_if = "Option::is_none")]
    code: Option<String>,
    /// What the game calls it — `Granite rock`. Absent where the mod has exported
    /// no names, or where the language files have none for this block; whatever
    /// reads this then has the code, which is what it had before there were names.
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    /// The surface height, which is the Y a player standing here would read.
    #[serde(skip_serializing_if = "Option::is_none")]
    y: Option<i16>,
    /// Degrees celsius, and the climate the world was generated with rather than
    /// today's weather.
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    /// Rainfall, from dry at zero to the wettest the game has at one.
    #[serde(skip_serializing_if = "Option::is_none")]
    rainfall: Option<f32>,
}

impl Block {
    fn read(
        x: i32,
        z: i32,
        surface: Surface,
        palette: &Palette,
        names: &HashMap<String, String>,
    ) -> Self {
        let column = surface.column();
        let code = column.and_then(|column| palette.code_of(column.block).map(ToOwned::to_owned));
        Self {
            x,
            z,
            state: surface.state(),
            block: column.map(|column| column.block),
            name: code.as_deref().and_then(|code| names.get(code).cloned()),
            code,
            y: column.map(|column| column.height),
            temperature: column.map(|column| column.celsius()),
            rainfall: column.map(|column| column.wetness()),
        }
    }
}

/// One named value out of a query string.
///
/// One reader for all of them, because the rule is not about generations or
/// coordinates but about how a query says anything at all — and a second copy of
/// it is a second chance to match `sincerely` where `since` was meant.
fn param<'a>(url: &'a str, key: &str) -> Option<&'a str> {
    url.split_once('?')?
        .1
        .split('&')
        .filter_map(|pair| pair.split_once('='))
        .find_map(|(name, value)| (name == key).then_some(value))
}

/// A query value as the person typing it meant it.
///
/// Every other value read out of a query here is a number or a word of hex, and
/// none of them has ever needed this. A search box does: a space arrives as `%20`
/// and matching against that finds nothing at all, which reads as a search that
/// simply has no answers rather than one that never asked the question.
///
/// A stray `%` that spells nothing is itself. What arrives is somebody typing,
/// and refusing the whole search over a loose percent sign helps nobody.
fn decoded(value: &str) -> String {
    let raw = value.as_bytes();
    let mut out = Vec::with_capacity(raw.len());
    let mut at = 0;

    while at < raw.len() {
        match raw[at] {
            b'+' => {
                out.push(b' ');
                at += 1;
            }
            b'%' if at + 2 < raw.len() => match hex(raw[at + 1]).zip(hex(raw[at + 2])) {
                Some((high, low)) => {
                    out.push(high * 16 + low);
                    at += 3;
                }
                None => {
                    out.push(b'%');
                    at += 1;
                }
            },
            byte => {
                out.push(byte);
                at += 1;
            }
        }
    }

    String::from_utf8_lossy(&out).into_owned()
}

fn hex(digit: u8) -> Option<u8> {
    match digit {
        b'0'..=b'9' => Some(digit - b'0'),
        b'a'..=b'f' => Some(digit - b'a' + 10),
        b'A'..=b'F' => Some(digit - b'A' + 10),
        _ => None,
    }
}

/// The `since` of a query string, naming the generation a viewer last drew.
fn since_of(url: &str) -> Option<u64> {
    param(url, "since")?.parse().ok()
}

/// The block position an inspector is asking about. Both halves or neither: half
/// a position names nowhere, and defaulting the other half would name somewhere
/// else entirely.
fn block_asked(url: &str) -> Option<(i32, i32)> {
    Some((param(url, "x")?.parse().ok()?, param(url, "z")?.parse().ok()?))
}

/// `/icons/{name}.svg`, where the name is a marker icon.
///
/// The name reaches here from a waypoint, which got it from whatever mods are
/// installed, and is about to become a path. Only the characters that cannot
/// mean anything but themselves are allowed through — no separators, no dots,
/// so nothing outside the icons directory can be named.
fn icon_name(url: &str) -> Option<&str> {
    stored_name(url, "/icons/", ".svg")
}

/// The name a player's picture is filed under, from `/portraits/{name}.png`.
fn portrait_name(url: &str) -> Option<&str> {
    stored_name(url, "/portraits/", ".png")
}

/// The name in a URL, when it is only ever a name.
///
/// One reader for every kind of stored file, because the rule is not about icons
/// or portraits but about what may be joined onto a directory and handed back: a
/// second copy of it is a second chance to get it wrong.
fn stored_name<'a>(url: &'a str, prefix: &str, suffix: &str) -> Option<&'a str> {
    let name = url.strip_prefix(prefix)?.strip_suffix(suffix)?;
    is_stored_name(name).then_some(name)
}

fn is_stored_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-')
}

/// `/tiles/{level}/{x}/{z}.png`. Level 0 is one block per pixel; each level above
/// covers twice as much world. Coordinates may be negative.
fn tile_coords(url: &str) -> Option<(u32, i32, i32)> {
    let rest = url.strip_prefix("/tiles/")?.strip_suffix(".png")?;
    let (level, rest) = rest.split_once('/')?;
    let (x, z) = rest.split_once('/')?;
    Some((level.parse().ok()?, x.parse().ok()?, z.parse().ok()?))
}

fn render_tile(renderer: &Renderer<'_>, tx: i32, tz: i32) -> Result<Vec<u8>> {
    let image = renderer.render(tx * TILE as i32, tz * TILE as i32, TILE);
    pyramid::encode(&image)
}

/// A marker icon. Rarely changed, but a mod being added can change the set, so
/// this is kept for an hour rather than forever.
fn svg(bytes: Vec<u8>) -> Response<Cursor<Vec<u8>>> {
    let mut response = Response::from_data(bytes);
    if let Ok(header) = Header::from_bytes(&b"Content-Type"[..], &b"image/svg+xml"[..]) {
        response.add_header(header);
    }
    if let Ok(header) = Header::from_bytes(&b"Cache-Control"[..], &b"public, max-age=3600"[..]) {
        response.add_header(header);
    }
    response
}

fn tile_response(bytes: Vec<u8>) -> Response<Cursor<Vec<u8>>> {
    let mut response = Response::from_data(bytes);
    if let Ok(header) = Header::from_bytes(&b"Content-Type"[..], &b"image/png"[..]) {
        response.add_header(header);
    }
    // Safe to cache hard: a changed world means a changed `?v=`, so this exact
    // URL will never stand for different pixels.
    if let Ok(header) = Header::from_bytes(
        &b"Cache-Control"[..],
        &b"public, max-age=31536000, immutable"[..],
    ) {
        response.add_header(header);
    }
    response
}

fn html(body: &str) -> Response<Cursor<Vec<u8>>> {
    typed(body, "text/html; charset=utf-8")
}

/// Something vendored, which never changes for a given build of this binary.
fn asset(body: &str, kind: &str) -> Response<Cursor<Vec<u8>>> {
    cached(body, kind, "public, max-age=31536000, immutable")
}

fn json(body: &str) -> Response<Cursor<Vec<u8>>> {
    typed(body, "application/json")
}

/// The `Cookie` header, or nothing where the browser sent none.
fn cookies(request: &Request) -> String {
    request
        .headers()
        .iter()
        .find(|header| header.field.equiv("Cookie"))
        .map(|header| header.value.as_str().to_owned())
        .unwrap_or_default()
}

/// The login word out of `/login?t=…`.
fn link_asked(url: &str) -> Option<&str> {
    param(url, "t").filter(|word| !word.is_empty())
}

/// Somewhere else, optionally leaving a cookie behind.
///
/// `303` rather than `302`, so the browser is told in as many words to fetch the
/// new address with a GET. It is the difference between a login that works on a
/// resubmitted form and one that does something surprising.
fn redirect(to: &str, cookie: Option<String>) -> Response<Cursor<Vec<u8>>> {
    let mut response = Response::from_data(Vec::new()).with_status_code(303);
    if let Ok(header) = Header::from_bytes(&b"Location"[..], to.as_bytes()) {
        response.add_header(header);
    }
    if let Some(cookie) = cookie
        && let Ok(header) = Header::from_bytes(&b"Set-Cookie"[..], cookie.as_bytes())
    {
        response.add_header(header);
    }
    // A redirect that a browser remembers is a login that cannot be repeated.
    if let Ok(header) = Header::from_bytes(&b"Cache-Control"[..], &b"no-store"[..]) {
        response.add_header(header);
    }
    response
}

/// Logged in, and sent to the map with nothing in the address to say so.
///
/// `HttpOnly` because no script on the page has any use for the word, and
/// `SameSite=Lax` because the only thing that should arrive carrying it is
/// somebody following a link to this map themselves.
///
/// Not `Secure`: this is served over plain HTTP on a LAN as often as not, and a
/// cookie a browser refuses to send is a login that silently never works. An
/// operator putting the map on the internet puts TLS in front of it, and that is
/// the same place the flag belongs.
fn seated(session: &str) -> Response<Cursor<Vec<u8>>> {
    let cookie = format!(
        "{}={session}; Path=/; Max-Age={}; HttpOnly; SameSite=Lax",
        crate::auth::COOKIE,
        60 * 60 * 24 * 30
    );
    redirect("/", Some(cookie))
}

fn typed(body: &str, content_type: &str) -> Response<Cursor<Vec<u8>>> {
    // The page and the two feeds are the things that must never be stale.
    cached(body, content_type, "no-store")
}

/// A player's picture.
///
/// Held for a minute and no longer. Its name is derived from who the player is
/// rather than from what the picture holds, so somebody who sends a new one keeps
/// the name they had, and this path on its own cannot tell the two apart. What the
/// map asks for carries the time the picture was drawn as a query, which changes
/// when the picture does; the minute here is what stands behind anyone who asks
/// for the bare path instead.
fn png(bytes: Vec<u8>) -> Response<Cursor<Vec<u8>>> {
    let mut response = Response::from_data(bytes);
    if let Ok(header) = Header::from_bytes(&b"Content-Type"[..], &b"image/png"[..]) {
        response.add_header(header);
    }
    if let Ok(header) = Header::from_bytes(&b"Cache-Control"[..], &b"public, max-age=60"[..]) {
        response.add_header(header);
    }
    response
}

/// A response that says how long it may be kept.
///
/// One `Cache-Control` and one only: two of them is not a stronger instruction,
/// it is an ambiguous one, and a browser takes the first — so an `immutable`
/// added after a `no-store` is an asset that is never cached and looks cached.
fn cached(body: &str, content_type: &str, keep: &str) -> Response<Cursor<Vec<u8>>> {
    let mut response = Response::from_data(body.as_bytes().to_vec());
    if let Ok(header) = Header::from_bytes(&b"Content-Type"[..], content_type.as_bytes()) {
        response.add_header(header);
    }
    if let Ok(header) = Header::from_bytes(&b"Cache-Control"[..], keep.as_bytes()) {
        response.add_header(header);
    }
    response
}

fn text(status: u16, body: &str) -> Response<Cursor<Vec<u8>>> {
    Response::from_data(body.as_bytes().to_vec()).with_status_code(status)
}

/// The viewer.
///
/// Kept in its own file rather than a format string: every brace in the page
/// would otherwise have to be doubled, which makes editing the thing a chore.
/// The bounds are substituted so the first paint is already in the right place,
/// and the page asks `/info.json` for the rest.
///
/// The version comes from the build rather than from `/info.json`, so what the
/// page shows is what compiled it — a page fetched from one build cannot report
/// the number of another.
fn viewer(min_x: i32, min_z: i32, max_x: i32, max_z: i32) -> String {
    include_str!("viewer.html")
        .replace("__TILE__", &TILE.to_string())
        .replace("__MIN_X__", &min_x.to_string())
        .replace("__MIN_Z__", &min_z.to_string())
        .replace("__MAX_X__", &max_x.to_string())
        .replace("__MAX_Z__", &max_z.to_string())
        .replace("__VERSION__", env!("CARGO_PKG_VERSION"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tile(bytes: usize) -> Vec<u8> {
        vec![7u8; bytes]
    }

    #[test]
    fn an_icon_name_is_only_ever_a_name() {
        // These arrive from whatever mods a server runs and become a path.
        for good in ["circle", "gravestone", "star1", "skull_and_crossbones", "my-mod_icon2"] {
            assert!(is_stored_name(good), "{good} should be allowed");
            assert_eq!(icon_name(&format!("/icons/{good}.svg")), Some(good));
        }

        for bad in [
            "../palette",
            "..",
            "a/b",
            "Gravestone",
            "grave stone",
            "",
            "with.dot",
            "%2e%2e",
            "under\\score\\back",
        ] {
            assert!(!is_stored_name(bad), "{bad} must not be allowed");
        }

        assert!(!is_stored_name(&"a".repeat(65)), "a name has to end somewhere");
        assert_eq!(icon_name("/icons/circle.png"), None, "only svg");
        assert_eq!(icon_name("/tiles/0/0/0.png"), None, "not a tile");
    }

    /// A player's picture is filed under a name derived from their uid, and a uid
    /// is base64 — it carries `/` and `+`, which is a path and not a name. The mod
    /// writes it in hex for exactly that reason, and nothing that arrives here is
    /// trusted to have done so.
    #[test]
    fn a_portrait_name_is_only_ever_a_name() {
        let hex = "3070564246376c42722b697159483442";
        assert_eq!(portrait_name(&format!("/portraits/{hex}.png")), Some(hex));

        for bad in [
            "/portraits/../../etc/passwd.png",
            "/portraits/a/b.png",
            "/portraits/A0FF.png",
            "/portraits/.png",
        ] {
            assert_eq!(portrait_name(bad), None, "{bad} must not be allowed");
        }

        assert_eq!(portrait_name("/portraits/abc.svg"), None, "only png");
        assert_eq!(portrait_name("/icons/abc.png"), None, "not an icon");
    }

    #[test]
    fn the_page_names_the_build_and_leaves_no_placeholder_behind() {
        let page = viewer(-512, -512, 512, 512);
        assert!(
            page.contains(&format!("v{}", env!("CARGO_PKG_VERSION"))),
            "the page should say which build served it"
        );
        // Every substitution the page asks for, checked by the absence of the
        // only spelling they use. One left unfilled is `__VERSION__` on screen,
        // or a world whose bounds are a syntax error — both silent until seen.
        assert!(
            !page.contains("__"),
            "a placeholder was left unsubstituted in the page"
        );
    }

    #[test]
    fn a_query_value_arrives_as_it_was_typed() {
        assert_eq!(decoded("granite%20rock"), "granite rock");
        assert_eq!(decoded("granite+rock"), "granite rock");
        assert_eq!(decoded("plain"), "plain");
        assert_eq!(decoded(""), "");
        assert_eq!(decoded("%C3%A9"), "é", "more than one byte to a letter");
        assert_eq!(decoded("a%2Fb"), "a/b");

        // Somebody typing a percent sign is not a request to refuse the search.
        assert_eq!(decoded("100%"), "100%");
        assert_eq!(decoded("50%z9"), "50%z9");
        assert_eq!(decoded("%"), "%");
    }

    #[test]
    fn a_query_value_is_matched_by_its_whole_name() {
        assert_eq!(since_of("/info.json?since=7"), Some(7));
        assert_eq!(block_asked("/block.json?x=-412&z=88"), Some((-412, 88)));
        assert_eq!(block_asked("/block.json?z=88&x=-412"), Some((-412, 88)));

        // A name that merely starts the same is a different name.
        assert_eq!(param("/info.json?sincerely=7", "since"), None);
        assert_eq!(param("/block.json?xz=1", "x"), None);

        assert_eq!(since_of("/info.json"), None, "no query at all");
        assert_eq!(block_asked("/block.json?x=1"), None, "half a position is nowhere");
        assert_eq!(block_asked("/block.json?x=1&z=here"), None, "z is a number");
    }

    #[test]
    fn a_tile_comes_back_out_the_way_it_went_in() {
        let mut cache = Cache::new(1024);
        cache.insert((0, 1, 2), tile(10));
        assert_eq!(cache.get(&(0, 1, 2)), Some(tile(10)));
        assert_eq!(cache.get(&(0, 9, 9)), None, "one that was never put in");
    }

    #[test]
    fn levels_are_separate_tiles() {
        let mut cache = Cache::new(1024);
        cache.insert((0, 1, 1), tile(10));
        cache.insert((3, 1, 1), tile(20));
        assert_eq!(cache.get(&(0, 1, 1)).map(|t| t.len()), Some(10));
        assert_eq!(cache.get(&(3, 1, 1)).map(|t| t.len()), Some(20));
    }

    #[test]
    fn it_stays_inside_its_budget() {
        let mut cache = Cache::new(100);
        for at in 0..50 {
            cache.insert((0, at, 0), tile(30));
            assert!(
                cache.bytes <= 100,
                "held {} bytes after {} tiles, budget is 100",
                cache.bytes,
                at + 1
            );
        }
        assert!(cache.held.len() < 50, "something must have been dropped");
    }

    #[test]
    fn what_is_dropped_is_what_nobody_asked_for() {
        // Room for three. The first is used again, so the second should go before
        // it — dropping by age alone would take the one still being looked at.
        let mut cache = Cache::new(30);
        cache.insert((0, 1, 0), tile(10));
        cache.insert((0, 2, 0), tile(10));
        cache.insert((0, 3, 0), tile(10));

        assert!(cache.get(&(0, 1, 0)).is_some(), "used again, so most recent");

        cache.insert((0, 4, 0), tile(10));
        assert!(cache.get(&(0, 1, 0)).is_some(), "kept, because it was used");
        assert!(cache.get(&(0, 2, 0)).is_none(), "dropped, because it was not");
        assert!(cache.get(&(0, 3, 0)).is_some());
        assert!(cache.get(&(0, 4, 0)).is_some());
    }

    #[test]
    fn replacing_a_tile_does_not_count_it_twice() {
        let mut cache = Cache::new(1000);
        cache.insert((0, 1, 0), tile(100));
        cache.insert((0, 1, 0), tile(40));
        assert_eq!(cache.bytes, 40, "the tile it replaced must stop counting");
        assert_eq!(cache.held.len(), 1);
    }

    #[test]
    fn removing_and_clearing_give_the_room_back() {
        let mut cache = Cache::new(1000);
        cache.insert((0, 1, 0), tile(100));
        cache.insert((0, 2, 0), tile(100));
        cache.remove(&(0, 1, 0));
        assert_eq!(cache.bytes, 100);
        cache.clear();
        assert_eq!(cache.bytes, 0);
        assert!(cache.held.is_empty());
    }

    #[test]
    fn a_tile_larger_than_the_whole_budget_does_not_wedge_it() {
        let mut cache = Cache::new(50);
        cache.insert((0, 1, 0), tile(500));
        // Nothing is left to evict, so it is over budget with one tile — but it
        // must not spin trying, and the next insert must still work.
        cache.insert((0, 2, 0), tile(10));
        assert!(cache.held.contains_key(&(0, 2, 0)));
    }
}
