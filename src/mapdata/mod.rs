//! Map storage and per-viewer visibility.
//!
//! [`store`] is the database. [`memory`] records what each player has explored.
//! [`scope`] decides which of it a viewer is shown. [`facts`] and [`stored`]
//! read the files the mod exported beside the map. [`plugins`] holds rows that
//! plugins keep.

pub mod facts;
pub mod memory;
pub mod plugins;
pub mod scope;
pub mod store;
pub mod stored;
