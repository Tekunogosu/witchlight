//! Assembles the page served to a browser.
//!
//! The page is built from three kinds of file in `assets/`. `page.html` is the
//! markup, `style.css` is the styling, and the `.js` files are the behaviour.
//!
//! The scripts are joined at compile time and served as one asset rather than as
//! many requests. They are ordinary scripts sharing one scope, in the order
//! listed below, and nothing runs until `poll.js` starts it. The split is for
//! reading and costs the browser nothing.
//!
//! Only the markup is templated, and only with the handful of numbers the server
//! knows before a browser has asked it anything. The style and the scripts are
//! the same bytes for every request of a given build, which lets them be cached
//! under `?v=` and never fetched again.

/// The page's stylesheet.
pub const STYLE: &str = include_str!("assets/style.css");

/// The library the page extends. It is named here rather than at the route that
/// serves it, so everything a browser caches under an address is listed in one
/// place and the stamp below covers all of it.
pub const LEAFLET_JS: &str = include_str!("vendor/leaflet.js");
pub const LEAFLET_CSS: &str = include_str!("vendor/leaflet.css");

/// The page's scripts, in the order they are joined.
///
/// `work.js` comes first because it opens the whole script with `'use strict'`.
/// A directive at the top of the first file is a directive at the top of the one
/// script the browser sees, and it makes a mistyped name an error rather than a
/// new global nobody declared.
pub const SCRIPT: &str = concat!(
    include_str!("assets/work.js"),
    include_str!("assets/frame.js"),
    // This comes before the windows, because `shutWindow` reads what a plugin's
    // panel declared and a `const` cannot be read before it is initialised.
    // Nothing here runs at load. What it declares is called when a plugin
    // registers, which happens after all of this.
    include_str!("assets/plugins.js"),
    include_str!("assets/mark.js"),
    include_str!("assets/settings.js"),
    include_str!("assets/map.js"),
    include_str!("assets/players.js"),
    include_str!("assets/who.js"),
    include_str!("assets/corner.js"),
    include_str!("assets/inspect.js"),
    include_str!("assets/windows.js"),
    include_str!("assets/search.js"),
    include_str!("assets/compose.js"),
    include_str!("assets/markers.js"),
    include_str!("assets/claims.js"),
    include_str!("assets/blocks.js"),
    include_str!("assets/presets.js"),
    include_str!("assets/directory.js"),
    include_str!("assets/bulk.js"),
    include_str!("assets/profile.js"),
    include_str!("assets/hotkeys.js"),
    include_str!("assets/poll.js"),
);

/// Returns the cache stamp the page asks for its style, scripts and library
/// under.
///
/// The stamp is derived from their content rather than from this build's version.
/// All four are served `immutable` for a year, so the address is the only thing
/// that can tell a browser its copy is stale. Tying that to a hand-bumped number
/// means a viewer changed without one would never be fetched again.
///
/// That has happened. A fix to the window resize shipped, the version did not
/// move, and every browser that had opened the map went on running the script
/// with the bug in it.
#[must_use]
pub fn stamp() -> &'static str {
    static STAMP: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    STAMP.get_or_init(|| fingerprint(&[STYLE, SCRIPT, LEAFLET_JS, LEAFLET_CSS]))
}

/// Returns the FNV-1a hash of everything given, in hex.
///
/// This is not a security boundary. It only has to change whenever any byte of
/// the page's assets does, and stay the same for the same bytes, so a rebuild of
/// unchanged sources does not discard every browser's cache.
fn fingerprint(parts: &[&str]) -> String {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in parts.iter().flat_map(|part| part.bytes()) {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{hash:016x}")
}

/// Renders the page with the world's bounds and this build's number filled in.
///
/// The version comes from the build rather than from `/info`, so the page reports
/// what compiled it. A page fetched from one build cannot report another build's
/// number.
///
/// `refresh_ms` is the operator's live poll interval. It is written into the page
/// rather than fetched, because the first poll happens before an answer to any
/// request could arrive.
#[must_use]
pub fn page((min_x, min_z, max_x, max_z): (i32, i32, i32, i32), refresh_ms: u64) -> String {
    include_str!("assets/page.html")
        .replace("__TILE__", &crate::render::pyramid::TILE.to_string())
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

    /// Checks that every address the page fetches is one the service answers.
    ///
    /// The page and the routes must agree about a list of strings, and nothing
    /// else notices when they stop. A page that fetches an address the service
    /// does not answer draws nothing, reports nothing, and logs nothing.
    ///
    /// The addresses are read off the script rather than listed here, so this
    /// test is not a third copy to keep in step.
    #[test]
    fn every_address_the_page_asks_for_is_one_the_service_answers() {
        // These are what `routes::route` matches on and what `urls` reads a
        // name out of. A prefix means the rest of the path is a name or a
        // position.
        const ANSWERED: &[&str] = &[
            "/", "/viewer.css", "/viewer.js", "/leaflet.js", "/leaflet.css", "/login", "/logout",
            "/me", "/me/preferences", "/live", "/colors", "/icons", "/info", "/blocks", "/block",
            "/plugins",
            "/markers", "/claims", "/events", "/tiles/", "/icons/", "/chrome/", "/portraits/",
            "/data/", "/plugins/", "/markers/", "/claims/",
        ];

        let mut missing = Vec::new();
        // Handle `fetch('/x')` and `fetch(`/x${...}`)` alike. The address ends
        // at the first character that is not part of one.
        for (at, _) in SCRIPT.match_indices("fetch(") {
            let rest = &SCRIPT[at + "fetch(".len()..];
            let Some(opened) = rest.chars().next() else { continue };
            if opened != '\'' && opened != '`' && opened != '"' {
                continue;
            }
            let address: String = rest[1..]
                .chars()
                .take_while(|c| !matches!(c, '\'' | '`' | '"' | '?' | '$' | '{'))
                .collect();
            if !address.starts_with('/') {
                continue;
            }

            let known = ANSWERED.iter().any(|answered| {
                // A prefix is an address whose remainder is a name or a
                // position. The page's own root is not one: `/` ends in a slash
                // and would otherwise match every address there is.
                if answered.len() > 1 && answered.ends_with('/') {
                    // What follows is a name or a position, which the page
                    // usually builds from a variable, so the address read off
                    // the script ends at the prefix itself.
                    address.starts_with(answered)
                } else {
                    address == *answered
                }
            });
            if !known {
                missing.push(address);
            }
        }

        missing.sort();
        missing.dedup();
        assert!(
            missing.is_empty(),
            "the page asks for {missing:?}, which the service does not answer — \
             either the route was renamed and the page was not, or the other way about"
        );
    }

    /// Checks that the page fetches the list of plugins and loads each one's
    /// script.
    ///
    /// Both halves of the plugin system can be complete and correct while the map
    /// draws nothing, because a plugin's script runs only when the page asks for
    /// it. If the service serves `/plugins/{id}/viewer.js` and the page never
    /// requests it, every plugin's `start` goes uncalled, which looks like a
    /// broken plugin from the outside.
    ///
    /// The test above does not cover this. It reads addresses out of `fetch(`,
    /// and a script is loaded by setting `src` on a tag.
    #[test]
    fn the_page_loads_every_registered_plugin() {
        assert!(
            SCRIPT.contains("fetch('/plugins'"),
            "the page has to ask which plugins are registered"
        );
        assert!(
            SCRIPT.contains("/plugins/${encodeURIComponent(id)}/viewer.js"),
            "the page has to load each plugin's script from where the service serves it"
        );
        // Look for the call rather than the name. `loadPlugins` appears in the
        // script as soon as the function is declared, so a test looking for the
        // bare name would pass whether or not anything calls it.
        assert!(
            SCRIPT.contains("then(loadPlugins)"),
            "and it has to actually call the loader at boot, after `pollMe` has answered"
        );
    }

    /// Checks by name what a plugin is handed.
    ///
    /// A plugin lives outside this repository and is written against this list,
    /// so a name that stops being offered breaks somebody else's plugin with no
    /// warning from here. The names are listed rather than counted, because which
    /// names matters and how many does not.
    #[test]
    fn a_plugin_is_handed_what_it_was_promised() {
        const OFFERED: &[&str] = &[
            "asset:", "at,", "said,", "meant,", "me:", "layer(", "mark(", "popup(",
            "panel(", "button(", "hotkey(", "setting(", "setSetting(", "reads:",
            "tool(", "pointing:", "kept(", "linked(", "say(", "fetch(", "style(",
            "sharedWith(", "shareWith(", "forget(", "beat:", "started:",
        ];

        let mut missing = Vec::new();
        for name in OFFERED {
            if !SCRIPT.contains(name) {
                missing.push(*name);
            }
        }
        assert!(missing.is_empty(), "a plugin is no longer handed {missing:?}");
    }

    /// Checks the hooks a plugin's own object may answer to.
    ///
    /// A plugin declares these and the page calls them, so one dropped here
    /// leaves a plugin's code unreached.
    #[test]
    fn every_hook_a_plugin_may_answer_to_is_called() {
        for hook in ["plugin.start", "plugin.onChange", "plugin.onTerrain", "plugin.onLink"] {
            assert!(SCRIPT.contains(hook), "nothing calls {hook}");
        }
    }

    /// Checks that a plugin's key, switch and pane are all named for the plugin.
    ///
    /// Two plugins on one map must not take each other's furniture by choosing
    /// the same word for it, and neither must reach the map's own.
    #[test]
    fn a_plugins_furniture_is_named_for_the_plugin() {
        assert!(
            SCRIPT.contains("const id = `${plugin}:${name}`"),
            "a plugin's key and switch are named for it"
        );
        assert!(
            SCRIPT.contains("`plugin-${id}-${pane}`"),
            "and so is a pane it draws into"
        );
    }

    #[test]
    fn the_page_names_the_build_and_leaves_no_placeholder_behind() {
        let page = page((-512, -512, 512, 512), 2000);
        assert!(
            page.contains(env!("CARGO_PKG_VERSION")),
            "the page should say which build served it"
        );
        // Check every substitution by the absence of the spelling they use. One
        // left unfilled shows `__VERSION__` on screen, or makes the world's
        // bounds a syntax error.
        assert!(!page.contains("__"), "a placeholder was left unsubstituted in the page");
    }

    /// Checks the property the whole cache rests on.
    ///
    /// Every asset the page names is served `immutable` for a year, so a browser
    /// asks again only when the address changes. If a changed script could keep
    /// its address, a correct fix would be served and permanently unreachable.
    #[test]
    fn a_changed_viewer_is_a_changed_address() {
        assert_ne!(
            fingerprint(&["function draw() {}"]),
            fingerprint(&["function draw() { }"]),
            "one byte of difference must reach the address"
        );
        // The joined script is one string, so a change in any file it is made
        // of must move the stamp.
        assert_ne!(fingerprint(&["a", "b"]), fingerprint(&["a", "c"]));
        assert_ne!(fingerprint(&["style", "script"]), fingerprint(&["script", "style"]));
    }

    #[test]
    fn unchanged_sources_keep_their_address() {
        // A rebuild that changed nothing must not discard every browser's copy
        // of a third of a megabyte.
        assert_eq!(fingerprint(&["a", "b"]), fingerprint(&["a", "b"]));
        assert_eq!(stamp(), stamp());
        assert_eq!(stamp().len(), 16, "hex, and all of it");
    }

    /// Checks that what a browser is told to keep forever is addressed by its
    /// content.
    ///
    /// The version number is bumped by hand and says what built the page. The
    /// stamp comes from the bytes and says which bytes. Serving an asset under
    /// the version is a promise the build cannot keep.
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
        // The first live poll goes out before an answer to any request could
        // come back, so a page that had to fetch this number would poll on some
        // other interval first.
        assert!(
            page((0, 0, 0, 0), 4500).contains("refresh: 4500"),
            "the page must open knowing how often to ask"
        );
    }

    #[test]
    fn the_page_asks_for_the_style_and_the_scripts() {
        // The style and the scripts are fetched rather than inlined, so a page
        // that forgets to ask for one is a map with no furniture or no
        // behaviour, which looks like a broken service rather than a missing tag.
        let page = page((0, 0, 0, 0), 2000);
        assert!(page.contains("/viewer.css?v="), "the page must ask for its style");
        assert!(page.contains("/viewer.js?v="), "and for its scripts");
        assert!(page.contains("/leaflet.js"), "and for the library they extend");
    }

    #[test]
    fn every_script_beside_this_one_is_actually_served() {
        // `include_str!` turns a missing named file into a build error, and says
        // nothing about a file that exists and is named nowhere. That second case
        // is behaviour written, reviewed and never run: the page loads and
        // whatever was in the file does not happen. Read the directory, because
        // the directory is the only copy that can be wrong.
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/page/assets");
        let mut on_disk: Vec<String> = std::fs::read_dir(&dir)
            .expect("the viewer's own directory")
            .flatten()
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .filter(|name| name.ends_with(".js"))
            .collect();
        on_disk.sort();

        let source = include_str!("viewer.rs");
        let mut joined: Vec<String> = source
            .match_indices("include_str!(\"assets/")
            .filter_map(|(at, _)| {
                let rest = &source[at + "include_str!(\"assets/".len()..];
                rest.split('"').next().map(str::to_owned)
            })
            .filter(|name| name.ends_with(".js"))
            .collect();
        joined.sort();

        assert_eq!(
            on_disk, joined,
            "every script in src/page/assets must be in SCRIPT and nothing else may be"
        );
    }

    #[test]
    fn the_scripts_are_joined_in_the_order_they_are_read() {
        // A directive counts only at the top of the script, so `work.js`
        // anywhere but first would stop the page being strict and turn a
        // mistyped name back into a new global. This checks the joined bytes
        // rather than the list above them.
        assert!(
            SCRIPT.trim_start().starts_with("// Things that answer later"),
            "the strict directive must open the joined script"
        );

        // The scripts share one scope and the last of them starts the page, so
        // `poll.js` anywhere but last would call a function before its file has
        // run.
        let bootstrap = SCRIPT.rfind("beat(pollWorld").expect("the page starts itself");
        let first = SCRIPT.find("window.witchlight").expect("and reads its opening values");
        assert!(first < bootstrap, "nothing may run before the values it reads");
        assert!(
            SCRIPT[bootstrap..].lines().count() < 10,
            "starting the page is the last thing the scripts do"
        );
    }
}
