//! Unguessable random tokens.
//!
//! Two callers need one: the token the mod authenticates with on the API
//! channel, and the token a browser authenticates with after following a login
//! link. Both come from here.
//!
//! Tokens are never derived from other values. A token computed from a player's
//! uid, an export path or a world name can be computed by anyone who has read
//! this program.

use std::fmt::Write as _;

/// Returns a fresh random token of `bytes` bytes, spelled in hex.
///
/// The encoding is hex rather than base64 because these travel in URLs, headers
/// and file paths, and base64 adds characters that are special in all three.
#[must_use]
pub fn word(bytes: usize) -> String {
    let mut raw = vec![0u8; bytes];
    if let Err(error) = getrandom::fill(&mut raw) {
        // Continuing with a guessable token is worse than stopping.
        panic!("witchlight: no randomness available: {error}");
    }

    let mut word = String::with_capacity(bytes * 2);
    for byte in raw {
        let _ = write!(word, "{byte:02x}");
    }
    word
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_word_is_hex_of_the_length_asked_for() {
        for bytes in [1, 8, 16, 32] {
            let word = word(bytes);
            assert_eq!(word.len(), bytes * 2);
            assert!(word.chars().all(|c| c.is_ascii_hexdigit()));
        }
    }

    #[test]
    fn two_words_do_not_agree() {
        // A hundred draws, because a single pair colliding by chance is the
        // only way a broken generator passes this.
        let mut seen = std::collections::HashSet::new();
        for _ in 0..100 {
            assert!(seen.insert(word(16)), "a word came round twice");
        }
    }
}
