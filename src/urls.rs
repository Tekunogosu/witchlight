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

/// `/plugins/{plugin}/assets/{path}`, where the path is a file a plugin shipped.
///
/// The one address whose name has more than one part in it, because a plugin
/// keeps its own files in its own shape — `assets/icons/mountains.svg` — and the
/// service is not the half that decides what that shape is.
///
/// Every segment is read to the same rule a single name is, which is what makes
/// the extra depth safe rather than a new way in: a segment that cannot be
/// anything but itself cannot be `..`, cannot hold a separator, and cannot name
/// anything outside the directory it is joined onto. The extension is read the
/// same way and kept, since what a plugin serves is its own business and this
/// says only that it is a file with a name.
///
/// Answered without touching the disk. Canonicalising a path and comparing it
/// against the directory would be a second rule, kept in a second place, whose
/// answer depends on what happens to be on the filesystem at the moment it is
/// asked — a symlink planted between the check and the read makes it wrong. What
/// is accepted here is decided by the bytes of the request alone.
#[must_use]
pub fn plugin_asset(path: &str) -> Option<(&str, &str)> {
    let rest = path.strip_prefix("/plugins/")?;
    let (plugin, file) = rest.split_once('/')?;

    // The plugin's own name is a name like any other, so a request cannot climb
    // out of the plugins directory before a file has even been read.
    if !is_stored_name(plugin) {
        return None;
    }

    let asset = file.strip_prefix("assets/")?;
    is_asset_path(asset).then_some((plugin, asset))
}

/// `/plugins/{plugin}/viewer.js`, everything one plugin runs on the page.
///
/// One address rather than any file at a plugin's root, because the root is
/// where its database lives: a rule that served what was named there would serve
/// `data.sqlite` to anybody who asked. The script has one name and this answers
/// for that name alone, which is why what a plugin ships for the page to show —
/// its pictures — lives under `assets/` and is read by a rule of its own.
///
/// A plugin written across several files is still one address: what is served
/// under this name is everything that plugin runs, joined and wrapped in a scope
/// of its own by whoever answers this. A file of a plugin's is never addressable
/// on its own, which is what keeps two plugins from reaching each other's.
#[must_use]
pub fn plugin_script(path: &str) -> Option<&str> {
    let rest = path.strip_prefix("/plugins/")?;
    let plugin = rest.strip_suffix("/viewer.js")?;
    is_stored_name(plugin).then_some(plugin)
}

/// `/data/{plugin}`, where the name is a plugin that has registered.
#[must_use]
pub fn plugin_data(path: &str) -> Option<&str> {
    let name = path.strip_prefix("/data/")?;
    is_stored_name(name).then_some(name)
}

/// `/data/{plugin}/shares`, which groups this reader shares that plugin with.
///
/// Read before a row's own address, since `shares` would otherwise read as the
/// key of a row — a plugin whose key is one text column could have a row called
/// that, and the two must not be the same address.
#[must_use]
pub fn plugin_shares(path: &str) -> Option<&str> {
    let rest = path.strip_prefix("/data/")?;
    let name = rest.strip_suffix("/shares")?;
    is_stored_name(name).then_some(name)
}

/// `/data/{plugin}/{key}`, where the key names one row of that plugin's.
///
/// The key is the values of the plugin's own key columns, in the order it
/// declared them, separated by commas — which is what a plugin's own page has to
/// hand, since it is what it was answered with.
#[must_use]
pub fn plugin_row(path: &str) -> Option<(&str, Vec<&str>)> {
    let rest = path.strip_prefix("/data/")?;
    let (name, key) = rest.split_once('/')?;
    if !is_stored_name(name) || key.is_empty() {
        return None;
    }

    // Read to the same rule the rest of a URL is: a key is values, and a value
    // that could be anything but itself is not one this answers for.
    let parts: Vec<&str> = key.split(',').collect();
    parts
        .iter()
        .all(|part| !part.is_empty() && part.len() <= 32 && part.bytes().all(is_key_byte))
        .then_some((name, parts))
}

/// Whether a byte may stand in the key that names one row.
///
/// Numbers, which is what a position is, and the letters and dashes a name or a
/// code carries. Not a separator and not a quote: what this allows through is
/// bound into a statement rather than written into one, and this is the belt to
/// that brace.
const fn is_key_byte(byte: u8) -> bool {
    byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_' || byte == b'-'
}

/// Whether a path may be joined onto a plugin's own directory.
///
/// Every segment a name, the last one a name and an extension, and at most as
/// deep as a plugin has any reason to go. Depth is capped for the same reason a
/// name's length is: a limit nobody legitimate reaches is a limit that only ever
/// stops somebody trying something.
#[must_use]
pub fn is_asset_path(path: &str) -> bool {
    if path.is_empty() || path.len() > 128 {
        return false;
    }

    let mut segments = path.split('/').peekable();
    let mut depth = 0;

    while let Some(segment) = segments.next() {
        depth += 1;
        if depth > 4 {
            return false;
        }

        // The last segment is the file, and a file has an extension. Every one
        // before it is a directory and is only a name.
        if segments.peek().is_none() {
            let Some((stem, extension)) = segment.rsplit_once('.') else {
                return false;
            };
            return is_stored_name(stem) && is_stored_name(extension);
        }

        if !is_stored_name(segment) {
            return false;
        }
    }

    false
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

/// Every range a query asks for, as `?x=-1000..1000&z=0..500`.
///
/// The column names are read to the same rule every other name is, so what
/// reaches the store is a word that could be a column — whether it *is* one, and
/// whether ranges may be asked of it, is the store's own answer, since only the
/// plugin's declaration knows that.
///
/// A pair that does not read as two numbers is left out rather than refused: a
/// query is what a page asked for, and the answer to an unreadable ask is the
/// rows without it rather than an error nobody will see.
#[must_use]
pub fn ranges(url: &str) -> Vec<(String, i64, i64)> {
    let Some((_, query)) = url.split_once('?') else {
        return Vec::new();
    };

    query
        .split('&')
        .filter_map(|pair| pair.split_once('='))
        .filter(|(name, _)| is_stored_name(name))
        .filter_map(|(name, said)| {
            let (low, high) = said.split_once("..")?;
            Some((name.to_owned(), low.parse().ok()?, high.parse().ok()?))
        })
        .collect()
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

    /// A plugin's own files, which are the only address with depth in it. What
    /// makes the depth safe is that every segment is read to the rule a single
    /// name already is, so this checks the ways somebody would try to leave the
    /// directory rather than only the shapes that ought to work.
    #[test]
    fn a_plugin_asset_cannot_leave_its_own_directory() {
        assert_eq!(
            plugin_asset("/plugins/wl-heatmap/assets/icons/mountains.svg"),
            Some(("wl-heatmap", "icons/mountains.svg"))
        );
        assert_eq!(
            plugin_asset("/plugins/wl-heatmap/assets/legend.png"),
            Some(("wl-heatmap", "legend.png")),
            "a file at the top of assets is still a file"
        );

        for bad in [
            // Climbing, spelled every way it is usually spelled.
            "/plugins/wl-heatmap/assets/../../../etc/passwd",
            "/plugins/wl-heatmap/assets/icons/../../../map.sqlite",
            "/plugins/wl-heatmap/assets/..%2f..%2fmap.sqlite",
            "/plugins/../map.sqlite",
            "/plugins/wl-heatmap/../../map.sqlite",
            // A segment that is only dots is not a name, however many there are.
            "/plugins/wl-heatmap/assets/../icons/a.svg",
            "/plugins/wl-heatmap/assets/./icons/a.svg",
            // Separators the other way round, which a filesystem may still read.
            "/plugins/wl-heatmap/assets/icons\\..\\..\\map.sqlite",
            // An absolute path where a relative one was expected.
            "/plugins/wl-heatmap/assets//etc/passwd",
            // Outside the assets directory, where a plugin keeps its database.
            "/plugins/wl-heatmap/data.sqlite",
            "/plugins/wl-heatmap/viewer.js",
            // A plugin named as a path rather than as a name.
            "/plugins/../../assets/a.svg",
            // Nothing to serve.
            "/plugins/wl-heatmap/assets/",
            "/plugins/wl-heatmap/assets",
            "/plugins/wl-heatmap/",
            "/plugins/",
            // A directory, not a file: no extension on the last segment.
            "/plugins/wl-heatmap/assets/icons",
            // A hidden file is not a name.
            "/plugins/wl-heatmap/assets/.env",
            // Not this address at all.
            "/icons/circle.svg",
        ] {
            assert_eq!(plugin_asset(bad), None, "{bad} must not be served");
        }

        assert_eq!(
            plugin_asset("/plugins/wl-heatmap/assets/a/b/c/d/e.svg"),
            None,
            "a plugin has no reason to go that deep"
        );
        assert!(
            !is_asset_path(&format!("icons/{}.svg", "a".repeat(130))),
            "a path has to end somewhere"
        );
        assert!(
            !is_asset_path("icons/Mountains.svg"),
            "the same rule about case every other stored name follows"
        );
    }

    /// A plugin's script is one name at its root, and the root is where its
    /// database is — so this answers for that one name and nothing else there.
    #[test]
    fn a_plugin_script_is_the_only_thing_served_from_its_root() {
        assert_eq!(plugin_script("/plugins/wl-heatmap/viewer.js"), Some("wl-heatmap"));

        for bad in [
            // The database lives beside the script. Nothing may ask for it.
            "/plugins/wl-heatmap/data.sqlite",
            "/plugins/wl-heatmap/data.sqlite-wal",
            "/plugins/wl-heatmap/data.shape889b.bak",
            // Nor anything else somebody guesses at.
            "/plugins/wl-heatmap/viewer.js.map",
            "/plugins/wl-heatmap/../map.sqlite",
            "/plugins/../viewer.js",
            "/plugins//viewer.js",
            "/plugins/wl-heatmap/sub/viewer.js",
            "/plugins/wl-heatmap/",
            "/plugins/viewer.js",
        ] {
            assert_eq!(plugin_script(bad), None, "{bad} must not be served");
        }

        // The two rules do not overlap: what one answers the other refuses.
        assert_eq!(plugin_asset("/plugins/wl-heatmap/viewer.js"), None, "the script is not an asset");
        assert_eq!(
            plugin_script("/plugins/wl-heatmap/assets/icons/a.svg"),
            None,
            "and an asset is not the script"
        );
    }

    /// The rule above, asked of a real directory rather than of itself.
    ///
    /// What the validator is for is that a path joined onto the plugins directory
    /// stays under it, and a test that only reads the validator's own answer back
    /// cannot say whether that is true. So this builds the directory, joins what
    /// was allowed through, and asks the filesystem where it landed — and asserts
    /// that what was refused would have escaped, so a validator that started
    /// accepting everything would fail here rather than pass quietly.
    #[test]
    fn what_a_plugin_asset_resolves_to_stays_under_its_plugin() {
        let scratch = crate::files::testing::Scratch::new("urls-plugin-traversal");
        let root = scratch.at();
        let plugins = root.join("plugins");
        std::fs::create_dir_all(plugins.join("wl-heatmap/assets/icons")).expect("the plugin");
        std::fs::write(root.join("map.sqlite"), b"the map").expect("something worth stealing");
        std::fs::write(plugins.join("wl-heatmap/assets/icons/mountains.svg"), b"<svg/>").expect("an asset");

        let served = |url: &str| {
            plugin_asset(url).map(|(plugin, asset)| plugins.join(plugin).join("assets").join(asset))
        };

        let good = served("/plugins/wl-heatmap/assets/icons/mountains.svg").expect("an asset is served");
        assert_eq!(std::fs::read(&good).expect("it reads"), b"<svg/>");

        for attack in [
            "/plugins/wl-heatmap/assets/../../../map.sqlite",
            "/plugins/wl-heatmap/assets/icons/../../../map.sqlite",
            "/plugins/../map.sqlite",
            "/plugins/wl-heatmap/data.sqlite",
        ] {
            assert!(served(attack).is_none(), "{attack} must not be served");
        }

        // The refusals above are load-bearing, not incidental: joined without one
        // this is the map's own database, which is the whole reason for the rule.
        let unguarded = plugins.join("wl-heatmap/assets/../../../map.sqlite");
        assert_eq!(
            std::fs::read(&unguarded).ok(),
            Some(b"the map".to_vec()),
            "an unchecked join reaches the map, which is what the validator is between"
        );
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
