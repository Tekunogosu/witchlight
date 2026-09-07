//! Builds every JSON body the viewer asks for.
//!
//! They are gathered here because they share one rule: the answer is worked out
//! on this side rather than sent and filtered on the page. A browser cannot be
//! asked to hide what it has already been handed.

use std::collections::HashMap;

use crate::mapdata::facts;
use crate::render::palette::Palette;
use crate::render::pyramid::TILE;
use crate::render::tiles::Surface;
use crate::mapdata::scope::Scope;
use crate::state::State;
use crate::util::urls::is_stored_name;

/// How many blocks a search returns. Past a screenful the answer is to type
/// more rather than to scroll.
const MOST_BLOCKS_FOUND: usize = 24;

impl State {
    /// Returns the state of the map, plus which tiles need refetching when the
    /// caller says which generation it last drew.
    pub fn info(&self, scope: &Scope, since: Option<u64>) -> String {
        let (min_x, min_z, max_x, max_z) = self.bounds_for(scope);
        let facts = facts::read(&self.data);
        let mut body = serde_json::json!({
            "minX": min_x, "minZ": min_z, "maxX": max_x, "maxZ": max_z,
            "tile": TILE,
            "spawnX": facts.spawn_x, "spawnZ": facts.spawn_z,
            "chunk": self.chunk_edge(),
            "levels": self.levels(),
            "chunks": self.chunks_for(scope),
            "generation": self.generation(),
        });

        // Without a `since` there is nothing to be behind on, so say nothing
        // about tiles and let a first-time viewer draw whatever it needs.
        if let Some(since) = since {
            match self.changes_for(scope, since) {
                Some(tiles) => body["tiles"] = serde_json::json!(tiles),
                None => body["all"] = serde_json::json!(true),
            }
        }

        body.to_string()
    }

    /// Returns what the page needs to know about whoever is looking at it.
    ///
    /// It always answers, and in the same shape whether the viewer is logged in
    /// or not. A page that had to tell an error from a stranger would have two
    /// ways to draw one state.
    ///
    /// `Waiting` is how many markers the game server has not collected. Without
    /// it, a form whose marker has not appeared cannot tell a stopped game server
    /// from a slow one, and those are different problems.
    pub fn me(&self, cookies: &str) -> String {
        let who = self.sessions.who(cookies);
        serde_json::json!({
            "Name": who.as_ref().map(|who| who.name.clone()),
            "Uid": who.as_ref().map(|who| who.uid.clone()),
            "MarkersPublic": self.rules.allow_public_markers,
            "PublicMarkersEditable": self.rules.allow_editing_public_markers,
            "PlayersPublic": self.rules.show_players_to_everyone,
            "PrivateMap": self.rules.personal_maps,
            "AnonymousSpawn": self.rules.show_spawn_to_guests,
            // The groups this person is in, for the settings form to offer
            // sharing with. A stranger is in none.
            "Groups": who
                .as_ref()
                .map(|who| self.memory.groups_of(&who.uid))
                .unwrap_or_default()
                .into_iter()
                .map(|(id, name)| serde_json::json!({ "Id": id, "Name": name }))
                .collect::<Vec<_>>(),
            "Waiting": self.pending.waiting(),
        })
        .to_string()
    }

    /// Returns which marker icons exist, so the viewer draws the picture where
    /// it has one and a plain shape where it does not, rather than a hole.
    pub fn icons(&self) -> String {
        let names: Vec<String> = crate::util::files::listing(&crate::mapdata::stored::icons_dir(&self.data))
            .unwrap_or_default()
            .iter()
            .filter_map(|path| {
                (path.extension()? == "svg").then_some(path.file_stem()?.to_str()?.to_owned())
            })
            .filter(|name| is_stored_name(name))
            .collect();

        serde_json::to_string(&names).unwrap_or_else(|_| "[]".to_owned())
    }

    /// Returns blocks whose code or name matches what somebody is typing.
    ///
    /// A preset is keyed on a block code, and nobody recalls what
    /// `game:smallplants-fern-normal` is called. The whole table is eleven
    /// thousand entries and several hundred kilobytes, which is too much to hand
    /// a map page on the chance it opens a form, so the page asks as it types and
    /// this answers with a screenful.
    ///
    /// The match runs against both the code and the name, because somebody typing
    /// "fern" and somebody typing "smallplants" are looking for the same block.
    pub fn blocks_like(&self, asked: &str) -> String {
        let asked = asked.trim().to_ascii_lowercase();
        if asked.is_empty() {
            return "[]".to_owned();
        }

        let Ok(names) = self.names.read() else {
            return "[]".to_owned();
        };

        let mut found: Vec<(&str, &str)> = names
            .iter()
            .map(|(code, name)| (code.as_str(), name.as_str()))
            .filter(|(code, name)| {
                code.to_ascii_lowercase().contains(&asked)
                    || name.to_ascii_lowercase().contains(&asked)
            })
            .collect();

        // Rank what somebody typed first. A search for "fern" that leads with
        // `bamboo-fern-shoot` because it sorts earlier has to be read through
        // rather than glanced at.
        found.sort_by_key(|(code, name)| {
            let short = code.split_once(':').map_or(*code, |(_, rest)| rest);
            (
                !short.to_ascii_lowercase().starts_with(&asked),
                !name.to_ascii_lowercase().starts_with(&asked),
                name.len(),
                *code,
            )
        });
        found.truncate(MOST_BLOCKS_FOUND);

        let listed: Vec<_> = found
            .into_iter()
            .map(|(code, name)| serde_json::json!({ "Code": code, "Name": name }))
            .collect();
        serde_json::to_string(&listed).unwrap_or_else(|_| "[]".to_owned())
    }

    /// Returns what is at one block, for the viewer's inspector.
    ///
    /// It uses the same reading the renderer made for that pixel, so the map
    /// never names a block it did not draw. It returns `None` while the map is
    /// being reloaded.
    pub fn block(&self, scope: &Scope, x: i32, z: i32) -> Option<String> {
        let surface = self.surface_for(scope, x, z)?;
        let Ok(palette) = self.palette.read() else {
            return None;
        };
        let Ok(names) = self.names.read() else {
            return None;
        };

        // Every field is a number, a fixed word, or a block code from the
        // palette, so nothing here can fail to serialise as JSON.
        serde_json::to_string(&Block::read(x, z, surface, &palette, &names)).ok()
    }
}

/// Holds what the map knows about one block, for the viewer's inspector.
///
/// This is a struct rather than a hand-built string like the other feeds. A block
/// code comes from a file this program did not write, and serde escapes it
/// without a second escaper here.
#[derive(serde::Serialize)]
struct Block {
    x: i32,
    z: i32,
    /// The column's reading against the palette, as `painted`, `blank`,
    /// `uncoloured`, `unknown` or `unmapped`. The viewer reports the first four
    /// and stays quiet for the last, because nothing is drawn there.
    state: &'static str,
    /// The block id this world gave it. It is absent where nothing was
    /// exported.
    #[serde(skip_serializing_if = "Option::is_none")]
    block: Option<u16>,
    /// The block's code, such as `game:rock-granite`. It is absent for a block
    /// the palette does not know, which is what `unknown` means.
    #[serde(skip_serializing_if = "Option::is_none")]
    code: Option<String>,
    /// What the game calls the block, such as `Granite rock`. It is absent when
    /// the mod exported no names, or when the language files have none for this
    /// block. A reader then falls back to the code.
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    /// The surface height, which is the Y a player standing here reads.
    #[serde(skip_serializing_if = "Option::is_none")]
    y: Option<i16>,
    /// The temperature in degrees celsius. It is the climate the world was
    /// generated with rather than today's weather.
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    /// The rainfall, from dry at zero to the wettest the game has at one.
    #[serde(skip_serializing_if = "Option::is_none")]
    rainfall: Option<f32>,
}

impl Block {
    fn read(
        x: i32,
        z: i32,
        surface: Surface,
        palette: &Palette,
        names: &HashMap<String, String>,
    ) -> Self {
        let column = surface.column();
        let code = column.and_then(|column| palette.code_of(column.block).map(ToOwned::to_owned));
        Self {
            x,
            z,
            state: surface.state(),
            block: column.map(|column| column.block),
            name: code.as_deref().and_then(|code| names.get(code).cloned()),
            code,
            y: column.map(|column| column.height),
            temperature: column.map(|column| column.celsius()),
            rainfall: column.map(|column| column.wetness()),
        }
    }
}
