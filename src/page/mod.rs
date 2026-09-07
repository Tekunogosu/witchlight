//! Builds the page served to a browser, including markup, styling and scripts.
//!
//! [`viewer`] assembles the page from `assets/`. [`chrome`] names the vendored
//! icons compiled into the binary.

pub mod chrome;
pub mod viewer;
