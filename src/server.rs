//! Serving the map.
//!
//! Tiles are rendered when asked for and kept, so starting up costs nothing and
//! only the part of the world someone actually looks at is ever drawn.

use std::collections::{HashMap, HashSet, VecDeque};
use std::fmt;
use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, SystemTime};

use image::{ImageFormat, RgbImage};
use tiny_http::{Header, Method, Request, Response, Server};

use crate::columns::{Region, World, columns_dir, region_coords, region_files};
use crate::live::Live;
use crate::pyramid;
use crate::render::UNMAPPED;
use crate::error::{Error, Result};
use crate::palette::Palette;
use crate::render::Renderer;

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

/// Where the mod posts what moves. A socket in `/tmp` by default, named after the
/// export directory so that both sides find it without being told and two game
/// servers on one machine do not collide.
pub enum ApiSocket {
    Socket(PathBuf),
    Address(String),
}

impl ApiSocket {
    /// Reads the `api_socket` setting. Empty means the default socket in `/tmp`; a
    /// value with a colon and no slash is an address; anything else is a socket
    /// path.
    #[must_use]
    pub fn resolve(setting: &str, exports: &Path) -> Self {
        if setting.is_empty() {
            return Self::Socket(crate::live::default_api_socket(exports));
        }
        if setting.contains(':') && !setting.contains('/') {
            return Self::Address(setting.to_owned());
        }
        Self::Socket(PathBuf::from(setting))
    }
}

impl fmt::Display for ApiSocket {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Socket(path) => write!(f, "{}", path.display()),
            Self::Address(address) => f.write_str(address),
        }
    }
}

/// The most a single post to the API socket may carry. Positions for a full
/// server are a couple of kilobytes and markers are tens; this only stops a
/// broken poster from being read into memory without limit.
const POST_LIMIT: u64 = 8 * 1024 * 1024;

/// What `sockaddr_un` has room for, less a byte for the terminator.
const SOCKET_PATH_LIMIT: usize = 107;

pub fn serve(
    bind: &str,
    data: &Path,
    palette: Palette,
    api: &ApiSocket,
    threads: usize,
) -> Result<()> {
    let columns = columns_dir(data);
    let state = Arc::new(State {
        world: RwLock::new(World::load(data)?),
        palette: RwLock::new(palette),
        seen: Mutex::new(modified(&columns)),
        regions: Mutex::new(region_times(&columns)),
        painted: Mutex::new(modified(&data.join("palette.json"))),
        live: Arc::new(Live::load(data)),
        generation: AtomicU64::new(1),
        history: Mutex::new(VecDeque::new()),
        stale: Mutex::new(HashSet::new()),
        cache: Mutex::new(HashMap::new()),
        data: data.to_path_buf(),
        columns,
    });

    // The map is the product and live data is a garnish, so an API socket that
    // will not bind is said out loud and stepped over rather than taken as fatal.
    if let Err(error) = serve_api(api, Arc::clone(&state.live)) {
        eprintln!("mapstique: {error}");
        eprintln!(
            "mapstique: nobody will show on the map. Set `api_socket` to a shorter \
             path or to a host:port, on both this and the server mod."
        );
    }

    let server = Server::http(bind).map(Arc::new).map_err(|error| {
        Error::io(
            format!("listening on {bind}"),
            std::io::Error::other(error.to_string()),
        )
    })?;
    for address in reachable_at(bind) {
        println!("mapstique: serving on {address}");
    }

    let threads = workers(threads);
    println!("mapstique: rendering on {threads} threads");

    // Levels built from a region format this build no longer reads would show
    // terrain that has since been cleared, so they go.
    if pyramid::reset_unless_built_from(data, crate::columns::VERSION) {
        println!("mapstique: the stored levels were built from an older format and have been cleared");
    }

    // What is left is kept. Only regions written since their level was built get
    // rebuilt, so a run whose levels are already current starts with nothing to
    // do rather than redrawing a world that has not moved.
    if let Ok(mut stale) = state.stale.lock()
        && let Ok(world) = state.world.read()
    {
        let behind = world
            .regions
            .iter()
            .filter(|(x, z)| {
                !pyramid::is_current(data, &state.columns.join(format!("r.{x}.{z}.msqr")), *x, *z)
            })
            .copied()
            .collect::<Vec<_>>();
        println!(
            "mapstique: {} of {} regions need their levels built",
            behind.len(),
            world.regions.len()
        );
        stale.extend(behind);
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
            eprintln!("mapstique: response failed: {error}");
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
    } else if path == "/live.json" {
        // Straight through: the mod knows what a waypoint is, and this knows it
        // is a JSON array to hand to a browser. That is the whole contract.
        json(&state.live.body())
    } else if path == "/info.json" {
        json(&state.info(since_of(&url)))
    } else if let Some((level, tx, tz)) = tile_coords(path) {
        match state.tile(level, tx, tz) {
            Ok(bytes) => png(bytes),
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
    cache: Mutex<HashMap<(u32, i32, i32), Vec<u8>>>,
}

impl State {
    /// Picks up whatever has changed on disk. The common case is two stat calls
    /// and nothing else.
    fn refresh(&self) {
        self.refresh_palette();
        self.refresh_world();
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

        let (named, source) = (palette.named, palette.source.clone());
        if let Ok(mut held) = self.palette.write() {
            *held = palette;
        }
        if let Ok(mut cache) = self.cache.lock() {
            cache.clear();
        }
        if let Ok(mut stale) = self.stale.lock()
            && let Ok(world) = self.world.read()
        {
            stale.extend(world.regions.iter().copied());
        }

        let generation = self.bump(None);
        println!(
            "mapstique: palette reloaded from disk — {named} blocks, source {source} \
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

        // The levels above are the builder's work, on its own slower clock, so
        // that a region changing repeatedly costs one rebuild rather than many.
        if let Ok(mut stale) = self.stale.lock() {
            for (_, x, z) in &repaint {
                stale.insert((*x, *z));
            }
        }

        // Coverage is a pass over every column in the world, which is worth it
        // when the palette changes because that changes every tile. A region
        // arriving changes one square, so it is reported by count alone.
        let generation = self.bump(Some(repaint));
        println!(
            "mapstique: {} regions reloaded — {} chunks (generation {generation})",
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
        println!("mapstique: surface {}", coverage.summary());
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

    /// The state of the map, and — when the caller says which generation it last
    /// drew — which tiles it needs to fetch again.
    fn info(&self, since: Option<u64>) -> String {
        let (min_x, min_z, max_x, max_z) = self.bounds();
        let mut body = format!(
            r#"{{"minX":{min_x},"minZ":{min_z},"maxX":{max_x},"maxZ":{max_z},"tile":{TILE},"levels":{},"chunks":{},"generation":{}"#,
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

    fn bounds(&self) -> (i32, i32, i32, i32) {
        self.world
            .read()
            .map_or((0, 0, 0, 0), |world| world.bounds())
    }

    fn chunks(&self) -> usize {
        self.world.read().map_or(0, |world| world.chunks.len())
    }

    fn tile(&self, level: u32, tx: i32, tz: i32) -> Result<Vec<u8>> {
        if let Ok(cache) = self.cache.lock()
            && let Some(bytes) = cache.get(&(level, tx, tz))
        {
            return Ok(bytes.clone());
        }

        let bytes = if level == 0 {
            let (Ok(world), Ok(palette)) = (self.world.read(), self.palette.read()) else {
                return Err(Error::Empty("the map is being reloaded".to_owned()));
            };
            render_tile(&Renderer::new(&world, &palette), tx, tz)?
        } else {
            // Built by the builder, not here. A coarse tile is made of four of the
            // level below, so making one on demand would make every tile beneath
            // it — a thousand renders for a level five, while somebody waits.
            let Some(image) = pyramid::read(&self.data, level, tx, tz) else {
                return Err(Error::Empty(format!("level {level} tile ({tx}, {tz}) is not built yet")));
            };
            let mut encoded = Vec::new();
            image
                .write_to(&mut Cursor::new(&mut encoded), ImageFormat::Png)
                .map_err(|error| {
                    Error::io("encoding a tile", std::io::Error::other(error.to_string()))
                })?;
            encoded
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

        let mapped: HashSet<(i32, i32)> = match self.world.read() {
            Ok(world) => world.regions.iter().copied().collect(),
            Err(_) => return,
        };

        let mut repainted: Vec<(u32, i32, i32)> = changed.iter().map(|&(x, z)| (0, x, z)).collect();
        let levels = self.levels();

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
                    eprintln!("mapstique: {error}");
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

        let generation = self.bump(Some(repainted.clone()));
        println!(
            "mapstique: {} tiles rebuilt across {levels} levels (generation {generation})",
            repainted.len()
        );
    }
}

/// Listens on the API socket for what the mod posts, on its own thread.
///
/// Separate from the map's own port on purpose: that one is meant to be reachable
/// and this one accepts writes, and anything that could reach a public write
/// endpoint could put people on the map who are not there.
fn serve_api(api: &ApiSocket, live: Arc<Live>) -> Result<()> {
    let server = match api {
        ApiSocket::Socket(path) => {
            if let Some(parent) = path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            // A socket left behind by a previous run would refuse the bind.
            let _ = std::fs::remove_file(path);

            // A socket address carries about a hundred bytes of path and no more,
            // which a deep data directory can exceed. Said here because the
            // failure that follows otherwise reads as `SUN_LEN` and nothing else.
            if path.as_os_str().len() >= SOCKET_PATH_LIMIT {
                return Err(Error::io(
                    format!(
                        "the socket path {} is {} bytes, over the {SOCKET_PATH_LIMIT} a \
                         unix socket allows",
                        path.display(),
                        path.as_os_str().len()
                    ),
                    std::io::Error::other("path too long"),
                ));
            }

            Server::http_unix(path)
        }
        ApiSocket::Address(address) => Server::http(address),
    }
    .map_err(|error| {
        Error::io(
            format!("listening for live data on {api}"),
            std::io::Error::other(error.to_string()),
        )
    })?;

    println!("mapstique: taking live data on {api}");

    std::thread::spawn(move || {
        for mut request in server.incoming_requests() {
            let response = posted(&mut request, &live);
            if let Err(error) = request.respond(response) {
                eprintln!("mapstique: API socket response failed: {error}");
            }
        }
    });

    Ok(())
}

/// One post from the mod.
fn posted(request: &mut Request, live: &Live) -> Response<Cursor<Vec<u8>>> {
    if *request.method() != Method::Post {
        return text(405, "the API socket takes posts only");
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

    let taken = match path {
        "/live/players" => live.set_players(body),
        "/live/markers" => live.set_markers(body),
        _ => return text(404, "not found"),
    };

    if taken {
        text(204, "")
    } else {
        text(400, "expected a JSON array")
    }
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

    let mut addresses = vec![format!("http://127.0.0.1:{port}")];
    if let Some(local) = local_address() {
        addresses.push(format!("http://{local}:{port}  (on your network)"));
    }
    addresses
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

/// The `since` of a query string, naming the generation a viewer last drew.
fn since_of(url: &str) -> Option<u64> {
    url.split_once('?')?
        .1
        .split('&')
        .find_map(|pair| pair.strip_prefix("since=")?.parse().ok())
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
    let mut bytes = Vec::new();
    image
        .write_to(&mut Cursor::new(&mut bytes), ImageFormat::Png)
        .map_err(|error| Error::io("encoding a tile", std::io::Error::other(error.to_string())))?;
    Ok(bytes)
}

fn png(bytes: Vec<u8>) -> Response<Cursor<Vec<u8>>> {
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

fn typed(body: &str, content_type: &str) -> Response<Cursor<Vec<u8>>> {
    // The page and the two feeds are the things that must never be stale.
    cached(body, content_type, "no-store")
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
fn viewer(min_x: i32, min_z: i32, max_x: i32, max_z: i32) -> String {
    include_str!("viewer.html")
        .replace("__TILE__", &TILE.to_string())
        .replace("__MIN_X__", &min_x.to_string())
        .replace("__MIN_Z__", &min_z.to_string())
        .replace("__MAX_X__", &max_x.to_string())
        .replace("__MAX_Z__", &max_z.to_string())
}
