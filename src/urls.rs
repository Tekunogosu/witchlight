//! Reading a request's address.
//!
//! Everything a URL can say arrives here and nowhere else. Two of these names
//! become a path on this machine and one becomes the identity of a waypoint, so
//! what they will and will not accept is the whole of the guard around them — and
//! a second copy of any of it is a second chance to get it wrong.

/// The part of a URL before the query.
#[must_use]
pub fn path(url: &str) -> &str {
    url.split('?').next().unwrap_or(url)
}

/// One named value out of a query string.
///
/// One reader for all of them, because the rule is not about generations or
/// coordinates but about how a query says anything at all — and a second copy of
/// it is a second chance to match `sincerely` where `since` was meant.
#[must_use]
pub fn param<'a>(url: &'a str, key: &str) -> Option<&'a str> {
    url.split_once('?')?
        .1
        .split('&')
        .filter_map(|pair| pair.split_once('='))
        .find_map(|(name, value)| (name == key).then_some(value))
}

/// A query value as the person typing it meant it.
///
/// Every other value read out of a query here is a number or a word of hex, and
/// none of them has ever needed this. A search box does: a space arrives as `%20`
/// and matching against that finds nothing at all, which reads as a search that
/// simply has no answers rather than one that never asked the question.
///
/// A stray `%` that spells nothing is itself. What arrives is somebody typing,
/// and refusing the whole search over a loose percent sign helps nobody.
#[must_use]
pub fn decoded(value: &str) -> String {
    let raw = value.as_bytes();
    let mut out = Vec::with_capacity(raw.len());
    let mut at = 0;

    while at < raw.len() {
        match raw[at] {
            b'+' => {
                out.push(b' ');
                at += 1;
            }
            b'%' if at + 2 < raw.len() => match hex(raw[at + 1]).zip(hex(raw[at + 2])) {
                Some((high, low)) => {
                    out.push(high * 16 + low);
                    at += 3;
                }
                None => {
                    out.push(b'%');
                    at += 1;
                }
            },
            byte => {
                out.push(byte);
                at += 1;
            }
        }
    }

    String::from_utf8_lossy(&out).into_owned()
}

fn hex(digit: u8) -> Option<u8> {
    match digit {
        b'0'..=b'9' => Some(digit - b'0'),
        b'a'..=b'f' => Some(digit - b'a' + 10),
        b'A'..=b'F' => Some(digit - b'A' + 10),
        _ => None,
    }
}

/// The `since` of a query string, naming the generation a viewer last drew.
#[must_use]
pub fn since_of(url: &str) -> Option<u64> {
    param(url, "since")?.parse().ok()
}

/// The block position an inspector is asking about. Both halves or neither: half
/// a position names nowhere, and defaulting the other half would name somewhere
/// else entirely.
#[must_use]
pub fn block_asked(url: &str) -> Option<(i32, i32)> {
    Some((param(url, "x")?.parse().ok()?, param(url, "z")?.parse().ok()?))
}

/// The login word out of `/login?t=…`.
#[must_use]
pub fn link_asked(url: &str) -> Option<&str> {
    param(url, "t").filter(|word| !word.is_empty())
}

/// The marker a `/markers/{key}` path names.
///
/// Only the shape of the path here; whether the key is a name this map ever
/// handed out is decided where the change is read, beside everything else that
/// arrived with it.
#[must_use]
pub fn marker_key(path: &str) -> Option<&str> {
    let key = path.strip_prefix("/markers/")?;
    (!key.is_empty() && !key.contains('/') && key != "pending").then_some(key)
}

/// The marker a `/markers/{key}/pin` path names.
///
/// A pin hangs off the marker rather than sitting beside it, because that is what
/// it is about: one marker, and whether the person asking keeps it in sight. Its
/// own address rather than a field on the marker, because it is nobody else's
/// business and changes nothing anybody else sees — a put on the marker itself is
/// an edit of the marker, which is a different permission and a different answer.
#[must_use]
pub fn marker_pin_key(path: &str) -> Option<&str> {
    let key = path.strip_prefix("/markers/")?.strip_suffix("/pin")?;
    (!key.is_empty() && !key.contains('/')).then_some(key)
}

/// `/claims/{key}`, where the key is the name the mod gave a claim.
///
/// The shape is the marker's, and so is the reason: which of the two things a
/// claim's own address means is the method and nothing else, so one place knows
/// a claim by name and one place says what may be done to it.
#[must_use]
pub fn claim_key(path: &str) -> Option<&str> {
    let key = path.strip_prefix("/claims/")?;
    (!key.is_empty() && !key.contains('/')).then_some(key)
}

/// `/icons/{name}.svg`, where the name is a marker icon.
///
/// The name reaches here from a waypoint, which got it from whatever mods are
/// installed, and is about to become a path. Only the characters that cannot
/// mean anything but themselves are allowed through — no separators, no dots,
/// so nothing outside the icons directory can be named.
#[must_use]
pub fn icon_name(url: &str) -> Option<&str> {
    stored_name(url, "/icons/", ".svg")
}

/// The name a player's picture is filed under, from `/portraits/{name}.png`.
#[must_use]
pub fn portrait_name(url: &str) -> Option<&str> {
    stored_name(url, "/portraits/", ".png")
}

/// `/chrome/{name}.svg`, where the name is a mark on the viewer's own furniture.
///
/// Read to the same rule as the others even though nothing is joined onto a
/// directory here — what a chrome icon names is a table compiled into the binary.
/// A name is still only ever a name, so that the one reader answers for every
/// address of this shape and a later change of storage cannot widen what is
/// accepted without somebody deciding to.
#[must_use]
pub fn chrome_name(url: &str) -> Option<&str> {
    stored_name(url, "/chrome/", ".svg")
}

/// The name in a URL, when it is only ever a name.
///
/// One reader for every kind of stored file, because the rule is not about icons
/// or portraits but about what may be joined onto a directory and handed back.
fn stored_name<'a>(url: &'a str, prefix: &str, suffix: &str) -> Option<&'a str> {
    let name = url.strip_prefix(prefix)?.strip_suffix(suffix)?;
    is_stored_name(name).then_some(name)
}

/// Whether a name may be joined onto a directory this service serves out of.
#[must_use]
pub fn is_stored_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-')
}

/// `/tiles/{level}/{x}/{z}.png`. Level 0 is one block per pixel; each level above
/// covers twice as much world. Coordinates may be negative.
#[must_use]
pub fn tile_coords(url: &str) -> Option<(u32, i32, i32)> {
    let rest = url.strip_prefix("/tiles/")?.strip_suffix(".png")?;
    let (level, rest) = rest.split_once('/')?;
    let (x, z) = rest.split_once('/')?;
    Some((level.parse().ok()?, x.parse().ok()?, z.parse().ok()?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_icon_name_is_only_ever_a_name() {
        // These arrive from whatever mods a server runs and become a path.
        for good in ["circle", "gravestone", "star1", "skull_and_crossbones", "my-mod_icon2"] {
            assert!(is_stored_name(good), "{good} should be allowed");
            assert_eq!(icon_name(&format!("/icons/{good}.svg")), Some(good));
        }

        for bad in [
            "../palette",
            "..",
            "a/b",
            "Gravestone",
            "grave stone",
            "",
            "with.dot",
            "%2e%2e",
            "under\\score\\back",
        ] {
            assert!(!is_stored_name(bad), "{bad} must not be allowed");
        }

        assert!(!is_stored_name(&"a".repeat(65)), "a name has to end somewhere");
        assert_eq!(icon_name("/icons/circle.png"), None, "only svg");
        assert_eq!(icon_name("/tiles/0/0/0.png"), None, "not a tile");
    }

    /// A player's picture is filed under a name derived from their uid, and a uid
    /// is base64 — it carries `/` and `+`, which is a path and not a name. The mod
    /// writes it in hex for exactly that reason, and nothing that arrives here is
    /// trusted to have done so.
    #[test]
    fn a_portrait_name_is_only_ever_a_name() {
        let hex = "3070564246376c42722b697159483442";
        assert_eq!(portrait_name(&format!("/portraits/{hex}.png")), Some(hex));

        for bad in [
            "/portraits/../../etc/passwd.png",
            "/portraits/a/b.png",
            "/portraits/A0FF.png",
            "/portraits/.png",
        ] {
            assert_eq!(portrait_name(bad), None, "{bad} must not be allowed");
        }

        assert_eq!(portrait_name("/portraits/abc.svg"), None, "only png");
        assert_eq!(portrait_name("/icons/abc.png"), None, "not an icon");
    }

    #[test]
    fn a_query_value_arrives_as_it_was_typed() {
        assert_eq!(decoded("granite%20rock"), "granite rock");
        assert_eq!(decoded("granite+rock"), "granite rock");
        assert_eq!(decoded("plain"), "plain");
        assert_eq!(decoded(""), "");
        assert_eq!(decoded("%C3%A9"), "é", "more than one byte to a letter");
        assert_eq!(decoded("a%2Fb"), "a/b");

        // Somebody typing a percent sign is not a request to refuse the search.
        assert_eq!(decoded("100%"), "100%");
        assert_eq!(decoded("50%z9"), "50%z9");
        assert_eq!(decoded("%"), "%");
    }

    #[test]
    fn a_query_value_is_matched_by_its_whole_name() {
        assert_eq!(since_of("/info.json?since=7"), Some(7));
        assert_eq!(block_asked("/block.json?x=-412&z=88"), Some((-412, 88)));
        assert_eq!(block_asked("/block.json?z=88&x=-412"), Some((-412, 88)));

        // A name that merely starts the same is a different name.
        assert_eq!(param("/info.json?sincerely=7", "since"), None);
        assert_eq!(param("/block.json?xz=1", "x"), None);

        assert_eq!(since_of("/info.json"), None, "no query at all");
        assert_eq!(block_asked("/block.json?x=1"), None, "half a position is nowhere");
        assert_eq!(block_asked("/block.json?x=1&z=here"), None, "z is a number");
    }

    #[test]
    fn a_path_is_what_comes_before_the_query() {
        assert_eq!(path("/tiles/0/1/2.png?v=7"), "/tiles/0/1/2.png");
        assert_eq!(path("/info.json"), "/info.json");
        assert_eq!(path("/?login=expired"), "/");
    }

    #[test]
    fn a_tile_is_named_by_its_level_and_place() {
        assert_eq!(tile_coords("/tiles/0/0/0.png"), Some((0, 0, 0)));
        assert_eq!(tile_coords("/tiles/11/-3/7.png"), Some((11, -3, 7)));
        assert_eq!(tile_coords("/tiles/0/0.png"), None, "a tile has both axes");
        assert_eq!(tile_coords("/tiles/x/0/0.png"), None, "a level is a number");
        assert_eq!(tile_coords("/icons/circle.svg"), None);
    }

    #[test]
    fn only_one_marker_is_named_at_a_time() {
        assert_eq!(marker_key("/markers/abc"), Some("abc"));
        assert_eq!(marker_key("/markers"), None, "that is the collection");
        assert_eq!(marker_key("/markers/"), None);
        assert_eq!(marker_key("/markers/a/b"), None, "one key, not a path");
        // The mod's own collection point, which is not a marker's name.
        assert_eq!(marker_key("/markers/pending"), None);
    }

    #[test]
    fn a_pin_hangs_off_the_marker_it_is_about() {
        assert_eq!(marker_pin_key("/markers/abc/pin"), Some("abc"));
        // The marker itself, which is a different thing to do to it.
        assert_eq!(marker_pin_key("/markers/abc"), None);
        assert_eq!(marker_pin_key("/markers//pin"), None);
        assert_eq!(marker_pin_key("/markers/a/b/pin"), None, "one key, not a path");
        // And the marker's own address does not answer for what hangs off it.
        assert_eq!(marker_key("/markers/abc/pin"), None);
    }
}
