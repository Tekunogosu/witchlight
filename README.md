# Witchlight

A web map server for Vintage Story. It renders a live, zoomable, Leaflet-based
map of a server's explored terrain, with player positions, markers, and land
claims.

Witchlight is the renderer half of a pair. It does not read the game's save
file and does not require the game to be installed — all map data comes from
exports written by the companion server mod,
[`witchlight-csharp`](https://github.com/Tekunogosu/witchlight-csharp). The
two are developed and released together, in separate repositories for
organization only: **both always carry the same version number**, even when
a release only changes one side.

## Features

- Live tile rendering from block data, with per-column climate and season
  tinting instead of a single baked palette.
- Player positions and markers, pushed to the browser over a long-poll
  connection so only what changed gets redrawn.
- Land claims, drawn as shaded regions with owner and permission info.
- Per-player "personal maps": each player sees only the terrain they've
  explored, as it looked when they last saw it. Optional — a server can show
  everyone the same live map instead.
- Configurable per-command privilege levels, marker visibility rules, and
  arbitrary player status bars (health/food plus anything another mod
  exposes on the player entity).
- Single static binary. Leaflet and the marker icon set are vendored, not
  fetched from a CDN, so the service works offline and doesn't leak visitor
  information to a third party.

See [ARCHITECTURE.md](ARCHITECTURE.md) for how any of this works internally,
the full configuration reference, and the HTTP interface.

## Plugins

Another mod can keep rows on the map and draw them on the web map. It declares
what its rows look like, sends them, and ships a script the page runs. The
service stores the rows and decides who may see each one.

See [docs/PLUGINS.md](docs/PLUGINS.md) for how to write one, and
[docs/witchlight.d.ts](docs/witchlight.d.ts) for the page API an editor reads.

## Installing

Most users don't need to build or run this directly. Install the
[server mod](https://github.com/Tekunogosu/witchlight-csharp) instead — it
bundles this binary and starts it automatically once the world is ready.
Settings then live in `witchlight.conf` in the game's `ModConfig` folder, and
logs go to `Logs/witchlight-service.log`.

## Building

Requires the Rust toolchain (stable) and Node.js (used only to run the
viewer's own test suite).

```sh
git clone https://github.com/Tekunogosu/witchlight.git
cd witchlight
cargo build --release
```

The binary is written to `target/release/witchlight`.

```sh
cargo test                     # everything, including the viewer's own tests
node tests/viewer.mjs          # just the viewer tests, with verbose output
```

See [ARCHITECTURE.md](ARCHITECTURE.md#testing) for what each test covers and
how the browser-based visual regression check works.

## Running

```sh
witchlight                                   # serve, using the saved settings
witchlight -d /srv/vs/data                   # point it at a server's --dataPath
witchlight -d /srv/vs/data -S                # ...and save that as the new default
witchlight render --out map.png              # render one PNG of everything exported
```

Set `autostart = false` in the mod's config to run Witchlight this way
instead of letting the mod manage it — useful if the map should stay
available while the game server is down.

Full configuration reference, including all defaults and what each setting
does, is in [ARCHITECTURE.md](ARCHITECTURE.md#configuration).

## License

MIT. See [LICENSE](LICENSE).
