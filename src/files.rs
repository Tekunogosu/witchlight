//! Writing a file nobody can catch half-written.
//!
//! Everything this service publishes is read by something else while it runs —
//! the mod reads `api.json`, a browser reads a tile, the next start reads back
//! the markers — so every write goes beside itself and is renamed into place.
//! That was spelled out at five call sites, which is five chances to leave one
//! out; it is one function here and every caller is a thin call against it.

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// Writes `body` where `path` is, atomically. Any directories are made first.
pub fn replace(path: &Path, body: &[u8]) -> std::io::Result<()> {
    write_with(path, body, |target| std::fs::File::create(target))
}

/// The same, for a file only its owner may read.
///
/// The API token is the whole of what stands between the write endpoint and
/// anything else on the machine, so it is not left at whatever the umask
/// happened to be. Windows has no mode bits and the file inherits the
/// directory's permissions, which is the answer a unix socket in a private
/// directory would have given.
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
            // A rename that failed leaves the half-written copy behind, and the
            // next attempt would find it in the way.
            let _ = std::fs::remove_file(&temporary);
            Err(error)
        }
    }
}

/// Where the half-written copy goes.
///
/// Appended rather than `with_extension`, which replaces whatever extension is
/// already there: `r.0.0.msqr` and `r.0.0.png` would both become `r.0.0.part`,
/// so two writes in one directory could land on the same temporary file.
fn beside(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(".part");
    PathBuf::from(name)
}

/// When a file was last written, or nothing where there is no such file.
#[must_use]
pub fn modified(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).ok()?.modified().ok()
}

/// What is in a directory, sorted, or nothing where the directory is not there.
///
/// A directory the mod has not written yet is not a fault. Every one of these the
/// service reads — the regions, the colour maps, the marker pictures — is made by
/// the other half when it first exports, and a map being served before that has
/// happened is a map with nothing in it rather than a map that failed. Three
/// callers each said so in their own words, one of them differently enough that
/// an unreadable directory read as an empty one.
///
/// Sorted, because a directory listing is in whatever order the filesystem hands
/// it back and every caller here wants a settled one.
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

/// A directory of one test's own.
///
/// Three modules had written this out, each with its own spelling of "empty it
/// first so a previous run cannot answer for this one, and take it away again
/// afterwards". It is one type here, and what each of them did with the directory
/// stays where it was.
#[cfg(test)]
pub mod testing {
    use std::path::{Path, PathBuf};

    pub struct Scratch(PathBuf);

    impl Scratch {
        /// `name` only has to be different from every other test's.
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
