//! Turning columns into pixels.
//!
//! One pixel is one block. Colour comes from the palette, tinted by the column's
//! climate; relief comes from comparing each column's height to its northern and
//! western neighbours, which is what stops a map of correct colours from looking
//! like a flat sheet of paper.

use image::RgbImage;
use rayon::prelude::*;

use crate::color::Rgb;
use crate::columns::World;
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

pub struct Renderer<'a> {
    pub world: &'a World,
    pub palette: &'a Palette,
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
    /// Not in the palette at all.
    pub unknown: usize,
}

impl Coverage {
    #[must_use]
    pub fn summary(&self) -> String {
        if self.columns == 0 {
            return "no columns to draw".to_owned();
        }
        let share = |count: usize| count as f32 * 100.0 / self.columns as f32;
        format!(
            "{:.0}% painted, {:.0}% nothing to draw, {:.0}% unknown blocks",
            share(self.painted),
            share(self.blank),
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
    pub const fn new(world: &'a World, palette: &'a Palette) -> Self {
        Self { world, palette }
    }

    /// Classifies every exported column, without drawing anything.
    #[must_use]
    pub fn coverage(&self) -> Coverage {
        let mut coverage = Coverage::default();
        for chunk in self.world.chunks.values() {
            for column in &chunk.columns {
                coverage.columns += 1;
                match self.palette.color_of(column.block, column, 128) {
                    Some(_) => coverage.painted += 1,
                    None if self.palette.knows(column.block) => coverage.blank += 1,
                    None => coverage.unknown += 1,
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
        let Some(column) = self.world.column_at(x, z) else {
            return UNMAPPED;
        };

        let base = match self.palette.color_of(column.block, &column, variation(x, z)) {
            Some(color) => color,
            None if self.palette.knows(column.block) => return UNMAPPED,
            None => UNKNOWN_BLOCK,
        };

        base.scale(self.shade(x, z, column.height))
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
