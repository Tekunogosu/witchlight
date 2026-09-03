//! Zoom levels.
//!
//! A tile is always [`TILE`] pixels square. Level 0 draws one block per pixel;
//! every level above it draws twice as many blocks per pixel as the one below, so
//! level `L` covers `TILE * 2^L` blocks and a view holds roughly the same number
//! of tiles however far out it is. Without that, the number of tiles on screen
//! grows as the square of how far out you are, and a wide view asks for tens of
//! thousands of them.
//!
//! Levels are numbered from the finest, which is the only numbering that survives
//! a world getting bigger: level 0 is one block per pixel whatever anyone has
//! explored, so new coarser levels are new numbers rather than a renumbering of
//! everything already written.
//!
//! ```text
//! tiles/{level}/{x >> 5}_{z >> 5}/{x}_{z}.png
//! ```
//!
//! The middle directory holds a thousand tiles at most. A large world has
//! millions, and a directory of millions of files is slow to open on every
//! filesystem worth naming.
//!
//! Level 0 is not stored. It is rendered from the world on demand, which is fast
//! and already cached in memory, and storing it would mean four million files for
//! a full world against a few thousand for everything above it.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use image::codecs::png::{CompressionType, FilterType, PngEncoder};
use image::{ExtendedColorType, ImageEncoder, RgbImage};

use crate::color::Rgb;
use crate::error::{Error, Result};
use crate::files;

/// Blocks per tile, and pixels per tile at the finest level: one pixel is one
/// block. Equal to a region, so a region that changes is exactly one tile.
pub const TILE: u32 = 512;

/// Tiles per directory along each axis. A thousand files to a directory.
const BUCKET: i32 = 5;

/// Where the levels live inside the export directory.
#[must_use]
pub fn tiles_dir(exports: &Path) -> PathBuf {
    exports.join("tiles")
}

/// One tile's file.
#[must_use]
pub fn path(exports: &Path, level: u32, x: i32, z: i32) -> PathBuf {
    tiles_dir(exports)
        .join(level.to_string())
        .join(format!("{}_{}", x >> BUCKET, z >> BUCKET))
        .join(format!("{x}_{z}.png"))
}

/// How many levels a world of this many tiles across needs.
///
/// Enough that the coarsest holds the whole world in one tile, and no more: a
/// level nobody can zoom out far enough to see is a level nobody should be
/// building.
#[must_use]
pub fn levels_for(tiles_across: i64, tiles_down: i64) -> u32 {
    let widest = tiles_across.max(tiles_down).max(1);
    let mut level = 0;
    while (1i64 << level) < widest {
        level += 1;
    }
    level
}

/// The tile at `level` that holds the level 0 tile `(x, z)`.
#[must_use]
pub fn ancestor(level: u32, x: i32, z: i32) -> (i32, i32) {
    (x >> level, z >> level)
}

/// The four tiles one level down that a tile is made of, in reading order.
#[must_use]
pub fn children(x: i32, z: i32) -> [(i32, i32); 4] {
    [
        (x * 2, z * 2),
        (x * 2 + 1, z * 2),
        (x * 2, z * 2 + 1),
        (x * 2 + 1, z * 2 + 1),
    ]
}

/// Averages four tiles into one covering twice as much world.
///
/// Averaged rather than sampled. Taking every other pixel is cheaper and is what
/// some maps do, but on a map where one pixel is one block it erases everything
/// narrower than the step — paths, walls, rivers — and they come back as the
/// viewer zooms in, which reads as the map being wrong rather than coarse.
///
/// Doing this against the level below rather than against the world is what makes
/// a coarse tile affordable: averaging `2^L` blocks per pixel straight from the
/// world costs `4^L` times a level 0 tile, and a thousand times is a level 5.
/// Two by two, repeatedly, is the same average for a constant cost.
#[must_use]
pub fn downsample(children: &[Option<RgbImage>; 4], size: u32, blank: Rgb) -> RgbImage {
    let half = size / 2;
    let mut parent = RgbImage::from_pixel(size, size, image::Rgb([blank.r, blank.g, blank.b]));

    for (index, child) in children.iter().enumerate() {
        let Some(child) = child else {
            continue;
        };

        let (offset_x, offset_z) = ((index as u32 % 2) * half, (index as u32 / 2) * half);
        for pz in 0..half {
            for px in 0..half {
                let (sx, sz) = (px * 2, pz * 2);
                let mut total = [0u32; 3];
                for (dx, dz) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                    let pixel = child.get_pixel(sx + dx, sz + dz).0;
                    for channel in 0..3 {
                        total[channel] += u32::from(pixel[channel]);
                    }
                }
                parent.put_pixel(
                    offset_x + px,
                    offset_z + pz,
                    image::Rgb([
                        (total[0] / 4) as u8,
                        (total[1] / 4) as u8,
                        (total[2] / 4) as u8,
                    ]),
                );
            }
        }
    }

    parent
}

/// Reads a stored tile, or nothing if it has not been built.
#[must_use]
pub fn read(exports: &Path, level: u32, x: i32, z: i32) -> Option<RgbImage> {
    let bytes = std::fs::read(path(exports, level, x, z)).ok()?;
    Some(image::load_from_memory(&bytes).ok()?.to_rgb8())
}

/// Writes a tile, beside itself and then into place so a reader never sees half.
pub fn write(exports: &Path, level: u32, x: i32, z: i32, image: &RgbImage) -> Result<()> {
    let target = path(exports, level, x, z);
    files::replace(&target, &encode(image)?)
        .map_err(|error| Error::io(format!("writing {}", target.display()), error))
}

/// Encodes one tile, losslessly, without filtering and at the fastest level.
///
/// PNG normally predicts each pixel from its neighbours and stores the
/// difference, which is a large win on smooth images and none on this one: a
/// tile is one block per pixel, each block its own shade under its own slope,
/// so the differences are as noisy as the values. Measured on shaded terrain,
/// a tile is about seventy percent of its raw size whatever is chosen: the
/// best filter recovers six percent, and the slowest deflate level recovers
/// under three percent over the fastest for twice the time. Level 0 is drawn
/// and encoded on demand, so the time is the whole cost and the bytes are not
/// worth it.
///
/// Nothing here can discard colour. A tile is continuous tone — slope shading
/// over climate tinting gives thousands of shades to a square of terrain — so an
/// indexed format would have to throw some away, and the ones it would throw away
/// are the rare ones: ore, water edges, anything small.
pub fn encode(image: &RgbImage) -> Result<Vec<u8>> {
    let mut encoded = Vec::new();
    PngEncoder::new_with_quality(&mut encoded, CompressionType::Fast, FilterType::NoFilter)
        .write_image(
            image.as_raw(),
            image.width(),
            image.height(),
            ExtendedColorType::Rgb8,
        )
        .map_err(|error| Error::io("encoding a tile", std::io::Error::other(error.to_string())))?;
    Ok(encoded)
}

/// What the levels on disk were built from.
///
/// Tiles are derived from regions, so a region format this build cannot read
/// leaves tiles it must not show: the mod clears the map on such a change, and
/// terrain that no longer exists would otherwise stay on screen at every level
/// above zero. Rebuilding everything on start would also fix it and does not
/// scale — a large world is millions of level 0 renders — so the levels are kept
/// and thrown away only when what made them changed.
pub fn stamp(exports: &Path) -> PathBuf {
    tiles_dir(exports).join("built-by")
}

/// How this build paints a column, counted up whenever that changes.
///
/// Separate from the region format, because the two go wrong in different ways
/// and only one of them costs the map. A region format the service no longer
/// reads means the ground itself has to be exported again; a change in here means
/// the same ground would now be drawn a different colour, so what is stored is
/// merely out of date. Bumping this throws the pictures away and redraws them
/// from region files that were never in doubt.
///
/// 1: the season tint stood in for part of the climate tint rather than being
///    multiplied over the top of it, weighted the way the game weights it, and
///    the climate maps stopped being read through their own border.
pub const PAINT: u16 = 1;

/// Throws away every level if it was built from a region format this build no
/// longer reads, or painted by a build that painted differently. Returns whether
/// it did.
pub fn reset_unless_built_from(exports: &Path, version: u16) -> bool {
    // The tile encoding is in here too: levels written by a different encoder are
    // still readable, but they are not the size this build would have made them,
    // and nothing else would notice the difference.
    let want = format!("{version}/png-nofilter/paint{PAINT}");
    if std::fs::read_to_string(stamp(exports)).is_ok_and(|found| found.trim() == want) {
        return false;
    }

    let cleared = tiles_dir(exports).exists();
    let _ = std::fs::remove_dir_all(tiles_dir(exports));
    let _ = files::replace(&stamp(exports), want.as_bytes());
    cleared
}

/// The best stored picture of this tile's ground, taken from a level above it.
///
/// A tile that cannot be drawn at its own level is not nothing: the levels above
/// it are pictures of the same ground, coarser. Enlarging one is a worse map than
/// the real tile and a far better map than no tile — and Leaflet does not
/// substitute a parent of its own accord, so an unanswerable tile is simply
/// absent, which looks exactly like a map that has broken.
///
/// Walks up until it finds a level that has this ground, so it survives a gap of
/// more than one. Nearest neighbour on the way back down, because the pixels this
/// enlarges are already averages and smoothing them again only invents detail
/// that was never there.
///
/// `read` answers for a tile at a level, wherever it is held — see
/// [`crate::levels`], which is where that is decided.
#[must_use]
pub fn from_above(
    read: impl Fn(u32, i32, i32) -> Option<RgbImage>,
    level: u32,
    x: i32,
    z: i32,
    size: u32,
    ceiling: u32,
) -> Option<RgbImage> {
    for up in 1..=ceiling.saturating_sub(level) {
        let (ax, az) = ancestor(up, x, z);
        let Some(above) = read(level + up, ax, az) else {
            continue;
        };

        // Which part of that ancestor is this tile's ground: `up` halvings in,
        // so the tile occupies one part in `2^up` of each edge.
        let across = 1i32 << up;
        let part = size / across as u32;
        let left = (x - ax * across) as u32 * part;
        let top = (z - az * across) as u32 * part;

        let mut grown = RgbImage::new(size, size);
        for row in 0..size {
            for column in 0..size {
                let from = above.get_pixel(left + column / across as u32, top + row / across as u32);
                grown.put_pixel(column, row, *from);
            }
        }
        return Some(grown);
    }
    None
}

/// Where the palette the levels were drawn with is recorded.
fn painted_by(exports: &Path) -> PathBuf {
    tiles_dir(exports).join("painted-by")
}

/// The block registry the stored levels were drawn against, if it was recorded.
///
/// Kept apart from [`stamp`], which decides whether the levels are *readable* and
/// throws them away when they are not. This decides only whether they agree with
/// the palette in use, and the answer to disagreement is to draw them again — not
/// to delete them. A pyramid drawn with the last good palette is the only thing
/// left to look at when the current one draws nothing.
#[must_use]
pub fn palette_built_from(exports: &Path) -> Option<String> {
    std::fs::read_to_string(painted_by(exports))
        .ok()
        .map(|found| found.trim().to_owned())
        .filter(|found| !found.is_empty())
}

/// Records which palette the levels have now been drawn with — when that is
/// not already what the file says, since it is asked on every build beat and
/// the answer changes once per palette.
pub fn record_palette(exports: &Path, fingerprint: &str) {
    if fingerprint.is_empty() || palette_built_from(exports).as_deref() == Some(fingerprint) {
        return;
    }
    let _ = files::replace(&painted_by(exports), fingerprint.as_bytes());
}

/// Which regions have a stored tile above them that is missing, or older than the
/// region itself.
///
/// One answer for the whole pyramid rather than one per level. Asking only about
/// level 1 cannot see a level that did not exist when the pyramid was last built:
/// a world that grows past a power of two gains a coarsest level, the walk upward
/// from whatever changed builds exactly one tile there, and every other tile at
/// that level has nothing beneath it that changed, so nothing ever asks for it.
/// That level is the one a viewer opens on, so the map reads as empty.
///
/// Comparing every level also means a run that starts with its levels already
/// current has nothing to do, and one that does not rebuilds only what moved
/// while it was away.
#[must_use]
pub fn behind(
    exports: &Path,
    regions: &HashMap<(i32, i32), SystemTime>,
    levels: u32,
) -> Vec<(i32, i32)> {
    regions
        .iter()
        .filter(|&(&(x, z), &exported)| {
            (1..=levels).any(|level| {
                let (ax, az) = ancestor(level, x, z);
                files::modified(&path(exports, level, ax, az)).is_none_or(|built| built < exported)
            })
        })
        .map(|(at, _)| *at)
        .collect()
}

/// How tall the stored pyramid is, read from the directories rather than
/// remembered: a level is a directory named for its number, so the highest of
/// those is what was actually built.
#[must_use]
pub fn levels_built(exports: &Path) -> u32 {
    std::fs::read_dir(tiles_dir(exports))
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| entry.file_name().to_str()?.parse::<u32>().ok())
        .max()
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::files::testing::Scratch;

    const BLANK: Rgb = Rgb::new(0, 0, 0);

    /// Writes a tile at `level` and returns when it was written.
    fn build(held: &Scratch, level: u32, x: i32, z: i32) -> SystemTime {
        write(held.at(), level, x, z, &flat(2, 0)).expect("a tile");
        files::modified(&path(held.at(), level, x, z)).expect("a timestamp")
    }

    /// Each quarter a flat value, so which quarter a child came from is readable
    /// straight off the result.
    fn quarters(size: u32) -> RgbImage {
        tile(size, |x, z| {
            let quarter = u8::try_from((z / (size / 2)) * 2 + (x / (size / 2))).unwrap();
            [quarter, quarter, quarter]
        })
    }

    #[test]
    fn a_tile_with_no_level_above_it_cannot_be_grown() {
        let at = Scratch::new("pyramid-nothing-above");
        assert!(from_above(|l, x, z| read(at.at(), l, x, z), 0, 5, 5, 8, 3).is_none());
    }

    #[test]
    fn a_missing_tile_is_grown_from_its_parents_own_quarter() {
        let at = Scratch::new("pyramid-own-quarter");
        write(at.at(), 1, 3, 3, &quarters(8)).expect("the parent stores");

        // Level 1 tile (3, 3) covers level 0 tiles (6, 7) in both axes.
        for (x, z, want) in [(6, 6, 0u8), (7, 6, 1), (6, 7, 2), (7, 7, 3)] {
            let grown = from_above(|l, x, z| read(at.at(), l, x, z), 0, x, z, 8, 3).expect("the parent serves");
            assert_eq!(grown.dimensions(), (8, 8));
            assert!(
                grown.pixels().all(|pixel| pixel.0[0] == want),
                "level 0 ({x}, {z}) took the wrong quarter"
            );
        }
    }

    #[test]
    fn a_gap_of_more_than_one_level_is_walked_past() {
        // Nothing at level 1, so the answer has to come from level 2 — and from
        // the sixteenth of it this tile actually covers.
        let at = Scratch::new("pyramid-gap");
        let grandparent = tile(8, |x, z| [u8::try_from(z * 8 + x).unwrap(), 0, 0]);
        write(at.at(), 2, 1, 1, &grandparent).expect("the grandparent stores");

        // Level 2 tile (1, 1) covers level 0 tiles (4..=7); (5, 6) is one across
        // and two down inside it, so the 2x2 patch at (2, 4), each pixel grown
        // fourfold.
        let grown = from_above(|l, x, z| read(at.at(), l, x, z), 0, 5, 6, 8, 3).expect("the grandparent serves");
        assert_eq!(grown.get_pixel(0, 0).0[0], grandparent.get_pixel(2, 4).0[0]);
        assert_eq!(grown.get_pixel(7, 7).0[0], grandparent.get_pixel(3, 5).0[0]);
    }

    #[test]
    fn a_negative_tile_takes_the_right_quarter_too() {
        // The quarter comes from subtracting the ancestor's origin, which is the
        // step that goes wrong when a coordinate is negative.
        let at = Scratch::new("pyramid-negative");
        write(at.at(), 1, -1, -1, &quarters(8)).expect("the parent stores");

        // Level 1 tile (-1, -1) covers level 0 tiles (-2, -1) in both axes.
        for (x, z, want) in [(-2, -2, 0u8), (-1, -2, 1), (-2, -1, 2), (-1, -1, 3)] {
            let grown = from_above(|l, x, z| read(at.at(), l, x, z), 0, x, z, 8, 3).expect("the parent serves");
            assert_eq!(grown.get_pixel(0, 0).0[0], want, "level 0 ({x}, {z})");
        }
    }

    /// A tile whose every pixel is a known function of its position.
    fn tile(size: u32, of: impl Fn(u32, u32) -> [u8; 3]) -> RgbImage {
        RgbImage::from_fn(size, size, |x, z| image::Rgb(of(x, z)))
    }

    fn flat(size: u32, value: u8) -> RgbImage {
        tile(size, |_, _| [value, value, value])
    }

    #[test]
    fn a_tile_of_one_colour_downsamples_to_that_colour() {
        let children = [
            Some(flat(8, 40)),
            Some(flat(8, 40)),
            Some(flat(8, 40)),
            Some(flat(8, 40)),
        ];
        let parent = downsample(&children, 8, BLANK);
        assert!(parent.pixels().all(|pixel| pixel.0 == [40, 40, 40]));
    }

    #[test]
    fn each_child_lands_in_its_own_quarter() {
        let children = [
            Some(flat(8, 10)),
            Some(flat(8, 20)),
            Some(flat(8, 30)),
            Some(flat(8, 40)),
        ];
        let parent = downsample(&children, 8, BLANK);

        assert_eq!(parent.get_pixel(0, 0).0[0], 10, "top left");
        assert_eq!(parent.get_pixel(7, 0).0[0], 20, "top right");
        assert_eq!(parent.get_pixel(0, 7).0[0], 30, "bottom left");
        assert_eq!(parent.get_pixel(7, 7).0[0], 40, "bottom right");
    }

    #[test]
    fn a_missing_child_leaves_its_quarter_blank() {
        let children = [Some(flat(8, 90)), None, None, None];
        let parent = downsample(&children, 8, Rgb::new(1, 2, 3));

        assert_eq!(parent.get_pixel(0, 0).0, [90, 90, 90], "the child that exists");
        assert_eq!(parent.get_pixel(7, 0).0, [1, 2, 3], "the three that do not");
        assert_eq!(parent.get_pixel(0, 7).0, [1, 2, 3]);
        assert_eq!(parent.get_pixel(7, 7).0, [1, 2, 3]);
    }

    #[test]
    fn one_pixel_is_the_average_of_the_four_below_it() {
        // Distinct values in one 2x2 group, so an average is the only answer that
        // could be right — a copy or a sample would give one of the four.
        let child = tile(4, |x, z| match (x, z) {
            (0, 0) => [0, 0, 0],
            (1, 0) => [10, 20, 30],
            (0, 1) => [20, 40, 60],
            (1, 1) => [30, 60, 90],
            _ => [0, 0, 0],
        });
        let parent = downsample(&[Some(child), None, None, None], 4, BLANK);
        assert_eq!(parent.get_pixel(0, 0).0, [15, 30, 45]);
    }

    /// The claim the whole design rests on.
    ///
    /// Levels are built two by two from the level below rather than by averaging
    /// `2^L` blocks straight out of the world, because the second costs `4^L`
    /// times as much. That is only sound if the answers agree.
    #[test]
    fn averaging_twice_by_two_equals_averaging_once_by_four() {
        let size = 8;
        let source = tile(size * 2, |x, z| {
            let value = ((x * 7 + z * 13) % 64) as u8 * 4;
            [value, value / 2, 255 - value]
        });

        // Two by two, twice: sixteen pixels of source into one, via four.
        let once = downsample(&[Some(source.clone()), None, None, None], size * 2, BLANK);
        let twice = downsample(&[Some(once), None, None, None], size * 2, BLANK);

        // Straight from the source, four by four into one.
        for pz in 0..size / 2 {
            for px in 0..size / 2 {
                let mut total = [0u32; 3];
                for dz in 0..4 {
                    for dx in 0..4 {
                        let pixel = source.get_pixel(px * 4 + dx, pz * 4 + dz).0;
                        for channel in 0..3 {
                            total[channel] += u32::from(pixel[channel]);
                        }
                    }
                }

                let stepwise = twice.get_pixel(px, pz).0;
                for channel in 0..3 {
                    let direct = (total[channel] / 16) as i32;
                    let difference = i32::from(stepwise[channel]) - direct;
                    assert!(
                        difference.abs() <= 1,
                        "pixel ({px}, {pz}) channel {channel}: two steps gave {}, one gives {direct}",
                        stepwise[channel]
                    );
                }
            }
        }
    }

    #[test]
    fn a_world_gets_a_level_for_every_halving_it_needs() {
        assert_eq!(levels_for(1, 1), 0, "one tile is already the whole world");
        assert_eq!(levels_for(2, 2), 1);
        assert_eq!(levels_for(3, 3), 2, "three needs four");
        assert_eq!(levels_for(4, 4), 2);
        assert_eq!(levels_for(5, 1), 3, "the widest side decides");
        assert_eq!(levels_for(1, 5), 3);
        assert_eq!(levels_for(0, 0), 0, "an empty world needs nothing");
        // A full size Vintage Story world, 1,024,000 blocks at 512 to a tile.
        assert_eq!(levels_for(2000, 2000), 11);
    }

    #[test]
    fn a_tile_is_the_ancestor_of_all_four_below_it() {
        for (x, z) in [(0, 0), (3, 7), (-1, -1), (-9, 4)] {
            for (cx, cz) in children(x, z) {
                assert_eq!(ancestor(1, cx, cz), (x, z), "child ({cx}, {cz}) of ({x}, {z})");
            }
        }
    }

    #[test]
    fn ancestry_holds_however_many_levels_up() {
        // Level 0 tile (5, 9) must be inside the same tile whichever way it is
        // reached: straight to level 3, or one level at a time.
        let (mut x, mut z) = (5, 9);
        for _ in 0..3 {
            (x, z) = ancestor(1, x, z);
        }
        assert_eq!((x, z), ancestor(3, 5, 9));
    }

    #[test]
    fn negative_tiles_floor_into_their_ancestor() {
        assert_eq!(ancestor(1, -1, -1), (-1, -1));
        assert_eq!(ancestor(1, -2, -2), (-1, -1));
        assert_eq!(ancestor(1, -3, -3), (-2, -2));
    }

    /// A region exported far enough in the past that anything built now is newer.
    fn exported_before(when: SystemTime) -> SystemTime {
        when - std::time::Duration::from_secs(60)
    }

    #[test]
    fn a_region_whose_levels_are_all_current_is_not_behind() {
        let at = Scratch::new("pyramid-current");
        let built = build(&at, 1, 0, 0);
        build(&at, 2, 0, 0);
        build(&at, 3, 0, 0);

        let regions = HashMap::from([((0, 0), exported_before(built))]);
        assert!(behind(at.at(), &regions, 3).is_empty());
    }

    #[test]
    fn a_region_written_since_its_level_was_built_is_behind() {
        let at = Scratch::new("pyramid-stale");
        let built = build(&at, 1, 0, 0);
        build(&at, 2, 0, 0);

        let regions = HashMap::from([((0, 0), built + std::time::Duration::from_secs(60))]);
        assert_eq!(behind(at.at(), &regions, 2), vec![(0, 0)]);
    }

    /// The bug this exists for.
    ///
    /// A world that grows past a power of two gains a coarsest level. Only the
    /// tiles above whatever changed in that moment get built, so every other tile
    /// at that level stays missing — and that level is the one a viewer opens on,
    /// which read as a map that would not load until something recoloured the lot.
    /// Looking only at level 1, as this once did, cannot see it: level 1 is
    /// current, and the map is still empty.
    #[test]
    fn a_level_that_has_never_been_built_leaves_its_regions_behind() {
        let at = Scratch::new("pyramid-grown");
        let built = build(&at, 1, 0, 0);
        build(&at, 2, 0, 0);
        let regions = HashMap::from([((0, 0), exported_before(built))]);

        assert!(behind(at.at(), &regions, 2).is_empty(), "two levels is current");
        assert_eq!(
            behind(at.at(), &regions, 3),
            vec![(0, 0)],
            "a third level exists nowhere on disk"
        );
    }

    #[test]
    fn the_pyramid_is_as_tall_as_the_levels_it_has_written() {
        let at = Scratch::new("pyramid-height");
        assert_eq!(levels_built(at.at()), 0, "nothing built yet");

        build(&at, 1, 0, 0);
        assert_eq!(levels_built(at.at()), 1);

        build(&at, 3, 0, 0);
        assert_eq!(levels_built(at.at()), 3, "the tallest, not the count");
    }

    #[test]
    fn the_stamp_is_not_mistaken_for_a_level() {
        let at = Scratch::new("pyramid-stamp");
        reset_unless_built_from(at.at(), 4);
        assert_eq!(levels_built(at.at()), 0, "built-by is not a number");
    }

    #[test]
    fn tiles_are_bucketed_so_no_directory_holds_too_many() {
        let dir = Path::new("/exports");
        let holding = |x, z| path(dir, 0, x, z).parent().unwrap().to_owned();

        let bucket = holding(0, 0);
        assert_eq!(holding(31, 31), bucket, "the last tile of a bucket shares it");
        assert_ne!(holding(32, 0), bucket, "the next one starts another");
        assert_ne!(holding(0, 32), bucket);
        assert_ne!(holding(-1, 0), bucket, "negatives get their own");

        assert!(path(dir, 3, -1, -1).ends_with("tiles/3/-1_-1/-1_-1.png"));
    }
}
