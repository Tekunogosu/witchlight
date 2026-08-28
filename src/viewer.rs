//! The page.
//!
//! Three kinds of file, kept apart: `page.html` is what the map *is*,
//! `style.css` is what it looks like, and `viewer/*.js` is what it does. They
//! were one file of three thousand lines, which is how six colour variables came
//! to be defined as themselves without anyone noticing.
//!
//! The scripts are joined at compile time and served as one asset rather than
//! ten requests. They are ordinary scripts sharing one scope, in the order below
//! — nothing runs until `poll.js` starts it — so the split is about reading them
//! and costs the browser nothing.
//!
//! Only the page is templated, and only with the handful of numbers the server
//! knows before a browser has asked it anything. The style and the scripts are
//! the same bytes for every request of a given build, which is what lets them be
//! cached under `?v=` and never fetched again.

/// Everything the page looks like.
pub const STYLE: &str = include_str!("viewer/style.css");

/// Everything the page does, in the order it is read.
///
/// `work.js` is first because it opens the whole script with `'use strict'` — a
/// directive at the top of the first file is a directive at the top of the one
/// script the browser sees, and it is what makes a mistyped name an error rather
/// than a new global nobody declared.
pub const SCRIPT: &str = concat!(
    include_str!("viewer/work.js"),
    include_str!("viewer/frame.js"),
    include_str!("viewer/settings.js"),
    include_str!("viewer/map.js"),
    include_str!("viewer/players.js"),
    include_str!("viewer/inspect.js"),
    include_str!("viewer/windows.js"),
    include_str!("viewer/compose.js"),
    include_str!("viewer/markers.js"),
    include_str!("viewer/blocks.js"),
    include_str!("viewer/profile.js"),
    include_str!("viewer/poll.js"),
);

/// The page, with the world's bounds and this build's number filled in.
///
/// The version comes from the build rather than from `/info.json`, so what the
/// page shows is what compiled it — a page fetched from one build cannot report
/// the number of another. It also versions the style and the scripts, which is
/// what lets those be cached forever and still change when this does.
#[must_use]
pub fn page((min_x, min_z, max_x, max_z): (i32, i32, i32, i32)) -> String {
    include_str!("viewer/page.html")
        .replace("__TILE__", &crate::pyramid::TILE.to_string())
        .replace("__MIN_X__", &min_x.to_string())
        .replace("__MIN_Z__", &min_z.to_string())
        .replace("__MAX_X__", &max_x.to_string())
        .replace("__MAX_Z__", &max_z.to_string())
        .replace("__VERSION__", env!("CARGO_PKG_VERSION"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_page_names_the_build_and_leaves_no_placeholder_behind() {
        let page = page((-512, -512, 512, 512));
        assert!(
            page.contains(env!("CARGO_PKG_VERSION")),
            "the page should say which build served it"
        );
        // Every substitution the page asks for, checked by the absence of the
        // only spelling they use. One left unfilled is `__VERSION__` on screen,
        // or a world whose bounds are a syntax error — both silent until seen.
        assert!(!page.contains("__"), "a placeholder was left unsubstituted in the page");
    }

    #[test]
    fn the_page_asks_for_the_style_and_the_scripts() {
        // Splitting the page into three files means two of them are now fetched
        // rather than inlined, and a page that forgets to ask for one of them is
        // a map with no furniture or no behaviour — which looks like a broken
        // service rather than a missing tag.
        let page = page((0, 0, 0, 0));
        assert!(page.contains("/viewer.css?v="), "the page must ask for its style");
        assert!(page.contains("/viewer.js?v="), "and for its scripts");
        assert!(page.contains("/leaflet.js"), "and for the library they extend");
    }

    #[test]
    fn the_scripts_are_joined_in_the_order_they_are_read() {
        // A directive is only a directive at the top of the script, so `work.js`
        // being anywhere but first is a page that silently stops being strict —
        // and a mistyped name goes back to being a new global rather than an
        // error. Checked on the bytes rather than on the list above them.
        assert!(
            SCRIPT.trim_start().starts_with("// Things that answer later"),
            "the strict directive must open the joined script"
        );

        // They share one scope and the last of them starts the page, so an order
        // that puts `poll.js` anywhere but last is a page that calls a function
        // before its file has run.
        let bootstrap = SCRIPT.rfind("beat(pollWorld").expect("the page starts itself");
        let first = SCRIPT.find("window.witchlight").expect("and reads its opening values");
        assert!(first < bootstrap, "nothing may run before the values it reads");
        assert!(
            SCRIPT[bootstrap..].lines().count() < 10,
            "starting the page is the last thing the scripts do"
        );
    }
}
