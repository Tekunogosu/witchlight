# Vendored dependencies

Third-party code kept in-tree rather than fetched at runtime, so the service
ships as a single binary that works with no network access and no install
step. Loading a library from a CDN on every page view would also break the
map whenever that CDN is unavailable, and would expose visitor information
to a third party.

## leaflet 1.9.4

- `leaflet.js`, `leaflet.css` — unmodified `dist/` contents from the release archive
- `leaflet-LICENSE` — BSD 2-Clause, (c) 2010-2023 Volodymyr Agafonkin

Downloaded from the project's own release, not a package registry:

```
https://github.com/Leaflet/Leaflet/releases/download/v1.9.4/leaflet.zip
  sha256  aaec1d5c3239a613a53e996087629aca1483cb2f0438b11b8a335c6cede4c16b
https://raw.githubusercontent.com/Leaflet/Leaflet/v1.9.4/LICENSE
```

Leaflet has **no dependencies of its own**, which is why vendoring it here
covers the library in full. Upstream: <https://github.com/Leaflet/Leaflet>,
maintained by its author and other contributors under the Leaflet
organization.

The archive's `dist/images/` (Leaflet's default marker icons) is omitted —
every marker this map draws is a `divIcon` styled in the page itself, so
those default assets are never requested.

### Updating

Download the release archive for the new tag, replace `leaflet.js` and
`leaflet.css` from its `dist/`, replace the license file from the matching
tag, and record the new archive hash above. Use the project's own release
archive, not a copy from a package registry or another project's bundle —
several such copies exist at the same version with different minification,
and none of them matches what upstream actually published.

## phosphor-icons 2.1.1

- `phosphor/{bold,duotone,fill,light,regular,thin}/` — unmodified `assets/`
  contents from the source tree: 1,512 icons per weight, six weights
- `phosphor-LICENSE` — MIT, (c) 2023 Phosphor Icons

Downloaded from the project's source tree at the commit below, not a package
registry:

```
https://codeload.github.com/phosphor-icons/core/tar.gz/2b75f3ad12b420c9504ef05df8d2564a28f8500e
  sha256  0af5d95aa1a57d8f47ef4dbe93623bf18743e233a5fe428519fc0ab3d097696b
```

Pinned to a commit rather than a tag because the tags are stale: `v2.0.8` is
the newest tag on GitHub, and it predates 264 of these icons, including
`map-pin-simple`, which the viewer uses on two buttons. The commit above
corresponds to the last version this project published to npm; every one of
its 9,072 assets is byte-identical to `@phosphor-icons/core@2.1.1`, checked
file-by-file across every weight. The only difference found anywhere is that
the npm package's license file uses CRLF line endings; the vendored copy here
uses the source tree's LF version instead, for consistency with the rest of
the repository.

Phosphor has **no dependencies**: the vendored content is 9,072 SVG files,
each a single `<path>` element with no script and no external references.

Only the icons listed in `src/chrome.rs` are compiled into the binary. The
rest are kept at a known, pinned version so that a future addition doesn't
need to be fetched separately. That file also specifies which weight is used
per icon: filled weight matches the game's own waypoint icon style, except
`x`, which is taken from `bold` because the filled weight renders as a solid
square with the cross cut out of it, rather than as an X shape.

### Updating

Download the tree archive for the new commit, replace all of `phosphor/`
with its `assets/`, replace the license file, and record the new hash above.
Verify the icon names in `src/chrome.rs` still resolve — each is loaded via
`include_str!`, so the build will fail if one doesn't.
