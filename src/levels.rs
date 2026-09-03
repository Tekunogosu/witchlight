//! The zoom levels above the finest, owned in one place whether they are in
//! memory or on disk.
//!
//! A level tile is a picture derived from the ground — see [`crate::pyramid`]
//! for the shape of the pyramid — and the ground arrives a few chunks at a
//! time. Written straight to disk, a tile over ground that is filling in is
//! written again on every build beat, a few hundred times an hour, for a
//! picture whose final form is the only one anybody will keep. This holds the
//! tile the builder made and writes it later: once it has been quiet for a
//! while, or once it has been waiting long enough, whichever comes first. A
//! walk that rebuilds the same tile every two seconds costs the disk one write
//! per quiet spell instead of one per beat.
//!
//! Every reader of a level tile asks here, so what is in memory is what is
//! served and what the next level up is built from; the file is the same
//! picture, later. A tile lost with the process before it was written is not
//! lost ground — the ground is in the database — and is built again on the
//! next start, which compares each region's last change against what its
//! level tiles' files say and marks anything behind. See `settle` in
//! [`crate::server`].
//!
//! Write-behind, in the storage sense of the word: reads are answered from
//! memory first and the disk second, writes land in memory and reach the disk
//! on a clock of their own.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, SystemTime};

use image::RgbImage;

use crate::cache::At;
use crate::error::{Error, Result};
use crate::log::warn;
use crate::pyramid;

/// How long a tile is left alone after its last rebuild before it is written.
/// Ground filling in under a tile rebuilds it every beat; a tile nobody has
/// rebuilt for this long is one the walk has moved past.
pub const QUIET_FOR: Duration = Duration::from_secs(30);

/// The longest a tile waits unwritten however often it is rebuilt, so that a
/// walk that never stops still leaves a bounded backlog for the next start.
pub const DIRTY_FOR: Duration = Duration::from_secs(5 * 60);

/// How many tiles may wait in memory. Each is three quarters of a megabyte
/// decoded; past this the one waiting longest is written early.
const MOST_HELD: usize = 64;

/// One tile the builder made and the disk has not seen yet.
struct Built {
    image: RgbImage,
    /// When the builder last made it, which is what the quiet period counts from
    /// and what says whether a flush in flight is still current.
    built: SystemTime,
    /// When it first went unwritten, which is what the ceiling counts from.
    dirty_since: SystemTime,
}

pub struct Levels {
    exports: PathBuf,
    held: Mutex<HashMap<At, Built>>,
}

impl Levels {
    #[must_use]
    pub fn new(exports: &Path) -> Self {
        Self { exports: exports.to_path_buf(), held: Mutex::new(HashMap::new()) }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<At, Built>> {
        self.held.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Takes a tile the builder has just made. It is served and built from at
    /// once, and written once it has been quiet — see [`flush`](Self::flush).
    pub fn put(&self, at: At, image: RgbImage, now: SystemTime) {
        let early = {
            let mut held = self.lock();
            let dirty_since = held.get(&at).map_or(now, |was| was.dirty_since);
            held.insert(at, Built { image, built: now, dirty_since });
            if held.len() > MOST_HELD {
                held.iter().min_by_key(|(_, built)| built.dirty_since).map(|(&at, _)| at)
            } else {
                None
            }
        };
        if let Some(at) = early {
            self.write(&[at]);
        }
    }

    /// One tile as a picture, from memory or else from disk, or nothing where
    /// neither has it.
    #[must_use]
    pub fn image(&self, (level, x, z): At) -> Option<RgbImage> {
        if let Some(built) = self.lock().get(&(level, x, z)) {
            return Some(built.image.clone());
        }
        pyramid::read(&self.exports, level, x, z)
    }

    /// One tile as PNG bytes, for serving: encoded from what is in memory, or
    /// else the file as it lies. A tile neither holds is missing rather than
    /// broken, which is what a viewer draws around.
    pub fn bytes(&self, (level, x, z): At) -> Result<Vec<u8>> {
        if let Some(built) = self.lock().get(&(level, x, z)) {
            return pyramid::encode(&built.image);
        }
        let path = pyramid::path(&self.exports, level, x, z);
        std::fs::read(&path).map_err(|error| match error.kind() {
            std::io::ErrorKind::NotFound => Error::Empty(format!("level {level} tile ({x}, {z}) is not built yet")),
            _ => Error::io(format!("reading {}", path.display()), error),
        })
    }

    /// The best stored picture of a tile's ground taken from a level above it —
    /// [`pyramid::from_above`], read through here so a level still in memory
    /// counts.
    #[must_use]
    pub fn from_above(&self, level: u32, x: i32, z: i32, size: u32, ceiling: u32) -> Option<RgbImage> {
        pyramid::from_above(|level, x, z| self.image((level, x, z)), level, x, z, size, ceiling)
    }

    /// How tall the pyramid is: the tallest level on disk or in memory. A
    /// level that exists only in memory is a level all the same, and the
    /// builder asking on every beat must not be told to make it again.
    #[must_use]
    pub fn built(&self) -> u32 {
        let in_memory = self.lock().keys().map(|(level, _, _)| *level).max().unwrap_or(0);
        in_memory.max(pyramid::levels_built(&self.exports))
    }

    /// How many tiles are waiting to be written, for the log.
    #[must_use]
    pub fn waiting(&self) -> usize {
        self.lock().len()
    }

    /// Writes every tile that has been quiet for [`QUIET_FOR`] or unwritten for
    /// [`DIRTY_FOR`], and lets go of it. Says how many were written.
    pub fn flush(&self, now: SystemTime) -> usize {
        let due: Vec<At> = self
            .lock()
            .iter()
            .filter(|(_, built)| {
                let quiet = now.duration_since(built.built).unwrap_or_default() >= QUIET_FOR;
                let overdue = now.duration_since(built.dirty_since).unwrap_or_default() >= DIRTY_FOR;
                quiet || overdue
            })
            .map(|(&at, _)| at)
            .collect();
        self.write(&due)
    }

    /// Writes these tiles and forgets them — unless one was rebuilt while its
    /// picture was on the way to disk, in which case the newer picture stays
    /// and waits its own turn. The lock is not held across the write: a
    /// picture is copied out, written, and only then compared and let go.
    fn write(&self, tiles: &[At]) -> usize {
        let mut written = 0;
        for &at in tiles {
            let Some((image, built)) = self.lock().get(&at).map(|held| (held.image.clone(), held.built)) else {
                continue;
            };
            let (level, x, z) = at;
            if let Err(error) = pyramid::write(&self.exports, level, x, z, &image) {
                warn!("{error}");
                continue;
            }
            written += 1;
            let mut held = self.lock();
            if held.get(&at).is_some_and(|now| now.built == built) {
                held.remove(&at);
            }
        }
        written
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::files::testing::Scratch;

    fn flat(value: u8) -> RgbImage {
        RgbImage::from_pixel(4, 4, image::Rgb([value, value, value]))
    }

    fn on_disk(held: &Scratch, at: At) -> bool {
        pyramid::path(held.at(), at.0, at.1, at.2).exists()
    }

    #[test]
    fn a_tile_put_is_served_at_once_and_written_only_once_quiet() {
        let held = Scratch::new("levels-quiet");
        let levels = Levels::new(held.at());
        let start = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000);

        levels.put((1, 2, 3), flat(9), start);
        assert!(!on_disk(&held, (1, 2, 3)), "nothing is written on a put");
        assert_eq!(levels.image((1, 2, 3)).unwrap().get_pixel(0, 0).0[0], 9, "served from memory");
        assert!(levels.bytes((1, 2, 3)).is_ok());
        assert_eq!(levels.built(), 1, "a level in memory is a level");

        assert_eq!(levels.flush(start + QUIET_FOR / 2), 0, "not quiet yet");
        assert!(!on_disk(&held, (1, 2, 3)));

        assert_eq!(levels.flush(start + QUIET_FOR), 1);
        assert!(on_disk(&held, (1, 2, 3)));
        assert_eq!(levels.waiting(), 0, "written and let go");
        assert_eq!(levels.image((1, 2, 3)).unwrap().get_pixel(0, 0).0[0], 9, "now read from disk");
        assert_eq!(levels.built(), 1);
    }

    #[test]
    fn a_tile_rebuilt_every_beat_is_written_at_the_ceiling() {
        let held = Scratch::new("levels-ceiling");
        let levels = Levels::new(held.at());
        let start = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000);
        let beat = Duration::from_secs(2);

        let mut now = start;
        let mut written = 0;
        while now < start + DIRTY_FOR + beat {
            levels.put((1, 0, 0), flat(1), now);
            written += levels.flush(now);
            now += beat;
        }
        assert_eq!(written, 1, "one write for a walk that never paused");
        assert!(on_disk(&held, (1, 0, 0)));
    }

    #[test]
    fn memory_wins_over_disk_and_a_rebuild_during_a_write_is_kept() {
        let held = Scratch::new("levels-newer");
        let levels = Levels::new(held.at());
        let start = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000);

        pyramid::write(held.at(), 1, 0, 0, &flat(1)).unwrap();
        assert_eq!(levels.image((1, 0, 0)).unwrap().get_pixel(0, 0).0[0], 1, "from disk");
        levels.put((1, 0, 0), flat(2), start);
        assert_eq!(levels.image((1, 0, 0)).unwrap().get_pixel(0, 0).0[0], 2, "memory wins");

        // Rebuilt after its quiet period elapsed but before the flush ran: the
        // flush writes the picture it finds and keeps it, since it is the one
        // that was built last.
        levels.put((1, 0, 0), flat(3), start + QUIET_FOR);
        assert_eq!(levels.flush(start + QUIET_FOR), 0, "the rebuild reset the quiet period");
        assert_eq!(levels.flush(start + QUIET_FOR * 2), 1);
        assert_eq!(pyramid::read(held.at(), 1, 0, 0).unwrap().get_pixel(0, 0).0[0], 3);
    }

    #[test]
    fn past_the_cap_the_longest_waiting_tile_is_written_early() {
        let held = Scratch::new("levels-cap");
        let levels = Levels::new(held.at());
        let start = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000);

        for index in 0..=MOST_HELD as i32 {
            levels.put((1, index, 0), flat(1), start + Duration::from_secs(index as u64));
        }
        assert!(on_disk(&held, (1, 0, 0)), "the first put went first");
        assert!(!on_disk(&held, (1, 1, 0)));
        assert_eq!(levels.waiting(), MOST_HELD);
    }
}
