# witchlight

The map service. It pairs with the [server mod](../../witchlight-csharp), and the
two **must match on minor version**: minor is the compatibility generation, moved
whenever the file format or the socket protocol changes. Patch is free to differ,
and covers anything that changes only one half.

## 0.20.2

**A marker opened by hovering comes down on its own**, a couple of seconds after
the pointer leaves it, rather than staying up over the map. Moving onto the box
to read it cancels that, and a marker opened by clicking stays open — a click is
not a hover however it started.

**The marker and presets buttons are one control.** Stacked in a single bar with
a rule between them, the way the zoom pair is built, because one makes a marker
and the other decides what a marker starts as.

## 0.20.1

The marker box reads left to right: where it is on the left, whose it is on the
right. The coordinates hold their width and a long name gives way to an ellipsis,
because a place is read digit by digit and a name that has run out of room still
says who.

## 0.20.0

**Your settings survive a reload.** They never had: a later `remember(marker)`
for presets silently replaced the `remember()` that writes the browser's own
settings, so every call reached the wrong one and nothing was stored. Nothing
reported it — not the parser, not the page. There is a test now that fails on any
two top-level declarations sharing a name.

**The interface is grey rather than black**, on a measured palette. Black hid
anything dark drawn on it — the game's own near-black waypoint colour came out
invisible in the picker — and read as a hole in the map rather than a panel over
it. Body text now clears 4.5:1 against the surface behind it, where the muted
grey used before came to 3.3:1; swatches carry a light hairline so a near-black
one is still a square; and the boxes you type into clear 3:1, which on a dark
theme no fill difference can do.

**Sizes and spacing come from a scale**, four steps of each, so a panel reads as
one thing rather than a collection of separately tuned boxes. Small print went up
a step, pointer targets are at least 24px square, and the windows have room to
breathe.

**A marker says what it is more clearly.** Its name is the heading; whose it is
and where it is sit under a rule in a footer, smaller and quieter — rather than
all five facts run together at one size.

**Two more things in Show**: marker names worn permanently, and a marker's
details opening on hover.

**The presets button is clear of the marker button.** It never moved: a rule for
the corner column outweighed the one setting its margin, and quietly zeroed it.

## 0.19.3

**The mark beside your name sits beside it.** The tool buttons are drawn as a
grid, which gave the mark a row of its own and stacked it above the name.

**Cancel closes the profile window**, putting the switches back on the way out —
the mark in the bar, further out where a hand is already reaching for Save.

**The size sliders are live again**, landing when the hand lets go rather than on
every step. Applying mid-drag rescaled the window the slider was in and walked it
out from under the pointer.

**"Set as preset" is back on every new marker**, whether or not a block was
clicked, with a box under the name saying what it is remembered against.

**Presets can be created**, from the button in the corner of their window. That
box searches the game's blocks as you type, by name or by code, so a preset can
be keyed on something without going and clicking it first. `GET /blocks.json?q=`
answers it.

A query value is percent-decoded now. Every other one this service reads is a
number or a word of hex; a search box is the first that can hold a space, and
matching against `%20` found nothing at all — which reads as a search with no
answers rather than one that never asked.

**More room between the marker button and the presets button.**

## 0.19.2

**The × on a window closes it.** The bar captured the pointer on every press,
including a press on its own close mark, which sent the click to the bar instead
of the button. A press on anything in the bar that does its own job no longer
starts a drag.

**Presets have their own button**, under the flag, and have left the profile
window. Picking one from the list opens it in the marker window beside it, filled
in, with Save reading **Update** — which is how a preset is changed rather than
only made and deleted.

**Nothing in the profile window applies until Save.** The size sliders were the
reason: applying as they moved rescaled the window the slider was in and walked
it out from under the pointer.

**The mark beside your name has room around it**, and a rule between it and the
name.

## 0.19.1

Shared markers are fixed in the mod; the service and viewer are unchanged and the
version moves only to keep the halves reporting the same number.

## 0.19.0

**Deploy both halves together.** The mod collects markers in a new shape — makes
and changes rather than makes alone — and posts a block name table the service
serves from.

**Markers can be edited.** Right click one on the web map to open it in the form.
Its owner always may; `markers_public_editable`, new and off, lets anybody correct
a marker anybody can see. Whoever asks, the mod decides again against the waypoint
itself before anything moves.

**Marker presets.** Right click a block and the form starts as that block: named
what the game calls it, and — once you have saved a preset for it — coloured,
pictured and shared the way you last chose. "Set as preset" on the form keeps one;
the presets window edits and deletes them. A pattern may use `*`, so one preset
saved on basalt copper ore can be widened to `game:ore-*-nativecopper-*` and
cover every rock it appears in.

**A settings window of your own**, behind your name in the corner. It holds
whether new markers become presets, whether they are private — over the server
default, either way — and three size sliders for the player list, the windows and
the map buttons. What is about you follows your account to any browser; what is
about the screen stays in it.

**The map pans past what has been drawn**, by a world's width on every side,
rather than pinning the edge of the export to the edge of the screen.

**The settings button has moved to the top left**, beside your name.

The service keeps its first file of its own, `preferences.json`, holding each
person's presets and defaults against their uid. `GET`/`PUT /me/preferences` reads
and writes it, `PUT /markers/{key}` changes a marker, and `/block.json` now says
what the game calls the block as well as its code.

## 0.18.1

The marker form is a window rather than a panel: it floats over everything, it is
moved by its bar, and it closes with the × in its corner. Opening it no longer
shifts the zoom and inspect buttons out from under it — they stay where they are
and it covers them. A window dragged past an edge keeps a grip of itself on
screen, and a browser resized under one pulls it back in.

The colour and picture rows lost their labels, which named what was already
visible, and the place row reads `Coords — relative` or `Coords — absolute` after
the setting that decides it.

## 0.18.0

**Deploy both halves together.** The mod posts markers in a new shape — sorted by
who may see them — and the service does not read the old one. A service on 0.17
paired with a mod on 0.18 shows an empty map, and the reverse shows one that never
updates.

**Markers can be made from the web map.** Right click the map for a form on the
left, or press the flag in the corner and type the coordinates — or press ⌖ and
click the spot. The form offers the same colours and the same pictures the game's
own waypoint dialog does, read off the game rather than written down, so a mod
that adds either adds it here. Making one needs a login link; the flag appears
once you have followed one.

**A marker can be kept to its owner.** The box beside Save decides, and it starts
where `markers_public` puts it. That setting now means what it always said it
meant — the default for a marker nobody has decided about — and a choice made on
the form overrides it both ways, on the web map and on other players' in-game maps
alike.

**Markers a viewer may not see no longer reach their browser.** The web map used
to send every marker to everybody regardless of the setting. With `markers_public`
off, which is the default, markers made in game are now their owner's alone there
too, and a map that showed everything will show less until their owners share them.

The service decides who is sent which marker, from the session a browser
carries; it still never reads a waypoint, because the mod hands it two lists
rather than one. `GET /colors.json` is new, `POST /markers` is the first thing
the public port accepts rather than serves, and `POST /markers/pending` on the
API channel is how the mod collects what was asked for.

## 0.17.1

Marker colours are fixed in the mod; the service and viewer are unchanged and the
version moves only to keep the halves reporting the same number.

## 0.17.0

**Deploy both halves together.** The mod and the service no longer speak the
protocol they did before this, and neither reads what the other minor wrote.

**A settings file older than this stops the service**, which says which name
replaced which: `api_socket` is now `api_bind`, an address rather than a unix
socket path. Leave it empty for loopback on a free port, which is where the mod
now looks.

**Markers are private now.** Every marker used to reach every player's in-game
map; `markers_public` decides that, and it is off. Until a player can mark a
single waypoint as shared, off means nothing is shared in game at all — a server
that wants what it had sets `markers_public = true` for now.

The map is **not** cleared: the region format is untouched.

### A palette that has not changed does not redraw the map

The palette is reloaded when its file's timestamp moves, and reloading one costs
every tile in the cache and a redraw of every stored level — seconds of blank map.
A file rewritten with the same colours in it is not a new palette, so the
timestamp moving is what prompts a look and the colours themselves are what
decide.

Compared by what would be drawn rather than by the file, because the file carries
things that do not decide a colour: a palette rewritten by a different admin with
the same assets is the same palette as far as anything drawn from it is concerned.

### The map knows who is looking at it

A player runs `/witchlight login` in the game and follows the link they are sent.
The service mints the word on the API channel, spends it at `/login`, and hands
back a session in a cookie — `HttpOnly`, `SameSite=Lax`, thirty days, its clock
going back to the full term on every request that carries it.

In a cookie rather than in the address, because the address is meant to be shared:
`#x,z,scale` exists so a view can be pasted to somebody, and a session in the path
would travel with it. It also keeps tile URLs out of a per-session namespace, which
is what makes them cacheable at all.

Sessions live in memory and go when this stops. That costs one click of one link,
and it means the map keeps nothing about anybody it was not asked to keep.

`/me.json` says who is looking, in the same shape logged in or not. **The map
itself stays public**: a session decides only whose settings and whose markers a
page may act on.

### Two columns down the left edge

What moves the map hangs from the middle, where a hand reaches for it and where
there is room either side to grow: zoom, and the block inspector. Who you are sits
in the top corner, out of the way of the map and where a page puts an account.

The account button says your name when you are logged in, and is greyed and reads
"Unauthenticated" when you are not. Always there rather than appearing on login —
a control that appears moves everything under it, and a page whose furniture jumps
is a page somebody clicks the wrong thing on. One clear button's height below it
sits what it gates, offered to anybody who can act on it: somebody logged in, or
anybody at all where the operator has set `markers_public`.

Neither button does anything yet.

### `markers_public`

Whether a marker nobody has decided about belongs to everybody. Off by default,
and read by the mod as well as here, so the in-game map and the web map cannot
disagree about who can see what.

### The page says which build served it

Softly, in grey, beside the settings cog. Compiled into the page rather than
fetched from `/info.json`, so a page can only ever report the build that sent it —
asking would let a cached page name a version it never came from.

The two halves must match on minor version, and until now the only way to check
the one being looked at was to go and ask the machine.

### The map no longer goes blank when you zoom in

Level 0 is drawn on demand from the palette; every level above it is a picture
stored when it was last built. So a palette with no colours in it blanked the
finest level while the rest of the pyramid went on showing the world — a map that
worked until you zoomed past about 0.7 pixels per block and then went dark. It
read as a zoom bug three times and was never one.

Three things change.

A palette that colours nothing no longer draws anything. Level 0 falls back to
the level above it, enlarged: a coarse map instead of an empty one. Leaflet does
not substitute a parent tile of its own accord, so a tile that cannot be answered
is simply absent, which looks exactly like a map that has broken.

A palette that colours nothing no longer redraws the stored levels either. Every
level is built from level 0, so reloading an empty palette used to replace a
working pyramid with a blank one — and those pictures are the only thing left to
look at until a real palette arrives. They are now left exactly as they are, and
the service says so.

The levels record which palette drew them, in `tiles/painted-by`. Levels drawn
with a different palette than the one in use disagree with the level below them,
which is a map that changes as it is zoomed; they are redrawn at start when they
disagree — but only when there is a palette to redraw them with.

### Ground with nothing on it no longer looks like a missing tile

Unexplored ground draws as `#141416` and the page behind the map was `#14141a`,
four units apart. A map with no tiles and a map with nothing on it were the same
picture, which is why four separate faults over four releases were all reported as
"the map is black" and each took a fresh investigation to tell apart. The page now
carries a faint hatch: flat means the map has been here and found nothing, striped
means no tile arrived.

### A zoom sweep, in a real browser

`tests/zoom-sweep.py` drives chromium across the zoom range and counts the colours
that reach the screen. Terrain is thousands; an empty screen is a few hundred. Each
of the four faults left the viewer's own arithmetic correct, so none was reachable
from `tests/viewer.mjs` — what they had in common was only ever visible on screen.

Not part of `cargo test`: it needs a map worth looking at and a browser.

### The mod and the service meet on loopback, not a unix socket

The two halves talked over a unix socket in `/tmp`, named after the export
directory so both sides found it without being told and two game servers on one
machine did not collide. The filesystem decided who was allowed to connect, which
is the right shape — and a mechanism Rust does not have on Windows, where a
Vintage Story server is perfectly happy to run.

The shape is kept and the mechanism replaced. The service now listens on
`127.0.0.1`, on whatever port the machine had free, and writes that port and a
fresh random token into `api.json` beside the map, mode `0600` where the system
has modes. Every post must carry `Authorization: Bearer {Token}`; without it the
answer is `401`. Nothing off the machine can reach loopback and nothing on it can
post without reading a file only its owner can read, which is what the socket's
permissions bought. The port is asked of the machine rather than derived from the
export path, so two servers on one box still collide with nothing, and neither
side is configured.

The port changes with every service start, so where the service is is a belief
about it rather than a fact. The mod reads `api.json` again whenever a post fails
or is refused `401`, which also covers the ordinary case of a mod that started the
service a moment ago and looked before the file existed.

`api_socket` is now **`api_bind`**, an address rather than a path, with
`api_token` beside it; both are empty by default and only worth setting for a mod
on another machine, which is the one case a file beside the map cannot reach.
A settings file naming `api_socket` stops the service and says which name replaced
it — an unknown setting is otherwise indistinguishable from a misspelled one, and
being read as a default is worse than being refused.

### A new portrait shows without a reload

A player who sends a new picture keeps the name they had — it is derived from who
they are, not from what the picture holds — so nothing downstream could tell that
anything had happened. The card compared one name against the same name, decided
nothing had changed, and left the picture alone; the browser, handed an address it
had seen before, was entitled to do the same. The map went on showing the old face
until somebody reloaded the page.

A live player now carries `PortraitAt`, when their picture was written, and the map
asks for `/portraits/{name}.png?v={PortraitAt}`. The address is what the card
compares and what the browser fetches, so a redrawn player changes both. It lands
on the next live poll, a couple of seconds later.

The time moves only when bytes were actually written, so a character taken apart
and put back the same way costs no refetch.

### The project is called Witchlight

Every mapstique here is now witchlight — the crate, the binary, the settings
directory, the socket, the page title, and everything it prints. Nothing about what
it renders or serves changed, which is why the version did not move.

Settings live in `~/.config/witchlight/config.toml` and exports are read from the
`witchlight` folder inside the game's data directory. Both are new paths rather
than renamed ones, so a first run after this writes fresh defaults and the map
rebuilds as the server exports; the old `mapstique` folder and config are left
where they are and want removing by hand.

The region format is untouched — `MSQR` and `.msqr` never carried the old name, and
changing them would have been a format change rather than a rename.

The viewer keeps its settings under `witchlight.settings`, so the panel comes back
at its defaults once.

## 0.16.1

### A tool for reading one block

The 🔍 below the zoom control turns the pointer into a picker. It outlines the
block under it — at one pixel per block the pointer covers whatever it is over, so
being shown which block the map means is most of the tool — and says in the corner
what is there: the block's code, the surface height, and the climate the column
was generated with. `/block.json?x=&z=` is where it comes from.

The pointer stays an arrow while the tool is armed. A crosshair centres on what it
names and so sits on top of it, which is the one thing a tool for looking at a
single block must not do.

The reading is the renderer's own. How a column resolves against the palette had
been decided in two places that happened to agree; it is now decided in one, so
the map cannot name a block it did not draw. A column nobody has exported says
that rather than naming something that is not there.

### A chunk grid

Off until switched on, beside the other things the settings panel shows. It
outlines the chunks the export names, faint enough to read the terrain through,
and stops being drawn below eight pixels to a chunk. It is not clipped to the
explored area — where the explored area stops is one of the things it is for.

### Fixed

`/info.json` says the chunk edge, which the viewer had no way to learn and the
region format has carried all along.

The routes table promised an `F` key that fits the map to the world. There has
never been one.

## 0.16.0

### A player is their portrait, or their initial

The face assembled from three colours is gone, and so is `/skincolors.json` and
everything that fed it. A card shows the picture a player's own client drew of
them, and where there is none it shows their initial — which is what the map shows
now rather than only until somebody runs the command.

There was never anything worth drawing in between. What a seraph looks like exists
only on the machine rendering it; everything a server can read about an appearance
amounts to less than a letter does.

**Both halves must be upgraded together.** The mod no longer sends skin part
colours and this no longer serves them.

## 0.15.0

### Players look like themselves

A player's card shows a picture their own client drew of their seraph — skin,
hair, clothes, armour, whatever they are wearing — in place of the face this used
to assemble out of three colours. `/portraits/{name}.png` serves them, filed under
a name the mod derives from a player uid, which is base64 and carries characters a
path cannot.

The drawn face is still there for anyone who has not sent a picture, and a picture
that will not load falls back to it rather than leaving a broken image in the card.

Portraits are held for a minute rather than an hour: a player who sends a new one
keeps the name they had, so an address that never changes must not be cached as
though it did.

### Fixed

One rule decides whether a name in a URL is only ever a name, rather than one copy
of it per kind of stored file.

## 0.14.0

### It says where it is, so the mod can tell people

The addresses the map answers on are written to `service.json` beside the rest of
the export as soon as it binds. Working out what `0.0.0.0` actually means is this
program's question and it had already answered it for its own log; the mod needed
the same answer and had no way to ask. The address on the network comes first,
because that is the one worth giving somebody else.

`announce` and `announce_url` are new, and like `autostart` this program does not
read them: they say whether the mod tells a joining player where the map is, and
what to tell them. `announce_url` is empty by default, meaning the address worked
out here — which is right on a machine a player can reach directly and wrong for a
server on the open internet, where the map is reached at a name, through a proxy,
on a port this never sees. Only an operator knows that address.

The startup lines now put the address on the network first and mark loopback as
what it is, rather than listing loopback first and annotating the other.

**Both halves must be upgraded together.** The settings file gained two keys, and
a 0.13 service refuses a file it does not recognise every field of. An existing
file is brought up to date, values kept, with:

```sh
witchlight -c <file> --save-config -p
```

## 0.13.0

### The server mod can run this

The map service rides along inside the server mod's archive and is started by it,
so installing the mod installs the map. It stays a separate program — it knows
pixels, the mod knows the game, and a map worth keeping outlives any one game
server — but it is no longer a second thing to fetch, configure and remember to
start.

Settings move with it. Started by the mod, the file is `witchlight.conf` in the
game's `ModConfig` folder, named on the command line, so everything about one
server's map sits with that server rather than in one shared file in a home
directory. Run by hand, `~/.config/witchlight/config.toml` is unchanged.

`autostart` is new, and is the one setting here that this program does not read:
it says whether the mod starts the service unasked. Turn it off to run
`witchlight serve` yourself, which is what a map that should stay up while the game
server is down wants.

Everything the service prints goes to `witchlight-service.log` in the game's own
`Logs` folder, so it can be tailed while it runs and is not interleaved with a
game server's log.

**Both halves must be upgraded together.** The settings file gained a key, and a
0.12 service refuses a file it does not recognise every field of.

## 0.12.1

### Fixed

The map was blank at the zoom it opens on, until an admin joined the game.

A world that grows past a power of two gains a coarsest level. The levels above
zero are rebuilt by walking up from whatever region changed, so exactly one tile
was built at the new level — the one above the region that had just arrived — and
every other tile there had nothing beneath it that had changed, so nothing ever
asked for it. That level is the one a viewer opens on, so the map read as empty.

Restarting did not fix it: the start-up scan asked only whether a region's level 1
tile was current, which it was. An admin joining did fix it, by accident — their
client supplies a palette, a new palette recolours every tile, and rebuilding
every tile filled the level in.

One function now answers what the levels owe the world, comparing a region
against every level above it rather than only the first, and it is asked both at
start and whenever the world turns out to need a level the pyramid does not have.

## 0.12.0

### Faces are drawn from a table of colours

A player's appearance arrives as the names of the parts they chose — `skin4`,
`mossgreen`, `azure` — and `/skincolors.json` says what each name looks like. A
name with no colour falls back to that player's initial.

The table is fetched once and again whenever a player names a part not in it,
which is what happens when a mod adds appearances or when an admin supplies the
colours after the map was already open.

## 0.11.3

### Fixed

The generation was bumped twice for one export: once when the watcher reloaded the
changed regions, again when the builder finished the levels above. The generation
versions every tile URL, so that was the same pixels fetched under two names — and
on a dense world a tile is a third of a megabyte. Whether the second fetch
happened depended on where the five second poll fell, which is why it looked
intermittent.

The builder now announces the whole export at once, including the level 0 tiles
the watcher reloaded.

## 0.11.2

### Fixed

The food bar is green, as it is in the game, rather than amber.

## 0.11.1

### Says when the mod is older than it is

A map counting from absolute zero rather than from spawn looks like a bug in the
map, and nothing on either side looks wrong — so a missing `world.json` is now
said at start, naming the reason.

## 0.11.0

### Coordinates the game would recognise

Vintage Story shows every coordinate a player sees relative to world spawn, while
the world is a million blocks across with spawn near the middle. This map showed
absolute positions — not wrong, but agreeing with nothing a player can compare
them against, so a marker the map called `511900` was `-100` on their screen. The
mod writes where spawn is and the map counts from there. Absolute is a setting,
since that is what region files and tile URLs use.

The corner also reports wherever the pointer is, falling back to the middle of the
view when it leaves.

### Following a player

Clicking a card keeps the map on that player. Zooming does not stop it — looking
closer is not choosing to look elsewhere — but dragging does.

### Nothing on screen is rebuilt because data arrived

Cards, player markers and map markers are built once and afterwards written into.
Everything on the live feed shares a two second beat and a walking player changes
position and food on every one, so anything that rebuilt on change rebuilt
constantly: forty markers and thirteen cards destroyed and recreated twice a
minute. That was the flicker.

### Fixed

- A player whose skin colours could not be read drew as a black face on a black
  background instead of falling back to their initial: the colours arrive as zero,
  which is a perfectly good `#000000`, so the card looked empty rather than plain.
- A long player name pushed its card and bars out past their own border.

## 0.10.0

### Who is online

A card each for the players on the server: a face, their name, and how much health
and food they have. Both readings come from the server, which already knows them,
so nothing is asked of any client. The list scrolls when there are more players
than fit.

The face is drawn from the colours that player chose — skin, hair and eyes, read
from their applied skin parts. It is not a likeness and does not pretend to be:
the game keeps no portrait of anyone, its character screen renders the live entity
into a panel and there is nothing to read back. The colours are enough to tell one
player from another.

### A settings panel

A cogwheel, and toggles for what to show. Kept in the browser rather than on the
server: these are one person's preferences about one screen, and everybody looking
at the same map is entitled to a different answer.

## 0.9.2

### Fixed

Markers were destroyed and rebuilt on every live poll — forty elements twice a
minute, each with a picture to re-resolve. Markers that have not changed are now
left alone entirely, and a player who moved is moved rather than replaced.

This is also why a popup vanished a moment after being opened: it was attached to
an element that no longer existed.

## 0.9.1

### Marker pictures

Waypoints are drawn with the game's own icons, tinted with the colour their owner
chose. A name with no picture falls back to a plain shape, and the set is
refetched when a marker names one that is not known — so a marker mod installed
while the map is open is picked up without a reload.

### Fixed

A tile the service had not built answered `404`, which the viewer had nothing to
put in place of, so the browser drew a broken image. It now draws nothing.

## 0.9.0

### A view has an address

`#x,z,px-per-block` in the address bar, the same three numbers the corner shows,
so a link says what it points at. It follows panning and zooming, and opening one
lands where it says rather than fitting the world first.

Built to make the zoom levels testable from outside the page, which they were not
before; that it is also the feature every map has is a happy coincidence.

## 0.8.2

### Fixed

Zooming past eight pixels per block went black. A tile layer defaults to a maximum
zoom of eighteen and tests the map's own zoom against it *before* clamping to the
finest level, so past that every tile was dropped — at exactly the magnification
the level system exists to provide. The layer now states the range it is used at.

## 0.8.1

### Fixed

The map tore itself down whenever the world grew. One pixel per block sat at
whatever zoom the world happened to need, so a player exploring far enough added a
level and everything shifted underneath: the tile layer was rebuilt and the view
snapped back to fit, on nearly every export. One pixel per block is now pinned to
a fixed zoom, and growth only changes how far out the view may go.

## 0.8.0

**Clears the map on upgrade.** Regions changed size and there is no reader for the
old ones. Deploy with mod 0.8.0; neither half reads what the other minor wrote.

### The map has zoom levels

Tiles were drawn at one block per pixel at every zoom and scaled in the browser,
so the number on screen grew as the square of how far out you were: about fifty to
fit a small world, and **seventy-nine thousand** at the zoom-out limit, which is
minutes of rendering and a browser holding seventy-nine thousand images.

Level 0 is one block per pixel; each level above covers twice as much world, so a
view holds roughly twenty tiles however far out it is. Levels are numbered from
the finest, which is the numbering a world can grow under: level 0 stays one block
per pixel whatever anyone explores, so a bigger world adds numbers rather than
renaming every tile already written.

Each level is built by averaging the level below two by two. Averaging `2^L`
blocks straight from the world gives the same answer and costs `4^L` times as
much — a thousand times, at level 5. Averaged rather than sampled, because
sampling erases everything narrower than the step: paths, walls, rivers.

Level 0 is not stored; it is rendered from the world and cached. Everything above
is written when a region changes, through a queue that drains every two seconds,
so a region changing repeatedly costs one rebuild rather than one per change. A
restart rebuilds only what moved while it was away — a region whose level is
already newer than it is left alone.

### A region is a tile is a game region

Regions went from eight chunks square to sixteen: 512 blocks, one tile at the
finest level, and the same square the game itself calls a map region. Three
concepts became one.

### The viewer is Leaflet

Vendored, not fetched — the service is still one binary that works offline. It
brings the zoom handling, tile eviction, touch and keyboard, and the marker and
popup machinery: players draw with their names, markers in their owner's colour,
both with coordinates on click.

Tiles the service says have changed are still replaced one at a time rather than
through `redraw()`, which would discard every tile on screen to update one.

### Also

- A map with nothing in it starts and serves, rather than refusing to. It is the
  state every server is in for a moment after a format change clears it, and the
  service being down then looks like the upgrade failed.
- A tile nobody has built answers `404` rather than `500`. It is missing, not
  broken, and a viewer can draw around one and not the other.
- Responses carry one `Cache-Control` and one only. Two is not a stronger
  instruction but an ambiguous one, and the browser takes the first — so the
  vendored library was being fetched again on every page load while appearing to
  be cached for a year.

## 0.7.2

### The viewer stops asking for tiles that are not there

`draw` walked every tile coordinate in the viewport with no regard for where the
world actually is. Zoomed out that is overwhelmingly space no chunk has ever
occupied, and each one was a request, a render and an image kept forever. It is
now clamped to the world's own bounds.

On the 289-chunk test world the difference is the whole problem: **9 tiles at
every zoom, against 79,125 at the zoom-out limit** and 7.9 million at the floor.

### Zoom-out stops where the budget does

A world too large to fit inside the tile budget whatever the scale now has a
floor, computed from the viewport so that no view asks for more than four hundred
tiles. A world small enough to fit entirely has no floor at all — the clamp
already bounds it, so zooming out simply makes a fixed handful of tiles smaller.

This is what a floor looks like without zoom levels, and it is the same answer
JourneyMap's web map reaches by capping Leaflet at `minZoom: -2`. It goes away
once there are levels to draw from: the coarsest level becomes the floor instead,
and it is far further out.

The heads-up display now shows how many tiles the view is drawing.

### The viewer has tests

`cargo test` now runs `tests/viewer.mjs`, which lifts the tile arithmetic out of
`src/viewer.html` by name and exercises it — small worlds, worlds larger than the
budget, worlds away from the origin and across it, and four window sizes.

It lifts rather than copies. The first version reimplemented the clamp in the test
instead of calling the viewer's, and removing that clamp from `draw` altogether
left every check passing.

## 0.7.1

### Tiles are rendered on more than one thread

Requests were answered one at a time, so several people opening a cold map queued
behind each other and a cold view arrived a tile at a time. `tiny_http` supports a
shared server across threads and always did; nothing needed replacing.

Measured on 576 cold tiles over 12 connections: **696 tiles a second on one
thread, 2,150 on eight** — a little over three times. Sixteen threads was slower
than eight, which is why the automatic choice is capped there.

`threads` in the configuration, or `-t`. Zero decides from the machine, capped at
eight: this usually shares a box with the game server, which has the better claim
on its cores.

### Looking for a new export moved off the request path

Every request used to check the filesystem for a newer export. With one thread
that was merely wasteful; with several it was two threads racing to reload the
same regions and bumping the generation twice for one export, which made every
viewer repaint twice. A single watcher now does it on its own clock, once a
second, and holds its gate across the reload.

### Whole-map renders use every core

`witchlight render` draws a row at a time in parallel — every pixel is decided from
the world and the palette alone, so rows are independent. A 6,144 by 6,144 world
went from 3.79s to 1.54s, and the output is byte for byte what one core produces.

## 0.7.0

The map became a directory of regions, and live data stopped going through a file.

### Terrain is stored per region

One file per 8×8 chunks — 256 blocks, exactly one tile — instead of one file for
the whole map. A chunk that changes now costs the square it sits in rather than
everything anyone has ever explored, and the tile cache drops one entry rather
than all of them.

Each region is a gzip stream, which measured between five and eight times smaller
on real exports: a 1,828-chunk map is 1,570 KiB where the records alone are 11 MB.

### Only what changed is re-read and repainted

`/info.json?since=N` answers with the tiles that have actually changed since
generation `N`, or `all` when the palette moved or the caller is further behind
than the 128 generations kept. The viewer repaints those and leaves the rest.

Tiles are also swapped only once their replacement has decoded, so the map no
longer blinks through the background colour on every export.

### Players and markers arrive over a socket

The mod posts to an **API socket** — by default `/tmp/witchlight-{hash of the
export path}.sock`, which both halves derive for themselves — instead of writing
`live.json` every two seconds. Players are held in memory and **expire after 30
seconds**, so a game server that stops leaves no dots behind. Markers are written
to `markers.json` when they arrive and differ.

The socket takes writes, which is why it is not on the map port. `api_socket` in
the configuration moves it, and accepts a `host:port` for a mod on another machine.

A socket that will not bind is now a warning rather than a failure to start: the
map is the product and live data is a garnish.

### Removed

- Reading `live.json`. It briefly stayed as a fallback and disguised a mod posting
  no markers as a map that merely had none.
- Reading the single-file `columns.msqc`. While Witchlight is alpha a format change
  clears the map rather than upgrading it, and the mod does the clearing.
