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

use std::path::{Path, PathBuf};

use image::RgbImage;

use crate::color::Rgb;
use crate::error::{Error, Result};

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
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| Error::io(format!("creating {}", parent.display()), error))?;
    }

    let temporary = target.with_extension("part");
    let mut encoded = Vec::new();
    image
        .write_to(&mut std::io::Cursor::new(&mut encoded), image::ImageFormat::Png)
        .map_err(|error| Error::io("encoding a tile", std::io::Error::other(error.to_string())))?;

    std::fs::write(&temporary, &encoded)
        .and_then(|()| std::fs::rename(&temporary, &target))
        .map_err(|error| Error::io(format!("writing {}", target.display()), error))
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

/// Throws away every level if it was built from a region format this build no
/// longer reads. Returns whether it did.
pub fn reset_unless_built_from(exports: &Path, version: u16) -> bool {
    let want = version.to_string();
    if std::fs::read_to_string(stamp(exports)).is_ok_and(|found| found.trim() == want) {
        return false;
    }

    let cleared = tiles_dir(exports).exists();
    let _ = std::fs::remove_dir_all(tiles_dir(exports));
    let _ = std::fs::create_dir_all(tiles_dir(exports));
    let _ = std::fs::write(stamp(exports), want);
    cleared
}

/// Whether a level 0 tile has been carried up into the level above it.
///
/// Compares what the region was written against what was built from it, so a run
/// that starts with its levels already current has nothing to do — and one that
/// does not rebuilds only what moved while it was away.
#[must_use]
pub fn is_current(exports: &Path, region: &Path, x: i32, z: i32) -> bool {
    let when = |path: &Path| std::fs::metadata(path).ok().and_then(|m| m.modified().ok());
    let Some(built) = when(&path(exports, 1, x >> 1, z >> 1)) else {
        return false;
    };
    when(region).is_some_and(|exported| built >= exported)
}

#[cfg(test)]
mod tests {
    use super::*;

    const BLANK: Rgb = Rgb::new(0, 0, 0);

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
