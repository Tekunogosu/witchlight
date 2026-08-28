//! The viewer's tile arithmetic, run as the browser runs it.
//!
//! How many tiles a view asks for is the difference between a map that draws and
//! one that stalls: the same view has cost nine tiles and seventy-nine thousand,
//! and nothing about reading the code said which. The numbers are small, the
//! rounding is where the mistakes live, and both of the ones made here were found
//! by running it rather than by looking at it.
//!
//! The functions are lifted out of `src/viewer/*.js` at run time rather than
//! copied, so this tests what is actually served.

use std::path::PathBuf;
use std::process::Command;

#[test]
fn the_viewer_asks_for_no_more_tiles_than_it_can_afford() {
    let script = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/viewer.mjs");

    let run = Command::new("node").arg(&script).status();

    let status = match run {
        Ok(status) => status,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            panic!(
                "node is needed to run the viewer's tests, and is not on the path.\n\
                 The viewer is JavaScript and is tested as JavaScript; there is no\n\
                 second copy of this arithmetic in Rust to check instead.\n\
                 Install node, or set WITCHLIGHT_SKIP_VIEWER_TESTS=1 to go without."
            );
        }
        Err(error) => panic!("could not run {}: {error}", script.display()),
    };

    assert!(status.success(), "the viewer's tile arithmetic is wrong; see above");
}
