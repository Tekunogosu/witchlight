//! What the page is told.
//!
//! Every JSON body the viewer asks for, gathered in one place because they share
//! one rule: the answer is worked out here rather than sent and filtered on the
//! page. A browser cannot be asked to hide what it has already been handed.

use std::collections::HashMap;

use crate::facts;
use crate::palette::Palette;
use crate::pyramid::TILE;
use crate::render::{Renderer, Surface};
use crate::state::State;
use crate::urls::is_stored_name;

/// How many blocks a search answers with. A list somebody reads down, not a
/// result set: past a screenful the answer is to type more, not to scroll.
const MOST_BLOCKS_FOUND: usize = 24;

impl State {
    /// The state of the map, and — when the caller says which generation it last
    /// drew — which tiles it needs to fetch again.
    pub fn info(&self, since: Option<u64>) -> String {
        let (min_x, min_z, max_x, max_z) = self.bounds();
        let facts = facts::read(&self.data);
        let mut body = serde_json::json!({
            "minX": min_x, "minZ": min_z, "maxX": max_x, "maxZ": max_z,
            "tile": TILE,
            "spawnX": facts.spawn_x, "spawnZ": facts.spawn_z,
            "chunk": self.chunk_edge(),
            "levels": self.levels(),
            "chunks": self.chunks(),
            "generation": self.generation(),
        });

        // Without a `since` there is nothing to be behind on, so nothing is said
        // about tiles and a first-time viewer draws whatever it needs.
        if let Some(since) = since {
            match self.changes_since(since) {
                Some(tiles) => body["tiles"] = serde_json::json!(tiles),
                None => body["all"] = serde_json::json!(true),
            }
        }

        body.to_string()
    }

    /// What the page needs to know about whoever is looking at it.
    ///
    /// Always answers, and answers the same shape logged in or not: a page that
    /// has to tell an error from a stranger has two ways to draw one state.
    ///
    /// `Waiting` is how many markers the game server has not collected. A form
    /// whose marker has not appeared cannot otherwise tell a game server that has
    /// stopped from one that is merely slow, and those are not the same problem.
    pub fn me(&self, cookies: &str) -> String {
        let who = self.sessions.who(cookies);
        serde_json::json!({
            "Name": who.as_ref().map(|who| who.name.clone()),
            "Uid": who.as_ref().map(|who| who.uid.clone()),
            "MarkersPublic": self.rules.markers_public,
            "PublicMarkersEditable": self.rules.markers_editable,
            "PlayersPublic": self.rules.players_public,
            "Waiting": self.pending.waiting(),
        })
        .to_string()
    }

    /// Which marker icons exist, so the viewer draws a marker it can and a plain
    /// shape for one it cannot rather than a hole where a picture should be.
    pub fn icons(&self) -> String {
        let names: Vec<String> = crate::files::listing(&crate::stored::icons_dir(&self.data))
            .unwrap_or_default()
            .iter()
            .filter_map(|path| {
                (path.extension()? == "svg").then_some(path.file_stem()?.to_str()?.to_owned())
            })
            .filter(|name| is_stored_name(name))
            .collect();

        serde_json::to_string(&names).unwrap_or_else(|_| "[]".to_owned())
    }

    /// Blocks whose code or name reads like what somebody is typing.
    ///
    /// A preset is keyed on a block code, and nobody knows what
    /// `game:smallplants-fern-normal` is called from memory. The whole table is
    /// eleven thousand entries and several hundred kilobytes, which is not a
    /// thing to hand a map page on the chance it opens a form — so the page asks
    /// as it types and this answers with a screenful.
    ///
    /// Matched against both the code and the name, because somebody typing
    /// "fern" and somebody typing "smallplants" are both looking for the same
    /// block and neither is wrong.
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

        // What somebody typed, first. A search for "fern" that opens on
        // `bamboo-fern-shoot` because it sorts earlier is a search that has to be
        // read through rather than glanced at.
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

    /// What is at one block, for the viewer's inspector.
    ///
    /// The same reading the renderer made for that pixel, so the map never names
    /// a block it did not draw. `None` while the map is between hands.
    pub fn block(&self, x: i32, z: i32) -> Option<String> {
        let (Ok(world), Ok(palette)) = (self.world.read(), self.palette.read()) else {
            return None;
        };
        let Ok(names) = self.names.read() else {
            return None;
        };

        let surface = Renderer::new(&world, &palette, self.sea_level()).surface_at(x, z);
        // Every field is a number, a fixed word, or a block code out of the
        // palette, so there is nothing here that can refuse to be JSON.
        serde_json::to_string(&Block::read(x, z, surface, &palette, &names)).ok()
    }
}

/// What the map knows about one block, as the viewer's inspector asks for it.
///
/// A struct rather than a hand-built string like the other feeds: a block code
/// comes out of a file this program did not write, and the one place a quote in
/// it could break the page is not worth a second escaper to guard.
#[derive(serde::Serialize)]
struct Block {
    x: i32,
    z: i32,
    /// How the column read against the palette: `painted`, `blank`, `uncoloured`,
    /// `unknown` or `unmapped`. The viewer speaks for the first four and stays
    /// quiet for the last, since there is nothing drawn there to be looking at.
    state: &'static str,
    /// The block id this world gave it. Absent where nothing was exported.
    #[serde(skip_serializing_if = "Option::is_none")]
    block: Option<u16>,
    /// Its code — `game:rock-granite`. Absent for a block the palette has never
    /// heard of, which is the whole of what `unknown` means.
    #[serde(skip_serializing_if = "Option::is_none")]
    code: Option<String>,
    /// What the game calls it — `Granite rock`. Absent where the mod has exported
    /// no names, or where the language files have none for this block; whatever
    /// reads this then has the code, which is what it had before there were names.
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    /// The surface height, which is the Y a player standing here would read.
    #[serde(skip_serializing_if = "Option::is_none")]
    y: Option<i16>,
    /// Degrees celsius, and the climate the world was generated with rather than
    /// today's weather.
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    /// Rainfall, from dry at zero to the wettest the game has at one.
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
