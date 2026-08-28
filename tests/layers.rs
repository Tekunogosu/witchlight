//! The one structural rule this crate has, checked rather than asserted.
//!
//! A utility is a utility because nothing above it can pull it back down: the
//! moment `urls` reaches for `state` to answer one question, it stops being a
//! thing another program could lift out and becomes part of the map service.
//!
//! That is easy to break by accident and invisible when broken — the code still
//! compiles, the tests still pass, and only a later reader trying to reuse one
//! finds out. So it is read off the source, which is the only copy that can be
//! wrong.

use std::collections::BTreeSet;
use std::path::Path;

/// The modules that know nothing about maps, and must go on knowing nothing.
const UTILITIES: [&str; 7] = ["http", "urls", "cache", "net", "files", "random", "error"];

/// Which modules a file reaches for.
fn reaches(module: &str) -> BTreeSet<String> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("src").join(format!("{module}.rs"));
    let source = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("{} should be readable: {error}", path.display()));

    source
        .lines()
        .filter_map(|line| line.trim().strip_prefix("use crate::"))
        .chain(source.match_indices("crate::").map(|(at, _)| &source[at + 7..]))
        .filter_map(|rest| {
            let name: String = rest.chars().take_while(char::is_ascii_alphanumeric).collect();
            (!name.is_empty()).then_some(name)
        })
        .collect()
}

#[test]
fn a_utility_reaches_for_nothing_but_another_utility() {
    let allowed: BTreeSet<String> = UTILITIES.iter().map(|name| (*name).to_owned()).collect();

    for module in UTILITIES {
        let reached: Vec<String> = reaches(module)
            .into_iter()
            .filter(|name| !allowed.contains(name))
            .collect();

        assert!(
            reached.is_empty(),
            "src/{module}.rs reaches for {reached:?}, which is not a utility — either that \
             module belongs down here too, or {module} has stopped being reusable"
        );
    }
}

#[test]
fn the_utilities_named_here_are_the_ones_that_exist() {
    // A module renamed or removed would leave this test quietly checking nothing.
    for module in UTILITIES {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("src").join(format!("{module}.rs"));
        assert!(path.exists(), "{} is named as a utility and is not there", path.display());
    }
}
