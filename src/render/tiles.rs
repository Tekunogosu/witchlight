//! Turns columns into pixels.
//!
//! One pixel is one block. Colour comes from the palette, tinted by the column's
//! climate. Relief comes from comparing each column's height to its northern and
//! western neighbours, which stops a map of correct colours from looking flat.

use image::RgbImage;
use rayon::prelude::*;

use crate::render::color::Rgb;
use crate::render::columns::{Column, World};
use crate::render::palette::Palette;

/// Returns per-position variation for the season tint's second axis, standing in
/// for the value noise the game's shader uses. It is deterministic, so the same
/// block is the same colour on every render.
fn variation(x: i32, z: i32) -> u8 {
    let mut hash = (x as u32).wrapping_mul(0x9e37_79b9) ^ (z as u32).wrapping_mul(0x85eb_ca6b);
    hash ^= hash >> 15;
    hash = hash.wrapping_mul(0xc2b2_ae35);
    // Kept off the extremes, where the game clamps it too.
    40 + (hash >> 24) as u8 % 176
}

/// The colour used where a chunk was never exported.
pub const UNMAPPED: Rgb = Rgb::new(0x14, 0x14, 0x16);
/// The colour used for a block the palette has never heard of. It is loud on
/// purpose, because such a block is a bug in the export rather than something to
/// hide behind a plausible grey.
const UNKNOWN_BLOCK: Rgb = Rgb::new(0xff, 0x00, 0xdc);
/// The colour used for exported ground the map cannot put a colour to. It is a
/// quiet bare earth.
///
/// Two surfaces are painted with it. [`Surface::Uncoloured`] is a block that
/// draws something the palette has no colour for, which the mod repairs by asking
/// a client. [`Surface::Blank`] is a block that draws nothing at all, which is
/// air over an empty column or one of the invisible placeholders a large
/// structure stands its real block beside. Both take slope shading like any other
/// terrain, so a pit dug through either still shows its shape.
///
/// Neither may be painted as [`UNMAPPED`]. Ground somebody has just dug would
/// then be the same colour as a world nobody has walked into, and the invisible
/// placeholders would appear as black specks scattered through explored
/// terrain.
const UNCOLOURED: Rgb = Rgb::new(0x6b, 0x62, 0x57);

pub struct Renderer<'a> {
    pub world: &'a World,
    pub palette: &'a Palette,
    /// Where the world's oceans sit. Height is measured against this when
    /// deciding how much of the season a column feels.
    pub sea_level: i32,
}

/// What is at one column.
///
/// The renderer paints this, the coverage report counts it, and the viewer's
/// inspector reports it. All three read the same value, so the map cannot name a
/// block it did not draw.
#[derive(Debug, Clone, Copy)]
pub enum Surface {
    /// Nothing has been exported here.
    Unmapped,
    /// A block the palette has never heard of. Drawn loud on purpose.
    Unknown { column: Column },
    /// Known to the palette, draws something, and the palette has no colour for
    /// it. This is terrain waiting on a colour rather than absence.
    Uncoloured { column: Column },
    /// Known to the palette with nothing to draw, such as air and the invisible
    /// placeholders a large structure stands beside its real block. The picture
    /// treats this as ground, because the column was exported and only its
    /// topmost block has nothing to show.
    Blank { column: Column },
    /// A colour, before slope shading.
    Painted { column: Column, color: Rgb },
}

impl Surface {
    /// Returns the column behind this surface, if anything was exported.
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

    /// Returns one word for how this column read, as the map reports it.
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

/// How the exported surface resolves against the palette.
///
/// An empty map and a grey map mean missing terrain and a missing palette
/// respectively, and neither file alone says which. This is reported on load so
/// the answer is in the log before anyone asks.
#[derive(Debug, Clone, Copy, Default)]
pub struct Coverage {
    pub columns: usize,
    /// Resolved to a colour.
    pub painted: usize,
    /// Known to the palette with nothing to draw, such as air.
    pub blank: usize,
    /// Known to the palette, draws something, and has no colour there.
    pub uncoloured: usize,
    /// Not in the palette at all.
    pub unknown: usize,
}

impl Coverage {
    /// Counts one column, however it read. A position with nothing exported is
    /// not a column and is not counted, because coverage walks the chunks that
    /// exist.
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
        // A share that rounds down to nothing must not read as zero. Forty-eight
        // columns of dug ground in a million is a fault somebody is looking at,
        // and `0%` keeps it out of the log. Only a count of zero reads as `0%`.
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

    /// Returns true when this coverage is bad enough to warn about rather than
    /// only report.
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

    /// Returns what is at one block position.
    ///
    /// This answers whether anything was exported, whether the palette knows the
    /// block, and what colour it comes out before shading.
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

    /// Classifies every exported column without drawing anything.
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
    /// block position. North is up, so world Z grows downward in the image.
    ///
    /// Rows are rendered in parallel. Every pixel is decided from the world and
    /// the palette alone with nothing carried between them, so rows are
    /// independent. The whole-map render is one image of every block anyone has
    /// explored, which is too large for one core.
    #[must_use]
    pub fn render(&self, origin_x: i32, origin_z: i32, size: u32) -> RgbImage {
        /// The smallest square worth rendering across the thread pool.
        const PARALLEL_FROM: u32 = 128;

        let width = size as usize * 3;
        let mut pixels = vec![0u8; width * size as usize];

        let row = |(row, line): (usize, &mut [u8])| {
            let z = origin_z + row as i32;
            for px in 0..size as usize {
                let color = self.pixel(origin_x + px as i32, z);
                line[px * 3..px * 3 + 3].copy_from_slice(&[color.r, color.g, color.b]);
            }
        };
        // A whole tile is worth spreading over the cores. A chunk-sized patch is
        // not, because handing thirty rows to a pool costs more than the rows.
        if size >= PARALLEL_FROM {
            pixels.par_chunks_mut(width).enumerate().for_each(row);
        } else {
            pixels.chunks_mut(width).enumerate().for_each(row);
        }

        RgbImage::from_raw(size, size, pixels)
            .expect("the buffer is three bytes for every pixel of a size by size image")
    }

    fn pixel(&self, x: i32, z: i32) -> Rgb {
        match self.surface_at(x, z) {
            Surface::Unmapped => UNMAPPED,
            // These are counted apart and painted the same. One is waiting for a
            // colour and the other will never have one, and both are ground whose
            // top the map cannot draw. Only an unexported column is absence.
            Surface::Blank { column } | Surface::Uncoloured { column } => {
                UNCOLOURED.scale(self.shade(x, z, column.height))
            }
            Surface::Unknown { column } => UNKNOWN_BLOCK.scale(self.shade(x, z, column.height)),
            Surface::Painted { column, color } => color.scale(self.shade(x, z, column.height)),
        }
    }

    /// Returns the slope shading factor. Comparing against the north and west
    /// neighbours lights the world from the north-west, which is the convention
    /// game maps use.
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
    use crate::render::columns::Chunk;
    use crate::util::files::testing::Scratch;

    /// Builds a world of one flat chunk with every column the same block.
    ///
    /// The chunk is flat so slope shading is exactly one, which makes a pixel the
    /// colour the palette decided and nothing else.
    fn world_of(block: u16) -> World {
        let mut world = World::empty();
        world.edge = 2;
        world.chunks.insert(
            (0, 0),
            Chunk::filled_with(Column { block, height: 100, temperature: 128, rainfall: 128, season: 0 }, 4),
        );
        world
    }

    /// Air, bare soil with no colour for it, and rock with one.
    fn palette(at: &std::path::Path) -> Palette {
        std::fs::write(
            crate::render::palette::path_in(at),
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
        // Bare soil is what a player uncovers every time they dig. Painting it
        // the colour of unexplored ground makes the map look as though it has
        // stopped following the world exactly where the world is changing.
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
        // A block that draws nothing is not absence. The column was exported,
        // its height is known, and only the block on top has nothing to show.
        // Painted as unexplored it puts black specks through explored terrain,
        // one for every invisible placeholder a large structure stands beside
        // its real block and one for every column a chunk returned as air before
        // it finished loading.
        //
        // It keeps its own name in the accounting, because a colour that will
        // never arrive and a colour that is being fetched differ to an operator
        // reading coverage even though they look the same on screen.
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
        // This is what makes the two cases above safe to paint alike. One colour
        // still means nothing was exported here, and nothing the exporter wrote
        // can take it.
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
        // Forty-eight columns of dug ground in a million is a fault a player is
        // looking at, and rounding reports it as `0%`.
        let mut coverage = Coverage { columns: 1_000_000, painted: 999_952, ..Coverage::default() };
        coverage.uncoloured = 48;
        assert_eq!(
            coverage.summary(),
            "100% painted, 0% nothing to draw, <1% waiting on a colour, 0% unknown blocks"
        );
    }

    #[test]
    fn coverage_counts_the_waiting_apart_from_the_painted_and_the_bare() {
        // This is what the log says on load. It tells an operator whether the
        // map is missing terrain or missing colours, two faults that look
        // identical on screen.
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
