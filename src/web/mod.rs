//! Serves the public HTTP interface.
//!
//! [`routes`] is the route table. [`feeds`] builds the JSON the page reads.
//! [`events`] pushes changes to connected browsers.

pub mod events;
pub mod feeds;
pub mod routes;
