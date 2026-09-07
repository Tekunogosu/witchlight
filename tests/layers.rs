//! The one structural rule this crate has, checked rather than asserted.
//!
//! A utility is a utility because nothing above it can pull it back down. Once
//! `urls` reaches for `state` to answer one question it stops being a thing
//! another program could lift out, and becomes part of the map service.
//!
//! Breaking this leaves the code compiling and the tests passing, so it is read
//! off the source rather than trusted.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// Where the modules that know nothing about maps live.
const UTILITIES: &str = "src/util";

/// The utility modules, by name, read off the directory itself.
fn utilities() -> BTreeSet<String> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join(UTILITIES);
    let entries = std::fs::read_dir(&dir)
        .unwrap_or_else(|error| panic!("{} should be readable: {error}", dir.display()));

    entries
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            let name = path.file_stem()?.to_str()?.to_owned();
            (path.extension()? == "rs" && name != "mod").then_some(name)
        })
        .collect()
}

/// Which modules a utility reaches for.
fn reaches(module: &str) -> BTreeSet<String> {
    let path: PathBuf =
        Path::new(env!("CARGO_MANIFEST_DIR")).join(UTILITIES).join(format!("{module}.rs"));
    let source = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("{} should be readable: {error}", path.display()));

    source
        .match_indices("crate::")
        .map(|(at, _)| &source[at + "crate::".len()..])
        .filter_map(|rest| {
            let name: String = rest.chars().take_while(char::is_ascii_alphanumeric).collect();
            (!name.is_empty()).then_some(name)
        })
        .collect()
}

#[test]
fn a_utility_reaches_for_nothing_but_another_utility() {
    let allowed = utilities();

    for module in &allowed {
        // A utility names its siblings as `crate::util::files`, so `util` is the
        // segment that appears and the sibling's own name follows it.
        let reached: Vec<String> =
            reaches(module).into_iter().filter(|name| name != "util").collect();

        assert!(
            reached.is_empty(),
            "{UTILITIES}/{module}.rs reaches for {reached:?}, which is not a utility — either \
             that module belongs down here too, or {module} has stopped being reusable"
        );
    }
}

#[test]
fn the_utility_tier_is_not_empty() {
    // A renamed or emptied directory would leave this file quietly checking
    // nothing at all.
    let found = utilities();
    assert!(found.len() >= 8, "the utility tier should still hold its modules, found {found:?}");
}
