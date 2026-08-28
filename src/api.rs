//! The private channel the server mod posts on.
//!
//! Separate from the map's own port because that one is meant to be reachable and
//! this one accepts writes: anything that could reach a public write endpoint
//! could put people on the map who are not there.
//!
//! It was a unix socket, where the filesystem decided who could connect. That is
//! the right shape and the wrong mechanism — Rust has no unix sockets on Windows,
//! and a game server runs on Windows. What replaces it keeps the property and
//! drops the platform: a listener on loopback, which nothing off the machine can
//! reach, and a token that only something able to read this program's files
//! knows. Both halves of that are the same code everywhere, so the platform this
//! is not tested on runs what the tested one runs.
//!
//! Where the listener ended up is published beside the map, in `api.json`. The
//! port is whatever the machine had free, so two game servers on one box collide
//! with nothing and neither needs configuring — which is what the hashed socket
//! name used to buy.

use std::path::{Path, PathBuf};

use tiny_http::Request;

use crate::files;

/// What the mod must present to post. Sixteen bytes as hex.
const TOKEN_BYTES: usize = 16;

/// The header the token travels in.
const BEARER: &str = "bearer ";

/// Where the two halves meet, and the word that proves which caller is the mod.
pub struct Api {
    /// The address to listen on. Loopback with a port the machine picks, unless
    /// an operator named one — which is what a mod on another machine needs.
    pub bind: String,
    pub token: String,
}

impl Api {
    /// Reads the settings. Empty `bind` means loopback on a free port; empty
    /// `token` means a fresh one, which is right for every case except the mod
    /// running somewhere this cannot publish a file to.
    #[must_use]
    pub fn resolve(bind: &str, token: &str) -> Self {
        Self {
            bind: if bind.is_empty() { "127.0.0.1:0".to_owned() } else { bind.to_owned() },
            token: if token.is_empty() { crate::random::word(TOKEN_BYTES) } else { token.to_owned() },
        }
    }

    /// Whether this request carries the token.
    #[must_use]
    pub fn authorized(&self, request: &Request) -> bool {
        let offered = request
            .headers()
            .iter()
            .find(|header| header.field.equiv("Authorization"))
            .map(|header| header.value.as_str())
            .unwrap_or_default();

        presented(offered, &self.token)
    }

    /// Writes down where the mod should post and what to say when it does.
    ///
    /// The port is only known once the listener is bound, so this is told it
    /// rather than working it out. Beside itself and then into place, so a reader
    /// never sees half of it.
    pub fn publish(&self, exports: &Path, port: u16) {
        let body = serde_json::json!({
            "Port": port,
            "Token": self.token,
            "Version": env!("CARGO_PKG_VERSION"),
        });

        let path = connection_path(exports);
        if let Err(error) = files::replace_private(&path, body.to_string().as_bytes()) {
            eprintln!("witchlight: could not write {}: {error}", path.display());
        }
    }

    /// Takes it away again, so nothing is handed the address of a listener that
    /// has gone. A stale file would otherwise send the mod's posts at whatever
    /// took the port next.
    pub fn unpublish(exports: &Path) {
        let _ = std::fs::remove_file(connection_path(exports));
    }
}

#[must_use]
pub fn connection_path(exports: &Path) -> PathBuf {
    exports.join("api.json")
}

/// Whether an `Authorization` header carries this token.
///
/// Apart from the request so that it can be asserted directly: what decides
/// whether a write is allowed should be answerable without standing up a server
/// to ask it.
///
/// Compared over every byte rather than stopping at the first that differs. The
/// saving is nothing and the habit costs nothing.
#[must_use]
fn presented(header: &str, token: &str) -> bool {
    let Some(offered) = strip_bearer(header) else {
        return false;
    };

    if offered.len() != token.len() {
        return false;
    }
    offered
        .bytes()
        .zip(token.bytes())
        .fold(0u8, |differences, (a, b)| differences | (a ^ b))
        == 0
}

/// `Bearer <token>`, however the caller cased it.
fn strip_bearer(header: &str) -> Option<&str> {
    let header = header.trim();
    if header.len() <= BEARER.len() || !header[..BEARER.len()].eq_ignore_ascii_case(BEARER) {
        return None;
    }
    Some(header[BEARER.len()..].trim())
}

#[cfg(test)]
mod tests {
    use super::*;

    const TOKEN: &str = "f990d57aafcf95b66649d5d2de667e7d";

    #[test]
    fn the_token_is_taken_however_the_caller_cased_bearer() {
        // .NET writes `Bearer`, curl users write whatever they like, and HTTP
        // says the scheme is case-insensitive.
        for header in [
            format!("Bearer {TOKEN}"),
            format!("bearer {TOKEN}"),
            format!("BEARER {TOKEN}"),
            format!("  Bearer   {TOKEN}  "),
        ] {
            assert!(presented(&header, TOKEN), "{header} should be taken");
        }
    }

    #[test]
    fn nothing_else_is_taken() {
        let wrong = "0".repeat(TOKEN.len());
        for header in [
            String::new(),
            "Bearer".to_owned(),
            "Bearer ".to_owned(),
            TOKEN.to_owned(),
            format!("Basic {TOKEN}"),
            format!("Bearer {wrong}"),
            // A prefix of the real token, which a length check catches before
            // the comparison ever runs.
            format!("Bearer {}", &TOKEN[..8]),
            format!("Bearer {TOKEN}extra"),
        ] {
            assert!(!presented(&header, TOKEN), "{header:?} must not be taken");
        }
    }

    #[test]
    fn an_unset_setting_means_loopback_and_a_word_nobody_can_guess() {
        let api = Api::resolve("", "");
        assert_eq!(api.bind, "127.0.0.1:0");
        assert_eq!(api.token.len(), TOKEN_BYTES * 2);
        assert!(api.token.chars().all(|c| c.is_ascii_hexdigit()));

        // Two starts must not agree, or a token read off one machine's file
        // would open the next.
        assert_ne!(api.token, Api::resolve("", "").token);
    }

    #[test]
    fn what_an_operator_sets_is_what_is_used() {
        // The one case a file beside the map cannot reach the mod.
        let api = Api::resolve("10.0.0.4:9000", "shared");
        assert_eq!(api.bind, "10.0.0.4:9000");
        assert_eq!(api.token, "shared");
        assert!(presented("Bearer shared", &api.token));
    }
}
