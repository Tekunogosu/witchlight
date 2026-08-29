# Vendored

Third-party code, kept here rather than fetched at run time, so that the service
is one binary that works with no network and no install step. A map that pulled a
library from a CDN on every page load would stop working when the CDN did, and
would tell whoever runs it that someone is looking at the map.

## leaflet 1.9.4

- `leaflet.js`, `leaflet.css` — `dist/` from the release archive, unmodified
- `leaflet-LICENSE` — BSD 2-Clause, (c) 2010-2023 Volodymyr Agafonkin

Taken from the project's own release, not from a package registry:

```
https://github.com/Leaflet/Leaflet/releases/download/v1.9.4/leaflet.zip
  sha256  aaec1d5c3239a613a53e996087629aca1483cb2f0438b11b8a335c6cede4c16b
https://raw.githubusercontent.com/Leaflet/Leaflet/v1.9.4/LICENSE
```

Leaflet has **no dependencies of its own**, which is the reason a map library is
an acceptable thing to take on at all: what is vendored here is the whole of it.
Upstream is <https://github.com/Leaflet/Leaflet>, maintained by its author and a
handful of others under the Leaflet organisation.

The `dist/images/` in the archive are Leaflet's own marker icons, and are left
out: every marker this map draws is a `divIcon` styled in the page, so nothing
ever asks for them.

### Moving version

Download the release archive for the new tag, replace `leaflet.js` and
`leaflet.css` from its `dist/`, replace the licence from the matching tag, and
record the new archive hash above. Do not substitute a copy from a registry or
another project's bundle: several of those exist at this same version, minified
differently, and none of them is what the project published.

## phosphor-icons 2.1.1

- `phosphor/{bold,duotone,fill,light,regular,thin}/` — `assets/` from the source
  tree, unmodified: 1,512 icons in each of six weights
- `phosphor-LICENSE` — MIT, (c) 2023 Phosphor Icons

Taken from the project's own tree at the commit below, not from a package
registry:

```
https://codeload.github.com/phosphor-icons/core/tar.gz/2b75f3ad12b420c9504ef05df8d2564a28f8500e
  sha256  0af5d95aa1a57d8f47ef4dbe93623bf18743e233a5fe428519fc0ab3d097696b
```

A commit rather than a tag, because the tags have gone stale: `v2.0.8` is the
last one GitHub carries and it predates 264 of these icons, `map-pin-simple`
among them, which the viewer wears on two of its buttons. The version above is
the one the project last published to npm, and all 9,072 assets in
that release are byte-identical to the commit vendored here — every file of every
weight, checked against `@phosphor-icons/core@2.1.1`. The one difference anywhere
is that the registry's copy of the licence carries CRLF line endings, and there
the tree won, on the same principle as the leaflet entry above.

Phosphor has **no dependencies**: what is vendored is 9,072 SVG files, each a
single `<path>` with no script, no external reference and nothing to resolve at
run time.

Only the handful named in `src/chrome.rs` is compiled into the binary — the rest
is kept so that a mark wanted later is already here, at a known version, rather
than being fetched by whoever wants it. That list is the one place a new mark is
added, and it names the weight per icon: the filled weight matches the game's own
waypoint silhouettes, but `x` is filled as a *square with the cross knocked out*,
so the mark that shuts a window is taken from `bold`.

### Moving version

Download the tree archive for the new commit, replace the whole of `phosphor/`
from its `assets/`, replace the licence, and record the new hash above. Check the
names in `src/chrome.rs` still resolve — the build says so, since each is an
`include_str!`.
