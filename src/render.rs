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

pub struct Renderer<'a> {
    pub world: &'a World,
    pub palette: &'a Palette,
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
    /// Known to the palette with nothing to draw: air and the other invisibles.
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
            Self::Unknown { column } | Self::Blank { column } | Self::Painted { column, .. } => {
                Some(column)
            }
        }
    }

    /// One word for how this column read, as the map reports it.
    #[must_use]
    pub const fn state(&self) -> &'static str {
        match self {
            Self::Unmapped => "unmapped",
            Self::Unknown { .. } => "unknown",
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

    /// What is at one block position.
    ///
    /// The whole of the reading: whether anything was exported, whether the
    /// palette has heard of it, and what colour it comes out before the light.
    #[must_use]
    pub fn surface_at(&self, x: i32, z: i32) -> Surface {
        let Some(column) = self.world.column_at(x, z) else {
            return Surface::Unmapped;
        };

        match self.palette.color_of(column.block, &column, variation(x, z)) {
            Some(color) => Surface::Painted { column, color },
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
            Surface::Unmapped | Surface::Blank { .. } => UNMAPPED,
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
