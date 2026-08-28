//! Words nobody can guess.
//!
//! One owner, because there are now two things that need one — the word the mod
//! proves itself with on the API channel, and the word a browser proves itself
//! with after following a login link — and a second copy of "make some bytes and
//! spell them in hex" is a second place for it to be got wrong.
//!
//! Never derived from anything. A token worked out from a player's uid, an export
//! path or a world's name is one that anybody who has read this program can work
//! out for themselves.

use std::fmt::Write as _;

/// A fresh word of `bytes` bytes, spelled in hex.
///
/// Hex rather than base64 because these travel in URLs, headers and file paths,
/// and the characters base64 adds are special in all three.
#[must_use]
pub fn word(bytes: usize) -> String {
    let mut raw = vec![0u8; bytes];
    if let Err(error) = getrandom::fill(&mut raw) {
        // Nothing sensible follows a machine that cannot produce random bytes,
        // and carrying on with a guessable one would be worse than stopping.
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
        // A hundred, because one pair agreeing by chance is the only way this
        // passes while being broken.
        let mut seen = std::collections::HashSet::new();
        for _ in 0..100 {
            assert!(seen.insert(word(16)), "a word came round twice");
        }
    }
}
