//! The marks on the viewer's own furniture.
//!
//! These are compiled into the binary, unlike `/icons/`, which serves the
//! waypoint marks a game client exported into the map directory. Those arrive at
//! run time and differ between worlds. The furniture has to draw on a map that
//! has never been exported and on a service that has never met a game, so its
//! marks travel with the program.
//!
//! Each mark is a vendored Phosphor icon rather than a Unicode character. A
//! symbol-font character is drawn in whatever face a machine happens to have, and
//! a colour emoji paints itself and ignores `color`, so neither can be recoloured
//! to signal state.
//!
//! The vendored pack holds every icon Phosphor draws in its filled weight. Only
//! the names below reach the binary, so an icon nobody asks for costs nothing.

/// Declares the marks the page may ask for and the bytes it gets.
///
/// Each entry is written as `"<mark>" @ "<weight>"`, from which the vendored path
/// is built. The name is written once, so what the page asks for and what the
/// binary carries cannot drift apart. A mark the pack does not have is a build
/// error rather than an empty square on somebody's screen.
///
/// Every weight but `regular` suffixes its files with its own name. A mark wanted
/// at `regular` needs its own line rather than this arm.
macro_rules! chrome {
    ($($name:literal @ $weight:literal),* $(,)?) => {
        &[$(($name, include_str!(
            concat!("vendor/phosphor/", $weight, "/", $name, "-", $weight, ".svg")
        ))),*]
    };
}

/// Names the mark each control wears.
///
/// Most are filled, because the waypoint marks they sit beside are solid
/// silhouettes and a hairline among them reads as a different set. A few are bold
/// instead, where the filled weight of that glyph is a solid square with the
/// shape knocked out of it and reads as a blot at these sizes.
const ICONS: &[(&str, &str)] = chrome![
    // Arms the pointer, then names the block under it.
    "scan" @ "fill",
    // Opens what the reader has chosen to see.
    "gear-six" @ "fill",
    // Marks a place, on the button that starts a marker.
    "map-pin-simple" @ "fill",
    // The compose window's button for taking a marker's coordinates from a
    // click. It aims at a place rather than standing on one, so it does not wear
    // the pin that opened the window it sits in.
    "crosshair" @ "fill",
    // Saves what a marker starts as.
    "bookmarks-simple" @ "fill",
    // The button on the marker form that opens the presets to fill it from. It
    // is a stack rather than the simple pair, so the presets and one preset are
    // not the same picture.
    "bookmarks" @ "fill",
    // Takes this preset and applies it elsewhere. A dropper is the mark for
    // lifting what a thing is made of and applying it somewhere else.
    "eyedropper" @ "fill",
    // The button that shows claimed land. It is an outline of an area rather
    // than a thing standing on one, because a claim is ground.
    "polygon" @ "fill",
    // Draws a new claim. It is the same outline with a plus in it, so the claims
    // and one more claim read as one subject and two verbs. It is bold rather
    // than filled, because the filled weight is a solid square with the outline
    // knocked out and reads as a blot at sixteen pixels.
    "selection-plus" @ "bold",
    // Keeps a marker where this reader can see it in game. The mark is a pin
    // because the game calls the flag a pin. A pinned waypoint is held against
    // the edge of the map rather than scrolling off it.
    "push-pin" @ "fill",
    // Takes the same flag off again. It is a pin with a stroke through it rather
    // than a second picture, so one subject reads as two states.
    "push-pin-slash" @ "fill",
    // Lists every marker, rather than showing them as pins on a map.
    "list-bullets" @ "fill",
    // Marks whoever is looking, beside their name, and stands in for a portrait
    // nobody has sent yet.
    "user" @ "fill",
    // Opens what the map can be asked to do differently for one reader.
    "person-arms-spread" @ "fill",
    // Shuts a window and discards a preset. It is bold rather than filled, for
    // the reason given above.
    "x" @ "bold",
    // Takes a marker away, on the form opened to change it. It is a lid over a
    // body rather than the simple bin, because at fourteen pixels the plain bin
    // is a rounded rectangle and reads as a note.
    "trash" @ "fill",
    // Sits inside a search box. It is bold, because a filled magnifier is a disc
    // with a handle and reads as a blot beside a caret.
    "magnifying-glass" @ "bold",
    // Shows who may see a marker, in the list and on the button that changes it.
    // A lock means a marker its owner keeps and a crowd means one the server can
    // see. Two different pictures read faster than a shut lock against an open
    // one, which differ only by a shackle.
    "lock" @ "fill",
    "users-three" @ "fill",
    // The map's zoom pair. Leaflet writes a `+` and a `\u{2212}` into those two
    // buttons as text, and each machine draws those characters in whatever face
    // it has. They are bold rather than filled, because a filled plus is a solid
    // square with the cross knocked out of it.
    "plus" @ "bold",
    "minus" @ "bold",
    // Shows which column a list is sorted by and which way. It is one mark
    // rather than two, because descending is the same caret rotated.
    "caret-up" @ "bold",
    // What a plugin's button wears when it ships no mark of its own. A plugin may
    // name any of the marks above or a file it shipped. This is drawn when it
    // names neither, so a button with no picture is still a button.
    "puzzle-piece" @ "fill",
];

/// Returns the icon filed under a name, if the binary carries one.
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

    /// Returns every mark the page asks for, read off the stylesheet rather than
    /// listed here.
    ///
    /// The stylesheet turns a `mark-lock` class into a request for
    /// `/chrome/lock.svg`, so it is the list of what the binary must carry. A
    /// third copy written out in a test would go stale silently, and a mark added
    /// to the page but missing from the binary leaves a control with a hole where
    /// its picture should be.
    fn asked_for() -> Vec<&'static str> {
        let mut names: Vec<&str> = crate::page::viewer::STYLE
            .match_indices("url(/chrome/")
            .filter_map(|(at, _)| {
                crate::page::viewer::STYLE[at + "url(/chrome/".len()..]
                    .split(".svg)")
                    .next()
                    .filter(|name| !name.is_empty() && !name.contains(['(', ')', '/']))
            })
            .collect();
        names.sort_unstable();
        names.dedup();
        names
    }

    #[test]
    fn every_mark_the_furniture_wears_is_carried() {
        let asked = asked_for();
        assert!(asked.len() > 5, "the style sheet should name marks: found {asked:?}");
        for name in asked {
            assert!(icon(name).is_some(), "the page asks for {name}");
        }
    }

    #[test]
    fn nothing_is_carried_that_the_page_never_asks_for() {
        // An icon in the table that nothing draws with is dead bytes in every
        // binary, under a name nobody would notice had stopped meaning anything.
        let asked = asked_for();
        for (name, _) in ICONS {
            assert!(asked.contains(name), "{name} is carried and the page never asks for it");
        }
    }

    #[test]
    fn a_name_nobody_vendored_is_nothing() {
        assert!(icon("compass-rose").is_none(), "only what is listed is carried");
        assert!(icon("").is_none(), "and a name that is not one is not a file");
    }

    #[test]
    fn what_is_carried_is_drawable_and_takes_a_colour() {
        for (name, body) in ICONS {
            assert!(body.starts_with("<svg"), "{name} should be an svg");
            assert!(body.contains("<path"), "{name} should have something to draw");
            // Each mark must take its colour from CSS. A mark that paints itself
            // cannot be turned the accent colour when its tool is armed.
            assert!(
                body.contains("currentColor"),
                "{name} should take the colour it is given"
            );
        }
    }
}
