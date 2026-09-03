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
use crate::auth::Who;
use crate::error::{Error, Result};
use crate::http::{self, Reply};
use crate::preferences::{Person, Preset};
use crate::state::State;
use crate::store::Arrived;
use crate::urls;
use crate::log::{say, warn};

/// Everything the mod may reach.
struct Channel {
    state: Arc<State>,
    api: Api,
}

/// Binds the API socket and answers on it, on a thread of its own.
pub fn serve(api: Api, state: Arc<State>, exports: &Path) -> Result<()> {
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

    let channel = Channel { state, api };
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
                &serde_json::json!({ "Token": channel.state.sessions.mint(who) }).to_string(),
            ),
            None => http::text(400, "expected {\"Uid\":…, \"Name\":…}"),
        },

        // What people asked for on the web — markers, and the land claims
        // somebody drew — which the mod cannot be sent and so comes to collect.
        // Emptied by the asking; see `Pending::take`.
        "/pending" => http::json(
            &serde_json::to_string(&channel.state.pending.take())
                .unwrap_or_else(|_| {
                    r#"{"Markers":{"Make":[],"Change":[],"Remove":[],"Pin":[]},"#.to_owned()
                        + r#""Claims":{"Make":[],"Change":[],"Remove":[]}}"#
                }),
        ),

        // What somebody has set for themselves, for the half of the mod that
        // makes a marker from in game. The map's own form reads and writes the
        // whole document over the public port under a session cookie; a game
        // client has no session and no browser, so the mod asks on its behalf —
        // it is the only party that knows which uid is which player, which is
        // the same trust minting a login word already needs.
        "/presets/of" => match uid_asked(&body) {
            Some(uid) => http::json(&said(&channel.state.preferences.of(&uid))),
            None => http::text(400, "expected {\"Uid\":…}"),
        },

        // One preset, made in front of somebody in game. Merged here rather than
        // read, changed and written back over two hops: a whole document written
        // from what it looked like when a window opened is a document that loses
        // whatever else moved in between.
        "/presets/keep" => match preset_asked(&body) {
            Some((uid, preset)) => http::json(&said(&channel.state.preferences.keep_one(&uid, preset))),
            None => http::text(400, "expected {\"Uid\":…, \"Preset\":{\"Pattern\":…}}"),
        },

        "/live/players" => {
            let ok = channel.state.live.set_players(body);
            if ok {
                // Who is in which group rides the player post, and is what a
                // map shared with a group is shared against.
                channel.state.memory.set_groups(channel.state.live.groups());
            }
            moved(channel, ok)
        }
        // The land claims, with the names of everybody the mod says may see
        // them. Who may is a privilege, and the mod is the half that knows who
        // holds one — see the `Live` this hands it to for why they travel beside
        // the claims rather than as a copy of them per person.
        "/live/claims" => moved(channel, channel.state.live.set_claims(body)),
        "/live/world" => moved(channel, channel.state.live.set_world(body)),
        "/live/markers" => moved(channel, channel.state.live.set_markers(body)),

        // What the map already holds, for a mod that has just started and has
        // no memory of what it sent a previous service: coordinates, season and
        // the checksum of each chunk's record, so a chunk loading again is not
        // read and sent for nothing.
        "/terrain/held" => match channel.state.store.held() {
            Ok(held) => {
                let edge = channel.state.chunk_edge();
                let chunks: Vec<serde_json::Value> = held
                    .into_iter()
                    .map(|(cx, cz, crc, season)| serde_json::json!([cx, cz, crc, season]))
                    .collect();
                http::json(&serde_json::json!({ "Edge": edge, "Chunks": chunks }).to_string())
            }
            Err(error) => http::text(500, &format!("could not read what is held: {error}")),
        },

        // The ground itself: chunks whose surface moved, as the mod read them,
        // and chunks whose season turned. Into the database, into the world,
        // and announced to every browser at once — see `State::take_chunks`.
        "/terrain" => match terrain_asked(&body) {
            Some(pushed) => {
                let stored = channel.state.take_chunks(pushed.edge, &pushed.arrived, std::time::SystemTime::now());
                channel.state.terrain_changed(&stored);

                let turned: Vec<(i32, i32)> = pushed
                    .seasons
                    .iter()
                    .filter(|&&(cx, cz, season)| channel.state.take_season(cx, cz, season))
                    .map(|&(cx, cz, _)| crate::store::region_of(cx, cz))
                    .collect();
                channel.state.tiles_changed(turned);
                http::text(204, "")
            }
            None => http::text(400, "expected {\"Edge\":…, \"Chunks\":[{\"X\":…, \"Z\":…, \"Season\":…, \"Record\":…}]}"),
        },

        _ => http::text(404, "not found"),
    }
}

/// A live post accepted, and every open browser told, or refused.
fn moved(channel: &Channel, ok: bool) -> Reply {
    if ok {
        channel.state.events.live_changed();
    }
    taken(ok)
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

/// What the mod pushed: the chunk edge, and one entry per chunk. An entry
/// with a record is ground that moved; one without is a season that turned.
struct Pushed {
    edge: usize,
    arrived: Vec<Arrived>,
    seasons: Vec<(i32, i32, u8)>,
}

/// The mod's terrain envelope, taken apart into what moved and what turned.
///
/// A record is base64 of a deflated record — the same bytes a region file held
/// for a chunk — and one that does not inflate to `edge * edge` entries is
/// refused with the whole post, since a chunk the mod mis-sent is not a chunk
/// worth half-storing.
fn terrain_asked(body: &str) -> Option<Pushed> {
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "PascalCase")]
    struct Envelope {
        edge: usize,
        #[serde(default)]
        chunks: Vec<Entry>,
    }

    #[derive(serde::Deserialize)]
    #[serde(rename_all = "PascalCase")]
    struct Entry {
        x: i32,
        z: i32,
        #[serde(default)]
        season: u8,
        #[serde(default)]
        record: Option<String>,
    }

    let envelope: Envelope = serde_json::from_str(body).ok()?;
    if envelope.edge == 0 || envelope.edge > 64 {
        return None;
    }
    let wanted = envelope.edge * envelope.edge * crate::columns::ENTRY_BYTES;

    let mut arrived = Vec::new();
    let mut seasons = Vec::new();
    for entry in envelope.chunks {
        match entry.record {
            Some(record) => {
                let record = crate::columns::unpack(&crate::wire::decode(&record).ok()?)?;
                if record.len() != wanted {
                    return None;
                }
                arrived.push(Arrived { cx: entry.x, cz: entry.z, season: entry.season, record });
            }
            None => seasons.push((entry.x, entry.z, entry.season)),
        }
    }
    Some(Pushed { edge: envelope.edge, arrived, seasons })
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

#[cfg(test)]
mod tests {
    use super::*;

    /// One record the way the mod packs one, as base64 of the deflate stream.
    fn packed(edge: usize, block: u16) -> String {
        let mut record = Vec::new();
        for _ in 0..edge * edge {
            record.extend_from_slice(&block.to_le_bytes());
            record.extend_from_slice(&5i16.to_le_bytes());
            record.push(80);
            record.push(90);
        }
        base64(&crate::columns::pack(&record))
    }

    /// Plain base64, the encoder side, for these tests only.
    fn base64(bytes: &[u8]) -> String {
        const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut out = String::new();
        for chunk in bytes.chunks(3) {
            let mut buf = [0u8; 3];
            buf[..chunk.len()].copy_from_slice(chunk);
            let n = u32::from_be_bytes([0, buf[0], buf[1], buf[2]]);
            for i in 0..4 {
                if i <= chunk.len() {
                    out.push(ALPHABET[((n >> (18 - 6 * i)) & 63) as usize] as char);
                } else {
                    out.push('=');
                }
            }
        }
        out
    }

    #[test]
    fn a_push_is_sorted_into_ground_that_moved_and_seasons_that_turned() {
        let body = format!(
            r#"{{"Edge":2,"Chunks":[
                {{"X":1,"Z":-2,"Season":4,"Record":"{}"}},
                {{"X":3,"Z":3,"Season":9}}
            ]}}"#,
            packed(2, 11)
        );
        let pushed = terrain_asked(&body).expect("the envelope this build reads");
        assert_eq!(pushed.edge, 2);
        assert_eq!(pushed.arrived.len(), 1);
        assert_eq!((pushed.arrived[0].cx, pushed.arrived[0].cz, pushed.arrived[0].season), (1, -2, 4));
        assert_eq!(pushed.arrived[0].record.len(), 2 * 2 * crate::columns::ENTRY_BYTES);
        assert_eq!(pushed.seasons, vec![(3, 3, 9)]);
    }

    #[test]
    fn a_record_of_the_wrong_size_refuses_the_whole_post() {
        let body = format!(r#"{{"Edge":4,"Chunks":[{{"X":0,"Z":0,"Record":"{}"}}]}}"#, packed(2, 11));
        assert!(terrain_asked(&body).is_none(), "a record for edge 2 is not one for edge 4");
        assert!(terrain_asked(r#"{"Edge":0,"Chunks":[]}"#).is_none());
        assert!(terrain_asked(r#"[[1,2]]"#).is_none(), "the coordinate list an older mod posted");
    }
}
