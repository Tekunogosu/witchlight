//! Starts the map service.
//!
//! This module runs once at startup. It reads what is on disk, logs what it
//! found, binds both ports, and hands the request threads a [`State`] to answer
//! from. [`crate::web::routes`] answers the requests.
//! [`crate::protocol::watch`] keeps the state current.
//!
//! Tiles are rendered on demand and cached, so startup draws nothing and only
//! the part of the world someone looks at is ever rendered.

use std::path::Path;
use std::sync::Arc;

use tiny_http::Server;

use crate::protocol::api::Api;
use crate::util::error::{Error, Result};
use crate::mapdata::facts;
use crate::util::faults;
use crate::util::net;
use crate::render::pyramid;
use crate::web::routes;
use crate::state::State;
use crate::protocol::watch;
use crate::util::log::{say, warn};

/// The upper limit on request threads.
///
/// This service usually shares a machine with the game server, which has the
/// better claim on its cores. Past a handful of threads a cold map is bound by
/// the tile cache rather than by rendering.
const MAX_WORKERS: usize = 64;

pub fn serve(
    bind: &str,
    state: Arc<State>,
    api: Api,
    threads: usize,
    backfill_radius_chunks: i32,
) -> Result<()> {
    let data = state.data.as_path();
    let puller = Arc::new(crate::protocol::pull::Puller::new(Arc::clone(&state.store), data, backfill_radius_chunks));

    // An API channel that will not bind is logged and stepped over. The map
    // still serves without live data.
    if let Err(error) = crate::protocol::apiport::serve(api, Arc::clone(&state), data) {
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

    start_frontier(&puller, &state);
    start_collecting(&state);
    crate::protocol::pull::start(Arc::clone(&puller), Arc::clone(&state));

    let mut others = Vec::with_capacity(threads - 1);
    for _ in 1..threads {
        let server = Arc::clone(&server);
        let state = Arc::clone(&state);
        others.push(std::thread::spawn(move || answer(&server, &state)));
    }

    // This thread answers requests as well, rather than only supervising.
    answer(&server, &state);
    for thread in others {
        let _ = thread.join();
    }

    Ok(())
}

/// The interval between re-offering the map's edge to the puller.
///
/// Finding the edge walks every chunk currently held, so this runs far less
/// often than the puller drains its queue.
const FRONTIER_EVERY: std::time::Duration = std::time::Duration::from_secs(5);

/// The interval between re-offering each player's position.
///
/// The mod posts a position every couple of seconds, set by `LiveIntervalMs` on
/// its side. Polling faster would read the same answer twice.
const NEAR_EVERY: std::time::Duration = std::time::Duration::from_secs(2);

/// Returns how far one player sees, in chunks.
///
/// Prefers the operator's setting, then the view distance the game granted that
/// player, then the radius the mod says the server loads for anybody. Returns
/// zero when none of the three is known, which records nothing for that player.
fn sight_of(state: &State, puller: &crate::protocol::pull::Puller, granted: i32) -> i32 {
    if state.rules.sight_radius_chunks > 0 {
        state.rules.sight_radius_chunks
    } else if granted > 0 {
        granted
    } else {
        puller.reach()
    }
}

/// Starts the two threads that keep the puller's frontier fed. One posts player
/// positions on a fast interval. The other posts the map's edge on a slow one.
fn start_frontier(puller: &Arc<crate::protocol::pull::Puller>, state: &Arc<State>) {
    {
        let puller = Arc::clone(puller);
        let state = Arc::clone(state);
        faults::forever("frontier-near", move || {
            loop {
                std::thread::sleep(NEAR_EVERY);
                let whereabouts = state.live.whereabouts();
                if whereabouts.is_empty() {
                    continue;
                }

                // Record what each player can see from where they stand, and
                // offer the puller the ground beside them.
                let edge = state.chunk_edge().max(1) as i32;
                let mut stood: Vec<((i32, i32), i32)> = Vec::with_capacity(whereabouts.len());
                for at in &whereabouts {
                    let reach = sight_of(&state, &puller, at.reach);
                    if reach <= 0 {
                        continue;
                    }
                    state.seen_from(&at.uid, at.x, at.z, reach);
                    stood.push(((at.x.div_euclid(edge), at.z.div_euclid(edge)), reach));
                }
                puller.visit(stood.iter().copied());

                // Query the world in place. Copying out the set of held chunks
                // would allocate a map-sized set twice a second for a question
                // answered by one lookup.
                let Ok(world) = state.world.read() else { continue };
                let held = |at: (i32, i32)| world.chunks.contains_key(&at);
                puller.seed_near(stood.iter().map(|(chunk, _)| *chunk), &held);
            }
        });
    }

    let puller = Arc::clone(puller);
    let state = Arc::clone(state);
    faults::forever("frontier-edge", move || {
        loop {
            std::thread::sleep(FRONTIER_EVERY);
            let Ok(world) = state.world.read() else { continue };
            let held = |at: (i32, i32)| world.chunks.contains_key(&at);
            puller.seed_edge(world.chunks.keys().copied(), &held);
        }
    });
}

/// The interval between sweeps that free unreferenced chunk versions.
const COLLECT_EVERY: std::time::Duration = std::time::Duration::from_secs(60);

/// Starts the thread that frees chunk versions nobody references.
fn start_collecting(state: &Arc<State>) {
    let state = Arc::clone(state);
    faults::forever("collector", move || {
        loop {
            std::thread::sleep(COLLECT_EVERY);
            state.memory.collect();
        }
    });
}

/// Reconciles the stored zoom levels with the world and palette in hand, and
/// logs anything that would otherwise show as a broken-looking map.
///
/// Levels that are still current are kept. Only regions whose level above is
/// missing or older than the region itself are rebuilt, so a run whose levels
/// are already current has nothing to do.
fn settle(state: &State, data: &Path) {
    // Logged, because the alternative is a map whose coordinates disagree with
    // every number the player reads off their own screen while nothing on
    // either side looks wrong.
    if !facts::written(data) {
        say!(
            "no world.json — coordinates will be absolute rather than \
             counted from spawn, which means the server mod is older than this build"
        );
    }

    // Levels built from a region format this build no longer reads would show
    // terrain that has since been cleared. Levels painted by an earlier renderer
    // would show the right ground in the wrong colours. Both are cleared and
    // redrawn from the region files.
    if pyramid::reset_unless_built_from(data, crate::render::columns::VERSION) {
        say!(
            "the stored levels were built by a different format or renderer, so they have been cleared — the map redraws as it is asked for"
        );
    }

    // A palette with no colours draws bare ground everywhere. Logged before
    // anything is served, so the operator knows the map is missing colours
    // rather than broken.
    let blank = state.palette.read().is_ok_and(|palette| palette.paints_nothing());
    if blank {
        say!(
            "the palette has no colours at all — the finest zoom will not draw \
             and the stored levels are whatever the last usable palette left behind. \
             An admin joining the game supplies one."
        );
    }

    // Levels drawn with a different palette than the one in use disagree with
    // the level below them, so the map changes as it is zoomed. Redraw them,
    // but only when there is a usable palette to redraw them with.
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

/// Takes requests until the server stops. Every request thread runs this, and
/// `recv` hands each request to whichever thread is free.
fn answer(server: &Server, state: &Arc<State>) {
    while let Ok(mut request) = server.recv() {
        // `/events` holds its response open, so it gets a thread of its own
        // and this one goes back to taking requests. See `crate::web::events`.
        if crate::util::urls::path(request.url()) == "/events" {
            let state = Arc::clone(state);
            std::thread::spawn(move || wait_for_events(request, &state));
            continue;
        }

        // Answer inside a catch, so that one request that panics costs that
        // request and not the service.
        //
        // This loop runs on the main thread as well as on the workers, and a
        // panic reaching the main thread unwinds out of `serve` and ends the
        // run. That is the whole service gone because one reader asked for one
        // thing. The panic is still reported by the hook in
        // [`crate::util::faults`], which names the request below.
        //
        // `AssertUnwindSafe` because the state behind the request is shared
        // and already guarded by its own locks, and those locks hand back
        // their contents after a panic rather than staying poisoned. See
        // `crate::util::log::gate` for the same choice.
        let url = request.url().to_owned();
        let answered = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            routes::route(&mut request, state)
        }));

        let response = match answered {
            Ok(response) => response,
            Err(_) => {
                warn!("the request that panicked was for {url}, and was answered 500");
                crate::util::http::text(500, "that request could not be answered")
            }
        };

        if let Err(error) = request.respond(response) {
            warn!("response failed: {error}");
        }
    }
}

/// Answers `/events`. Waits until the map or the live feed has moved past what
/// the page last saw, then reports what changed. The wait happens on the thread
/// this function was given.
fn wait_for_events(request: tiny_http::Request, state: &State) {
    let url = request.url().to_owned();
    let since = crate::util::urls::param(&url, "since").and_then(|s| s.parse::<u64>().ok()).unwrap_or(0);
    let live = crate::util::urls::param(&url, "live").and_then(|s| s.parse::<u64>().ok()).unwrap_or(0);
    let who = state.sessions.who(&crate::util::http::cookies(&request));
    let uid = who.map(|who| who.uid);

    let waited = state.events.wait(|| state.generation() > since || state.events.live_seq() > live);
    let reply = match waited {
        None => crate::util::http::text(503, "too many clients waiting — retry later"),
        Some(_) => {
            let scope = state.scope_for(uid.as_deref());
            let generation = state.generation();
            let live_now = state.events.live_seq();
            // Carry only what changed. A page told the map moved forty times a
            // second must not receive forty copies of every player's position.
            let info = if generation > since { state.info(&scope, Some(since)) } else { "null".to_owned() };
            let feed = if live_now > live { state.live.body(uid.as_deref(), &state.preferences.colors()) } else { "null".to_owned() };
            crate::util::http::json(&format!(
                r#"{{"generation":{generation},"liveSeq":{live_now},"info":{info},"live":{feed}}}"#
            ))
        }
    };
    let _ = request.respond(reply);
}

/// Returns the number of request threads to run. A setting of zero picks a
/// count from the CPU count.
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
        // A setting past the cap is a typo or a very large machine. Either way
        // this service shares a box with the game server.
        assert_eq!(workers(10_000), MAX_WORKERS);
    }

    #[test]
    fn deciding_for_itself_leaves_cores_for_the_game() {
        let decided = workers(0);
        assert!((1..=8).contains(&decided), "decided {decided}");
    }
}
