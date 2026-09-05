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

/// The library the page extends. Here rather than at the route that serves it,
/// so that everything a browser caches under an address is named in one place
/// and the stamp below can cover all of it.
pub const LEAFLET_JS: &str = include_str!("vendor/leaflet.js");
pub const LEAFLET_CSS: &str = include_str!("vendor/leaflet.css");

/// Everything the page does, in the order it is read.
///
/// `work.js` is first because it opens the whole script with `'use strict'` — a
/// directive at the top of the first file is a directive at the top of the one
/// script the browser sees, and it is what makes a mistyped name an error rather
/// than a new global nobody declared.
pub const SCRIPT: &str = concat!(
    include_str!("viewer/work.js"),
    include_str!("viewer/frame.js"),
    include_str!("viewer/mark.js"),
    include_str!("viewer/settings.js"),
    include_str!("viewer/map.js"),
    include_str!("viewer/players.js"),
    include_str!("viewer/who.js"),
    include_str!("viewer/corner.js"),
    include_str!("viewer/inspect.js"),
    include_str!("viewer/windows.js"),
    include_str!("viewer/search.js"),
    include_str!("viewer/compose.js"),
    include_str!("viewer/markers.js"),
    include_str!("viewer/claims.js"),
    include_str!("viewer/blocks.js"),
    include_str!("viewer/presets.js"),
    include_str!("viewer/directory.js"),
    include_str!("viewer/bulk.js"),
    include_str!("viewer/profile.js"),
    include_str!("viewer/hotkeys.js"),
    include_str!("viewer/poll.js"),
);

/// What the page asks for its style, its scripts and its library under.
///
/// Their content, not this build's version. All four are served `immutable` for a
/// year, so the address is the only thing that can tell a browser the copy it
/// already has is stale — and tying that to a number somebody bumps by hand means
/// a viewer changed without one is a viewer no browser will ever fetch again.
///
/// That is not hypothetical. A fix to the window resize shipped, the version did
/// not move, and every browser that had opened the map once went on running the
/// script with the bug in it — the fix was correct, served, and unreachable.
/// Content cannot be forgotten about.
#[must_use]
pub fn stamp() -> &'static str {
    static STAMP: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    STAMP.get_or_init(|| fingerprint(&[STYLE, SCRIPT, LEAFLET_JS, LEAFLET_CSS]))
}

/// FNV-1a over everything given, spelled in hex.
///
/// Not a security boundary — nobody is trying to forge a stylesheet. It only has
/// to change whenever any byte of the page's assets does, and be the same for the
/// same bytes so that a rebuild of unchanged sources does not throw away every
/// browser's cache for nothing.
fn fingerprint(parts: &[&str]) -> String {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in parts.iter().flat_map(|part| part.bytes()) {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{hash:016x}")
}

/// The page, with the world's bounds and this build's number filled in.
///
/// The version comes from the build rather than from `/info.json`, so what the
/// page shows is what compiled it — a page fetched from one build cannot report
/// the number of another. It also versions the style and the scripts, which is
/// what lets those be cached forever and still change when this does.
///
/// `refresh_ms` is the operator's live poll gap. It is written into the page
/// rather than asked for, because the first beat happens before an answer to any
/// question could arrive.
#[must_use]
pub fn page((min_x, min_z, max_x, max_z): (i32, i32, i32, i32), refresh_ms: u64) -> String {
    include_str!("viewer/page.html")
        .replace("__TILE__", &crate::pyramid::TILE.to_string())
        .replace("__MIN_X__", &min_x.to_string())
        .replace("__MIN_Z__", &min_z.to_string())
        .replace("__MAX_X__", &max_x.to_string())
        .replace("__MAX_Z__", &max_z.to_string())
        .replace("__REFRESH__", &refresh_ms.to_string())
        .replace("__ASSETS__", stamp())
        .replace("__VERSION__", env!("CARGO_PKG_VERSION"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_page_names_the_build_and_leaves_no_placeholder_behind() {
        let page = page((-512, -512, 512, 512), 2000);
        assert!(
            page.contains(env!("CARGO_PKG_VERSION")),
            "the page should say which build served it"
        );
        // Every substitution the page asks for, checked by the absence of the
        // only spelling they use. One left unfilled is `__VERSION__` on screen,
        // or a world whose bounds are a syntax error — both silent until seen.
        assert!(!page.contains("__"), "a placeholder was left unsubstituted in the page");
    }

    /// The property the whole cache rests on.
    ///
    /// Every asset the page names is served `immutable` for a year, so a browser
    /// asks again only when the address changes. If a changed script can keep its
    /// address, a fix can be correct, served, and permanently unreachable — which
    /// is what happened when the address was the build's version number and
    /// somebody changed the viewer without bumping it.
    #[test]
    fn a_changed_viewer_is_a_changed_address() {
        assert_ne!(
            fingerprint(&["function draw() {}"]),
            fingerprint(&["function draw() { }"]),
            "one byte of difference must reach the address"
        );
        // The joined script is one string, so a change in any file it is made of
        // has to move it whichever file that was.
        assert_ne!(fingerprint(&["a", "b"]), fingerprint(&["a", "c"]));
        assert_ne!(fingerprint(&["style", "script"]), fingerprint(&["script", "style"]));
    }

    #[test]
    fn unchanged_sources_keep_their_address() {
        // A rebuild that changed nothing must not throw away every browser's copy
        // of a third of a megabyte for the sake of it.
        assert_eq!(fingerprint(&["a", "b"]), fingerprint(&["a", "b"]));
        assert_eq!(stamp(), stamp());
        assert_eq!(stamp().len(), 16, "hex, and all of it");
    }

    /// What a browser is told to keep forever must be addressed by what it is.
    ///
    /// The version number is a person's to bump and says what built the page; the
    /// stamp is the bytes' own and says which bytes. Serving an asset under the
    /// first is a promise the build cannot keep.
    #[test]
    fn every_asset_kept_forever_is_addressed_by_its_content() {
        let page = page((0, 0, 0, 0), 2000);
        for asset in ["/viewer.css", "/viewer.js", "/leaflet.css", "/leaflet.js"] {
            assert!(
                page.contains(&format!("{asset}?v={}", stamp())),
                "{asset} must be asked for under the stamp"
            );
            assert!(
                !page.contains(&format!("{asset}?v={}", env!("CARGO_PKG_VERSION"))),
                "{asset} must not be asked for under the version"
            );
        }
    }

    #[test]
    fn the_page_carries_the_beat_it_is_to_ask_on() {
        // The first live poll goes out before an answer to any question could
        // come back, so a page that had to ask for this number would spend its
        // opening beats on some other one.
        assert!(
            page((0, 0, 0, 0), 4500).contains("refresh: 4500"),
            "the page must open knowing how often to ask"
        );
    }

    #[test]
    fn the_page_asks_for_the_style_and_the_scripts() {
        // Splitting the page into three files means two of them are now fetched
        // rather than inlined, and a page that forgets to ask for one of them is
        // a map with no furniture or no behaviour — which looks like a broken
        // service rather than a missing tag.
        let page = page((0, 0, 0, 0), 2000);
        assert!(page.contains("/viewer.css?v="), "the page must ask for its style");
        assert!(page.contains("/viewer.js?v="), "and for its scripts");
        assert!(page.contains("/leaflet.js"), "and for the library they extend");
    }

    #[test]
    fn every_script_beside_this_one_is_actually_served() {
        // `include_str!` makes a named file that is missing a build error, and
        // says nothing at all about a file that exists and is named nowhere. That
        // one is behaviour written, reviewed and never run: the page loads, and
        // whatever was in it silently does not happen. Read off the directory,
        // because the directory is the only copy that can be wrong.
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/viewer");
        let mut on_disk: Vec<String> = std::fs::read_dir(&dir)
            .expect("the viewer's own directory")
            .flatten()
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .filter(|name| name.ends_with(".js"))
            .collect();
        on_disk.sort();

        let source = include_str!("viewer.rs");
        let mut joined: Vec<String> = source
            .match_indices("include_str!(\"viewer/")
            .filter_map(|(at, _)| {
                let rest = &source[at + "include_str!(\"viewer/".len()..];
                rest.split('"').next().map(str::to_owned)
            })
            .filter(|name| name.ends_with(".js"))
            .collect();
        joined.sort();

        assert_eq!(
            on_disk, joined,
            "every script in src/viewer must be in SCRIPT and nothing else may be"
        );
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
