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
}

/// A climate lookup image: horizontal axis temperature, vertical axis rainfall,
/// exactly as the game's own shader samples it.
pub struct ColorMap {
    pub name: String,
    width: u32,
    height: u32,
    pixels: Vec<Rgb>,
}

impl ColorMap {
    /// Samples the map for a column's climate. Both inputs are the bytes the
    /// game packs into its colour map data, so this is the same lookup the game
    /// does, without the shader.
    #[must_use]
    pub fn sample(&self, temperature: u8, rainfall: u8) -> Rgb {
        let x = (u32::from(temperature) * self.width / 256).min(self.width - 1);
        let y = (u32::from(rainfall) * self.height / 256).min(self.height - 1);
        self.pixels[(y * self.width + x) as usize]
    }
}

impl Palette {
    /// Loads `palette.json` and the colour maps beside it.
    pub fn load(dir: &Path) -> Result<Self> {
        let path = dir.join("palette.json");
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

        Ok(Self {
            game_version: raw.game_version,
            source: if raw.source.is_empty() { "unknown".to_owned() } else { raw.source },
            named: raw.blocks.len(),
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
    /// Grass, leaves and water are greyscale masks in the game's assets: the
    /// climate map colours them for their temperature and rainfall, and the
    /// season map turns them through the year. The game applies both, so this
    /// does too.
    #[must_use]
    pub fn color_of(&self, block: u16, column: &crate::columns::Column, variation: u8) -> Option<Rgb> {
        let appearance = self.by_id.get(block as usize)?;
        if !appearance.known || appearance.invisible {
            return None;
        }

        let mut color = appearance.base;
        if let Some(map) = appearance.climate_map.and_then(|at| self.color_maps.get(at as usize)) {
            color = color.multiply(map.sample(column.temperature, column.rainfall));
        }
        if let Some(map) = appearance.season_map.and_then(|at| self.color_maps.get(at as usize)) {
            // The game varies the second axis with per-position noise so that a
            // forest is not one flat colour; a hash of the position stands in for
            // it and keeps the map stable between renders.
            color = color.multiply(map.sample(column.season, variation));
        }
        Some(color)
    }
}

fn load_color_maps(dir: &Path) -> Result<Vec<ColorMap>> {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        // A world with no tinted blocks is unusual but not an error.
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(Error::io(format!("reading {}", dir.display()), error)),
    };

    let mut maps = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_none_or(|ext| ext != "png") {
            continue;
        }
        let Some(name) = path.file_stem().map(|stem| stem.to_string_lossy().into_owned()) else {
            continue;
        };

        let image = image::open(&path)
            .map_err(|error| Error::parse(&path, error.to_string()))?
            .to_rgb8();
        maps.push(ColorMap {
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
