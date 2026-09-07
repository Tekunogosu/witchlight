//! Checks on short strings that arrive from a browser.
//!
//! Each rule here has one reader. A value that reaches another browser's page or
//! another player's screen is checked once, in one place, so two copies of a rule
//! cannot disagree about what they let through.

/// Normalises `#rrggbb` to lower case, or returns `None` when the text is not a
/// six-digit hex colour.
///
/// One reader for every colour that arrives from a browser. Such a colour is
/// written into another browser's stylesheet, so a stray string must not reach
/// one, and a second copy of this rule is a second chance to let one through.
#[must_use]
pub fn hex_colour(said: &str) -> Option<String> {
    let digits = said.trim().strip_prefix('#')?;
    if digits.len() != 6 || !digits.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    Some(format!("#{}", digits.to_ascii_lowercase()))
}
