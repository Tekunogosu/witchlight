//! Atomic file writes.
//!
//! Everything this service publishes is read by something else while the service
//! runs. The mod reads `api.json`, a browser reads a tile, the next start reads
//! back the markers. Every write here goes to a temporary file next to the
//! target and is then renamed into place, so a reader never sees a half-written
//! file.

use crate::util::log::warn;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// Writes `body` where `path` is, atomically. Any directories are made first.
pub fn replace(path: &Path, body: &[u8]) -> std::io::Result<()> {
    write_with(path, body, |target| std::fs::File::create(target))
}

/// Writes `body` where `path` is, atomically, as a file only its owner may read.
///
/// The API token is all that stands between the write endpoint and anything else
/// on the machine, so the mode is set explicitly rather than left to the umask.
/// Windows has no mode bits and the file inherits the directory's permissions.
pub fn replace_private(path: &Path, body: &[u8]) -> std::io::Result<()> {
    write_with(path, body, |target| {
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create(true).truncate(true);

        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt as _;
            options.mode(0o600);
        }

        options.open(target)
    })
}

fn write_with(
    path: &Path,
    body: &[u8],
    open: impl FnOnce(&Path) -> std::io::Result<std::fs::File>,
) -> std::io::Result<()> {
    if let Some(parent) = path.parent().filter(|parent| !parent.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)?;
    }

    let temporary = beside(path);
    open(&temporary)?.write_all(body)?;
    match std::fs::rename(&temporary, path) {
        Ok(()) => Ok(()),
        Err(error) => {
            // A failed rename leaves the temporary file behind, and the next
            // attempt would find it in the way.
            let _ = std::fs::remove_file(&temporary);
            Err(error)
        }
    }
}

/// Returns the path of the temporary file used while writing `path`.
///
/// The suffix is appended rather than set with `with_extension`, which replaces
/// any extension already there. `r.0.0.msqr` and `r.0.0.png` would both become
/// `r.0.0.part`, so two writes in one directory could collide.
fn beside(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(".part");
    PathBuf::from(name)
}

/// Writes `body` where `path` is, and logs a warning if the write fails.
///
/// Callers that publish an unattended file cannot do anything about a failure
/// beyond reporting it. The warning includes the reason, because a refused write
/// with no reason given is a fault nobody can act on.
pub fn publish(path: &Path, body: &[u8]) {
    if let Err(error) = replace(path, body) {
        warn!("could not write {}: {error}", path.display());
    }
}

/// Returns when a file was last written, or `None` if there is no such file.
#[must_use]
pub fn modified(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).ok()?.modified().ok()
}

/// Returns the sorted contents of a directory, or an empty list if the directory
/// does not exist.
///
/// A missing directory is not a fault. The regions, the colour maps and the
/// marker pictures are all created by the mod on its first export, so a map
/// served before that has happened is empty rather than broken. Any other error
/// is returned, so an unreadable directory does not read as an empty one.
///
/// The result is sorted because a directory listing comes back in filesystem
/// order and every caller here needs a stable one.
pub fn listing(dir: &Path) -> std::io::Result<Vec<PathBuf>> {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error),
    };

    let mut paths: Vec<PathBuf> = entries.flatten().map(|entry| entry.path()).collect();
    paths.sort();
    Ok(paths)
}

/// A scratch directory for one test.
///
/// It is emptied on creation so a previous run cannot answer for this one, and
/// removed again on drop.
#[cfg(test)]
pub mod testing {
    use std::path::{Path, PathBuf};

    pub struct Scratch(PathBuf);

    impl Scratch {
        /// `name` only has to differ from every other test's.
        #[must_use]
        pub fn new(name: &str) -> Self {
            let at = std::env::temp_dir().join(format!("witchlight-{name}"));
            let _ = std::fs::remove_dir_all(&at);
            std::fs::create_dir_all(&at).expect("a scratch directory");
            Self(at)
        }

        #[must_use]
        pub fn at(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::testing::Scratch;
    use super::*;

    #[test]
    fn a_write_lands_and_the_temporary_does_not_stay() {
        let held = Scratch::new("files-lands");
        let path = held.at().join("deep").join("service.json");
        replace(&path, b"{}").expect("the directories are made on the way");

        assert_eq!(std::fs::read_to_string(&path).unwrap(), "{}");
        assert!(!beside(&path).exists(), "nothing is left beside it");
    }

    #[test]
    fn two_files_differing_only_in_extension_do_not_share_a_temporary() {
        // `with_extension` would name both of these `r.0.0.part`, so a tile
        // being written could clobber a region being written next to it.
        let held = Scratch::new("files-extensions");
        let at = held.at();
        assert_ne!(beside(&at.join("r.0.0.msqr")), beside(&at.join("r.0.0.png")));
    }

    #[test]
    fn writing_again_replaces_what_was_there() {
        let held = Scratch::new("files-replaces");
        let path = held.at().join("markers.json");
        replace(&path, b"first").expect("a first write");
        replace(&path, b"second").expect("and a second");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "second");
    }

    #[test]
    #[cfg(unix)]
    fn a_private_file_is_readable_by_nobody_else() {
        use std::os::unix::fs::PermissionsExt as _;
        let held = Scratch::new("files-private");
        let path = held.at().join("api.json");
        replace_private(&path, b"{}").expect("a private write");

        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o077, 0, "the token must not be group or world readable");
    }
}
