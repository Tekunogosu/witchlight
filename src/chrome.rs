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
    // A place, on the button that starts a marker.
    "map-pin-simple" @ "fill",
    // Aims at a place rather than standing on one: the compose window's button
    // for taking a marker's coordinates from a click. It wore the pin, which is
    // the mark of the button that opened the window it sits in — one picture for
    // both left the reader to work out that the inner one meant something else.
    "crosshair" @ "fill",
    // What a marker starts as, saved.
    "bookmarks-simple" @ "fill",
    // The same thing offered rather than listed: the button on the marker form
    // that opens the presets to fill it in from. A stack rather than the simple
    // pair, so that "the presets" and "one of the presets" are not one picture.
    "bookmarks" @ "fill",
    // Take this preset and put it somewhere. A dropper is the mark for lifting
    // what a thing is made of and applying it elsewhere, which is the whole of
    // what the button does.
    "eyedropper" @ "fill",
    // Every marker there is, as a list rather than as pins on a map.
    "list-bullets" @ "fill",
    // Whoever is looking, beside their name; and standing in for a portrait
    // nobody has sent yet.
    "user" @ "fill",
    // What the map can be asked to do differently for one pair of eyes.
    "person-arms-spread" @ "fill",
    // Shuts a window, and discards a preset. Bold rather than filled — see above.
    "x" @ "bold",
    // Takes a marker away, on the form that was opened to change it. Filled, and
    // a lid over a body rather than the simple bin, because at fourteen pixels
    // the plain one is a rounded rectangle and reads as a note.
    "trash" @ "fill",
    // Sits inside a search box. Bold for the reason `x` is: filled, a magnifier
    // is a disc with a handle, which beside a caret reads as a blot.
    "magnifying-glass" @ "bold",
    // Who may see a marker, in the list and on the button that changes it. A lock
    // is a marker its owner keeps and a crowd is one the server can see — two
    // pictures of two different things, where a shut lock against an open one was
    // one picture twice over, told apart by a shackle a reader had to look for.
    // Filled, like the marks they stand among.
    "lock" @ "fill",
    "users-three" @ "fill",
    // Which column a list is sorted by, and which way. One mark rather than two:
    // descending is the same caret turned over, and a second file would be a
    // second thing to keep pointing the right way.
    "caret-up" @ "bold",
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

    /// Every mark the page asks for, read off the page rather than listed here.
    ///
    /// The style sheet is what turns a `mark-lock` class into a request for
    /// `/chrome/lock.svg`, so it is the list of what the binary has to carry. A
    /// third copy of that list written out in a test is a third thing to keep in
    /// step, and the one that would have gone stale silently: a mark added to the
    /// page and forgotten here is a control with a hole where its picture should
    /// be, which reads as a broken build rather than a missing string.
    fn asked_for() -> Vec<&'static str> {
        let mut names: Vec<&str> = crate::viewer::STYLE
            .match_indices("url(/chrome/")
            .filter_map(|(at, _)| {
                crate::viewer::STYLE[at + "url(/chrome/".len()..]
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
        // An icon in the table nobody draws with is bytes in every binary and a
        // name nobody will notice has stopped meaning anything.
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
            // The whole reason these replaced the emoji: a mark that paints
            // itself cannot be turned the accent colour when its tool is armed.
            assert!(
                body.contains("currentColor"),
                "{name} should take the colour it is given"
            );
        }
    }
}
