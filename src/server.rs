//! Bringing the map up.
//!
//! Everything here happens once: read what is on disk, say what was found, bind
//! the two ports, and hand the request threads a [`State`] to answer from. What
//! they answer is [`crate::routes`]; what keeps the state current is
//! [`crate::watch`].
//!
//! Tiles are rendered when asked for and kept, so starting up costs nothing and
//! only the part of the world someone actually looks at is ever drawn.

use std::path::Path;
use std::sync::Arc;

use tiny_http::Server;

use crate::api::Api;
use crate::config::Rules;
use crate::error::{Error, Result};
use crate::facts;
use crate::net;
use crate::palette::Palette;
use crate::pyramid;
use crate::routes;
use crate::state::State;
use crate::watch;
use crate::log::{say, warn};

/// How many threads take requests when the setting says to decide here.
///
/// The cap is deliberate: this shares a machine with the game server, which has
/// the better claim on its cores, and past a handful of threads a cold map is
/// bound by the tile cache rather than by rendering.
const MAX_WORKERS: usize = 64;

pub fn serve(
    bind: &str,
    data: &Path,
    palette: Palette,
    api: Api,
    threads: usize,
    cache_mb: usize,
    rules: Rules,
    autosave_interval: std::time::Duration,
    backfill_radius_chunks: i32,
) -> Result<()> {
    let state = Arc::new(State::load(data, palette, cache_mb.max(1) * 1024 * 1024, rules)?);
    let puller = Arc::new(crate::pull::Puller::new(data, backfill_radius_chunks));

    // The map is the product and live data is a garnish, so an API channel that
    // will not bind is said out loud and stepped over rather than taken as fatal.
    if let Err(error) = crate::apiport::serve(
        api,
        Arc::clone(&state.live),
        Arc::clone(&state.sessions),
        Arc::clone(&state.pending),
        Arc::clone(&state.preferences),
        Arc::clone(&puller),
        data,
    ) {
        warn!(
            "{error} — nobody will show on the map. Set `api_bind` to an address \
             this machine has free."
        );
    }

    let server = Server::http(bind).map(Arc::new).map_err(|error| {
        Error::io(format!("listening on {bind}"), std::io::Error::other(error.to_string()))
    })?;

    let addresses = net::reachable_at(bind);
    for address in &addresses {
        let note = if net::only_here(address) { "  (this machine only)" } else { "" };
        say!("serving on {address}{note}");
    }
    net::publish_addresses(data, bind, &addresses);

    let threads = workers(threads);
    say!("rendering on {threads} threads");

    settle(&state, data);
    watch::start(&state);
    start_autosave(&state, data, autosave_interval);

    start_frontier(&puller, &state);
    crate::pull::start(Arc::clone(&puller), Arc::clone(&state));

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

/// How often the map's own edge is re-offered to the puller.
///
/// Slower than a pull step: this walks every chunk currently held to find its
/// edge, which is worth doing far less often than the puller drains what it
/// already has queued.
const FRONTIER_EVERY: std::time::Duration = std::time::Duration::from_secs(5);

/// How often a player's own position is re-offered.
///
/// The mod posts a position every couple of seconds — see `LiveIntervalMs` on
/// its side — so asking more often than that would only ever see the same
/// answer twice.
const NEAR_EVERY: std::time::Duration = std::time::Duration::from_secs(2);

/// Starts the two clocks that keep the puller's frontier fed: a player's own
/// position, fast and first, and the map's own edge, slow and behind it.
fn start_frontier(puller: &Arc<crate::pull::Puller>, state: &Arc<State>) {
    {
        let puller = Arc::clone(puller);
        let state = Arc::clone(state);
        std::thread::spawn(move || {
            loop {
                std::thread::sleep(NEAR_EVERY);
                let positions = state.live.positions();
                if positions.is_empty() {
                    continue;
                }

                let edge = state.world.read().map(|world| world.edge).unwrap_or(0).max(1) as i32;
                let chunks: Vec<(i32, i32)> =
                    positions.iter().map(|&(x, z)| (x.div_euclid(edge), z.div_euclid(edge))).collect();

                puller.visit(chunks.iter().copied());
                let held: std::collections::HashSet<(i32, i32)> = state
                    .world
                    .read()
                    .map(|world| world.chunks.keys().copied().collect())
                    .unwrap_or_default();
                puller.seed_near(chunks, &held);
            }
        });
    }

    let puller = Arc::clone(puller);
    let state = Arc::clone(state);
    std::thread::spawn(move || {
        loop {
            std::thread::sleep(FRONTIER_EVERY);
            let Ok(world) = state.world.read() else { continue };
            let held: std::collections::HashSet<(i32, i32)> = world.chunks.keys().copied().collect();
            puller.seed_edge(held.iter().copied(), &held);
        }
    });
}

/// Starts the clock that writes this service's own snapshot of the world.
///
/// Its own thread rather than folded into [`watch::start`]'s clocks: those read
/// what changed on disk, and this writes what is held in memory, on a gap an
/// operator sets rather than one this decides for them.
fn start_autosave(state: &Arc<State>, data: &Path, every: std::time::Duration) {
    let state = Arc::clone(state);
    let data = data.to_path_buf();
    std::thread::spawn(move || {
        loop {
            std::thread::sleep(every);
            autosave(&state, &data);
        }
    });
}

/// Writes the snapshot once. A world nobody has loaded anything into yet writes
/// nothing — see [`crate::snapshot::write`] — so an idle server between mod
/// exports does not spend this thread's beat on an empty file.
fn autosave(state: &State, data: &Path) {
    let Ok(world) = state.world.read() else {
        return;
    };

    match crate::snapshot::write(data, &world) {
        Ok(()) => {}
        Err(error) => warn!("could not write the service snapshot: {error}"),
    }
}

/// Reconciles the stored zoom levels with the world and the palette in hand, and
/// says out loud anything that would otherwise show as a map that looks broken.
///
/// Whatever is still current is kept. Only regions with a level above them
/// missing or older than the region itself are rebuilt, so a run whose levels
/// are already current starts with nothing to do rather than redrawing a world
/// that has not moved.
fn settle(state: &State, data: &Path) {
    // Said out loud, because the alternative is a map whose coordinates quietly
    // disagree with every number the player can read off their own screen — and
    // nothing on either side would look wrong.
    if !facts::written(data) {
        say!(
            "no world.json — coordinates will be absolute rather than \
             counted from spawn, which means the server mod is older than this build"
        );
    }

    // Levels built from a region format this build no longer reads would show
    // terrain that has since been cleared, and levels painted by a build that
    // painted differently would show the right ground in the wrong colours. Both
    // go; both are redrawn from region files that are not in question.
    if pyramid::reset_unless_built_from(data, crate::columns::VERSION) {
        say!(
            "the stored levels were built by a different format or painter, so they have been cleared — the map redraws as it is asked for"
        );
    }

    // A palette with no colours in it draws bare ground everywhere. Said before
    // anything is served, because the map that follows is not broken — its
    // colours are missing, and those are two different things to go and fix.
    let blank = state.palette.read().is_ok_and(|palette| palette.paints_nothing());
    if blank {
        say!(
            "the palette has no colours at all — the finest zoom will not draw \
             and the stored levels are whatever the last usable palette left behind. \
             An admin joining the game supplies one."
        );
    }

    // Levels drawn with a different palette than the one in use disagree with the
    // level below them, which is a map that changes as it is zoomed. Redrawing
    // them settles it — but only when there is something to redraw them with.
    let drawn_with = pyramid::palette_built_from(data);
    let painting = state.palette.read().ok().map(|palette| palette.fingerprint.clone());
    let repaint = !blank && matches!((&drawn_with, &painting), (Some(was), Some(now)) if was != now);
    if repaint {
        say!("the stored levels were drawn with a different palette — redrawing them");
    }

    let levels = state.levels();
    let Ok(regions) = state.regions.lock() else {
        return;
    };
    let behind = pyramid::behind(data, &regions, levels);
    say!("{} of {} regions need their levels built", behind.len(), regions.len());

    state.mark_stale(behind);
    if repaint {
        state.mark_stale(regions.keys().copied());
    }
}

/// Takes requests until the server stops. Every thread runs this, and `recv`
/// hands each request to whichever is free — the whole reason a cold map no
/// longer arrives one tile at a time.
fn answer(server: &Server, state: &State) {
    while let Ok(mut request) = server.recv() {
        let response = routes::route(&mut request, state);
        if let Err(error) = request.respond(response) {
            warn!("response failed: {error}");
        }
    }
}

/// How many threads take requests. Zero means decide here.
fn workers(setting: usize) -> usize {
    if setting > 0 {
        return setting.min(MAX_WORKERS);
    }

    std::thread::available_parallelism().map_or(4, |cores| cores.get().clamp(1, 8))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_the_operator_asks_for_is_what_runs() {
        assert_eq!(workers(1), 1);
        assert_eq!(workers(12), 12);
        // A setting past the cap is a typo or a very large machine, and either
        // way this shares a box with the game server.
        assert_eq!(workers(10_000), MAX_WORKERS);
    }

    #[test]
    fn deciding_for_itself_leaves_cores_for_the_game() {
        let decided = workers(0);
        assert!((1..=8).contains(&decided), "decided {decided}");
    }
}
