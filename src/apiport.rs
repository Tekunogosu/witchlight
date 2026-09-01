//! The private channel the server mod posts on.
//!
//! Separate from the map's own port on purpose: that one is meant to be
//! reachable and this one accepts writes, and anything that could reach a public
//! write endpoint could put people on the map who are not there.
//!
//! Where the listener ends up is in [`crate::api`]; what it will answer is here.

use std::path::Path;
use std::sync::Arc;

use tiny_http::{Method, Request, Server};

use crate::api::Api;
use crate::auth::{Sessions, Who};
use crate::error::{Error, Result};
use crate::http::{self, Reply};
use crate::live::Live;
use crate::pending::Pending;
use crate::preferences::{Person, Preferences, Preset};
use crate::urls;
use crate::log::{say, warn};

/// Everything the mod may reach, gathered so the listener carries one value
/// rather than four.
struct Channel {
    live: Arc<Live>,
    sessions: Arc<Sessions>,
    pending: Arc<Pending>,
    preferences: Arc<Preferences>,
    api: Api,
}

/// Binds the API socket and answers on it, on a thread of its own.
pub fn serve(
    api: Api,
    live: Arc<Live>,
    sessions: Arc<Sessions>,
    pending: Arc<Pending>,
    preferences: Arc<Preferences>,
    exports: &Path,
) -> Result<()> {
    // Before the bind rather than after the failure: a file naming a listener
    // that does not exist sends the mod's posts at whatever holds that port now,
    // and the window where that is true should not include this function.
    Api::unpublish(exports);

    let listening = |what: &str| {
        Error::io(
            format!("listening for live data on {}", api.bind),
            std::io::Error::other(what.to_owned()),
        )
    };

    let server = Server::http(&api.bind).map_err(|error| listening(&error.to_string()))?;

    // Asked of the listener rather than read back from the setting, because the
    // setting is usually a request for whatever port is free and says nothing
    // about which one that turned out to be.
    let address = server
        .server_addr()
        .to_ip()
        .ok_or_else(|| listening("the listener has no address"))?;

    api.publish(exports, address.port());
    say!("taking live data on {address}");

    let channel = Channel { live, sessions, pending, preferences, api };
    std::thread::spawn(move || {
        for mut request in server.incoming_requests() {
            let response = posted(&mut request, &channel);
            if let Err(error) = request.respond(response) {
                warn!("API response failed: {error}");
            }
        }
    });

    Ok(())
}

/// One post from the mod.
fn posted(request: &mut Request, channel: &Channel) -> Reply {
    if *request.method() != Method::Post {
        return http::text(405, "the API channel takes posts only");
    }

    // Loopback is not a trust boundary on a machine other people have accounts
    // on, so reaching the port is not the same as being the mod.
    if !channel.api.authorized(request) {
        return http::text(401, "the API channel needs the token from api.json");
    }

    let url = request.url().to_owned();
    let path = urls::path(&url).to_owned();

    let Some(body) = http::body(request) else {
        return http::text(400, "unreadable body");
    };

    match path.as_str() {
        // The one thing on this channel that answers with something rather than
        // merely accepting it. Minting lives here because this is the only
        // listener the mod can reach and the only party that knows which uid is
        // which player is the mod — so the trust this needs is already here.
        "/auth/mint" => match asked_for(&body) {
            Some(who) => http::json(
                &serde_json::json!({ "Token": channel.sessions.mint(who) }).to_string(),
            ),
            None => http::text(400, "expected {\"Uid\":…, \"Name\":…}"),
        },

        // The markers people asked for on the web, which the mod cannot be sent
        // and so comes to collect. Emptied by the asking; see `Pending::take`.
        "/markers/pending" => http::json(
            &serde_json::to_string(&channel.pending.take())
                .unwrap_or_else(|_| r#"{"Make":[],"Change":[],"Remove":[]}"#.to_owned()),
        ),

        // What somebody has set for themselves, for the half of the mod that
        // makes a marker from in game. The map's own form reads and writes the
        // whole document over the public port under a session cookie; a game
        // client has no session and no browser, so the mod asks on its behalf —
        // it is the only party that knows which uid is which player, which is
        // the same trust minting a login word already needs.
        "/presets/of" => match uid_asked(&body) {
            Some(uid) => http::json(&said(&channel.preferences.of(&uid))),
            None => http::text(400, "expected {\"Uid\":…}"),
        },

        // One preset, made in front of somebody in game. Merged here rather than
        // read, changed and written back over two hops: a whole document written
        // from what it looked like when a window opened is a document that loses
        // whatever else moved in between.
        "/presets/keep" => match preset_asked(&body) {
            Some((uid, preset)) => http::json(&said(&channel.preferences.keep_one(&uid, preset))),
            None => http::text(400, "expected {\"Uid\":…, \"Preset\":{\"Pattern\":…}}"),
        },

        "/live/players" => taken(channel.live.set_players(body)),
        "/live/world" => taken(channel.live.set_world(body)),
        "/live/markers" => taken(channel.live.set_markers(body)),

        _ => http::text(404, "not found"),
    }
}

/// A post accepted, or refused because it is not the shape this build reads.
fn taken(ok: bool) -> Reply {
    if ok {
        http::text(204, "")
    } else {
        http::text(
            400,
            "expected what this build posts: an array of players, or markers sorted by who may see them",
        )
    }
}

/// What one person has set, as the mod reads it.
fn said(person: &Person) -> String {
    serde_json::to_string(person).unwrap_or_else(|_| "{}".to_owned())
}

/// Whose settings the mod is asking about.
fn uid_asked(body: &str) -> Option<String> {
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "PascalCase")]
    struct Asked {
        uid: String,
    }

    let asked: Asked = serde_json::from_str(body).ok()?;
    (!asked.uid.is_empty()).then_some(asked.uid)
}

/// Whose preset this is and what it says. A preset with nothing to match is
/// refused here rather than stored and never reached.
fn preset_asked(body: &str) -> Option<(String, Preset)> {
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "PascalCase")]
    struct Asked {
        uid: String,
        preset: Preset,
    }

    let asked: Asked = serde_json::from_str(body).ok()?;
    (!asked.uid.is_empty() && !asked.preset.pattern.trim().is_empty())
        .then_some((asked.uid, asked.preset))
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
