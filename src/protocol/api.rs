//! Defines the private channel the server mod posts on.
//!
//! This channel is separate from the map's own port. The map's port is meant to
//! be reachable, while this one accepts writes, and anything that could reach a
//! public write endpoint could put people on the map who are not there.
//!
//! Two things restrict it. The listener binds to loopback, which nothing off the
//! machine can reach. Callers must present a token that only something able to
//! read this program's files knows. Both work the same way on every platform.
//!
//! The listener's address is published beside the map in `api.json`. The port is
//! whatever the machine had free, so two game servers on one box do not collide
//! and neither needs configuring.

use std::path::{Path, PathBuf};

use tiny_http::Request;

use crate::util::files;
use crate::util::log::warn;

/// The token length in bytes. The token is written as hex, so it is twice this
/// many characters.
const TOKEN_BYTES: usize = 16;

/// The authorization scheme prefix the token travels behind.
const BEARER: &str = "bearer ";

/// Holds the address the two halves meet on and the token that identifies the
/// mod.
pub struct Api {
    /// The address to listen on. It defaults to loopback with a port the
    /// machine picks. An operator names one when the mod runs on another
    /// machine.
    pub bind: String,
    pub token: String,
}

impl Api {
    /// Resolves the settings. An empty `bind` means loopback on a free port. An
    /// empty `token` means a fresh one, which suits every case except a mod
    /// running somewhere this service cannot publish a file to.
    #[must_use]
    pub fn resolve(bind: &str, token: &str) -> Self {
        Self {
            bind: if bind.is_empty() { "127.0.0.1:0".to_owned() } else { bind.to_owned() },
            token: if token.is_empty() { crate::util::random::word(TOKEN_BYTES) } else { token.to_owned() },
        }
    }

    /// Reports whether this request carries the token.
    #[must_use]
    pub fn authorized(&self, request: &Request) -> bool {
        presented(&crate::util::http::header(request, "Authorization").unwrap_or_default(), &self.token)
    }

    /// Writes where the mod should post and what token to present.
    ///
    /// The port is known only once the listener is bound, so the caller passes
    /// it in. The file is written alongside and then moved into place, so a
    /// reader never sees half of it.
    pub fn publish(&self, exports: &Path, port: u16) {
        let body = serde_json::json!({
            "Port": port,
            "Token": self.token,
            "Version": env!("CARGO_PKG_VERSION"),
        });

        let path = connection_path(exports);
        if let Err(error) = files::replace_private(&path, body.to_string().as_bytes()) {
            warn!("could not write {}: {error}", path.display());
        }
    }

    /// Deletes the published file, so nothing is handed the address of a
    /// listener that is gone. A stale file would send the mod's posts to
    /// whatever took the port next.
    pub fn unpublish(exports: &Path) {
        let _ = std::fs::remove_file(connection_path(exports));
    }
}

#[must_use]
pub fn connection_path(exports: &Path) -> PathBuf {
    exports.join("api.json")
}

/// Reports whether an `Authorization` header carries this token.
///
/// This takes the header rather than the request, so a test can assert on it
/// without standing up a server.
///
/// The comparison reads every byte rather than stopping at the first that
/// differs.
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

/// Strips a `Bearer <token>` prefix, whatever case the caller used.
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
        // .NET writes `Bearer`, callers write whatever they like, and HTTP
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
            // A prefix of the real token, caught by the length check before
            // the comparison runs.
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

        // Two starts must not produce the same token, or a token read off one
        // machine's file would open the next.
        assert_ne!(api.token, Api::resolve("", "").token);
    }

    #[test]
    fn what_an_operator_sets_is_what_is_used() {
        // This is the case where a file beside the map cannot reach the mod.
        let api = Api::resolve("10.0.0.4:9000", "shared");
        assert_eq!(api.bind, "10.0.0.4:9000");
        assert_eq!(api.token, "shared");
        assert!(presented("Bearer shared", &api.token));
    }
}
