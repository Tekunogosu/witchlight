//! What a block looks like on the map.
//!
//! The server mod exports a palette keyed by block code, each entry carrying the
//! block id for this world, an average colour, and the names of the colour maps
//! the game would tint it with. Water, grass and leaves ship as greyscale masks,
//! so without the tint they render as fog.

use std::collections::HashMap;
use std::path::Path;

use serde::Deserialize;

use crate::color::Rgb;
use crate::error::{Error, Result};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct RawEntry {
    id: u32,
    /// Absent for a block with nothing to draw: air, a helper, a shape-only block.
    rgb: Option<String>,
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
    /// `server` or `client`: which machine's assets these colours came from.
    #[serde(default)]
    source: String,
    /// The block registry these colours are keyed on, as the mod hashed it.
    /// Carried so that levels built from one palette can be told from levels
    /// built from another.
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
    /// It has an entry but nothing to draw — air and other invisible blocks.
    pub invisible: bool,
}

impl Appearance {
    /// Whether this block would be drawn the same way.
    #[must_use]
    fn same_as(&self, other: &Self) -> bool {
        self.known == other.known
            && self.invisible == other.invisible
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
    /// Every block's code in this world — `game:rock-granite` — indexed the same
    /// way `by_id` is.
    ///
    /// A colour cannot say what it is a colour of, and the map's inspector is
    /// asked exactly that. Held beside the appearances rather than inside them,
    /// so the type the renderer copies for every pixel stays small and `Copy`.
    codes: Vec<Option<Box<str>>>,
    pub color_maps: Vec<ColorMap>,
    pub named: usize,
    /// The block registry these colours were built against.
    pub fingerprint: String,
    /// How many blocks have a colour to draw.
    ///
    /// Held rather than counted on demand because it is asked before every level
    /// 0 tile: a palette that colours nothing can only ever render bare ground,
    /// and rendering it anyway is how a broken palette came to look like a broken
    /// map.
    pub coloured: usize,
}

impl Palette {
    /// Whether this palette can draw anything at all.
    ///
    /// Not a judgement about quality — a palette missing most of its colours
    /// still draws a map worth looking at. This is the case where every block
    /// there is would come out as bare ground, which no world produces and only
    /// a palette built without readable textures does.
    #[must_use]
    pub fn paints_nothing(&self) -> bool {
        self.coloured == 0
    }

    /// Whether another palette would draw the same map as this one.
    ///
    /// Compares the appearances rather than the file, because the file carries
    /// things that do not decide a colour — who built it, which mod set they had
    /// — and a palette rewritten by a different admin with the same assets is the
    /// same palette as far as anything drawn from it is concerned.
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

/// A climate lookup image: horizontal axis temperature, vertical axis rainfall,
/// exactly as the game's own shader samples it.
pub struct ColorMap {
    pub name: String,
    width: u32,
    height: u32,
    /// How many pixels of border the usable map sits inside.
    ///
    /// The climate maps are a 256 square drawn inside a 264 one; the border is
    /// there for the game's texture atlas and is not part of the lookup. Read
    /// from the index the mod writes beside the pictures, because it is a fact
    /// about the asset and cannot be told from the pixels.
    padding: u32,
    pixels: Vec<Rgb>,
}

impl ColorMap {
    /// Samples the map for a column's climate. Both inputs are the bytes the
    /// game packs into its colour map data, so this is the same lookup the game
    /// does, without the shader.
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

/// Where the palette lives inside the export directory.
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
        // Every entry is marked known, colour or not, so the renderer can tell an
        // invisible block from one this palette has never heard of.
        for (code, entry) in &raw.blocks {
            codes[entry.id as usize] = Some(Box::from(code.as_str()));
            by_id[entry.id as usize] = Appearance {
                base: entry
                    .rgb
                    .as_deref()
                    .and_then(Rgb::parse)
                    .unwrap_or_default(),
                invisible: entry.rgb.is_none(),
                climate_map: index_of(&entry.climate_map),
                season_map: index_of(&entry.season_map),
                known: true,
            };
        }

        let coloured = by_id.iter().filter(|a| a.known && !a.invisible).count();

        Ok(Self {
            game_version: raw.game_version,
            source: if raw.source.is_empty() { "unknown".to_owned() } else { raw.source },
            named: raw.blocks.len(),
            fingerprint: raw.fingerprint,
            coloured,
            by_id,
            codes,
            color_maps,
        })
    }

    /// Whether the palette knows this block at all, however it looks.
    #[must_use]
    pub fn knows(&self, block: u16) -> bool {
        self.by_id.get(block as usize).is_some_and(|a| a.known)
    }

    /// What this block is called in this world — `game:rock-granite`. `None` for
    /// a block the palette has never heard of, which is the same thing `knows`
    /// says no to.
    #[must_use]
    pub fn code_of(&self, block: u16) -> Option<&str> {
        self.codes.get(block as usize)?.as_deref()
    }

    /// The colour of one column, tinted for where and when it is.
    ///
    /// Grass, leaves and water are greyscale masks in the game's assets. One tint
    /// is built for them and the block's own colour is multiplied by it once —
    /// which is the part this used to get wrong. The climate map makes the tint,
    /// from temperature across and rainfall down. The season map does not darken
    /// that tint but *stands in for* part of it, in the proportion the season is
    /// felt at this temperature and height at all. Multiplying the two instead
    /// compounded them, which is why warm ground came out redder and browner than
    /// the game draws it, and why nowhere ever looked as though the season had
    /// left it alone.
    ///
    /// Taken from `colormap.fsh` and `colormap.vsh` in the game's own shaders,
    /// which are the only statement of this that cannot drift.
    #[must_use]
    pub fn color_of(
        &self,
        block: u16,
        column: &crate::columns::Column,
        variation: u8,
        sea_level: i32,
    ) -> Option<Rgb> {
        let appearance = self.by_id.get(block as usize)?;
        if !appearance.known || appearance.invisible {
            return None;
        }

        let mut tint = Rgb::new(255, 255, 255);
        if let Some(map) = appearance.climate_map.and_then(|at| self.color_maps.get(at as usize)) {
            tint = map.sample(column.temperature, column.rainfall);
        }

        if let Some(map) = appearance.season_map.and_then(|at| self.color_maps.get(at as usize)) {
            let weight = season_weight(column, sea_level);
            if weight > 0.0 {
                // The game varies the second axis with per-position noise so that
                // a forest is not one flat colour; a hash of the position stands
                // in for it and keeps the map stable between renders.
                tint = tint.mix(map.sample(column.season, variation), weight);
            }
        }

        Some(appearance.base.multiply(tint))
    }
}

/// How much of the season's colour is felt at a column, from none to all of it.
///
/// The game's own curve, and it is not a gentle one: foliage in the tropics never
/// turns, temperate ground turns almost completely, and cold ground turns only a
/// little because it is drab to begin with. Height counts as cold — a mountainside
/// keeps its needles while the valley below it goes to autumn — which is what the
/// sea level is for.
///
/// Copied from `calcColorMapUvs` in the game's `colormap.vsh`, including the
/// constants, because a curve rewritten in one's own words is a curve that no
/// longer matches the game.
fn season_weight(column: &crate::columns::Column, sea_level: i32) -> f32 {
    let above_sea = (f32::from(column.height) - sea_level as f32).max(0.0);
    let x = f32::from(column.temperature) + above_sea * 1.5;
    let weight = 0.5 - (x / 42.0).cos() / 2.3 + (128.0 - x).max(0.0) / 256.0 / 2.0
        - (x - 130.0).max(0.0) / 200.0;
    weight.clamp(0.0, 1.0)
}

fn load_color_maps(dir: &Path) -> Result<Vec<ColorMap>> {
    // A world with no tinted blocks has no such directory, which is unusual and
    // not an error.
    let entries = crate::files::listing(dir)
        .map_err(|error| Error::io(format!("reading {}", dir.display()), error))?;

    // What the mod recorded about each picture's border. Absent where the mod is
    // older than this build, and a border of nothing is what the maps looked like
    // to every build before this one — wrong at the edges, but drawn.
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
            // A border that leaves nothing to sample is not a border.
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
