//! Carries messages between the server mod and this service.
//!
//! [`apiport`] listens for what the mod posts. [`api`] publishes the address it
//! listens on. [`pull`] asks the mod for terrain this service is missing.
//! [`live`], [`pending`], [`preferences`] and [`auth`] define the payloads that
//! cross. [`watch`] notices when the mod writes new palette or block-name data.

pub mod api;
pub mod apiport;
pub mod auth;
pub mod live;
pub mod pending;
pub mod preferences;
pub mod pull;
pub mod watch;
