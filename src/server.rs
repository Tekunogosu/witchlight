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
) -> Result<()> {
    let state = Arc::new(State::load(data, palette, cache_mb.max(1) * 1024 * 1024, rules)?);

    // The map is the product and live data is a garnish, so an API channel that
    // will not bind is said out loud and stepped over rather than taken as fatal.
    if let Err(error) = crate::apiport::serve(
        api,
        Arc::clone(&state.live),
        Arc::clone(&state.sessions),
        Arc::clone(&state.pending),
        Arc::clone(&state.preferences),
        data,
    ) {
        eprintln!("witchlight: {error}");
        eprintln!(
            "witchlight: nobody will show on the map. Set `api_bind` to an address \
             this machine has free."
        );
    }

    let server = Server::http(bind).map(Arc::new).map_err(|error| {
        Error::io(format!("listening on {bind}"), std::io::Error::other(error.to_string()))
    })?;

    let addresses = net::reachable_at(bind);
    for address in &addresses {
        let note = if net::only_here(address) { "  (this machine only)" } else { "" };
        println!("witchlight: serving on {address}{note}");
    }
    net::publish_addresses(data, bind, &addresses);

    let threads = workers(threads);
    println!("witchlight: rendering on {threads} threads");

    settle(&state, data);
    watch::start(&state);

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
        println!(
            "witchlight: no world.json — coordinates will be absolute rather than \
             counted from spawn, which means the server mod is older than this build"
        );
    }

    // Levels built from a region format this build no longer reads would show
    // terrain that has since been cleared, and levels painted by a build that
    // painted differently would show the right ground in the wrong colours. Both
    // go; both are redrawn from region files that are not in question.
    if pyramid::reset_unless_built_from(data, crate::columns::VERSION) {
        println!(
            "witchlight: the stored levels were built by a different format or painter, so they have been cleared — the map redraws as it is asked for"
        );
    }

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
    let repaint = !blank && matches!((&drawn_with, &painting), (Some(was), Some(now)) if was != now);
    if repaint {
        println!("witchlight: the stored levels were drawn with a different palette — redrawing them");
    }

    let levels = state.levels();
    let Ok(regions) = state.regions.lock() else {
        return;
    };
    let behind = pyramid::behind(data, &regions, levels);
    println!(
        "witchlight: {} of {} regions need their levels built",
        behind.len(),
        regions.len()
    );

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
            eprintln!("witchlight: response failed: {error}");
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
