//! Turning columns into pixels.
//!
//! One pixel is one block. Colour comes from the palette, tinted by the column's
//! climate; relief comes from comparing each column's height to its northern and
//! western neighbours, which is what stops a map of correct colours from looking
//! like a flat sheet of paper.

use image::RgbImage;
use rayon::prelude::*;

use crate::color::Rgb;
use crate::columns::{Column, World};
use crate::palette::Palette;

/// Per-position variation for the season tint's second axis, standing in for the
/// value noise the game's shader uses. Deterministic, so the same block is the
/// same colour every render.
fn variation(x: i32, z: i32) -> u8 {
    let mut hash = (x as u32).wrapping_mul(0x9e37_79b9) ^ (z as u32).wrapping_mul(0x85eb_ca6b);
    hash ^= hash >> 15;
    hash = hash.wrapping_mul(0xc2b2_ae35);
    // Kept off the extremes, which is where the game clamps it too.
    40 + (hash >> 24) as u8 % 176
}

/// Colour used where a chunk was never exported, or a column has nothing to draw.
pub const UNMAPPED: Rgb = Rgb::new(0x14, 0x14, 0x16);
/// Deliberately loud: a block the palette has never heard of is a bug in the
/// export, not something to hide behind a plausible grey.
const UNKNOWN_BLOCK: Rgb = Rgb::new(0xff, 0x00, 0xdc);
/// Ground this map knows it has stood on and cannot put a colour to.
///
/// Bare earth, and quiet on purpose. Two different columns are painted with it
/// and they have one thing in common: something was exported here, and the block
/// on top of it has no colour to give. One is [`Surface::Uncoloured`] — a block
/// that draws something the palette has no colour for, which the mod repairs by
/// asking a client. The other is [`Surface::Blank`] — a block that draws nothing
/// at all, which is air over a column with nothing under it, or one of the
/// invisible placeholders a large structure stands its real block beside. Both
/// take the slope shading like any other terrain, so a pit dug through either
/// still shows its own shape.
///
/// What neither may be is [`UNMAPPED`]. Painted as that, the ground under grass
/// somebody had just dug up was the same colour as a world nobody had ever
/// walked into, and the map read as though it had stopped following the world in
/// exactly the places the world was changing. The invisible placeholders were
/// the same fault wearing different clothes: a handful of black specks scattered
/// through explored terrain, each one reading as a hole in a world that has no
/// hole in it.
const UNCOLOURED: Rgb = Rgb::new(0x6b, 0x62, 0x57);

pub struct Renderer<'a> {
    pub world: &'a World,
    pub palette: &'a Palette,
    /// Where the world's oceans sit, which is what height is measured against
    /// when deciding how much of the season a column feels.
    pub sea_level: i32,
}

/// What is at one column, read once and used everywhere.
///
/// The renderer paints this, the coverage report counts it, and the viewer's
/// inspector says it out loud. Three readings of the same four-way decision is
/// three chances for the map to name a block it did not draw, so there is one.
#[derive(Debug, Clone, Copy)]
pub enum Surface {
    /// Nothing has been exported here.
    Unmapped,
    /// A block the palette has never heard of, drawn loud on purpose.
    Unknown { column: Column },
    /// Known to the palette, draws something, and the palette has no colour for
    /// it. Terrain waiting on a colour, not absence.
    Uncoloured { column: Column },
    /// Known to the palette with nothing to draw: air, and the invisible
    /// placeholders a large structure stands beside its real block. Ground, as
    /// far as the picture goes — the column was exported, and only its topmost
    /// block has nothing to show.
    Blank { column: Column },
    /// A colour, before the slope shading.
    Painted { column: Column, color: Rgb },
}

impl Surface {
    /// The column behind it, where anything was exported.
    #[must_use]
    pub const fn column(&self) -> Option<Column> {
        match *self {
            Self::Unmapped => None,
            Self::Unknown { column }
            | Self::Uncoloured { column }
            | Self::Blank { column }
            | Self::Painted { column, .. } => Some(column),
        }
    }

    /// One word for how this column read, as the map reports it.
    #[must_use]
    pub const fn state(&self) -> &'static str {
        match self {
            Self::Unmapped => "unmapped",
            Self::Unknown { .. } => "unknown",
            Self::Uncoloured { .. } => "uncoloured",
            Self::Blank { .. } => "blank",
            Self::Painted { .. } => "painted",
        }
    }
}

/// How the exported surface actually resolves against the palette.
///
/// The difference between "the map is empty" and "the map is grey" is the
/// difference between missing terrain and a missing palette, and it is not
/// visible from either file alone. Reported on load so the answer is in the log
/// before anyone has to ask for it.
#[derive(Debug, Clone, Copy, Default)]
pub struct Coverage {
    pub columns: usize,
    /// Resolved to a colour.
    pub painted: usize,
    /// Known to the palette, with nothing to draw — air and other invisibles.
    pub blank: usize,
    /// Known to the palette, draws something, and has no colour there.
    pub uncoloured: usize,
    /// Not in the palette at all.
    pub unknown: usize,
}

impl Coverage {
    /// Counts one column, however it read. A position with nothing exported is
    /// not a column and is not counted: coverage walks the chunks that exist.
    fn count(&mut self, surface: Surface) {
        let counted = match surface {
            Surface::Unmapped => return,
            Surface::Painted { .. } => &mut self.painted,
            Surface::Blank { .. } => &mut self.blank,
            Surface::Uncoloured { .. } => &mut self.uncoloured,
            Surface::Unknown { .. } => &mut self.unknown,
        };
        *counted += 1;
        self.columns += 1;
    }

    #[must_use]
    pub fn summary(&self) -> String {
        if self.columns == 0 {
            return "no columns to draw".to_owned();
        }
        // A share that rounds to nothing is not nothing. Forty-eight columns of
        // dug ground in a million is a fault somebody is looking at and `0%` is
        // how it stays out of the log — so a count that is there says so, and only
        // a count that is genuinely zero reads as zero.
        let share = |count: usize| {
            let percent = count as f32 * 100.0 / self.columns as f32;
            match count {
                0 => "0%".to_owned(),
                _ if percent < 0.5 => "<1%".to_owned(),
                _ => format!("{percent:.0}%"),
            }
        };
        format!(
            "{} painted, {} nothing to draw, {} waiting on a colour, {} unknown blocks",
            share(self.painted),
            share(self.blank),
            share(self.uncoloured),
            share(self.unknown)
        )
    }

    /// Whether this is worth complaining about rather than merely reporting.
    #[must_use]
    pub fn is_poor(&self) -> bool {
        self.columns > 0 && self.painted * 4 < self.columns
    }
}

impl<'a> Renderer<'a> {
    #[must_use]
    pub const fn new(world: &'a World, palette: &'a Palette, sea_level: i32) -> Self {
        Self { world, palette, sea_level }
    }

    /// What is at one block position.
    ///
    /// The whole of the reading: whether anything was exported, whether the
    /// palette has heard of it, and what colour it comes out before the light.
    #[must_use]
    pub fn surface_at(&self, x: i32, z: i32) -> Surface {
        let Some(column) = self.world.column_at(x, z) else {
            return Surface::Unmapped;
        };

        match self.palette.color_of(column.block, &column, variation(x, z), self.sea_level) {
            Some(color) => Surface::Painted { column, color },
            None if self.palette.uncoloured(column.block) => Surface::Uncoloured { column },
            None if self.palette.knows(column.block) => Surface::Blank { column },
            None => Surface::Unknown { column },
        }
    }

    /// Classifies every exported column, without drawing anything.
    #[must_use]
    pub fn coverage(&self) -> Coverage {
        let mut coverage = Coverage::default();
        let edge = self.world.edge as i32;
        for &(chunk_x, chunk_z) in self.world.chunks.keys() {
            for dz in 0..edge {
                for dx in 0..edge {
                    coverage.count(self.surface_at(chunk_x * edge + dx, chunk_z * edge + dz));
                }
            }
        }
        coverage
    }

    /// Renders a square of world, `size` blocks on a side, starting at the given
    /// block position. North is up: world Z grows downward in the image.
    ///
    /// A row at a time, in parallel. Every pixel is decided from the world and
    /// the palette alone, with nothing carried between them, so rows are
    /// independent and the whole-map render — which is one image of every block
    /// anyone has explored — stops being one core's problem.
    #[must_use]
    pub fn render(&self, origin_x: i32, origin_z: i32, size: u32) -> RgbImage {
        let width = size as usize * 3;
        let mut pixels = vec![0u8; width * size as usize];

        pixels
            .par_chunks_mut(width)
            .enumerate()
            .for_each(|(row, line)| {
                let z = origin_z + row as i32;
                for px in 0..size as usize {
                    let color = self.pixel(origin_x + px as i32, z);
                    line[px * 3..px * 3 + 3].copy_from_slice(&[color.r, color.g, color.b]);
                }
            });

        RgbImage::from_raw(size, size, pixels)
            .expect("the buffer is three bytes for every pixel of a size by size image")
    }

    fn pixel(&self, x: i32, z: i32) -> Rgb {
        match self.surface_at(x, z) {
            Surface::Unmapped => UNMAPPED,
            // Counted apart and painted the same, which is the honest picture:
            // one is waiting for a colour and the other will never have one, and
            // to a reader both are ground whose top the map cannot draw. Only a
            // column nobody has exported is absence.
            Surface::Blank { column } | Surface::Uncoloured { column } => {
                UNCOLOURED.scale(self.shade(x, z, column.height))
            }
            Surface::Unknown { column } => UNKNOWN_BLOCK.scale(self.shade(x, z, column.height)),
            Surface::Painted { column, color } => color.scale(self.shade(x, z, column.height)),
        }
    }

    /// Slope shading. Comparing against the north and west neighbours lights the
    /// world from the north-west, the convention every game map uses.
    fn shade(&self, x: i32, z: i32, height: i16) -> f32 {
        let neighbour = |dx: i32, dz: i32| {
            self.world
                .column_at(x + dx, z + dz)
                .map_or(height, |column| column.height)
        };

        let slope = i32::from(height) * 2 - i32::from(neighbour(-1, 0)) - i32::from(neighbour(0, -1));
        1.0 + (slope as f32).clamp(-6.0, 6.0) * 0.045
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::columns::Chunk;
    use crate::files::testing::Scratch;

    /// A world of one flat chunk, every column the same block.
    ///
    /// Flat on purpose: the slope shading is then exactly one, so what a pixel
    /// comes out as is the colour the palette decided and nothing else.
    fn world_of(block: u16) -> World {
        let mut world = World::empty();
        world.edge = 2;
        world.chunks.insert(
            (0, 0),
            Chunk {
                columns: vec![
                    Column {
                        block,
                        height: 100,
                        temperature: 128,
                        rainfall: 128,
                        season: 0
                    };
                    4
                ],
            },
        );
        world
    }

    /// Air, bare soil with no colour for it, and rock with one.
    fn palette(at: &std::path::Path) -> Palette {
        std::fs::write(
            crate::palette::path_in(at),
            r##"{"Version":1,"GameVersion":"1.22.7","Source":"client","Fingerprint":"abc",
                 "Blocks":{"game:air":{"Id":0,"Rgb":null,"Invisible":true},
                           "game:soil-medium-none":{"Id":1,"Rgb":null,"Invisible":false},
                           "game:rock-granite":{"Id":2,"Rgb":"#806040"}}}"##,
        )
        .expect("a palette to read back");
        Palette::load(at).expect("it parses")
    }

    #[test]
    fn ground_waiting_on_a_colour_is_drawn_as_ground() {
        // The bug this exists to keep out. Bare soil is what a player uncovers
        // every time they dig, and painting it the colour of unexplored ground
        // made the map read as though it had stopped following the world in
        // exactly the places the world was changing.
        let held = Scratch::new("render-uncoloured");
        let palette = palette(held.at());
        let world = world_of(1);
        let renderer = Renderer::new(&world, &palette, 110);

        assert_eq!(renderer.surface_at(0, 0).state(), "uncoloured");

        let image = renderer.render(0, 0, 2);
        let drawn = Rgb::new(image[(0, 0)][0], image[(0, 0)][1], image[(0, 0)][2]);
        assert_eq!(drawn, UNCOLOURED);
        assert_ne!(drawn, UNMAPPED, "it must not read as ground nobody has walked into");
    }

    #[test]
    fn a_block_that_draws_nothing_is_still_a_column_somebody_exported() {
        // The other half of the same rule, and the one that was wrong. A block
        // that draws nothing is not absence: the column was exported, its height
        // is known, and only the block on top of it has nothing to show. Painted
        // as unexplored it put black specks through explored terrain — one for
        // every invisible placeholder a large structure stands beside its real
        // block, and one for every column a chunk handed back as air before it
        // had finished loading.
        //
        // It keeps its own name in the accounting, because a colour that will
        // never arrive and a colour that is being fetched are different things
        // to an operator reading coverage. They are the same thing to look at.
        let held = Scratch::new("render-blank");
        let palette = palette(held.at());
        let world = world_of(0);
        let renderer = Renderer::new(&world, &palette, 110);

        assert_eq!(renderer.surface_at(0, 0).state(), "blank");
        let image = renderer.render(0, 0, 2);
        let drawn = Rgb::new(image[(0, 0)][0], image[(0, 0)][1], image[(0, 0)][2]);
        assert_eq!(drawn, UNCOLOURED, "a column that exists reads as ground");
        assert_ne!(drawn, UNMAPPED, "and never as a world nobody has walked into");
    }

    #[test]
    fn only_a_column_nobody_exported_reads_as_absence() {
        // Which is what makes the two above safe to paint alike: there is still
        // one colour that means "there is nothing here to know", and nothing the
        // exporter has written can wear it.
        let held = Scratch::new("render-unmapped");
        let palette = palette(held.at());
        let world = World::empty();
        let renderer = Renderer::new(&world, &palette, 110);

        assert_eq!(renderer.surface_at(0, 0).state(), "unmapped");
        let image = renderer.render(0, 0, 2);
        assert_eq!(Rgb::new(image[(0, 0)][0], image[(0, 0)][1], image[(0, 0)][2]), UNMAPPED);
    }

    #[test]
    fn a_share_too_small_to_round_to_a_percent_still_shows() {
        // How a real fault stayed out of the log: forty-eight columns of dug
        // ground in a million is what a player is looking at and `0%` is what it
        // was reported as.
        let mut coverage = Coverage { columns: 1_000_000, painted: 999_952, ..Coverage::default() };
        coverage.uncoloured = 48;
        assert_eq!(
            coverage.summary(),
            "100% painted, 0% nothing to draw, <1% waiting on a colour, 0% unknown blocks"
        );
    }

    #[test]
    fn coverage_counts_the_waiting_apart_from_the_painted_and_the_bare() {
        // What the log says on load, and what tells an operator whether the map
        // is missing terrain or missing colours — two faults that look identical
        // on screen.
        let held = Scratch::new("render-coverage");
        let palette = palette(held.at());

        for (block, state) in [(0u16, "blank"), (1, "uncoloured"), (2, "painted"), (9, "unknown")] {
            let world = world_of(block);
            let coverage = Renderer::new(&world, &palette, 110).coverage();
            let counted = match state {
                "blank" => coverage.blank,
                "uncoloured" => coverage.uncoloured,
                "painted" => coverage.painted,
                _ => coverage.unknown,
            };
            assert_eq!(coverage.columns, 4, "every column of the chunk is counted");
            assert_eq!(counted, 4, "and all four read as {state}");
        }
    }
}
