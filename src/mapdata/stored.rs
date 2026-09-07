//! Locates exported files that are addressed by name rather than by position.
//!
//! Marker pictures and player portraits are the two exports this service serves
//! by name. This module resolves their paths.
//! [`crate::util::urls::is_stored_name`] validates the name itself.

use std::path::{Path, PathBuf};

/// Returns the directory holding marker pictures.
#[must_use]
pub fn icons_dir(exports: &Path) -> PathBuf {
    exports.join("icons")
}

/// Reads one marker picture by the name a waypoint draws itself with.
#[must_use]
pub fn icon(exports: &Path, name: &str) -> Option<Vec<u8>> {
    std::fs::read(icons_dir(exports).join(format!("{name}.svg"))).ok()
}

/// Returns the directory holding player portraits.
#[must_use]
pub fn portraits_dir(exports: &Path) -> PathBuf {
    exports.join("portraits")
}

/// Reads one player portrait by the name the mod files it under.
#[must_use]
pub fn portrait(exports: &Path, name: &str) -> Option<Vec<u8>> {
    std::fs::read(portraits_dir(exports).join(format!("{name}.png"))).ok()
}
