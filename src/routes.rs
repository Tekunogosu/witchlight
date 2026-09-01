//! What the public port answers, and to what.
//!
//! One request, decided by its path alone: tile URLs carry a `?v=` so that a new
//! export is a new URL, and the query says nothing about what to serve.

use tiny_http::{Method, Request};

use crate::error::Error;
use crate::http::{self, Reply};
use crate::pending::{Gone, Marker};
use crate::preferences::Person;
use crate::state::State;
use crate::stored;
use crate::urls;
use crate::viewer;

pub fn route(request: &mut Request, state: &State) -> Reply {
    let url = request.url().to_owned();
    let path = urls::path(&url);

    match path {
        "/" => http::html(&viewer::page(state.bounds())),
        "/viewer.css" => http::asset(viewer::STYLE, "text/css"),
        "/viewer.js" => http::asset(viewer::SCRIPT, "application/javascript"),
        "/leaflet.js" => http::asset(viewer::LEAFLET_JS, "application/javascript"),
        "/leaflet.css" => http::asset(viewer::LEAFLET_CSS, "text/css"),

        // The one address that turns a word into a browser somebody knows. It
        // answers with a redirect so the word leaves the address bar at once:
        // what stays in history, in a bookmark and in a pasted link is `/`.
        "/login" => match urls::link_asked(&url).and_then(|link| state.sessions.redeem(link)) {
            Some(session) => http::redirect("/", Some(&crate::auth::seat(&session))),
            None => http::redirect("/?login=expired", None),
        },
        "/logout" => {
            state.sessions.forget(&http::cookies(request));
            http::redirect("/", Some(&crate::auth::unseat()))
        }

        "/me.json" => http::json(&state.me(&http::cookies(request))),
        // Whose markers these are depends on who is asking, and the answer is
        // worked out here rather than sent and filtered on the page: a browser
        // cannot be asked to hide what it has already been handed.
        "/live.json" => {
            let who = state.sessions.who(&http::cookies(request));
            http::json(&state.live.body(who.as_ref().map(|who| who.uid.as_str())))
        }
        "/colors.json" => http::json(&state.live.colors()),
        "/icons.json" => http::json(&state.icons()),
        "/info.json" => http::json(&state.info(urls::since_of(&url))),
        "/blocks.json" => {
            http::json(&state.blocks_like(&urls::decoded(urls::param(&url, "q").unwrap_or_default())))
        }
        "/block.json" => match urls::block_asked(&url).map(|(x, z)| state.block(x, z)) {
            Some(Some(body)) => http::json(&body),
            Some(None) => http::text(503, "the map is being reloaded"),
            None => http::text(400, "name the block with ?x= and ?z="),
        },

        "/markers" => made(request, state),
        "/me/preferences.json" | "/me/preferences" => preferences(request, state),

        _ => stored(request, state, path),
    }
}

/// The addresses whose shape carries a name or a position in it.
fn stored(request: &mut Request, state: &State, path: &str) -> Reply {
    if let Some(key) = urls::marker_key(path) {
        // Which of the three a marker's own address means is the method and
        // nothing else, so the one place that knows a marker by name is also the
        // one place that says what may be done to it.
        return match *request.method() {
            Method::Put => changed(request, state, key),
            Method::Delete => removed(request, state, key),
            _ => http::text(405, "a marker is changed with a put and taken away with a delete"),
        };
    }

    if let Some(name) = urls::icon_name(path) {
        return match stored::icon(&state.data, name) {
            Some(bytes) => http::svg(&bytes),
            None => http::text(404, "no icon by that name"),
        };
    }

    // Compiled in rather than read from the map directory, and so served like
    // the library rather than like a waypoint's mark: these cannot change for a
    // given build, and the page asks for them under its build's number.
    if let Some(name) = urls::chrome_name(path) {
        return match crate::chrome::icon(name) {
            Some(body) => http::asset(body, "image/svg+xml"),
            None => http::text(404, "the furniture wears no such mark"),
        };
    }

    if let Some(name) = urls::portrait_name(path) {
        return match stored::portrait(&state.data, name) {
            Some(bytes) => http::portrait(&bytes),
            None => http::text(404, "nobody by that name has sent a picture"),
        };
    }

    if let Some(at) = urls::tile_coords(path) {
        return match state.tile(at) {
            Ok(bytes) => http::tile(&bytes),
            // A tile nobody has built is missing, not broken. Saying so lets a
            // viewer draw around it rather than treat the map as failing, and
            // keeps a real failure worth noticing.
            Err(Error::Empty(why)) => http::text(404, &why),
            Err(error) => http::text(500, &format!("render failed: {error}")),
        };
    }

    http::text(404, "not found")
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
fn made(request: &mut Request, state: &State) -> Reply {
    if *request.method() != Method::Post {
        return http::text(405, "markers are made with a post");
    }

    let (who, body) = match asked(request, state, "make a marker") {
        Ok(both) => both,
        Err(refusal) => return refusal,
    };

    let wanted = match Marker::wanted(&who.uid, &body) {
        Ok(wanted) => wanted,
        Err(why) => return http::text(400, why),
    };

    let key = wanted.key.clone();
    if !state.pending.want(wanted) {
        return http::text(503, "the game server is not collecting markers");
    }

    accepted(&key)
}

/// A change to a marker that already exists.
///
/// Whether this person may is asked twice and answered by the mod. What is
/// decided here is only that they are somebody: the service knows who owns what
/// from a post that is seconds old, so the gate is the half holding the waypoint.
fn changed(request: &mut Request, state: &State, key: &str) -> Reply {
    let (who, body) = match asked(request, state, "change a marker") {
        Ok(both) => both,
        Err(refusal) => return refusal,
    };

    let edit = match Marker::changed(&who.uid, key, &body) {
        Ok(edit) => edit,
        Err(why) => return http::text(400, why),
    };

    if !state.pending.change(edit) {
        return http::text(503, "the game server is not collecting markers");
    }

    accepted(key)
}

/// A marker somebody asked to be taken away.
///
/// No body: a removal names a waypoint rather than describing one. Whether this
/// person may is the mod's to answer against the waypoint itself, exactly as it
/// is for a change — what is decided here is only that they are somebody.
///
/// Accepted, not done, like the other two. The page watches for the marker to
/// stop arriving, which is the only honest confirmation there is.
fn removed(request: &mut Request, state: &State, key: &str) -> Reply {
    let Some(who) = state.sessions.who(&http::cookies(request)) else {
        return unknown("delete a marker");
    };

    let gone = match Gone::asked(&who.uid, key) {
        Ok(gone) => gone,
        Err(why) => return http::text(400, why),
    };

    if !state.pending.remove(gone) {
        return http::text(503, "the game server is not collecting markers");
    }

    accepted(key)
}

/// What one person has set for themselves, and the setting of it.
///
/// A whole document either way rather than a field at a time. It is a handful of
/// presets and two switches — small enough that sending all of it costs nothing,
/// and a page that holds the lot and puts the lot back needs no merge rules and
/// no route per field.
fn preferences(request: &mut Request, state: &State) -> Reply {
    let Some(who) = state.sessions.who(&http::cookies(request)) else {
        return unknown("keep settings");
    };

    match *request.method() {
        Method::Get => http::json(&kept(state, &who.uid)),
        Method::Put => {
            let Some(body) = http::body(request) else {
                return http::text(400, "unreadable body");
            };
            let Ok(person) = serde_json::from_str::<Person>(&body) else {
                return http::text(400, "expected presets and defaults");
            };
            if state.preferences.set(&who.uid, person) {
                http::json(&kept(state, &who.uid))
            } else {
                http::text(500, "those could not be kept")
            }
        }
        _ => http::text(405, "settings are read with a get and kept with a put"),
    }
}

fn kept(state: &State, uid: &str) -> String {
    serde_json::to_string(&state.preferences.of(uid)).unwrap_or_default()
}

/// Who is asking and what they sent, for the two routes that take a write.
///
/// Both refusals live here rather than at each: they are the same two, worded
/// the same way, and a session that has expired must not read as a body that
/// could not be parsed.
fn asked(
    request: &mut Request,
    state: &State,
    doing: &str,
) -> std::result::Result<(crate::auth::Who, String), Reply> {
    let Some(who) = state.sessions.who(&http::cookies(request)) else {
        return Err(unknown(doing));
    };
    let Some(body) = http::body(request) else {
        return Err(http::text(400, "unreadable body"));
    };
    Ok((who, body))
}

fn unknown(doing: &str) -> Reply {
    http::text(401, &format!("run /witchlight login in the game to {doing}"))
}

/// Accepted, not done. The status says so, and so does the page.
fn accepted(key: &str) -> Reply {
    http::json(&serde_json::json!({ "Key": key }).to_string()).with_status_code(202)
}
