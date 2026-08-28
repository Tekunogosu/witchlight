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
use crate::urls;

/// Everything the mod may reach, gathered so the listener carries one value
/// rather than four.
struct Channel {
    live: Arc<Live>,
    sessions: Arc<Sessions>,
    pending: Arc<Pending>,
    api: Api,
}

/// Binds the API socket and answers on it, on a thread of its own.
pub fn serve(
    api: Api,
    live: Arc<Live>,
    sessions: Arc<Sessions>,
    pending: Arc<Pending>,
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
    println!("witchlight: taking live data on {address}");

    let channel = Channel { live, sessions, pending, api };
    std::thread::spawn(move || {
        for mut request in server.incoming_requests() {
            let response = posted(&mut request, &channel);
            if let Err(error) = request.respond(response) {
                eprintln!("witchlight: API response failed: {error}");
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
                .unwrap_or_else(|_| r#"{"Make":[],"Change":[]}"#.to_owned()),
        ),

        "/live/players" => taken(channel.live.set_players(body)),
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
