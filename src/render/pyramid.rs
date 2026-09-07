//! Zoom levels.
//!
//! A tile is always [`TILE`] pixels square. Level 0 draws one block per pixel.
//! Every level above draws twice as many blocks per pixel as the one below, so
//! level `L` covers `TILE * 2^L` blocks and a view holds roughly the same number
//! of tiles however far out it is. Without that, the number of tiles on screen
//! grows as the square of the zoom distance and a wide view asks for tens of
//! thousands.
//!
//! Levels are numbered from the finest. Level 0 is one block per pixel whatever
//! anyone has explored, so a world that grows gains new coarser numbers rather
//! than renumbering everything already written.
//!
//! ```text
//! tiles/{level}/{x >> 5}_{z >> 5}/{x}_{z}.png
//! ```
//!
//! The middle directory holds a thousand tiles at most. A large world has
//! millions of tiles, and a directory of millions of files is slow to open on
//! every filesystem.
//!
//! Level 0 is not stored. It is rendered from the world on demand, which is fast
//! and already cached in memory. Storing it would mean four million files for a
//! full world against a few thousand for everything above it.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use image::codecs::png::{CompressionType, FilterType, PngEncoder};
use image::{ExtendedColorType, ImageEncoder, RgbImage};

use crate::render::color::Rgb;
use crate::util::error::{Error, Result};
use crate::util::files;

/// Blocks per tile, and pixels per tile at the finest level, where one pixel is
/// one block. This equals a region, so a region that changes is exactly one
/// tile.
pub const TILE: u32 = 512;

/// Tiles per directory along each axis. A thousand files to a directory.
const BUCKET: i32 = 5;

/// Returns the directory the levels live in inside the export directory.
#[must_use]
pub fn tiles_dir(exports: &Path) -> PathBuf {
    exports.join("tiles")
}

/// Returns the path of one tile's file.
#[must_use]
pub fn path(exports: &Path, level: u32, x: i32, z: i32) -> PathBuf {
    tiles_dir(exports)
        .join(level.to_string())
        .join(format!("{}_{}", x >> BUCKET, z >> BUCKET))
        .join(format!("{x}_{z}.png"))
}

/// Returns how many levels a world of this many tiles across needs.
///
/// This is enough that the coarsest level holds the whole world in one tile, and
/// no more. A level nobody can zoom out far enough to see is not worth
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

/// Returns the tile at `level` that holds the level 0 tile `(x, z)`.
#[must_use]
pub fn ancestor(level: u32, x: i32, z: i32) -> (i32, i32) {
    (x >> level, z >> level)
}

/// Returns the four tiles one level down that a tile is made of, in reading
/// order.
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
/// Pixels are averaged rather than sampled. Taking every other pixel is cheaper,
/// but on a map where one pixel is one block it erases everything narrower than
/// the step, such as paths, walls and rivers. They reappear as the viewer zooms
/// in, which reads as the map being wrong rather than coarse.
///
/// Building each level from the level below rather than from the world keeps a
/// coarse tile affordable. Averaging `2^L` blocks per pixel straight from the
/// world costs `4^L` times a level 0 tile, which at level 5 is a thousand times.
/// Halving repeatedly gives the same average at constant cost.
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

/// Reads a stored tile. Returns `None` if it has not been built.
#[must_use]
pub fn read(exports: &Path, level: u32, x: i32, z: i32) -> Option<RgbImage> {
    let bytes = std::fs::read(path(exports, level, x, z)).ok()?;
    Some(image::load_from_memory(&bytes).ok()?.to_rgb8())
}

/// Writes a tile atomically, so a reader never sees a half-written file.
pub fn write(exports: &Path, level: u32, x: i32, z: i32, image: &RgbImage) -> Result<()> {
    let target = path(exports, level, x, z);
    files::replace(&target, &encode_for_disk(image)?)
        .map_err(|error| Error::io(format!("writing {}", target.display()), error))
}

/// Encodes one tile for the wire, losslessly and quickly.
///
/// A tile is encoded far more often than it is built. Under a private map every
/// reader is served their own composition of it, so a busy server pays this cost
/// a dozen times a second. Speed therefore matters more here than size.
///
/// The fast compression level needs the adaptive filter. Without a filter this
/// crate's fast path stores shaded terrain close to raw. Measured on one mapped
/// tile: 371 KB in 24 ms at the default level unfiltered, 487 KB in 1.6 ms fast
/// and adaptively filtered, 768 KB fast and unfiltered.
///
/// The encoding keeps every colour. A tile is continuous tone, because slope
/// shading over climate tinting gives thousands of shades to a square of
/// terrain, so an indexed format would drop the rare colours: ore, water edges
/// and anything small. A reader who wants fewer bytes asks for
/// [`TileFormat::Jpeg`].
pub fn encode(image: &RgbImage) -> Result<Vec<u8>> {
    encode_png(image, CompressionType::Fast, FilterType::Adaptive)
}

/// Encodes one tile for the disk, as small as the lossless encoder makes it.
///
/// A file is written once and read many times, so the encoding time is worth
/// spending here. A stored level is served as the file's own bytes, so bytes
/// saved here are saved on the wire for every reader. The encoding is
/// unfiltered, which is what the deflate levels do best with on this data. The
/// encoder stamp names this setting, so a change here rebuilds the levels.
pub fn encode_for_disk(image: &RgbImage) -> Result<Vec<u8>> {
    encode_png(image, CompressionType::Default, FilterType::NoFilter)
}

fn encode_png(image: &RgbImage, level: CompressionType, filter: FilterType) -> Result<Vec<u8>> {
    let mut encoded = Vec::new();
    PngEncoder::new_with_quality(&mut encoded, level, filter)
        .write_image(
            image.as_raw(),
            image.width(),
            image.height(),
            ExtendedColorType::Rgb8,
        )
        .map_err(|error| Error::io("encoding a tile", std::io::Error::other(error.to_string())))?;
    Ok(encoded)
}

/// How a tile is encoded for one reader.
///
/// PNG is exact, so every block keeps its own colour, which is what finding a
/// single ore pixel needs. Shaded terrain is noise to a lossless encoder, so a
/// tile stays tens to hundreds of kilobytes however hard it is squeezed. JPEG
/// produces the same picture in about a fifth of the bytes and blurs lone
/// blocks. The reader chooses between them in their settings.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TileFormat {
    #[default]
    Png,
    Jpeg,
}

/// The JPEG quality a lossy tile is encoded at. Terrain reads as it does in PNG
/// at a glance. The blur shows in single blocks and hard edges.
const JPEG_QUALITY: u8 = 85;

impl TileFormat {
    /// Returns the MIME type for the response.
    #[must_use]
    pub fn mime(self) -> &'static str {
        match self {
            Self::Png => "image/png",
            Self::Jpeg => "image/jpeg",
        }
    }

    /// Returns the word a page writes into a tile's address, so a change of
    /// format is a change of address.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Jpeg => "jpeg",
        }
    }
}

/// Encodes one tile in the format a reader asked for. PNG goes through
/// [`encode`].
pub fn encode_as(image: &RgbImage, format: TileFormat) -> Result<Vec<u8>> {
    match format {
        TileFormat::Png => encode(image),
        TileFormat::Jpeg => {
            let mut encoded = Vec::new();
            image::codecs::jpeg::JpegEncoder::new_with_quality(&mut encoded, JPEG_QUALITY)
                .write_image(image.as_raw(), image.width(), image.height(), ExtendedColorType::Rgb8)
                .map_err(|error| Error::io("encoding a tile", std::io::Error::other(error.to_string())))?;
            Ok(encoded)
        }
    }
}

/// Returns the path of the file recording what the levels on disk were built
/// from.
///
/// Tiles are derived from regions, so a region format this build cannot read
/// leaves tiles it must not show. The mod clears the map on such a change, and
/// terrain that no longer exists would otherwise stay on screen at every level
/// above zero. Rebuilding everything on start does not scale, because a large
/// world is millions of level 0 renders, so the levels are kept and thrown away
/// only when what made them changed.
pub fn stamp(exports: &Path) -> PathBuf {
    tiles_dir(exports).join("built-by")
}

/// How this build paints a column. Incremented whenever that changes.
///
/// This is separate from the region format because the two fail differently. A
/// region format the service no longer reads means the ground itself must be
/// exported again. A change here means the same ground is now drawn a different
/// colour, so the stored pictures are out of date but the region files are not.
/// Bumping this discards the pictures and redraws them from the existing region
/// files.
///
/// 1: the season tint replaces part of the climate tint rather than multiplying
///    over it, weighted as the game weights it, and the climate maps are no
///    longer sampled through their border.
pub const PAINT: u16 = 1;

/// Discards every level built from a region format this build no longer reads,
/// or painted by a build that painted differently. Returns true when it did.
pub fn reset_unless_built_from(exports: &Path, version: u16) -> bool {
    // The tile encoding is part of the stamp. Levels written by a different
    // encoder are still readable but are not the size this build would have made
    // them, and nothing else would notice.
    let want = format!("{version}/png-nofilter-deflate/paint{PAINT}");
    if std::fs::read_to_string(stamp(exports)).is_ok_and(|found| found.trim() == want) {
        return false;
    }

    let cleared = tiles_dir(exports).exists();
    let _ = std::fs::remove_dir_all(tiles_dir(exports));
    let _ = files::replace(&stamp(exports), want.as_bytes());
    cleared
}

/// Returns the best stored picture of this tile's ground, enlarged from a level
/// above it.
///
/// The levels above a tile are coarser pictures of the same ground. Enlarging one
/// gives a worse map than the real tile and a better map than no tile. Leaflet
/// does not substitute a parent tile itself, so an unanswerable tile is absent
/// and looks like a broken map.
///
/// This walks up until it finds a level that has this ground, so it survives a
/// gap of more than one level. The enlargement is nearest neighbour, because the
/// pixels are already averages and smoothing them again invents detail that was
/// never there.
///
/// `read` returns a tile at a level, wherever it is held. See
/// [`crate::render::levels`].
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

        // The tile is `up` halvings inside the ancestor, so it occupies one
        // part in `2^up` of each edge.
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

/// Returns the path of the file recording which palette the levels were drawn
/// with.
fn painted_by(exports: &Path) -> PathBuf {
    tiles_dir(exports).join("painted-by")
}

/// Returns the block registry the stored levels were drawn against, if it was
/// recorded.
///
/// This is separate from [`stamp`], which decides whether the levels are readable
/// and deletes them when they are not. This decides only whether they agree with
/// the palette in use, and the response to disagreement is to redraw them rather
/// than delete them. A pyramid drawn with the last good palette is the only thing
/// left to look at when the current one draws nothing.
#[must_use]
pub fn palette_built_from(exports: &Path) -> Option<String> {
    std::fs::read_to_string(painted_by(exports))
        .ok()
        .map(|found| found.trim().to_owned())
        .filter(|found| !found.is_empty())
}

/// Records which palette the levels have been drawn with. Writes nothing when
/// the file already says so, because this is called on every build beat and the
/// answer changes once per palette.
pub fn record_palette(exports: &Path, fingerprint: &str) {
    if fingerprint.is_empty() || palette_built_from(exports).as_deref() == Some(fingerprint) {
        return;
    }
    let _ = files::replace(&painted_by(exports), fingerprint.as_bytes());
}

/// Returns the regions with a stored tile above them that is missing or older
/// than the region itself.
///
/// This checks every level rather than only level 1. A world that grows past a
/// power of two gains a coarsest level. The walk upward from whatever changed
/// builds exactly one tile there, and every other tile at that level has nothing
/// beneath it that changed, so nothing asks for it. That level is the one a
/// viewer opens on, so the map reads as empty.
///
/// Checking every level also means a run that starts with current levels has
/// nothing to do, and one that does not rebuilds only what moved while it was
/// away.
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

/// Returns how tall the stored pyramid is.
///
/// This reads the directories rather than a stored count. A level is a directory
/// named for its number, so the highest of those is what was built.
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
    use crate::util::files::testing::Scratch;

    const BLANK: Rgb = Rgb::new(0, 0, 0);

    /// Writes a tile at `level` and returns when it was written.
    fn build(held: &Scratch, level: u32, x: i32, z: i32) -> SystemTime {
        write(held.at(), level, x, z, &flat(2, 0)).expect("a tile");
        files::modified(&path(held.at(), level, x, z)).expect("a timestamp")
    }

    /// Builds a tile whose four quarters each hold a flat value, so the quarter
    /// a child came from is readable from the result.
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
        // Nothing at level 1, so the answer comes from level 2, and from the
        // sixteenth of it this tile covers.
        let at = Scratch::new("pyramid-gap");
        let grandparent = tile(8, |x, z| [u8::try_from(z * 8 + x).unwrap(), 0, 0]);
        write(at.at(), 2, 1, 1, &grandparent).expect("the grandparent stores");

        // Level 2 tile (1, 1) covers level 0 tiles 4 to 7. Tile (5, 6) is one
        // across and two down inside it, so the 2x2 patch at (2, 4), with each
        // pixel grown fourfold.
        let grown = from_above(|l, x, z| read(at.at(), l, x, z), 0, 5, 6, 8, 3).expect("the grandparent serves");
        assert_eq!(grown.get_pixel(0, 0).0[0], grandparent.get_pixel(2, 4).0[0]);
        assert_eq!(grown.get_pixel(7, 7).0[0], grandparent.get_pixel(3, 5).0[0]);
    }

    #[test]
    fn a_negative_tile_takes_the_right_quarter_too() {
        // The quarter comes from subtracting the ancestor's origin, which is
        // the step that goes wrong when a coordinate is negative.
        let at = Scratch::new("pyramid-negative");
        write(at.at(), 1, -1, -1, &quarters(8)).expect("the parent stores");

        // Level 1 tile (-1, -1) covers level 0 tiles (-2, -1) in both axes.
        for (x, z, want) in [(-2, -2, 0u8), (-1, -2, 1), (-2, -1, 2), (-1, -1, 3)] {
            let grown = from_above(|l, x, z| read(at.at(), l, x, z), 0, x, z, 8, 3).expect("the parent serves");
            assert_eq!(grown.get_pixel(0, 0).0[0], want, "level 0 ({x}, {z})");
        }
    }

    /// Builds a tile whose every pixel is a known function of its position.
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
        // Four distinct values in one 2x2 group, so only an average can give
        // this result. A copy or a sample would give one of the four.
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

    /// Checks the invariant the whole design rests on.
    ///
    /// Levels are built two by two from the level below rather than by averaging
    /// `2^L` blocks straight out of the world, because the second costs `4^L`
    /// times as much. That is only correct if the two agree.
    #[test]
    fn averaging_twice_by_two_equals_averaging_once_by_four() {
        let size = 8;
        let source = tile(size * 2, |x, z| {
            let value = ((x * 7 + z * 13) % 64) as u8 * 4;
            [value, value / 2, 255 - value]
        });

        // Two by two, twice: sixteen source pixels into one, via four.
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
        // Level 0 tile (5, 9) must land in the same tile whichever way it is
        // reached, straight to level 3 or one level at a time.
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

    /// Returns a time far enough in the past that anything built now is newer.
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

    /// A world that grows past a power of two gains a coarsest level. Only the
    /// tiles above whatever changed at that moment get built, so every other tile
    /// at that level stays missing. That level is the one a viewer opens on, so
    /// the map reads as one that will not load. Checking level 1 alone cannot see
    /// this, because level 1 is current while the map is still empty.
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
