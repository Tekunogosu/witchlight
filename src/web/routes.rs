//! Routes requests on the public port.
//!
//! Each request is decided by its path alone. Tile URLs carry a `?v=` so a new
//! export is a new URL, and the query never decides what is served.

use tiny_http::{Method, Request};

use crate::util::error::Error;
use crate::util::http::{self, Reply};
use crate::protocol::pending::{Claim, ClaimEdit, ClaimGone, Gone, Marker, Pin};
use crate::protocol::preferences::Person;
use crate::state::State;
use crate::mapdata::stored;
use crate::util::urls;
use crate::page::viewer;

pub fn route(request: &mut Request, state: &State) -> Reply {
    let url = request.url().to_owned();
    let path = urls::path(&url);

    match path {
        "/" => http::html(&viewer::page(state.bounds(), state.rules.live_refresh_ms)),
        "/viewer.css" => http::asset(viewer::STYLE, "text/css"),
        "/viewer.js" => http::asset(viewer::SCRIPT, "application/javascript"),
        "/leaflet.js" => http::asset(viewer::LEAFLET_JS, "application/javascript"),
        "/leaflet.css" => http::asset(viewer::LEAFLET_CSS, "text/css"),

        // This is the one address that turns a login token into a known
        // browser. It answers with a redirect so the token leaves the address
        // bar at once, and history, bookmarks and pasted links hold `/`.
        "/login" => match urls::link_asked(&url).and_then(|link| state.sessions.redeem(link)) {
            Some(session) => http::redirect("/", Some(&state.sessions.seat(&session))),
            None => http::redirect("/?login=expired", None),
        },
        "/logout" => {
            state.sessions.forget(&http::cookies(request));
            http::redirect("/", Some(&crate::protocol::auth::unseat()))
        }

        "/me" => http::json(&state.me(&http::cookies(request))),
        // Who is asking decides which markers these are. The answer is worked
        // out here rather than sent and filtered on the page, because a browser
        // cannot be asked to hide what it has already been handed.
        "/live" => {
            let who = state.sessions.who(&http::cookies(request));
            http::json(&state.live.body(who.as_ref().map(|who| who.uid.as_str()), &state.preferences.colors()))
        }
        // List which plugins have registered, so the page can load each one's
        // script. This is served rather than written into the page, because a
        // plugin registers while the server runs and a page built once would not
        // know about one that arrived later.
        "/plugins" => http::json(
            &serde_json::to_string(&state.plugins.names()).unwrap_or_else(|_| "[]".to_owned()),
        ),
        "/colors" => http::json(&state.live.colors()),
        "/icons" => http::json(&state.icons()),
        "/info" => {
            let who = state.sessions.who(&http::cookies(request));
            let scope = state.scope_for(who.as_ref().map(|who| who.uid.as_str()));
            http::json(&state.info(&scope, urls::since_of(&url)))
        }
        "/blocks" => {
            http::json(&state.blocks_like(&urls::decoded(urls::param(&url, "q").unwrap_or_default())))
        }
        "/block" => match urls::block_asked(&url).map(|(x, z)| {
            let who = state.sessions.who(&http::cookies(request));
            state.block(&state.scope_for(who.as_ref().map(|who| who.uid.as_str())), x, z)
        }) {
            Some(Some(body)) => http::json(&body),
            Some(None) => http::text(503, "the map is being reloaded"),
            None => http::text(400, "name the block with ?x= and ?z="),
        },

        "/markers" => made(request, state),
        "/claims" => claimed(request, state),
        "/me/preferences" => preferences(request, state),

        _ => stored(request, state, path),
    }
}

/// Routes the addresses whose shape carries a name or a position.
fn stored(request: &mut Request, state: &State, path: &str) -> Reply {
    // Match this before the marker's own address, because it is a longer path
    // with the same prefix.
    if let Some(key) = urls::marker_pin_key(path) {
        // Pinning and unpinning act on the same thing, so the method says which.
        // A marker's own address follows the same rule.
        return match *request.method() {
            Method::Put => pinned(request, state, key, true),
            Method::Delete => pinned(request, state, key, false),
            _ => http::text(405, "a marker is pinned with a PUT and unpinned with a DELETE"),
        };
    }

    if let Some(key) = urls::marker_key(path) {
        // The method alone decides which of the three a marker's address means,
        // so the one place that knows a marker by name is also the one place
        // that says what may be done to it.
        return match *request.method() {
            Method::Put => changed(request, state, key),
            Method::Delete => removed(request, state, key),
            _ => http::text(405, "a marker is changed with a PUT and taken away with a DELETE"),
        };
    }

    // The method alone decides which of the two a claim's address means, as it
    // does for a marker.
    if let Some(key) = urls::claim_key(path) {
        return match *request.method() {
            Method::Put => claim_changed(request, state, key),
            Method::Delete => claim_removed(request, state, key),
            _ => http::text(405, "a claim is changed with a PUT and given up with a DELETE"),
        };
    }

    // Serve a plugin's rows as this reader may see them. The session decides
    // whose rows those are, as it does for markers on `/live`. A plugin declares
    // the shape of its rows and never who may read one.
    if let Some(name) = urls::plugin_data(path) {
        let Some(plugin) = state.plugins.get(name) else {
            return http::text(404, "no plugin by that name has registered");
        };
        let who = state.sessions.who(&http::cookies(request));
        // Use this plugin's own sharing rather than the terrain's. A reader who
        // may see where somebody has been has not been shown what they found
        // there. See `State::plugin_sources`.
        let sources = state.plugin_sources(name, who.as_ref().map(|who| who.uid.as_str()));
        // Use the whole address rather than the path, because the ranges asked
        // for are in the query and this is the one address that reads one.
        let asked = urls::ranges(request.url());
        return match plugin.read(&sources, &asked) {
            Ok(body) => http::json(&body),
            // A reader can ask for a range of a column the plugin never
            // declared ranged. The answer names that column rather than failing
            // quietly.
            Err(error) => http::text(400, &error.to_string()),
        };
    }

    // Serve who this reader shares one plugin's rows with. Match it before a
    // row's own address, which is a longer path of the same shape.
    if let Some(name) = urls::plugin_shares(path) {
        if state.plugins.get(name).is_none() {
            return http::text(404, "no plugin by that name has registered");
        }
        let Some(who) = state.sessions.who(&http::cookies(request)) else {
            return http::text(401, "only somebody signed in shares anything");
        };
        return match *request.method() {
            Method::Get => match state.store.plugin_shared_with(name, &who.uid) {
                Ok(groups) => http::json(&serde_json::to_string(&groups).unwrap_or_else(|_| "[]".to_owned())),
                Err(error) => http::text(500, &error.to_string()),
            },
            // Write the whole set at once. A half-written set would leave
            // somebody sharing with a group they had just deselected.
            Method::Put => {
                let Some(body) = http::body(request) else {
                    return http::text(400, "unreadable body");
                };
                let Ok(groups) = serde_json::from_str::<Vec<i32>>(&body) else {
                    return http::text(400, "expected an array of group ids");
                };
                match state.keep_plugin_shares(name, &who.uid, &groups) {
                    Ok(()) => http::text(204, ""),
                    Err(error) => http::text(500, &error.to_string()),
                }
            }
            _ => http::text(405, "sharing is read with a GET and set with a PUT"),
        };
    }

    // Delete one of a plugin's rows, on behalf of whoever owns it.
    if let Some((name, key)) = urls::plugin_row(path) {
        if *request.method() != Method::Delete {
            return http::text(405, "a plugin's row is taken away with a DELETE");
        }
        let Some(plugin) = state.plugins.get(name) else {
            return http::text(404, "no plugin by that name has registered");
        };
        let Some(who) = state.sessions.who(&http::cookies(request)) else {
            return http::text(401, "only somebody signed in may take a row away");
        };
        return match plugin.forget(&who.uid, &key) {
            Ok(true) => http::text(204, ""),
            Ok(false) => http::text(404, "no row of yours by that key"),
            Err(error) => http::text(500, &error.to_string()),
        };
    }

    // Serve the script a plugin runs on the page. That script is the plugin
    // itself rather than an asset it ships, so it sits at the plugin's own root
    // beside its database, and only this one name is answered.
    if let Some(plugin) = urls::plugin_script(path) {
        let root = crate::mapdata::plugins::plugins_dir(&state.data);
        return match crate::mapdata::plugins::bundle(&root, plugin) {
            Some(body) => http::plugin_script(body.as_bytes()),
            None => http::text(404, "that plugin ships no script"),
        };
    }

    // Serve a file a plugin shipped for the page to show. It is read from disk
    // rather than compiled in, because a plugin cannot be compiled into a binary
    // that shipped before it existed.
    if let Some((plugin, asset)) = urls::plugin_asset(path) {
        let at = crate::mapdata::plugins::plugins_dir(&state.data).join(plugin).join("assets").join(asset);
        return match std::fs::read(&at) {
            Ok(bytes) => http::plugin_asset(&bytes, asset_type(asset)),
            Err(_) => http::text(404, "no such file in that plugin"),
        };
    }

    if let Some(name) = urls::icon_name(path) {
        return match stored::icon(&state.data, name) {
            Some(bytes) => http::svg(&bytes),
            None => http::text(404, "no icon by that name"),
        };
    }

    // These are compiled in rather than read from the map directory, so they
    // are served like the library rather than like a waypoint's mark. They
    // cannot change for a given build, and the page asks for them under its
    // build's number.
    if let Some(name) = urls::chrome_name(path) {
        return match crate::page::chrome::icon(name) {
            Some(body) => http::asset(body, "image/svg+xml"),
            None => http::text(404, "no such mark"),
        };
    }

    if let Some(name) = urls::portrait_name(path) {
        return match stored::portrait(&state.data, name) {
            Some(bytes) => http::portrait(&bytes),
            None => http::text(404, "nobody by that name has sent a picture"),
        };
    }

    if let Some(at) = urls::tile_coords(path) {
        // Who is asking decides whose tile this is. A tile drawn for one person
        // is marked private, so nothing between here and their browser hands it
        // to anybody else.
        //
        // The encoding comes from the reader's own setting, read from the
        // session and never from the address. The `f=` a page writes there only
        // makes a change of setting a change of address.
        let who = state.sessions.who(&http::cookies(request));
        let scope = state.scope_for(who.as_ref().map(|who| who.uid.as_str()));
        let format = who.as_ref().map_or_else(Default::default, |who| state.preferences.tile_format_of(&who.uid));
        return match state.tile_for(&scope, at, format) {
            Ok(bytes) if scope.is_whole() => http::tile(&bytes, format.mime()),
            Ok(bytes) => http::private_tile(&bytes, format.mime()),
            // A tile nobody has built is missing rather than broken. Saying so
            // lets a viewer draw around it rather than treat the map as failing,
            // and keeps a real failure worth noticing.
            Err(Error::Empty(why)) => http::text(404, &why),
            Err(error) => http::text(500, &format!("render failed: {error}")),
        };
    }

    http::text(404, "not found")
}

/// Accepts a marker somebody asked for on the map's form.
///
/// This is a write, so it must know who is asking. The session cookie is the
/// whole proof, the same one that decides whose private markers a page is sent.
///
/// It answers with the name the marker will be made under. Nothing has been made
/// yet, and the game will not hear of it until the mod next collects. The page
/// watches its markers for that name to appear, which is the only confirmation
/// available. A service that said "done" here would report on something it does
/// not do.
fn made(request: &mut Request, state: &State) -> Reply {
    if *request.method() != Method::Post {
        return http::text(405, "markers are made with a POST");
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

/// Accepts a land claim somebody drew on the map.
///
/// The request is accepted rather than performed, as a marker is. The game has
/// not heard of it, and the mod decides whether it may exist at all against the
/// game's own privilege, allowance, minimum size and overlap rules. Answering any
/// of those here would require a second copy of the game's rules, which would
/// give a second answer.
///
/// The page is told the request was taken and watches for the claim to appear
/// among the ones it is sent.
fn claimed(request: &mut Request, state: &State) -> Reply {
    if *request.method() != Method::Post {
        return http::text(405, "claims are made with a POST");
    }

    let (who, body) = match asked(request, state, "claim land") {
        Ok(both) => both,
        Err(refusal) => return refusal,
    };

    let drawn = match Claim::drawn(&who.uid, &body) {
        Ok(drawn) => drawn,
        Err(why) => return http::text(400, why),
    };

    if !state.pending.claim(drawn) {
        return http::text(503, "the game server is not collecting claims");
    }

    // There is nothing to name a claim by. A marker is answered with the name
    // it will be made under, because this service mints that name. A land claim
    // is the game's own and carries nothing this service could decide
    // beforehand, so the page is told it was taken and watches the ground it drew
    // on.
    http::json(&serde_json::json!({ "Asked": true }).to_string()).with_status_code(202)
}

/// Accepts a change to a marker that already exists.
///
/// The mod decides whether this person may make the change. This route decides
/// only that they are signed in. The service knows who owns what from a post that
/// is seconds old, so the half holding the waypoint is the one that can say.
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

/// Accepts a marker somebody asked to be taken away.
///
/// The request carries no body, because a removal names a waypoint rather than
/// describing one. The mod decides whether this person may, against the waypoint
/// itself, as it does for a change. This route decides only that they are signed
/// in.
///
/// The request is accepted rather than performed. The page watches for the marker
/// to stop arriving.
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

/// Accepts a marker somebody asked to keep in sight on their own map, or to stop
/// keeping.
///
/// The request carries no body. The method says which way it goes, and a pin
/// names a waypoint rather than describing one. The mod decides whether they may,
/// against the waypoint itself. Anybody the marker is shared with may pin it,
/// which is a lower bar than changing one, because a pin affects only the
/// pinner's own map.
///
/// The request is accepted rather than performed. The page watches for the pin to
/// appear among the ones it is sent.
fn pinned(request: &mut Request, state: &State, key: &str, on: bool) -> Reply {
    let Some(who) = state.sessions.who(&http::cookies(request)) else {
        return unknown("pin a marker");
    };

    let pin = match Pin::asked(&who.uid, key, on) {
        Ok(pin) => pin,
        Err(why) => return http::text(400, why),
    };

    if !state.pending.pin(pin) {
        return http::text(503, "the game server is not collecting markers");
    }

    accepted(key)
}

/// Accepts a change to a land claim that already exists.
///
/// The change carries what the claim is called and who it lets in. The mod
/// decides whether this person may, against the claim itself. The service knows
/// who owns what only from a post that is seconds old, so the half holding the
/// land is the one that can say.
fn claim_changed(request: &mut Request, state: &State, key: &str) -> Reply {
    let (who, body) = match asked(request, state, "change a claim") {
        Ok(both) => both,
        Err(refusal) => return refusal,
    };

    let edit = match ClaimEdit::asked(&who.uid, key, &body) {
        Ok(edit) => edit,
        Err(why) => return http::text(400, why),
    };

    if !state.pending.claim_edit(edit) {
        return http::text(503, "the game server is not collecting claims");
    }

    accepted(key)
}

/// Accepts a claim somebody asked to give up.
///
/// The request carries no body, because giving up land names a claim rather than
/// describing one. The mod decides whether they may, against the claim itself, as
/// it does for a change.
fn claim_removed(request: &mut Request, state: &State, key: &str) -> Reply {
    let Some(who) = state.sessions.who(&http::cookies(request)) else {
        return unknown("give up a claim");
    };

    let gone = match ClaimGone::asked(&who.uid, key) {
        Ok(gone) => gone,
        Err(why) => return http::text(400, why),
    };

    if !state.pending.claim_gone(gone) {
        return http::text(503, "the game server is not collecting claims");
    }

    accepted(key)
}

/// Serves and accepts what one person has set for themselves.
///
/// It reads and writes the whole document rather than one field at a time. The
/// document is a handful of presets and a few switches, small enough that sending
/// all of it costs nothing, and a page that holds and returns all of it needs no
/// merge rules and no route per field.
fn preferences(request: &mut Request, state: &State) -> Reply {
    let Some(who) = state.sessions.who(&http::cookies(request)) else {
        return unknown("save settings");
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
            if state.keep_person(&who.uid, person) {
                http::json(&kept(state, &who.uid))
            } else {
                http::text(500, "those could not be saved")
            }
        }
        _ => http::text(405, "settings are read with a GET and saved with a PUT"),
    }
}

fn kept(state: &State, uid: &str) -> String {
    serde_json::to_string(&state.preferences.of(uid)).unwrap_or_default()
}

/// Returns who is asking and what they sent, for the routes that take a write.
///
/// Both refusals live here rather than at each route, so they are worded the same
/// way and an expired session never reads as a body that could not be parsed.
fn asked(
    request: &mut Request,
    state: &State,
    doing: &str,
) -> std::result::Result<(crate::protocol::auth::Who, String), Reply> {
    let Some(who) = state.sessions.who(&http::cookies(request)) else {
        return Err(unknown(doing));
    };
    let Some(body) = http::body(request) else {
        return Err(http::text(400, "unreadable body"));
    };
    Ok((who, body))
}

/// Returns the content type for a file a plugin shipped.
///
/// It is a fixed table rather than a guess, and a plugin's own word is never one
/// of the answers. The file's extension decides the type, and an extension this
/// does not know is served as opaque bytes. A plugin ships pictures and scripts
/// the page already asked for, and anything else arriving under a type of its own
/// choosing would serve something never meant to be served.
fn asset_type(name: &str) -> &'static str {
    match name.rsplit_once('.').map(|(_, end)| end) {
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("webp") => "image/webp",
        Some("gif") => "image/gif",
        Some("js") => "application/javascript",
        Some("css") => "text/css",
        Some("json") => "application/json",
        _ => "application/octet-stream",
    }
}

fn unknown(doing: &str) -> Reply {
    http::text(401, &format!("run /witchlight login in the game to {doing}"))
}

/// Returns the response for a request that was accepted rather than performed.
/// The status code says so, and so does the page.
fn accepted(key: &str) -> Reply {
    http::json(&serde_json::json!({ "Key": key }).to_string()).with_status_code(202)
}
