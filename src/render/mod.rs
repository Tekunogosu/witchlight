//! The rendering pipeline: stored chunk records to finished tiles.
//!
//! Records come in as bytes from [`columns`], become pixels in [`tiles`] using
//! the colours [`palette`] resolves, and are stored and scaled down by
//! [`pyramid`] and [`levels`].

pub mod color;
pub mod columns;
pub mod levels;
pub mod palette;
pub mod pyramid;
pub mod tiles;
