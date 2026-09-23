//! Answers the private channel the server mod posts on.
//!
//! This channel is separate from the map's own port. The map's port is meant to
//! be reachable, while this one accepts writes, and anything that could reach a
//! public write endpoint could put people on the map who are not there.
//!
//! [`crate::protocol::api`] decides where the listener binds. This module
//! decides what it answers.

use std::path::Path;
use std::sync::Arc;

use tiny_http::{Method, Request, Server};

use crate::protocol::api::Api;
use crate::protocol::auth::Who;
use crate::util::error::{Error, Result};
use crate::util::http::{self, Reply};
use crate::protocol::live::Took;
use crate::protocol::preferences::{Person, Preset};
use crate::state::State;
use crate::mapdata::store::Arrived;
use crate::util::urls;
use crate::web::events::Feed;
use crate::util::log::{say, warn};

/// Holds everything the mod may reach on this channel.
struct Channel {
    state: Arc<State>,
    api: Api,
}

/// Binds the API listener and answers on it, on a thread of its own.
pub fn serve(api: Api, state: Arc<State>, exports: &Path) -> Result<()> {
    // Clear the published file before binding rather than after a failure. A
    // file naming a listener that does not exist would send the mod's posts to
    // whatever holds that port now.
    Api::unpublish(exports);

    let listening = |what: &str| {
        Error::io(
            format!("listening for live data on {}", api.bind),
            std::io::Error::other(what.to_owned()),
        )
    };

    let server = Server::http(&api.bind).map_err(|error| listening(&error.to_string()))?;

    // Ask the listener for the port rather than reading the setting. The
    // setting usually asks for whatever port is free and does not say which one
    // that turned out to be.
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

/// Answers one post from the mod.
fn posted(request: &mut Request, channel: &Channel) -> Reply {
    if *request.method() != Method::Post {
        return http::text(405, "the API channel takes posts only");
    }

    // Loopback is not a trust boundary on a machine other people have accounts
    // on, so reaching the port does not prove the caller is the mod.
    if !channel.api.authorized(request) {
        return http::text(401, "the API channel needs the token from api.json");
    }

    let url = request.url().to_owned();
    let path = urls::path(&url).to_owned();

    let Some(body) = http::body(request) else {
        return http::text(400, "unreadable body");
    };

    match path.as_str() {
        // This is the only route on this channel that returns something rather
        // than only accepting it. Minting lives here because this is the only
        // listener the mod can reach, and only the mod knows which uid is which
        // player.
        "/auth/mint" => match asked_for(&body) {
            Some(who) => http::json(
                &serde_json::json!({ "Token": channel.state.sessions.mint(who) }).to_string(),
            ),
            None => http::text(400, "expected {\"Uid\":…, \"Name\":…}"),
        },

        // Hand over what people asked for on the web, which is markers and the
        // land claims somebody drew. The mod cannot be pushed to, so it collects
        // these. Reading empties the queue. See `Pending::take`.
        "/pending" => http::json(
            &serde_json::to_string(&channel.state.pending.take())
                .unwrap_or_else(|_| {
                    r#"{"Markers":{"Make":[],"Change":[],"Remove":[],"Pin":[]},"#.to_owned()
                        + r#""Claims":{"Make":[],"Change":[],"Remove":[]}}"#
                }),
        ),

        // Return what somebody has set for themselves, for the part of the mod
        // that makes a marker in game. The map's own form reads and writes the
        // whole document over the public port under a session cookie. A game
        // client has no session and no browser, so the mod asks on its behalf.
        // Only the mod knows which uid is which player, which is the same trust
        // minting a login token needs.
        "/presets/of" => match uid_asked(&body) {
            Some(uid) => http::json(&said(&channel.state.preferences.of(&uid))),
            None => http::text(400, "expected {\"Uid\":…}"),
        },

        // Merge one preset made in front of a player in game. Merging here
        // replaces a read, change and write back across two hops, which would
        // write the whole document as it looked when the window opened and lose
        // whatever else changed in between.
        "/presets/keep" => match preset_asked(&body) {
            Some((uid, preset)) => http::json(&said(&channel.state.preferences.keep_one(&uid, preset))),
            None => http::text(400, "expected {\"Uid\":…, \"Preset\":{\"Pattern\":…}}"),
        },

        "/live/players" => {
            let took = channel.state.live.set_players(body);
            if took.ok() {
                // Group membership arrives on the player post, and a map shared
                // with a group is shared against it.
                channel.state.memory.set_groups(channel.state.live.groups());
            }
            moved(channel, Feed::Players, took)
        }
        // Take the land claims, with the names of everybody the mod says may
        // see them. Seeing claims is a privilege, and only the mod knows who
        // holds one. See the `Live` this hands them to for why the names travel
        // beside the claims rather than as a per-person copy.
        "/live/claims" => moved(channel, Feed::Claims, channel.state.live.set_claims(body)),
        "/live/world" => moved(channel, Feed::World, channel.state.live.set_world(body)),
        "/live/markers" => moved(channel, Feed::Markers, channel.state.live.set_markers(body)),

        // Report what the map already holds, for a mod that just started and
        // has no memory of what it sent a previous service. It returns
        // coordinates, season and the checksum of each chunk's record, so a
        // chunk loading again is not read and sent for nothing.
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

        // Take the ground itself, which is chunks whose surface moved as the
        // mod read them, plus chunks whose season turned. These go into the
        // database, into the world, and are announced to every browser at once.
        // See `State::take_chunks`.
        "/terrain" => match terrain_asked(&body) {
            Some(pushed) => {
                let stored = channel.state.take_chunks(pushed.edge, &pushed.arrived, std::time::SystemTime::now());
                channel.state.terrain_changed(&stored);

                let turned: Vec<(i32, i32)> = pushed
                    .seasons
                    .iter()
                    .filter(|&&(cx, cz, season)| channel.state.take_season(cx, cz, season))
                    .map(|&(cx, cz, _)| crate::mapdata::store::region_of(cx, cz))
                    .collect();
                channel.state.tiles_changed(turned);
                http::text(204, "")
            }
            None => http::text(400, "expected {\"Edge\":…, \"Chunks\":[{\"X\":…, \"Z\":…, \"Season\":…, \"Record\":…}]}"),
        },

        // Register a plugin. Its database is created if there is none, and a
        // column it has added since is carried onto the existing table. Any
        // other change is refused with its rows left alone. See
        // `crate::mapdata::plugins`.
        other if other.starts_with("/plugins/register/") => {
            let Some(id) = other.strip_prefix("/plugins/register/") else {
                return http::text(404, "not found");
            };
            registered(channel, id, &body)
        }

        // Store rows from a plugin's own collector. The mod decides whose they
        // are, from the player it knows, and the plugin never does.
        other if other.starts_with("/plugins/data/") => {
            let Some(id) = other.strip_prefix("/plugins/data/") else {
                return http::text(404, "not found");
            };
            kept_rows(channel, id, &body)
        }

        _ => http::text(404, "not found"),
    }
}

/// Handles one plugin's registration, carried by the mod.
fn registered(channel: &Channel, id: &str, body: &str) -> Reply {
    let Ok(shape) = serde_json::from_str::<crate::mapdata::plugins::Shape>(body) else {
        return http::text(400, "expected a shape: {\"columns\":{…}, \"key\":[…], \"scope\":\"owner\"}");
    };

    // Read what it declared last time, so a plugin registering again unchanged
    // costs a lookup rather than a migration.
    let was = match channel.state.store.plugin(id) {
        Ok(found) => found.map(|(shape, _)| shape),
        Err(error) => return http::text(500, &format!("could not read the register: {error}")),
    };

    let root = crate::mapdata::plugins::plugins_dir(&channel.state.data);
    if let Err(error) = channel.state.plugins.register(&root, id, &shape, was.as_deref()) {
        // A plugin that will not register has its rows unserved, so the
        // operator must be able to see that in the log.
        warn!("plugin {id}: {error}");
        return http::text(400, &error.to_string());
    }

    // Store the declaration itself as well as its fingerprint. The fingerprint
    // says whether a shape moved. Only the declaration says what the shape is,
    // which is what lets the service open this plugin on its next start without
    // waiting to be told again.
    if let Err(error) =
        channel.state.store.keep_plugin(id, &shape.fingerprint(), body, std::time::SystemTime::now())
    {
        return http::text(500, &format!("could not keep the registration: {error}"));
    }

    // Read what people already said about sharing this plugin, once here rather
    // than per request. Every later change writes both the database and the
    // in-memory copy. See `State::keep_plugin_shares`.
    channel.state.recall_plugin_shares(id);

    say!("plugin {id}: registered");
    http::text(204, "")
}

/// Handles rows a plugin has collected.
fn kept_rows(channel: &Channel, id: &str, body: &str) -> Reply {
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "PascalCase")]
    struct Sent {
        #[serde(default)]
        owner: String,
        /// The owner's display name. The mod sends it because only the mod
        /// knows it, and it is stored so a reader can be told whose a shared row
        /// is without that person being online.
        #[serde(default)]
        owner_name: String,
        #[serde(default)]
        rows: Vec<serde_json::Value>,
    }

    let Ok(sent) = serde_json::from_str::<Sent>(body) else {
        return http::text(400, "expected {\"Owner\":…, \"Rows\":[…]}");
    };

    let Some(plugin) = channel.state.plugins.get(id) else {
        return http::text(404, "no plugin by that name has registered");
    };

    match plugin.write(&sent.owner, &sent.owner_name, &sent.rows) {
        Ok(_) => {
            // Tell every browser, as a marker's arrival does. A plugin's rows
            // are live data, and a page holding them should not wait on a poll
            // to learn they moved.
            channel.state.events.live_changed(Feed::Plugins);
            http::text(204, "")
        }
        Err(error) => http::text(500, &error.to_string()),
    }
}

/// Accepts a live post and tells every open browser, or refuses it.
///
/// Wakes browsers only where the post changed what is held. The mod posts the
/// clock every second and the markers only when they differ, so waking on every
/// well-formed post sent every marker to every browser once a second.
fn moved(channel: &Channel, feed: Feed, took: Took) -> Reply {
    if took.changed() {
        channel.state.events.live_changed(feed);
    }
    taken(took.ok())
}

/// Accepts a post, or refuses it because it is not the shape this build reads.
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

/// Holds what one person has set, as the mod reads it.
fn said(person: &Person) -> String {
    serde_json::to_string(person).unwrap_or_else(|_| "{}".to_owned())
}

/// Holds what the mod pushed: the chunk edge and one entry per chunk. An entry
/// with a record is ground that moved. One without is a season that turned.
struct Pushed {
    edge: usize,
    arrived: Vec<Arrived>,
    seasons: Vec<(i32, i32, u8)>,
}

/// Splits the mod's terrain envelope into ground that moved and seasons that
/// turned.
///
/// A record is base64 of a deflated record, the same bytes a region file held
/// for a chunk. A record that does not inflate to `edge * edge` entries fails
/// the whole post, because a chunk the mod mis-sent is not worth half-storing.
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
    let wanted = envelope.edge * envelope.edge * crate::render::columns::ENTRY_BYTES;

    let mut arrived = Vec::new();
    let mut seasons = Vec::new();
    for entry in envelope.chunks {
        match entry.record {
            Some(record) => {
                let record = crate::render::columns::unpack(&crate::util::wire::decode(&record).ok()?)?;
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

/// Names whose settings the mod is asking about.
fn uid_asked(body: &str) -> Option<String> {
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "PascalCase")]
    struct Asked {
        uid: String,
    }

    let asked: Asked = serde_json::from_str(body).ok()?;
    (!asked.uid.is_empty()).then_some(asked.uid)
}

/// Holds whose preset this is and what it says. A preset whose pattern is empty
/// names no block and is kept, because it is picked from the list by hand.
fn preset_asked(body: &str) -> Option<(String, Preset)> {
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "PascalCase")]
    struct Asked {
        uid: String,
        preset: Preset,
    }

    let asked: Asked = serde_json::from_str(body).ok()?;
    (!asked.uid.is_empty()).then_some((asked.uid, asked.preset))
}

/// Names who the mod is asking a login token for.
///
/// The uid is the whole identity. The name only decides what the page displays.
/// Both come from the game and neither is checked here, because the mod is the
/// only thing that can reach this channel and the only thing that knows.
fn asked_for(body: &str) -> Option<Who> {
    // Use PascalCase, because a C# serializer writes everything the mod posts.
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

    /// Builds one record the way the mod packs one, as base64 of the deflate
    /// stream.
    fn packed(edge: usize, block: u16) -> String {
        let mut record = Vec::new();
        for _ in 0..edge * edge {
            record.extend_from_slice(&block.to_le_bytes());
            record.extend_from_slice(&5i16.to_le_bytes());
            record.push(80);
            record.push(90);
        }
        base64(&crate::render::columns::pack(&record))
    }

    /// Encodes plain base64, for these tests only.
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
        assert_eq!(pushed.arrived[0].record.len(), 2 * 2 * crate::render::columns::ENTRY_BYTES);
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
