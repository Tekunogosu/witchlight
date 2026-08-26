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

## Moving version

Download the release archive for the new tag, replace `leaflet.js` and
`leaflet.css` from its `dist/`, replace the licence from the matching tag, and
record the new archive hash above. Do not substitute a copy from a registry or
another project's bundle: several of those exist at this same version, minified
differently, and none of them is what the project published.
