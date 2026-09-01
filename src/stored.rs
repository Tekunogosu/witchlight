//! The files the mod exported beside the map, read back by name.
//!
//! Marker pictures and players' portraits are the two stored things this service
//! serves whose address carries a name rather than a position. Every other export
//! names its own path in the module that owns it — the palette, the world's
//! facts, the markers, the block names — and these two were spelled out at the
//! call sites instead, which is how one directory came to be written three times
//! and the join onto it twice.
//!
//! What a name may be is [`crate::urls::is_stored_name`], because that is a fact
//! about a URL. Where the file it names is, is here.

use std::path::{Path, PathBuf};

/// Where the marker pictures live.
#[must_use]
pub fn icons_dir(exports: &Path) -> PathBuf {
    exports.join("icons")
}

/// One marker picture, by the name a waypoint draws itself with.
#[must_use]
pub fn icon(exports: &Path, name: &str) -> Option<Vec<u8>> {
    std::fs::read(icons_dir(exports).join(format!("{name}.svg"))).ok()
}

/// Where the pictures players have sent of themselves live.
#[must_use]
pub fn portraits_dir(exports: &Path) -> PathBuf {
    exports.join("portraits")
}

/// One player's picture, by the name the mod files it under.
#[must_use]
pub fn portrait(exports: &Path, name: &str) -> Option<Vec<u8>> {
    std::fs::read(portraits_dir(exports).join(format!("{name}.png"))).ok()
}
