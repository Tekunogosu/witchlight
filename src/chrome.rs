//! The marks on the viewer's own furniture.
//!
//! Not `/icons/`, which serves the waypoint marks a game client exported into the
//! map directory. Those are data: they arrive at run time, they differ between
//! worlds, and a map with no world behind it has none of them. These are part of
//! the program, because the furniture has to draw on a map that has never been
//! exported and on a service that has never met a game.
//!
//! Every mark here was a Unicode character until it was not. Two were colour
//! emoji, which paint themselves and ignore `color` — which is why an armed tool
//! had to signal with a ring drawn around the mark rather than by colouring it —
//! and the rest were symbol-font characters each machine drew in whatever face it
//! happened to have, when it had one at all.
//!
//! The vendored pack holds every icon Phosphor draws in its filled weight. Only
//! the names below reach the binary, so an icon nobody asks for costs nothing.

/// The marks the page may ask for, and the bytes it gets.
///
/// Written as `"<mark>" @ "<weight>"`, from which the vendored path is built. The
/// name is not repeated, so what the page asks for and what the binary carries
/// cannot drift apart: a mark the pack does not have is a build error rather than
/// a square that turns up empty on somebody's screen.
///
/// Every weight but `regular` suffixes its files with its own name. A mark wanted
/// at that one needs a line of its own rather than this arm.
macro_rules! chrome {
    ($($name:literal @ $weight:literal),* $(,)?) => {
        &[$(($name, include_str!(
            concat!("vendor/phosphor/", $weight, "/", $name, "-", $weight, ".svg")
        ))),*]
    };
}

/// What the furniture is marked with.
///
/// Filled, because the waypoint marks these live beside are solid silhouettes and
/// a hairline drawn among them reads as a different set. The exception is the
/// mark that shuts a window, and it is the exception for the same reason: in the
/// filled weight an `x` is a filled square with the cross knocked out of it,
/// which beside a heading reads as a blot rather than as a way out of a window.
const ICONS: &[(&str, &str)] = chrome![
    // Arms the pointer, then names the block under it.
    "scan" @ "fill",
    // What the reader has chosen to see.
    "gear-six" @ "fill",
    // A place: on the button that starts a marker, and again inside the compose
    // window on the button that takes its coordinates from a click.
    "map-pin-simple" @ "fill",
    // What a marker starts as, saved.
    "bookmarks-simple" @ "fill",
    // Whoever is looking, beside their name; and standing in for a portrait
    // nobody has sent yet.
    "user" @ "fill",
    // What the map can be asked to do differently for one pair of eyes.
    "person-arms-spread" @ "fill",
    // Shuts a window, and discards a preset. Bold rather than filled — see above.
    "x" @ "bold",
];

/// The icon filed under a name, if the binary carries one.
#[must_use]
pub fn icon(name: &str) -> Option<&'static str> {
    ICONS
        .iter()
        .find(|(known, _)| *known == name)
        .map(|(_, body)| *body)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_mark_the_furniture_wears_is_carried() {
        // The page asks for these by name from three files, and a name that
        // reaches nothing is a control with a hole where its mark should be —
        // which looks like a broken build rather than a missing string.
        for name in ["scan", "gear-six", "map-pin-simple", "bookmarks-simple", "user", "person-arms-spread", "x"] {
            assert!(icon(name).is_some(), "the page asks for {name}");
        }
    }

    #[test]
    fn a_name_nobody_vendored_is_nothing() {
        assert!(icon("magnifying-glass").is_none(), "only what is listed is carried");
        assert!(icon("").is_none(), "and a name that is not one is not a file");
    }

    #[test]
    fn what_is_carried_is_drawable_and_takes_a_colour() {
        for (name, body) in ICONS {
            assert!(body.starts_with("<svg"), "{name} should be an svg");
            assert!(body.contains("<path"), "{name} should have something to draw");
            // The whole reason these replaced the emoji: a mark that paints
            // itself cannot be turned the accent colour when its tool is armed.
            assert!(
                body.contains("currentColor"),
                "{name} should take the colour it is given"
            );
        }
    }
}
