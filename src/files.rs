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

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let at = std::env::temp_dir().join(format!("witchlight-files-{name}"));
        let _ = std::fs::remove_dir_all(&at);
        at
    }

    #[test]
    fn a_write_lands_and_the_temporary_does_not_stay() {
        let at = scratch("lands");
        let path = at.join("deep").join("service.json");
        replace(&path, b"{}").expect("the directories are made on the way");

        assert_eq!(std::fs::read_to_string(&path).unwrap(), "{}");
        assert!(!beside(&path).exists(), "nothing is left beside it");
        let _ = std::fs::remove_dir_all(&at);
    }

    #[test]
    fn two_files_differing_only_in_extension_do_not_share_a_temporary() {
        // `with_extension` would name both of these `r.0.0.part`, so a tile
        // being written could clobber a region being written next to it.
        let at = scratch("extensions");
        assert_ne!(beside(&at.join("r.0.0.msqr")), beside(&at.join("r.0.0.png")));
    }

    #[test]
    fn writing_again_replaces_what_was_there() {
        let at = scratch("replaces");
        let path = at.join("markers.json");
        replace(&path, b"first").expect("a first write");
        replace(&path, b"second").expect("and a second");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "second");
        let _ = std::fs::remove_dir_all(&at);
    }

    #[test]
    #[cfg(unix)]
    fn a_private_file_is_readable_by_nobody_else() {
        use std::os::unix::fs::PermissionsExt as _;
        let at = scratch("private");
        let path = at.join("api.json");
        replace_private(&path, b"{}").expect("a private write");

        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o077, 0, "the token must not be group or world readable");
        let _ = std::fs::remove_dir_all(&at);
    }
}
