# witchlight

Renders and serves a browsable map of a Vintage Story world.

This is the renderer half. It never reads a save file and does not need the game
installed: everything it draws comes from exports written by the companion server
mod in [`witchlight-csharp`](../../witchlight-csharp). The mod knows the game; this
knows pixels.

## Tests

```sh
cargo test                     # everything, including the viewer's
node tests/viewer.mjs          # just the viewer's, with its output
./tests/zoom-sweep.py URL      # against a running map, in a real browser
```

The viewer is JavaScript and is tested as JavaScript. `tests/viewer.mjs` lifts the
functions out of `src/viewer/*.js` by name and runs them, rather than keeping a
second copy to check — a copy passes happily while the page it stands for is
broken, which is how the clamp was once removed from `draw` without a single
check noticing. It reads the file list out of `src/viewer.rs`, so a script added
to the page is a script the tests see. `cargo test` shells out to `node`, and says
so if it is missing.

`zoom-sweep.py` is the one that needs a map and a browser, so it is not part of
`cargo test`. It drives chromium across the zoom range and counts the colours that
reach the screen. The map has gone blank past some zoom four times, for four
unrelated reasons, and every one of them looked identical from the page and left
the viewer's own arithmetic correct — so this checks the only thing they had in
common, which is that the picture stops arriving.

## Running

The usual way is not to. The [server mod](../../witchlight-csharp) carries this
binary inside its archive and starts it once the world is ready, so installing the
mod installs the map — its settings are then `witchlight.conf` in the game's
`ModConfig` folder and everything it prints goes to `Logs/witchlight-service.log`.
Set `autostart = false` there to run it yourself instead, which is what a map that
should stay up while the game server is down wants.

Run by hand it looks like this:

```sh
witchlight                                   # serve, using the saved settings
witchlight -d /srv/vs/data                   # point it at a server's --dataPath
witchlight -d /srv/vs/data -S                # ...and remember that
witchlight render --out map.png              # one PNG of everything exported
```

Settings live in `~/.config/witchlight/config.toml`, written with the defaults on a
first run — or wherever `-c` names, which is how the server mod points this at its
own `ModConfig/witchlight.conf`:

```toml
vs_data = "/home/vintagestory/data"   # the server's --dataPath
map_data = ""                         # where maps are kept; empty means <vs_data>/witchlight
per_world = true                      # a directory per world inside it
bind = "0.0.0.0:8080"                 # every interface; 127.0.0.1 for this machine only
api_bind = ""                         # where the mod posts; empty means loopback on a free port
api_token = ""                        # what it must present; empty means a fresh one each start
markers_public = false                # whether a marker nobody has chosen for is everyone's
private_map = true                    # each person sees the map as they last saw it
anonymous_spawn = true                # under a private map, spawn is everybody's to see
anonymous_spawn_radius_chunks = 8     # how far from spawn that reaches
sight_radius_chunks = 0               # how far a player sees; 0 means their own view distance
live_refresh_ms = 1000                # how often the page asks, where it has to ask
export_interval_ms = 10000            # how often the mod checks the season and re-reads what moved
autostart = true                      # whether the server mod starts this itself
announce = true                       # whether it tells joining players where the map is
announce_url = ""                     # and what to tell them; empty means work it out

[commands]                            # who may run each `wl` command in game
login = "player"                      # a link to your own page of the map
mark = "player"                       # a marker where you are standing
portrait = "player"                   # ask a client for a picture of its player
palette = "admin"                     # ask a client for the map's block colours
icons = "player"                      # ask a client for the marker pictures
export = "admin"                      # write the surface of every loaded chunk
status = "admin"                      # the whole of what state the map is in
service = "admin"                     # start and stop the map service

[claims]                              # who may do what with the land claims
view = "player"                       # see where the claims are, on the map
create = "claimland"                  # draw a new one from the map
worldgen = false                      # draw the world's own trader perimeters
```

That sample is the settings trimmed to one line each. The file this writes carries
a note above every setting saying what it is for, so editing it needs nothing else
open beside it; the sample is here to show the shape at a glance.

`autostart`, `announce`, `announce_url`, `[commands]`, `[claims]` and `[bars]` are the settings
here that this program never reads. They say who starts the map, who is told about it and who
may ask it for things, which are questions about the pair rather than about either
half, so they sit with everything else about the map instead of in a file of their
own.

`private_map` is what a public server wants and a server of friends does not.
On, nobody is shown the world: each person is shown what they have been near,
and ground that changed while they were away stays as they remember it until
they go back — see *A map per person* below. Off, everybody is shown the same
map, which is what the map always did. `anonymous_spawn` and its radius decide
what a browser nobody has signed in on is shown under a private map, which is
the ground around spawn and nothing else; `sight_radius_chunks` is how far
standing somewhere reaches, as the crow flies, and zero is each player's own
view distance as the game granted it.

`live_refresh_ms` is the gap the page leaves between one live answer and the next
question, where it has to ask at all. The page is told of changes the moment
they arrive — see *Serving* — and falls back to asking on this clock only where
that is not working. Then players, markers, claims and the confirmation that a
marker just asked for was made all arrive on this one beat. Anything below
250 is served as 250 and anything above 60000 as 60000, and the page is told the
number when it is served — so a change reaches a browser once the service has
restarted and the page has been reloaded.

`[bars]` adds a bar to each player's card beside their health and their food:

```toml
[bars]
mana = "Mana | entitybehavior-resource-currentmana_rm | entitybehavior-resource-totalmaxmana_rm | #7c5cff"
```

A mod that gives players a resource keeps it on the player's own entity, in the
same watched attributes the game keeps health and hunger in — so the mod half
reads a number out of it without knowing anything about the mod that put it
there, and nothing anywhere compiles against one. Each entry is
`name | value attribute | maximum attribute | colour | group`; the key names the
entry and the entries are drawn in the order the file gives them.

The group is what the map files the bar under in **Bar display**, where a reader
switches bars on and off — the section builds itself from the bars a server has
actually sent, so it says nothing on a server with none. Left out of the entry,
the mod looks for an installed mod whose id appears in the attribute's own name
and uses that. It answers for a mod that names its attributes after itself and
for no other: an entity attribute is a name and a number with no record of what
wrote it, so this cannot be worked out properly and the entries that ship name
their group outright.

**A settings file with no `[bars]` section gets the two written above**, the way
one with no `[commands]` gets the command defaults — a file written before this
existed behaves as a fresh one, so upgrading and seeing nothing means the feature
does not apply rather than that it is broken. A `[bars]` section somebody has
emptied is them asking for none.

**A bar is drawn only for a player who has one.** A missing attribute, or one
whose maximum is zero, is a player this does not apply to — somebody who has not
taken up magic, a server without that mod — and no bar is the right picture of
that. It is also what makes naming an attribute nothing on this server keeps cost
nothing. The two entries written on a first run are what a stock
[Rustbound Magic](https://mods.vintagestory.at/rustboundmagic) uses.

`[commands]` takes `admin`, `player`, or any privilege the game itself knows —
`controlserver`, `chat`, `commandplayer` and the rest — which is how a server gives
a command to its moderators rather than to everybody or to nobody. A name the game
does not know is refused to everyone but an admin and said so in the log, so a typo
locks a command rather than opening it. The defaults split the commands that change
what the server is doing from the ones that answer a question about the person
typing them.

Each decides who may start the request, not whom it may be sent to: the mod asks
whichever client can answer, and what comes back is taken on the same terms
whoever asked for it — only an admin's palette or marker picture may replace one
already chosen, and anybody else's fills gaps. `wl status` prints the table in
force, which is the only place a server upgrading into this can see it — a settings
file written before `[commands]` existed says nothing about it, and nothing here
rewrites a file an operator owns just to add a section of defaults it is already
following. `witchlight -c <file> -S` writes one, at the cost of any comments in it.

`[claims]` is spelled the same way and takes the same words. It answers two
questions rather than one, because they are two: seeing where a claim is tells
somebody whether they may build there, and drawing one takes land. A server can
reasonably show every boundary to everybody and still let nobody but its
landholders draw one.

`view` starts at `player` because the game already sends every claim to every
client and draws the borders for anyone holding the right tool — a map that hid
them would be telling players less than the game does. `create` starts at
`claimland`, which is exactly what the game asks of `/land claim`: **the map is
never a way round a rule the server already has.** Narrowing `create` narrows the
map alone, and the mod checks the game's own privilege as well as this one, along
with the world's `allowLandClaiming`, the role's allowance and smallest size, how
many claims that person already holds, and whether the rectangle lands on
anybody's. A claim the map takes is one `/land claim` would have taken.

`worldgen` is the third question in that table and is not a permission: the game
protects a trader camp, a story structure and a tiled dungeon with a land claim of
its own, and those carry an owner's name with no owner behind it. They exist from
the moment that ground generated rather than from the moment anybody found one, so
drawing them hands every reader the location of every trader on the server — which
is why this starts `false`. The mod leaves them out of what it sends rather than
the page declining to draw them, because a claim that reached a browser is a claim
anybody may read out of it. `wl status` says how many claims the map draws beside
how many the server has, so the difference is visible from in game.

Set `announce_url` on any server a player cannot reach directly. Left empty, the
address given out is the one this machine can see for itself, which is right on a
LAN and wrong behind a proxy, a domain or NAT.

Flags win over the file and stay one-off unless `-S` is given, so a quick look at
another world does not rewrite your setup. `-p` prints the resolved settings.

`vs_data` is the game's data directory and exports are read from the `witchlight`
folder inside it, unless `map_data` names somewhere else. A directory holding
`palette.json` directly is accepted too, so files copied off a server with `scp`
need no second flag. Whichever it picks is printed on start.

`per_world` files each world's map in a directory of its own inside that folder,
named after the world. It is on unless turned off: every singleplayer save shares
one data path and would otherwise write its terrain into the last world's map, and
a dedicated server that wants its one map directly in the folder turns it off.
Nothing is shared between
those directories, not even the files that would be identical — one written once
and left alone costs nothing to keep, while one rewritten every time you switch
between a world with no mods and a world with fifty costs a disk.

The mod names the directory outright with `--exports` when it starts this, because
it is the half that knows which world is running. By hand it is only needed when
`per_world` is on and more than one world has been exported; with one, that one is
served, and with several the service says which it found rather than guessing.

It binds every interface by default, so the map is reachable from the rest of your
network as soon as it starts. On start it prints the addresses it can actually be
reached at, since `0.0.0.0` is not something anyone can type into a browser, and
writes them to `service.json` beside the export — the address on the network first,
because that is the one worth giving somebody else. That file is how the server mod
knows what to put in its own log and what to tell a player who joins.

Reaching it from the open internet is a further step: put it behind a reverse
proxy that terminates TLS. The service has no authentication, and everything it
serves — terrain, player positions, every marker with its owner's name — is
readable by anyone who can reach the port.

## What it tells you

```
witchlight 0.2.0
witchlight: reading /srv/vs/data/witchlight
witchlight: 635 chunks, 864x1024 blocks
witchlight: palette from client, 45418 blocks, 26 colour maps (game 1.22.7)
witchlight: surface 96% painted, 4% nothing to draw, 0% waiting on a colour, 0% unknown blocks
witchlight: serving on http://192.168.1.158:8080  (on your network)
```

The version comes first on every run, because `--version` only helps if you think
to ask. **`palette from server` versus `from client`** says which machine's assets
the colours came from — a dedicated server's own palette is nearly empty, and that
is the usual reason a map looks blank. The **surface** line walks every exported
column and classifies it, which separates "no terrain" from "no palette": a map
reading `0% painted, 100% nothing to draw` has plenty of terrain and no colours for
it. Below 25% painted it says so explicitly on stderr.

**Waiting on a colour** is a third answer and a narrower one: those columns hold a
block the palette says draws something and has no colour for. They are painted as
bare earth rather than as unexplored ground, because they are terrain — the map is
missing a colour, not the world. The mod repairs them by asking a player's client,
which is why this is reported rather than warned about; nothing here can fix one.

**Nothing to draw** is painted the same way and counted apart, which is the honest
picture of it: the column was exported and its height is known, and only the block
on top has nothing to show — air over a column with nothing under it, or one of
the invisible placeholders a large structure stands beside its real block. To an
operator reading coverage the two are different (one colour is being fetched and
the other will never exist); to anybody looking at the map they are both ground
whose top cannot be drawn. **Only a column nobody exported reads as absence.**

A flat square of that colour, chunk-aligned, in the middle of finished terrain is
not either of those. It is a chunk the mod exported while the server had let go of
its blocks and kept only the flat record above them, so every position in it read
as air — see `Readable` in the mod's `Columns.cs`, which now holds such a chunk
back rather than writing a chunk of sky. Squares written before that check repair
themselves the next time the chunk loads.
Painted otherwise it put black specks through finished terrain, each one reading
as a hole in a world that has no hole in it.

Reloads are narrated the same way, so a palette arriving mid-session is visible:

```
witchlight: palette reloaded from disk — 45418 blocks, source client (generation 7, tiles dropped)
witchlight: surface 96% painted, 4% nothing to draw, 0% waiting on a colour, 0% unknown blocks
```

## What it reads

| File | What it is |
|---|---|
| `palette.json` | every block: its id in this world, an average colour or which kind of colourless it is, and which colour maps tint it |
| `colormaps/*.png` | the game's lookup images, sampled by climate and by season |
| `map.sqlite` | **written here**: every chunk the map holds, every version of one somebody still remembers, and what each person has seen |
| `tiles/{level}/…` | **written here**, the zoom levels above one block per pixel |

The terrain is the service's own. The mod reads the surface of a chunk whose
blocks moved and posts it over the API channel within a quarter of a second —
a record of six bytes per column, deflated — and this side puts it in the
database, in memory, and on the screen of every browser looking at that square.
A chunk is written when it changes and never otherwise, and the file is updated
in place, so a quiet server touches the disk for nothing. The database is one
SQLite file, compiled into the binary, so nothing on the machine has to provide
one.

A region is sixteen chunks on a side, which at a chunk edge of 32 is 512 blocks
— **exactly one tile** at the finest level, and the same square the game calls
a map region. A chunk that changes therefore belongs to one tile, so this side
drops one tile rather than starting over.

The map used to be a directory of region files, `columns/r.{x}.{z}.msqr`,
written by the mod and watched from here. A service that starts with an empty
database and finds those files reads them once, whole, into the database, and
never again; the layout is still documented at the head of `src/columns.rs` for
that reading. The files may be deleted once the import has been logged.

**A map per person.** Under `private_map`, what somebody is shown is decided
from two things the database keeps about them — see `src/memory.rs`. Every
chunk within sight of where they have stood is *discovered*, one bit per chunk.
A discovered chunk that changed while they were not there is a *divergence*: a
pointer at the version they last saw, which stays in the database for as long
as anybody points at it. So a player leaving spawn keeps spawn as it was, a
player coming back has the divergence cleared and sees it as it is, and twenty
people who all remember the same old spawn share one kilobyte. A tile for a
reader is the tile everybody gets with ground they have never been near painted
out and ground they remember drawn from the version they remember, composed
when asked for and kept in memory only. Sharing is a choice each person makes
per group, in their own settings on the page: tick a group and everyone in it
sees what you have explored, as you last saw it. The ground around spawn is
everybody's, a browser nobody has signed in on included, as far as
`anonymous_spawn_radius_chunks` reaches.

**The palette and the colour maps are reloaded while the service runs.** On a
dedicated server the palette arrives from an admin's client some time after
start-up, and a service that read it once would show nothing until restarted.

Players and markers do not come from a file. The mod posts them on the **API
channel** — a second listener on loopback, on whatever port the machine had free,
whose port and token are written to `api.json` beside the map so the mod finds it
without being told. They are held in memory, because a position is worthless by
the time a disk has finished with it. Markers are the exception and are written to
`markers.json` when they arrive, so the map still has something to show when the
game server is off. That channel takes writes, which is why it is not on the map
port. Nothing here reads a file this build does not write, so an empty map means
nothing was posted rather than nothing was found. Every interface is written up in
[API.md](../../witchlight-csharp/API.md).

## How a pixel is decided

One pixel is one block. The column's block id gives a base colour from the palette.

A block the palette cannot draw is drawn anyway, and which of three ways depends on
what the palette says about it. A block it has never heard of is **loud magenta**,
because that is a bug in the export rather than a fact about the world. A block it
says draws nothing — air, an invisible helper — is left as **bare ground**, which
is what is there. A block it says draws something and has no colour for is
**earth**, taking the slope shading like any other terrain: the map is waiting on a
colour, and the ground a player just dug should read as ground while it waits. The
three used to be two, and dug soil shared a colour with a world nobody had walked
into.

Grass, leaves and water ship as **greyscale masks** in the game's assets, so a
tinted block is multiplied by its colour maps: the climate map sampled at that
column's own temperature and rainfall, and the season map at the chunk's position
in the year. Both are the same lookups the game's own shader does, taken from
`colormap.vsh`. The season map's second axis is per-position noise in the game,
stopping a forest being one flat colour; a hash of the block position stands in for
it, so the same block is the same colour on every render.

Per-column tinting is the part the established map mods do not do — they bake one
tint into the palette, sampled wherever the exporting player happened to be
standing, so a desert and a rainforest come out the same colour.

Relief comes from comparing each column's height with its northern and western
neighbours, lighting the world from the north-west the way every game map does.

## Serving

Tiles are 512×512 at one pixel per block, rendered **on request** and then cached,
so start-up costs nothing and only the part of the world someone looks at is drawn.

Terrain arriving bumps a **generation** counter, which appears in `/info.json`
and in tile URLs as `?v=N`. That is what gets new terrain past the browser
cache; tiles are otherwise marked immutable and cached for a year — privately,
where they were composed for one reader — while the page and both feeds are
`no-store`.

| Route | |
|---|---|
| `/` | the viewer: drag to pan, scroll to zoom |
| `/tiles/{level}/{x}/{z}.png` | one tile, versioned by `?v=`, drawn for whoever is asking |
| `/info.json` | bounds, chunk edge, chunk count, generation, as whoever is asking sees them |
| `/events?since=&live=` | held until the map or the live feed has moved past what the page last saw, then what moved |
| `/block.json?x=&z=` | what is at one block: its code, its surface height, its climate |
| `/live.json` | players and markers, from memory. A player carries `Facing`, which way they are looking in degrees clockwise from north, and `Portrait`, the name of their picture, with `PortraitAt`, when it was drawn, where they have sent one |
| `/icons.json`, `/icons/{name}.svg` | the pictures markers are drawn with |
| `/portraits/{name}.png` | a picture a player's own client drew of their seraph. Ask with `?v={PortraitAt}`: the name is the player's and does not change when the picture does |

The page asks `/events` and is answered the moment something moves — a long
poll, which is an ordinary request the service holds until there is something
to say, and which the page reissues as soon as it is answered. It carries the
tiles that actually changed since the page's generation, so a server where
someone is building repaints one square rather than the map, and each tile is
swapped only once its replacement has decoded — the old one stays up meanwhile,
so the map never blinks. Where the waiting request is refused or will not stay
open, the page falls back to asking `/info.json` every two seconds and
`/live.json` on the beat `live_refresh_ms` sets. Players draw
as cyan dots with their name; markers as diamonds in their owner's colour, with the
title above and the owner below — every death marker is titled "You died here", so
the owner is the only thing that says whose it is.

## The viewer

[Leaflet](https://leafletjs.com/), vendored into the binary rather than fetched
from a CDN, so the service stays one file that works offline and tells nobody who
is looking at the map. The marks its furniture wears are vendored beside it, from
[Phosphor](https://phosphoricons.com/): filled silhouettes, which is what the
game's own waypoint marks are, so the two read as one set. Only the marks the page
uses are compiled in, and `src/chrome.rs` is the list. See `src/vendor/README.md`
for the exact releases both came from and how to move version.

The **picker** — the block in a frame, below the zoom control — reads one block rather than
looking at all of them. It leaves the pointer an arrow, since a crosshair centres
on the block it is naming and so covers it. It outlines the block instead, which
at one pixel per block is the whole point of the tool, and asks `/block.json` what
is there:
the block's code, the height a player standing on it would read, and the climate
that column was generated with. A column nobody has exported says so instead of
naming a block that is not there. The reading is the same one the renderer made
for that pixel, so the words and the colour underneath them always agree.

The **marker list** answers what there is, which a map cannot once there are forty
markers spread over a million blocks. Split by who can see each one, searched by
name or owner, and ordered by any of its columns — a name, how far the place is
from spawn, how far it is from wherever that reader is standing, whose it is, or
who can see it. The distance from the reader is rewritten on every poll as they
walk; the order is taken when the list is drawn and left alone, since a list that
resorted itself every two seconds would move the row out from under the hand
reaching for it. Typing in its search box draws the markers it found **larger on
the map**, which is the other half of finding one.

**The list has a bulk edit.** Pressing it puts a column of boxes down the left of
the rows, headed by one that ticks the lot, and the row of buttons under the list
then acts on what is ticked rather than on what is listed — one rule for all four
of them, and each says the number it found. Two of those are new: a bin that asks
twice, and *From preset*, which opens a searchable list of that reader's presets
and rewrites every ticked marker's name, picture, colour and block from the one
they pick. Where the markers are is left alone, and so is who may see each of
them: that is a choice about one marker rather than a property of the kind of
thing it is.

**Right-clicking a marker opens it**, whether or not it is that reader's to
change. Their own opens as a form: the name, the colour, the picture, who may see
it, and a bin. Somebody else's opens as a record, with every box filled in and
none of them writable, offering the two things about it that *are* this reader's
own — keeping it in sight, and a preset shaped like it. The marker list opens the
same window from a row.

**A marker carries which block it is about**, read by the game under it when it
was made and again wherever it moves. That is what a preset made from a marker is
keyed on, so turning one into a preset arrives with the pattern already filled in
— it had nothing, and a preset with no pattern names no block. A marker made
before the mod kept the answer falls back to what the map drew at that spot.

**Keeping a marker in sight** pins it on that reader's own map in game: the game
holds a pinned waypoint against the edge of the map instead of letting it scroll
off. It is one person's choice about one marker and changes nothing anybody else
sees — pinning somebody's marker puts it on the pinner's map and on no other. Any
marker a reader is sent may be pinned, since being able to see it is the whole of
the permission. Their own is the flag the game already keeps on the waypoint, so
the in-game map dialog and this one are the same switch; everybody else's is a
decision the mod stores beside the visibility choices.

Markers can also be drawn larger for good — *Marker size*, in the accessibility
panel, beside the colour and colour-vision settings. It multiplies the size the
game itself draws a mark at, so where a mark sits on its block and the shadow it
is read against scale with it, and the search's own step sits on top of whatever
is set.

Every button the map wears is in **one column down the left edge** — who you are,
what the map shows, the markers, the claims, the zoom and the block picker, in
that order. The zoom used to hang from the middle of that edge instead, which
made two stacks that slid against each other as the window changed size and put
one straight through the other on a short one.

The **world's clock** is in the opposite corner, beside the list of who is on:
what the world is doing rather than something to do to it, and a readout in the
middle of a column of controls reads as one more thing to press. It takes the
corner itself on a server with nobody on, since the list takes itself off then.

The **land claims** are a layer of their own, switched by the button in the tool
column or from the settings panel — the two are one switch and each follows the
other. Every claim is drawn one shaded rectangle per area, so a claim built out of
several adjacent boxes keeps the shape of its boundary rather than being wrapped
in a box around the lot; opening one says who holds it, what they called it, and
how far up and down it reaches. Each owner's land is its own colour, taken from
their own identity so that the same person's ground is the same colour on every
screen and in every session — near hues can collide, which is why whose a claim is
stays written in the popup and in the list. A claim is drawn on ground the map
holds and nowhere else: one reaching past the explored edge is drawn as far as the
map goes, and one entirely outside it is not drawn, since a boundary ruled across
the black would be the map showing what it has never been told. Which claims a reader is sent is decided by the
service against a list the mod supplies, so a reader who may not see them is sent
an empty array rather than a full one to hide — a browser cannot be asked to
forget what it has been handed.

Somebody the server lets claim land gets a second button, which **draws a new
claim**: one drag across the ground, with a crosshair and the map held still
under it, then a window holding what was dragged. The window says what the claim
comes to in cubic metres and what that leaves of their allowance, and refuses one
that is plainly too large before spending a round trip on it — the game decides
for real either way, and this only saves twenty seconds of watching for a claim
that was never going to arrive.

The form is a column of labelled sections rather than a row of boxes: a name, the
area as *West*/*North* and *East*/*South*, a depth, and who else may. The last is
the game's own two everybody-permissions and a list of players who may build,
which is exactly what `/land claim grant` offers — a third of anything would be a
permission system the game would not enforce.

A third button lists **every claim there is**: what there is, whose it is, where,
and a way into the ones this reader may change. Renaming, saying who else may,
and giving a claim up all happen there. The ground is shown and not editable —
moving a boundary has to be judged against every other claim and against an
allowance, and a map cannot show somebody what they would be giving up, so
redrawing is making a new one.

Depth is asked for rather than assumed, though the map is drawn from above. An
allowance is counted in cubic metres and depth is most of what a claim's volume
is, so making every claim the whole height of the world would hand a survival
player a square thirty-two blocks across. It starts as a band around the ground
under the middle of the rectangle, which is what somebody marking out a base
means, and *All* is there for the claims that really do want the lot.

The **chunk grid** outlines the chunks the export names, which the game puts at 32
blocks. It is off until switched on, faint enough to read the terrain through, and
it stops being drawn below eight pixels to a chunk — past that a grid stops being
a grid and becomes a wash. It is deliberately not clipped to the explored area:
seeing where that stops, squarely, is half of what a grid is for.

## How it is laid out

One subject to a file, and a utility never sits inside the system that first
needed it. Reading order, roughly outside in:

| | |
|---|---|
| `main.rs` | the command line, and what each subcommand does |
| `config.rs` | the settings file, and the flags laid over it |
| `server.rs` | bringing the map up: read what is there, bind, hand out threads |
| `routes.rs` | what the public port answers, and to what |
| `apiport.rs` `api.rs` | the private channel the mod posts on, and where it is published |
| `state.rs` | what the request threads share |
| `store.rs` | the map's own database: chunks, remembered versions, what each person has seen |
| `memory.rs` | what each person remembers of the map, and who shares it with whom |
| `scope.rs` | what one reader is shown: the whole map, or their memory of it |
| `events.rs` | telling a browser the moment something changes |
| `watch.rs` | noticing that the mod has written a palette or the block names |
| `feeds.rs` | the JSON the page asks for |
| `viewer.rs` `viewer/` | the page: markup, style, and the scripts joined in order |
| `chrome.rs` | which marks the furniture wears, and which of the vendored pack reach the binary |
| `columns.rs` `pyramid.rs` `render.rs` `palette.rs` `color.rs` | the map itself, from region file to pixel |
| `live.rs` `pending.rs` `preferences.rs` `auth.rs` `facts.rs` `wire.rs` | what the two halves say to each other |
| `http.rs` `urls.rs` `cache.rs` `net.rs` `files.rs` `random.rs` `error.rs` | utilities, which know nothing about maps |

Nothing in the last row imports anything above it. That is the whole of what
keeps them reusable, and it is worth checking before adding to one.

## Rough edges

- **No authentication on the map port.** Anything that can reach it sees
  everything, apart from markers their owners kept private — those are filtered
  before the page is answered, since a browser cannot be asked to hide what it
  has been handed. The API channel is separate and takes writes, so it answers
  only on loopback and only to a caller carrying the token from `api.json`.
- **Sessions live in memory.** A restart of the service costs everyone one click
  of one login link, and the map keeps nothing about anybody it was not asked to.
