//! Decides what a block looks like on the map.
//!
//! The server mod exports a palette keyed by block code. Each entry carries the
//! block id for this world, an average colour, and the names of the colour maps
//! the game would tint it with. Water, grass and leaves ship as greyscale masks,
//! so without the tint they render as fog.

use std::collections::HashMap;
use std::path::Path;

use serde::Deserialize;

use crate::render::color::Rgb;
use crate::util::error::{Error, Result};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct RawEntry {
    id: u32,
    /// Absent for a block this palette cannot draw.
    rgb: Option<String>,
    /// Says which kind of colourless an entry with no `rgb` is. True when the
    /// block draws nothing at all, such as air or an invisible helper. False when
    /// it draws something the mod could not work out a colour for.
    ///
    /// An `Option` rather than a `bool` because a palette written before the mod
    /// recorded this has no value here. A palette that says nothing must be
    /// distinguishable from one that says the block draws, or every old palette
    /// reports its air as terrain waiting for a colour.
    #[serde(default)]
    invisible: Option<bool>,
    #[serde(default)]
    climate_map: Option<String>,
    #[serde(default)]
    season_map: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct RawPalette {
    version: u32,
    game_version: String,
    /// Which machine's assets these colours came from, `server` or `client`.
    #[serde(default)]
    source: String,
    /// The block registry these colours are keyed on, as the mod hashed it.
    /// Levels built from one palette are distinguished from levels built from
    /// another by this value.
    #[serde(default)]
    fingerprint: String,
    blocks: HashMap<String, RawEntry>,
}

/// How one block renders.
#[derive(Debug, Clone, Copy, Default)]
pub struct Appearance {
    pub base: Rgb,
    /// Index into [`Palette::color_maps`], when the block is climate tinted.
    pub climate_map: Option<u16>,
    /// Index into [`Palette::color_maps`], when the block turns with the year.
    pub season_map: Option<u16>,
    /// The palette has an entry for this block.
    pub known: bool,
    /// The palette has an entry and the block draws nothing, such as air. Bare
    /// ground on the map is the correct rendering.
    pub invisible: bool,
    /// The palette has an entry, the block draws something, and this palette has
    /// no colour for it.
    ///
    /// This is distinct from both fields above. Treating it as invisible paints
    /// the block the same near-black as unexplored ground, so ground a player has
    /// just dug reads as a hole in the world. The mod repairs these by asking a
    /// client. Until it does, they are painted as ground rather than as absence.
    pub uncoloured: bool,
}

impl Appearance {
    /// Returns true when this block would be drawn the same way as `other`.
    #[must_use]
    fn same_as(&self, other: &Self) -> bool {
        self.known == other.known
            && self.invisible == other.invisible
            && self.uncoloured == other.uncoloured
            && self.base == other.base
            && self.climate_map == other.climate_map
            && self.season_map == other.season_map
    }
}

pub struct Palette {
    pub game_version: String,
    pub source: String,
    /// Indexed by block id, which is what the exported columns carry.
    by_id: Vec<Appearance>,
    /// Every block's code in this world, such as `game:rock-granite`, indexed
    /// the same way `by_id` is.
    ///
    /// The map's inspector reports the block under the cursor, which a colour
    /// cannot say. These are held beside the appearances rather than inside them,
    /// so the type the renderer copies for every pixel stays small and `Copy`.
    codes: Vec<Option<Box<str>>>,
    pub color_maps: Vec<ColorMap>,
    pub named: usize,
    /// The block registry these colours were built against.
    pub fingerprint: String,
    /// How many blocks have a colour to draw.
    ///
    /// Stored rather than counted on demand because it is read before every level
    /// 0 tile. A palette that colours nothing can only render bare ground, so the
    /// renderer checks this before drawing.
    pub coloured: usize,
    /// How many blocks draw something this palette has no colour for.
    ///
    /// Nothing here can repair one, because the colours come off a client's
    /// assets that this program cannot reach. The count is logged on load and the
    /// mod asks a player for the missing colours.
    pub uncoloured: usize,
}

impl Palette {
    /// Returns true when this palette has no colours at all.
    ///
    /// A palette missing most of its colours still draws a usable map. This is
    /// the case where every block comes out as bare ground, which only happens
    /// when the palette was built without readable textures.
    #[must_use]
    pub fn paints_nothing(&self) -> bool {
        self.coloured == 0
    }

    /// Returns true when another palette would draw the same map as this one.
    ///
    /// This compares the appearances rather than the file. The file carries
    /// values that do not decide a colour, such as who built it, so a palette
    /// rewritten by a different admin from the same assets compares equal.
    #[must_use]
    pub fn same_as(&self, other: &Self) -> bool {
        self.fingerprint == other.fingerprint
            && self.coloured == other.coloured
            && self.by_id.len() == other.by_id.len()
            && self
                .by_id
                .iter()
                .zip(&other.by_id)
                .all(|(a, b)| a.same_as(b))
            && self.color_maps.len() == other.color_maps.len()
            && self
                .color_maps
                .iter()
                .zip(&other.color_maps)
                .all(|(a, b)| a.name == b.name && a.pixels == b.pixels)
    }
}

/// A climate lookup image. Temperature runs along the horizontal axis and
/// rainfall down the vertical one, as the game's own shader samples it.
pub struct ColorMap {
    pub name: String,
    width: u32,
    height: u32,
    /// How many pixels of border the usable map sits inside.
    ///
    /// The climate maps are a 256 square drawn inside a 264 one. The border
    /// exists for the game's texture atlas and is not part of the lookup. The
    /// value comes from the index the mod writes beside the pictures, because the
    /// pixels do not say where the border ends.
    padding: u32,
    pixels: Vec<Rgb>,
}

impl ColorMap {
    /// Samples the map for a column's climate.
    ///
    /// Both inputs are the bytes the game packs into its colour map data, so this
    /// performs the same lookup the game's shader does.
    #[must_use]
    pub fn sample(&self, across: u8, down: u8) -> Rgb {
        let inset = |value: u8, size: u32| {
            let usable = size.saturating_sub(self.padding * 2).max(1);
            self.padding + (u32::from(value) * usable / 256).min(usable - 1)
        };
        let x = inset(across, self.width).min(self.width - 1);
        let y = inset(down, self.height).min(self.height - 1);
        self.pixels[(y * self.width + x) as usize]
    }
}

/// Returns the path of the palette inside the export directory.
#[must_use]
pub fn path_in(exports: &Path) -> std::path::PathBuf {
    exports.join("palette.json")
}

impl Palette {
    /// Loads `palette.json` and the colour maps beside it.
    pub fn load(dir: &Path) -> Result<Self> {
        let path = path_in(dir);
        let text = std::fs::read_to_string(&path)
            .map_err(|source| Error::io(format!("reading {}", path.display()), source))?;
        let raw: RawPalette = serde_json::from_str(&text)
            .map_err(|source| Error::parse(&path, source.to_string()))?;

        if raw.version != 1 {
            return Err(Error::parse(
                &path,
                format!("palette version {} is not supported", raw.version),
            ));
        }

        let color_maps = load_color_maps(&dir.join("colormaps"))?;
        let index_of = |name: &Option<String>| -> Option<u16> {
            let name = name.as_ref()?;
            color_maps
                .iter()
                .position(|map| &map.name == name)
                .and_then(|at| u16::try_from(at).ok())
        };

        let highest = raw.blocks.values().map(|entry| entry.id).max().unwrap_or(0);
        let mut by_id = vec![Appearance::default(); highest as usize + 1];
        let mut codes = vec![None; highest as usize + 1];
        // Every entry is marked known, colour or not, so the renderer can tell
        // an invisible block from one this palette has never heard of.
        for (code, entry) in &raw.blocks {
            // An entry with no colour is either invisible or waiting for a
            // colour, and only the mod says which. Where the field is absent,
            // every such entry reads as invisible.
            let colourless = entry.rgb.is_none();
            let uncoloured = colourless && entry.invisible == Some(false);

            codes[entry.id as usize] = Some(Box::from(code.as_str()));
            by_id[entry.id as usize] = Appearance {
                base: entry.rgb.as_deref().and_then(Rgb::parse).unwrap_or_default(),
                invisible: colourless && !uncoloured,
                uncoloured,
                climate_map: index_of(&entry.climate_map),
                season_map: index_of(&entry.season_map),
                known: true,
            };
        }

        let coloured = by_id.iter().filter(|a| a.known && !a.invisible && !a.uncoloured).count();
        let uncoloured = by_id.iter().filter(|a| a.uncoloured).count();

        Ok(Self {
            game_version: raw.game_version,
            source: if raw.source.is_empty() { "unknown".to_owned() } else { raw.source },
            named: raw.blocks.len(),
            fingerprint: raw.fingerprint,
            coloured,
            uncoloured,
            by_id,
            codes,
            color_maps,
        })
    }

    /// Returns true when the palette has an entry for this block.
    #[must_use]
    pub fn knows(&self, block: u16) -> bool {
        self.by_id.get(block as usize).is_some_and(|a| a.known)
    }

    /// Returns true when the palette knows this block draws something and has no
    /// colour for it. The mod repairs this state by asking a client. The map must
    /// not paint these blocks as absent.
    #[must_use]
    pub fn uncoloured(&self, block: u16) -> bool {
        self.by_id.get(block as usize).is_some_and(|a| a.uncoloured)
    }

    /// Returns what this block is called in this world, such as
    /// `game:rock-granite`. Returns `None` for a block [`Palette::knows`] returns
    /// false for.
    #[must_use]
    pub fn code_of(&self, block: u16) -> Option<&str> {
        self.codes.get(block as usize)?.as_deref()
    }

    /// Returns the colour of one column, tinted for its position and the season.
    ///
    /// Grass, leaves and water are greyscale masks in the game's assets. One tint
    /// is built and the block's own colour is multiplied by it once. The climate
    /// map produces the tint, from temperature across and rainfall down. The
    /// season map replaces part of that tint in proportion to how strongly the
    /// season is felt at this temperature and height. The two tints must not be
    /// multiplied together, which compounds them and makes warm ground redder and
    /// browner than the game draws it.
    ///
    /// This follows `colormap.fsh` and `colormap.vsh` in the game's own shaders.
    #[must_use]
    pub fn color_of(
        &self,
        block: u16,
        column: &crate::render::columns::Column,
        variation: u8,
        sea_level: i32,
    ) -> Option<Rgb> {
        let appearance = self.by_id.get(block as usize)?;
        if !appearance.known || appearance.invisible || appearance.uncoloured {
            return None;
        }

        let mut tint = Rgb::new(255, 255, 255);
        if let Some(map) = appearance.climate_map.and_then(|at| self.color_maps.get(at as usize)) {
            tint = map.sample(column.temperature, column.rainfall);
        }

        if let Some(map) = appearance.season_map.and_then(|at| self.color_maps.get(at as usize)) {
            let weight = season_weight(column, sea_level);
            if weight > 0.0 {
                // The game varies the second axis with per-position noise so a
                // forest is not one flat colour. A hash of the position stands in
                // for that noise and keeps the map stable between renders.
                tint = tint.mix(map.sample(column.season, variation), weight);
            }
        }

        Some(appearance.base.multiply(tint))
    }
}

/// Returns how much of the season's colour is felt at a column, from 0 to 1.
///
/// The curve is steep. Foliage in the tropics never turns, temperate ground turns
/// almost completely, and cold ground turns only a little because it is drab
/// already. Height counts as cold, so a mountainside keeps its needles while the
/// valley below turns. The sea level argument sets that reference height.
///
/// This is copied from `calcColorMapUvs` in the game's `colormap.vsh`, constants
/// included. A rewritten curve no longer matches the game.
fn season_weight(column: &crate::render::columns::Column, sea_level: i32) -> f32 {
    let above_sea = (f32::from(column.height) - sea_level as f32).max(0.0);
    let x = f32::from(column.temperature) + above_sea * 1.5;
    let weight = 0.5 - (x / 42.0).cos() / 2.3 + (128.0 - x).max(0.0) / 256.0 / 2.0
        - (x - 130.0).max(0.0) / 200.0;
    weight.clamp(0.0, 1.0)
}

fn load_color_maps(dir: &Path) -> Result<Vec<ColorMap>> {
    // A world with no tinted blocks has no such directory. That is unusual and
    // not an error.
    let entries = crate::util::files::listing(dir)
        .map_err(|error| Error::io(format!("reading {}", dir.display()), error))?;

    // What the mod recorded about each picture's border. Absent when the mod is
    // older than this build, in which case the border is treated as zero. That is
    // wrong at the edges but still draws.
    let padding: std::collections::HashMap<String, u32> =
        std::fs::read_to_string(dir.join("padding.json"))
            .ok()
            .and_then(|body| serde_json::from_str(&body).ok())
            .unwrap_or_default();

    let mut maps = Vec::new();
    for path in entries {
        if path.extension().is_none_or(|ext| ext != "png") {
            continue;
        }
        let Some(name) = path.file_stem().map(|stem| stem.to_string_lossy().into_owned()) else {
            continue;
        };

        let image = image::open(&path)
            .map_err(|error| Error::parse(&path, error.to_string()))?
            .to_rgb8();
        let border = padding.get(&name).copied().unwrap_or(0);
        maps.push(ColorMap {
            // A border that leaves nothing to sample is ignored.
            padding: if border * 2 < image.width().min(image.height()) { border } else { 0 },
            name,
            width: image.width(),
            height: image.height(),
            pixels: image
                .pixels()
                .map(|pixel| Rgb::new(pixel[0], pixel[1], pixel[2]))
                .collect(),
        });
    }

    maps.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(maps)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::files::testing::Scratch;

    /// Writes a palette of these entries and loads it back.
    ///
    /// This goes through the file rather than building the struct directly,
    /// because the field under test is one the mod writes and a hand-built
    /// `Palette` would agree with itself whatever the parser did.
    fn loaded(entries: &str, at: &Path) -> Palette {
        let body = format!(
            r#"{{"Version":1,"GameVersion":"1.22.7","Source":"client",
                 "Fingerprint":"abc","Blocks":{{{entries}}}}}"#
        );
        std::fs::write(path_in(at), body).expect("a palette to read back");
        Palette::load(at).expect("it parses")
    }

    /// Air, bare soil the builder could not colour, and rock that it could.
    const AIR: &str = r#""game:air":{"Id":0,"Rgb":null,"Invisible":true}"#;
    const SOIL: &str = r#""game:soil-medium-none":{"Id":1,"Rgb":null,"Invisible":false}"#;
    const ROCK: &str = r##""game:rock-granite":{"Id":2,"Rgb":"#806040"}"##;

    #[test]
    fn a_block_that_draws_and_has_no_colour_is_not_the_same_as_air() {
        // The repair depends on this distinction. Both are entries with no
        // colour, and treating them alike paints dug ground as unexplored.
        let held = Scratch::new("palette-uncoloured");
        let palette = loaded(&format!("{AIR},{SOIL},{ROCK}"), held.at());

        assert!(palette.knows(0) && palette.knows(1) && palette.knows(2));
        assert!(!palette.uncoloured(0), "air draws nothing and is not waiting on a colour");
        assert!(palette.uncoloured(1), "soil draws, and this palette has no colour for it");
        assert!(!palette.uncoloured(2), "rock has one");

        assert_eq!(palette.coloured, 1, "only rock can be painted");
        assert_eq!(palette.uncoloured, 1, "and only soil is waiting");
        assert!(!palette.paints_nothing());
    }

    #[test]
    fn a_palette_older_than_the_field_reads_as_it_always_did() {
        // An older palette can only say that an entry has no colour. Reading
        // those as terrain waiting on a colour would repaint a working map as
        // soon as this build met one.
        let held = Scratch::new("palette-older");
        let old = r#""game:air":{"Id":0,"Rgb":null},"game:soil-medium-none":{"Id":1,"Rgb":null}"#;
        let palette = loaded(&format!("{old},{ROCK}"), held.at());

        assert!(palette.knows(0) && palette.knows(1));
        assert!(!palette.uncoloured(0) && !palette.uncoloured(1));
        assert_eq!(palette.uncoloured, 0);
    }

    #[test]
    fn a_palette_that_only_learned_a_colour_is_a_different_palette() {
        // This comparison decides whether the map is redrawn. A colour arriving
        // for a block that had none is the point of the request, and a comparison
        // that missed it would leave the holes on screen.
        let (before, after) = (Scratch::new("palette-same-a"), Scratch::new("palette-same-b"));
        let held = loaded(&format!("{AIR},{SOIL},{ROCK}"), before.at());
        let filled = r##""game:soil-medium-none":{"Id":1,"Rgb":"#6b6257"}"##;

        assert!(held.same_as(&loaded(&format!("{AIR},{SOIL},{ROCK}"), after.at())));
        assert!(!held.same_as(&loaded(&format!("{AIR},{filled},{ROCK}"), after.at())));
    }
}
