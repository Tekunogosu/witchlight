# witchlight

The map service. It pairs with the [server mod](../../witchlight-csharp), and the
two **must match on minor version**: minor is the compatibility generation, moved
whenever the file format or the socket protocol changes. Patch is free to differ,
and covers anything that changes only one half.

## 0.28.1

**Deploy note:** the service half only; the mod stays at 0.28.1. Nothing is
cleared.

**Marker size is a slider, and marker names have one of their own.** It was three
named steps — normal, large, largest — which is three guesses at a judgement about
one person's eyes and one person's screen. Both run from a little under the game's
own size to three times it, and a marker's name is lifted by the size of the mark
it belongs to, so it clears a large one instead of sitting across it.

**Every size the page can be set to comes from one table.** The three in the
profile window were written out in the markup *and* looped over in the scripts,
which is two places to keep a slider's range in step, and the marker sizes were a
third mechanism beside them. There is one table now; a slider is an entry in it,
and which panel offers it is a field. Somebody who had chosen one of the three
named marker sizes keeps the size they chose.

## 0.28.0

**Deploy note: both halves.** Minor is the compatibility generation and it moves
together, so the mod goes up with the service. Nothing is cleared and no map is
rebuilt.

**A marker can be deleted from the map.** The form that changes one has a bin
beside its Cancel: the first press says what a second would do and the second asks
the game server, which is the rule the list's bulk buttons already follow. There
is no way back from it, and there is no way to reach it except from a marker that
already exists and is yours. `DELETE /markers/{key}` carries no body — a removal
names a waypoint rather than describing one — and the page watches for the marker
to *stop* arriving, which is the same watch an edit uses read the other way round.

**Only a marker's owner may delete it.** The operator's `public_markers_editable`
lets somebody correct a marker they can see, which is not the same permission as
taking it off the map of the person who made it. The mod decides it against the
waypoint itself, so a refusal shows up as the marker still being there.

**The marker list says how far away each one is.** A new Distance column, measured
from wherever that reader's own player is standing, which is the question somebody
looking at a list of places is actually asking. It is written into the cell on
every poll rather than by redrawing the list, so the number keeps up with a
walking player without the row moving out from under the hand reaching for it —
the order is taken when the list is drawn and left alone. The Coords column keeps
its own sort, which is distance from spawn, since spawn is what every coordinate
on the row is counted from.

**Markers the search has found are drawn larger on the map.** A name typed into
the list narrows a hundred markers to three and then leaves the reader to find
those three on the map, which is the question they opened the list to stop asking.
Only while something is typed: an empty box is the whole list, and a map with
every marker singled out has singled out none of them.

**Markers can be drawn larger for good.** *Marker size* joins the colour and
colour-vision settings in the accessibility panel — normal, large, largest. It is
a multiplier over the size the game itself draws a mark at, so where a mark sits on
its block and the shadow it is read against are answered once and scale with it.
The search's own step sits on top of whatever is set.

**The mod can read and keep one person's presets.** Two new addresses on the API
channel, `POST /presets/of` and `POST /presets/keep`, so a marker made from inside
the game can start from the same presets the map's own form uses. A game client
has no session and no browser; the mod is the only party that knows which uid is
which player, which is the trust minting a login word already needs. `keep` takes
one preset and merges it on its pattern rather than replacing the document, since
the game side knows the one preset made in front of somebody and nothing else
about what they have kept.

## 0.27.5

**A click on a player says who they are, with their own face.** The popup was a
name and three numbers with nothing between them; it is the shape a click on a
marker already answers in — the name a heading, and what is true of it under a
rule — with the portrait their own client drew beside the name. A dot and a mark
are two things on one map, and being told about them in two different shapes was
the reader's problem rather than the map's.

**A face is drawn from one rule wherever it appears.** The player card and the
profile bar each carried their own copy of what one looks like, and the popup
would have been a third. What is left in each place is how large it is, which is
the only thing the three disagree about.

**The profile's Cancel and Save look like buttons again.** Naming the row's
worded buttons in 0.27.3 reached the two in the marker form and the four in the
lists, and left the two in the profile wearing the browser's own styling. A check
now reads every button in a window's last row and fails on one dressed as
neither a word nor a mark.

## 0.27.4

**The lock is a padlock.** Phosphor's simple lock is a shackle over a rounded
body, which at fourteen pixels reads as a shopping bag rather than as a lock.

**The two marks a marker form holds are the size of the buttons beside them.**
Both they and the answers in the marker list are drawn from one box and one mark
inside it, so the pair cannot come to differ.

**The marker list's last heading sits over its own column.** It was centred with
its caret rather than by itself, which put it a caret's width to the left of every
mark it headed, and the list's scrollbar took another sixteen out from under it —
so the heading holds the same gutter the list does, and centres the mark alone.

## 0.27.3

**Who may see a marker is one mark, drawn the same everywhere it is asked.** A
lock is a marker its owner keeps and a crowd is one the whole server can see —
two pictures of two different things, each in a colour of its own, where a shut
lock against an open one was one picture twice over told apart by a shackle a
reader had to look for. The marker list and the marker form now draw it from the
same function, so the answer on the form and the answer in the list cannot come
to differ, and the title says where it is before it says what a press would do.
The form's checkbox is gone with it.

**The marker list is a table, and its headings are over their own data.** Four
columns could be sorted by and only one of them had anything under it: the name
was a column and the rest — where it is, whose it is, who can see it — were one
grey line beneath the name. Each has a column now, the heading over it, and the
coordinates are headed with what they are rather than with what they are sorted
by. The column that sorts by who can see it wears whichever of the two the list
currently has at the top.

**A marker being changed can be kept as a preset, like one being made.** The one
window has always served all four jobs; the choice to remember it was offered on
only one of them. It is now the presets' own mark beside the privacy mark, and
asking for a preset with nothing to key it on says so rather than quietly
dropping it.

**The picker shows what it is offering when the colour chosen is black.** The
pictures are drawn in the marker's own colour on a panel that is near black, so
choosing black left thirty holes to choose between. The ground lifts where the
colour cannot be read against it.

**The marker form ends the way a form ends.** The two states it is holding sit at
one end of the last row and the two ways out at the other, with cancel beside
save rather than across the window from it. The name box says what it is instead
of carrying a heading over it.

## 0.27.2

**The button that takes a marker's coordinates from a click wears a crosshair.**
It wore the same pin as the button that opens the window it sits in, so one
picture stood for both starting a marker and choosing where it goes, and the
inner one had to be worked out from where it sat. A crosshair aims at a place
rather than standing on one, which is what that button does.

## 0.27.1

**A position is one number in three parts.** The readout at the foot of the map
said x and z as two labelled columns and left y to the inspector's line, so the
three numbers a place is made of were read in two places and only ever two of them
at once. They are one group now — `x, y, z` over `189, 125, -837` — and the
inspector's line no longer repeats the height. Each number is still held to its
own width, so the panel cannot twitch as the pointer crosses zero. The height is
what the picker was told and is a dash while the picker is off, since it is the
one number of the three the page cannot work out for itself.

**The world's clock reads in the order it is spoken.** The year and the season sat
under the date and the time they belong to, and two lines of type came to within a
pixel of the box's own height, so the clock read as text that had outgrown it. The
quiet line goes above the loud one the way a heading goes above what it heads, and
the box is now a floor rather than a fixed height — it grows the few pixels that
put an even margin above and below, and the row centres what stands beside it.

## 0.27.0

**Deploy note: both halves.** Minor is the compatibility generation and it moves
together, so the mod goes up with the service. Nothing is cleared and no map is
rebuilt. A server left on the old mod is not broken by this: it says nothing about
which way anybody is looking, and the map draws a plain dot rather than pointing
everybody north.

**A player is drawn the way the game's own map draws one** — a dot with a cone
over it for the way they are looking — so that the map in the browser and the map
in the client mean the same thing. The picture is the client's own, transcribed
from `IconUtil.DrawMapPlayer`, and it is turned by the bearing the mod sends. A
player the mod says nothing about keeps the dot and loses the cone, because zero
degrees is north and north is an answer nobody gave.

**Every mark sits on the block it names.** Leaflet's answer for a picture it did
not draw is a twelve pixel box offset by half of itself, and every mark on this
map was then centred on that box's corner — six pixels up and to the left of the
place it was about. The marker pictures, the player dots and the names above them
now agree on where a position is.

## 0.26.2

**Deploy note:** nothing to do by hand, nothing is cleared, and nothing behaves
differently. A refactoring pass with no change to what the map does.

**One owner for reading a directory that may not be there yet.** The regions, the
colour maps and the marker pictures each said "a missing directory is an empty
one" in their own words, and one of them said it differently enough that an
unreadable directory read as an empty one. `files::listing` says it once.

**One owner for a test's scratch directory**, which three modules had each
written out with their own spelling of "empty it first, take it away after".

**The viewer is smaller files, by subject.** The marker mark and the row a list
shows one in are their own file, because four things draw a mark and two draw a
row. `meant` sits beside `said` in `frame.js`, being the other direction of one
translation that is tested as a pair. Which preset names a block moved to the
presets, being their question and not the form's. The corner furniture left the
block inspector, which its own heading had been admitting were two subjects.

**The two functions that opened the preset form are one.** Making a preset and
changing one set the same eleven fields in the same order; what differs is the
row it will be written to, and that is now the argument.

**Every script beside `viewer.rs` is served, and a test says so.** `include_str!`
makes a named file that is missing a build error and says nothing about a file
that exists and is named nowhere — behaviour written, reviewed, and never run.

## 0.26.1

**Deploy note:** nothing to do by hand, and nothing is cleared. The mod half is
untouched and stays at 0.26.0.

**The marker list no longer rearranges itself when you change who can see a
marker.** It had no order of its own, so it was showing the order the markers
arrived in — and they arrive as the ones everybody can see followed by the ones
only you can. Making a marker private moved its row to the bottom of the list.
Nothing said that was going to happen, and from the outside it read as the list
shuffling for no reason.

**A header at the top of the list says what the order is, and changes it.** Name,
Away, Owner, and the lock column, each sorted either way and marked with a caret
on the one in force. Name to begin with; the choice is kept in the browser with
the other things about this screen.

The order is total — ties fall back to the name and then to the marker's own key —
so nothing moves on a poll that changed nothing, and the only thing that can move
a row now is the column the list is actually sorted by.

Away is measured from spawn rather than from the middle of the view. Spawn is what
the coordinates on every row are counted from, and an order that changed every
time the map was dragged would be the same complaint arriving by a different road.

## 0.26.0

**Deploy note:** both halves go together, as a minor always means. Nothing in the
format or the protocol changed and no map is cleared — the mod half moves only to
stay on the same compatibility generation, and its own behaviour is untouched.

**The mark that shuts a window works again, on the two windows where it did not.**
The presets and the markers windows were laid out by a rule naming them by id, and
`display` is the property `.window` uses to show and hide — so an id rule setting
it outranked both `.window` and `.window.open`, and those two windows were on the
screen whatever class they wore. They opened with the page, they opened in the
same place every time, and their close mark removed a class that decided nothing.

The rule now says `.open`, and a test fails if any rule ever shows a window
without saying so. What made this survive a check was the difference between the
class a window wears and whether it is on the screen; the test looks at the
second.

**Who may see a marker is a switch in the list.** A lock on every row: shut for a
marker its owner keeps, open for one the server can see, and a press flips it.
Somebody else's marker wears the same mark without the button around it — knowing
who else can see a thing on the map is worth having whether or not it is yours to
change.

**And a pair of buttons that flip everything you are looking at.** Make all
private, make all public, each saying the number it would touch. That number is
what is on the screen rather than what is on the tab: somebody who has typed into
the search box has narrowed what they are looking at, and a button under a list of
three that quietly changes forty is a button nobody can trust. Markers already the
way the button would put them, and markers that are not yours to change, are not
counted and not touched.

It asks twice. Making a marker public shows somebody's base to the server, which
is not a thing to do because a pointer was in the wrong place, and making a
screenful private at once is the same size of surprise from the other direction.
The second press is on the same button, and changing tab or typing in the search
box puts it back to asking — a confirmation is about the markers that were on the
screen when it was given.

Nothing waits on the game here the way the form does. A form is somebody sitting
in front of a half-filled window; a switch in a list is one click among several,
and the marker arriving changed on the next poll is the answer.

## 0.25.0

**Deploy note:** both halves go together, as a minor always means, and this one
means it literally — the channel the mod posts players on changed shape, so a
mismatched pair shows an empty player list and says so in the service log. No map
is cleared and no marker is touched. Nothing is hidden that was not hidden before
unless an operator sets `players_public = false`.

**An operator can decide that where somebody is standing is their own group's
business.** `players_public` in the settings file, on by default, which is what
the map has always done. Turned off, a player appears on the map to whoever the
game has in a group with them and to nobody else.

Vintage Story has no setting of its own to follow here: its server config says
nothing about who may see whom, and the nearest thing in the world config,
`allowMap`, decides whether there is a map at all. So this is witchlight's own.

How many are online is still said to everybody, on the tab and in the corner
readout. That is a fact about the server rather than about anybody on it, and a
map that would not say it is a map nobody can tell from a dead one.

The sorting is the mod's, the way the markers' already was. It is the half that
knows what groups the game has people in, and a service holding positions it must
not send is one bug away from sending them — so what reaches a browser is only
ever what that browser may see. Group membership is read off players who are on,
so somebody reading the map while their own player is offline sees what everybody
sees.

**All and Group above the player list.** Which of the people the server shares
you want to read down: everybody, or the ones in a group with you. Each tab counts
what it holds. The dots on the map are not touched by it — which players the map
draws is what the Players switch is for, and two controls over one thing is one of
them being ignored.

The panel takes itself off the map when nobody is on at all, rather than showing
two empty tabs. Where it is showing fewer people than are online it says which of
the reasons that is, because "the server does not share this" and "nobody from
your group is on" are not the same thing.

**An All tab on the markers window**, showing both kinds at once and counting
them, first and open by default. The first question anybody brings to a list of
markers is which markers there are; who else can see one is the second.

**A window comes to the front when you touch it.** Every window sat at one
stacking order, which left the markup deciding which of two overlapping ones was
in front — permanently, and in an order nobody chose. The window written last in
the page won every time, so **the mark that shuts a window underneath did nothing
at all**: the click never reached it.

**The marker form opens on whichever side of the list has room for it.** It went
to the right always, so a list dragged near the right edge opened the form off the
screen, where the clamp that keeps a window reachable left a finger's width of it
showing.

**Corners are two pixels rounder**, from one pair of variables rather than a
number written at each box — the eight places that had written their own now ask,
including the library's own controls, which had been rounded to the number this
page used to use.

## 0.24.0

**Deploy note:** both halves go together, as a minor always means. Nothing in the
format or the protocol changed and no map is cleared — the mod half moves only to
stay on the same compatibility generation, and its own behaviour is untouched.

**Every marker there is, as a list.** A new window under the presets, on the same
control: two tabs, one for the markers everybody can see and one for the markers
only their owner can, each counting what it holds. A row says what the marker is
called, where it is and whose it is, and picking one puts the map on it. Where the
marker is yours to change — or public on a server that lets anybody correct a
public marker — the same click opens the marker window on it, beside the list, and
what is saved goes back to the marker that was picked rather than making a second
one beside it.

A map answers where a marker is. It cannot answer what there is, because the
answer is spread over a million blocks with most of it off the screen, and a
marker made last week gets found by remembering roughly where somebody was
standing — which is not finding it at all.

**The presets window is a list you can read down.** A search over names and
patterns, every other row shaded, the rows scrolling under a heading that stays
put, and a corner that resizes the window — by dragging it, or with the arrow keys
once it has the keyboard. The marker window is put beside whichever list it was
opened from, so picking a second row no longer means moving a window first.

The search filters as it is typed and Escape empties it. A row keeps the place it
has in what the service holds rather than the place it has on the screen, so
editing the third visible row edits the preset that is drawn there and not the
third preset kept.

**A marker whose picture the map has not got is drawn as a diamond rather than as
nothing.** A mask whose image never loads draws nothing at all, so a preset saved
against an icon a mod has since taken away vanished out of its own list, and a
marker naming one was a hole on the map. There is now one place that draws a
marker's mark — the map, the picker in the form, and both lists all go through it
— which is what makes the stand-in reach all four.

**Windows measure themselves the way they are told to.** They were content box
while the number in the stylesheet read as the whole window, so a window asked for
340 pixels came out 374. Harmless while nothing did arithmetic on it, and a
thirty-four pixel jump the moment the resize corner did.

## 0.23.3

**Deploy note:** nothing to do by hand, and nothing is cleared. The mod half is
untouched.

**The panels in the corner open over the zoom rather than under it.** Leaflet
puts its own controls at a stacking order of 1000 and the corner column sat at
800 — and because that column stacks as one thing, a panel hung off a button
inside it could not climb out however high it was told to go. Both panels were
900 and both went underneath. The column sits at 1050 now: over the map's own
furniture, still under a window somebody dragged out on purpose.

Only the accessibility panel was long enough to reach the zoom and show it, but
the settings panel had it too.

## 0.23.2

**Deploy note:** nothing to do by hand, and nothing is cleared. The mod half is
untouched.

**The map can be repainted for a reader who cannot separate two of its colours.**
Under Colour vision in the accessibility panel: red-green in both its forms,
protan and deutan, and blue-yellow. Off to begin with, and remembered.

It is daltonisation rather than simulation. Simulating colour blindness shows
somebody what they can already see; this works out what the deficiency loses and
puts that back into the channels the eye still has, so that ground and markers
arriving as one colour arrive as two. The deficiency is simulated from Machado,
Oliveira and Fernandes (2009) at full severity, the loss is the difference from
the original, and it is redistributed the usual way — all of which is linear, so
each of the three is a single colour matrix rather than a pipeline.

**It covers the whole map, not only the terrain**, unlike the colour presets
above it. That is where it earns itself: measured against the marker colours the
game hands out, every pair a red-green reader loses came back apart, about two
and a half times further apart in perceptual distance. The terrain gains far
less, being mostly olive and tan already, which little of this can confuse — so
markers, which somebody chose a colour for on purpose, are the point.

The matrices are applied as SVG filters in **linear RGB**, said out loud in the
markup. They are ratios of light, and sRGB numbers are not; applying them to
sRGB gives a confidently wrong answer that still looks like something.

It will look strange to a reader who does not need it. That is what it is for: it
is tuned for telling colours apart, not for looking like the world.

## 0.23.1

**Deploy note:** nothing to do by hand, and nothing is cleared. The mod half is
untouched.

**The colour button is an accessibility button.** Same place, under the settings
and above the marker controls, still shown to everybody. It holds what the map
can be asked to do differently for one pair of eyes, which the colours were only
ever one of.

**Deeper zoom** is the first of the rest. The map stops at eight pixels to the
block; turned on, it goes to sixteen. It costs no tiles either way — the finest
level is stretched rather than a finer one fetched — so this is only how far the
stretching is allowed to go. Sixteen rather than the twelve asked for because the
rungs are powers of two, as the zoom is: eight, sixteen, and nothing between.

Turning it off brings the view back down with it. Leaflet lowers the limit
without moving a map that is already past it, which leaves the map above its own
ceiling with no tiles to draw — blank at exactly the magnification somebody just
turned off.

**A `Default` preset**, first in the list, which is the map with no filter over
it at all. `Vivid` is still what a viewer starts on, since that is the one that
reads most easily, and the choice is remembered.

**The clock wears the account button's box.** It was a little taller than the
button beside it and lined up with nothing. It now takes its height from the same
two numbers Leaflet sizes that button by — 26, or 30 where Leaflet decides the
machine has a touch screen — so the two stay level whichever it picks.

## 0.23.0

**Deploy note:** both halves go together, as a minor always means — the mod posts
the world's clock on a channel this build is the first to answer. No map is
cleared and nothing is rewritten; a mismatched pair simply shows no clock.

**The map has a colour filter, and it starts on.** Five of them, in a panel hung
off a new button under the settings: Vivid, Natural, Strong, Muted and Greyscale.
They sit on the terrain tiles rather than on the map, so the markers, the grid and
the player dots keep the colours somebody actually chose for them. The choice is
kept in the browser, because it is about one pair of eyes.

The button is not gated on being signed in, unlike the marker controls under it.
Nothing it does is anybody else's business.

**Why it earns its place.** 0.22.6 made the ground the colours the game would
draw, which was right and is harder to read: measured over one world, the older
build's ground varied half again as much in brightness as the corrected one does.
That variation is what a map is looked at for.

**None of the presets is the older build, and none can be.** That map's advantage
was that its ground changed *hue* from region to region — a rust-brown north
against a green middle — because it multiplied two tints at every pixel. A filter
shifts every pixel the same way, so it can put the contrast back and cannot put
that back. Bringing it back properly would be a second way of painting a tile,
which is a change to the renderer rather than to the page.

**The date, the time and the season have a widget of their own**, beside whoever
is looking, in two columns of two: the date over the year, the time over the
season. The lower line of each is the quieter one, because a year is what a date
is *in*.

**And the clock actually runs.** It was written into `world.json` once, when the
world came up, and never again — so it showed the moment the server started for
as long as the server ran. It arrives on the live channel now, every two seconds,
with the players. A clock is the thing a map has least business writing to a
disk: it is stale before the write finishes.

## 0.22.6

**Deploy note:** nothing to do by hand, and no map is cleared. The stored zoom
levels are, because they were painted by a build that painted differently — they
redraw from the region files as they are asked for, and the region files are not
in question. Pairs with mod 0.22.7, which is where the sea level and the date
come from; without it the map still draws correctly, and every column is treated
as though it were at sea level.

**The tints were being compounded.** The game builds *one* tint for a block and
multiplies the block's colour by it once. The climate map makes that tint, from
temperature across and rainfall down. The season map does not darken it — it
stands in for part of it, in the proportion the season is felt at that place at
all. This multiplied the two instead, which compounds them: warm ground came out
redder and browner than the game draws it, everything came out darker, and
nowhere ever looked as though the season had left it alone.

Measured over the same world and the same view, red against green fell from 1.19
to 1.07 and the whole map lightened, which is what removing a second tint from
underneath the first does.

**How much season a place feels is now asked rather than assumed.** The game
weighs it by temperature and by height above the sea: foliage in the tropics
never turns, temperate ground turns almost completely, cold ground turns a
little because it is drab already, and a mountainside keeps its needles while
the valley below goes to autumn. This applied the season at full strength
everywhere. The curve is the game's own, constants included.

**The climate maps are no longer read through their own border.** They are a 256
square drawn inside a 264 one, and the border belongs to the game's texture
atlas rather than to the lookup. Reading it as though it were the map put every
sample a few pixels out, worst at the extremes — which is where the hottest and
coldest ground is. The mod now writes down each map's border beside the pictures,
since it is a fact about the asset that cannot be told from the pixels.

All three come from `colormap.fsh` and `colormap.vsh` in the game's own shaders,
which are the only statement of this that cannot drift.

**Still not done, and it shows in winter:** the game whitens frostable blocks
below about 0°C, and works out that temperature with a seasonal offset this half
is never told. So a winter map is the right colours for winter foliage without
the frost over the top of it. That needs the mod to say what the season has done
to the temperature, and is a separate change.

**The corner says the date and the season.** Both come from the game's own
words, taken at spawn — a season is a fact about a place, and the hemispheres
disagree about it.

## 0.22.5

**Deploy note:** nothing to do by hand, and nothing is cleared.

**A tile being replaced no longer flashes.** Leaflet keeps a `load` listener on
every tile it makes, and answers one by setting the tile to nothing and fading
it back in over a fifth of a second. That is right for a tile arriving into an
empty square. It is wrong for one being replaced in place — and it fired anyway,
because putting new pixels into a tile is done by giving it a new `src`, and the
listener is still there when they land.

So each repaint faded from transparent, whatever the map had going for it. On one
tile, measured: the old way wrote `opacity: 0` and left it there for the fade to
undo; the new way writes `1` and never writes `0` at all. Leaflet's handler runs
first and this one second, both inside the same event and so both before the
browser has painted either, which is what makes it a swap rather than a fade.

This was always the case. What changed is how often it shows: 0.22.3 moved the
export to ten seconds and the terrain poll to two, so a map that repainted twice
a minute now repaints six times, and every one of them was a flash. Zoomed out it
is worse again, because one tile at a coarse level covers many regions and almost
any change anywhere lands on a tile you can see.

**What is repainting is not wrong, for the record.** The service was asked. Of
the exports on a live server, all but one rewrote a single region holding one or
two changed columns. The exception rewrote 59, because the season had moved —
which really does change the colour of every column stored under it. Nothing is
repainting that has not changed; it was only that changing looked like breaking.

## 0.22.4

**Deploy note:** nothing to do by hand, and nothing is cleared.

**Every button on the map is one size again.** Leaflet draws its bar four pixels
larger on a machine it believes has a touch screen, and it says so with a class
on the container it owns. The corner column borrows Leaflet's bar and sits
outside that container, so the rule never reached it: the cog and the account
came out 26 square against a zoom and a picker at 30, on exactly the machines
where Leaflet chose the larger size. The column is now told what the map was
told, so the two agree whichever size Leaflet picks — rather than a number
written down here that would be wrong on whatever it disagreed with.

The account keeps its width, which is however wide the name in it needs to be.

**The name is back beside the version.** `#version` was laid out as a grid, and
a grid makes a row of each of its children — so the moment the name became a
child of its own, to be given a colour, it went above the number instead of
beside it. It is laid out as the line of text it is.

## 0.22.3

**Deploy note:** nothing to do by hand, and nothing is cleared. Pairs with mod
0.22.4, which is where most of the waiting was; this half is worth deploying on
its own but the two together are what makes new ground appear quickly.

**New ground is drawn about three times sooner.** The page asked what had changed
every five seconds, on top of everything else between a player walking somewhere
and seeing it: the mod's export timer, the service noticing, the levels above
being built. Asking is cheap — `?since=` usually answers with nothing at all — so
it asks every two, the same clock the players and markers already use. Most of
the rest of that wait is the mod's, and mod 0.22.4 takes it from thirty seconds
to ten.

**What the picker is looking at sits above the readout.** Underneath, it pushed
the six numbers down the screen whenever it appeared and let them drop back when
it went — so the row being read moved out from under the reader. The panel is
anchored to the bottom of the screen, so the line grows upward off the top of it
and the numbers stay where they are.

**The name drifts.** `Witchlight` in the corner moves through a soft purple, blue
and green and back over thirty seconds. Slow enough never to be seen moving, and
close enough in weight to the text beside it to stay furniture rather than
decoration. A machine asked to stop things moving gets one of the three colours
and no animation.

## 0.22.2

**Deploy note:** nothing to do by hand, and nothing is cleared. The mod half is
untouched; repackage it to carry this binary.

**A repaint no longer empties the map.** When the service cannot say which tiles
changed — a new palette recolours all of them, or a viewer has been away longer
than the service remembers — the page fell back to Leaflet's `redraw()`, which
removes every tile and asks for them again. The map went blank and filled back
in, at the one moment it was most obviously working. It now swaps each tile the
way a named change already did: the replacement is loaded out of sight and put in
place only once it has decoded, so nothing is ever detached and no tile is empty
between the old pixels and the new.

**The settings hang off the button that opens them.** The panel was pinned 92
pixels down the page, which is a guess at where the cog is — one that a scaled
toolbar makes wrong, and which left it floating well below the control it belongs
to. It is the cog's own panel now, so it follows the button.

**The corner readout is read rather than parsed.** Six numbers ran together in
one line with spaces between them, which asks whoever is looking to remember the
order to know which is which. They are three labelled pairs now — where the
pointer is, how the map is drawn, what the world holds — ruled apart, with the
unit in the label so every value is a bare number. The digits are tabular and
each field has a floor under its width, so a coordinate crossing zero no longer
shoves the rest of the panel along.

A world with nothing exported says so in a sentence rather than showing six
zeroes, which were six wrong answers where there is honestly no answer yet.

**The page says which page it is.** The corner read `v0.22.1` and now reads
`Witchlight - v0.22.1`.

**Player cards cast the same shadow as the controls.** They are furniture laid
over the map exactly as a button is, and without it a card read as a hole cut in
the terrain rather than as something resting on it.

## 0.22.1

**Deploy note:** nothing to do by hand, and nothing is cleared. The mod half is
untouched and stays where it is; repackage it to carry this binary.

**The viewer's furniture is drawn rather than typed.** Every mark on it was a
Unicode character: a magnifying glass on the block picker, a gear, a flag, a
trigram, a position indicator, a bust and a multiplication sign. Two of those were
colour emoji, which paint themselves and ignore what the page asks for — the
reason an armed tool signalled with a ring drawn around its mark instead of by
colouring it. The rest were symbol-font characters each machine drew in whatever
face it happened to have, or drew as a box, having none: `⌖` is missing from
enough fonts to be a real hole rather than a theoretical one.

They are silhouettes now, filled with the colour of the button they sit in, which
is how this map has always drawn a waypoint. So an armed tool says so twice — the
surface lifts a step and the mark takes the accent — where before it had one
signal and a workaround.

The block picker is marked with a frame around a block rather than a magnifying
glass. A glass is what a browser puts on a search box, and the tool names the
block under the pointer.

**`/chrome/{name}.svg`** serves them, compiled in and cached like the library
rather than read from the map directory. `/icons/` is a game's waypoint marks —
data, exported per world, absent until something has been. Furniture has to draw
on a map that has never been exported, so it cannot come from there.

**Phosphor is vendored**, all six weights of it, at 2.1.1 — MIT, no dependencies,
9,072 files that are each a single path. Only the six marks named in `chrome.rs`
reach the binary; the rest is kept so the next one is already here at a known
version. `src/vendor/README.md` records the commit, its hash, and why a commit
rather than a tag.

One weight is mixed: the mark that shuts a window is bold, not filled. In the
filled weight an `x` is a square with the cross knocked out of it, which beside a
heading reads as a blot rather than as a way out.

Three copies of the mask boilerplate in the stylesheet became one `.masked`, and
the account button's mark became an element instead of a `::before` — a mask
clips an element's border along with everything else it draws, which had quietly
taken the rule between the mark and the name with it.

## 0.22.0

**Deploy note:** the settings file gains `map_data` and `per_world`. This build
reads a 0.21 file unchanged; a 0.21 build refuses one naming settings it has never
heard of, so both halves go together as a minor bump always means. Nothing changes
for an existing dedicated server: `per_world` is off there, and a map already on
disk stays exactly where it is. No map is cleared.

**Each world can keep its own map.** `per_world` files every world's map in a
directory of its own inside the map folder, named for the world. It is off for a
dedicated server, which runs one world out of one data path and has no reason to
move anything. The mod turns it on for singleplayer, where every save shares one
data path — and one folder between them meant the second world wrote its terrain
into the first world's map at the same region coordinates, giving a map of two
worlds at once with nothing anywhere saying so.

Nothing is shared between those directories, including the files that would be
identical. A palette written once and then left alone costs nothing to keep, while
one rewritten on every switch between a world with no mods and a world with fifty
costs a disk.

**`map_data` says where maps are kept**, for a larger disk or a directory a web
server already serves. Empty is `<vs_data>/witchlight`, which is where it has
always been.

**`--exports` names the map to serve.** The mod passes it, because the mod is the
half that knows which world is running. By hand it is needed only where a world
has been given a directory of its own and more than one has been exported: with
one, that one is served; with several, the service lists what it found and says
how to name one, rather than picking.

## 0.21.1

**The page started eleven things it never waited on, and a failure in any of them
was silent.** A browser does not wait for a handler, so an async function wired to
a click, a clock or a keystroke hands back a promise nobody is holding: a throw
inside one is a rejection in a console nobody has open, while the page carries on
as though the work had happened. Saving a marker, deleting a preset, keeping your
settings, both pollers, the block search and the block lookup were all started
this way. They now go through one function that says which work failed and in
what words, and a test fails on the next call added without it.

**Both pollers counted the beat whether or not the last one had been answered.**
`setInterval` does not wait, so a service slower than the gap was asked again
while it was still answering, and the asking only outran it further — worst on
the two-second live poll, which is the one under load. The gap is now between one
answer and the next question.

**Nine names were hidden underneath the ones the rest of the page uses.** The
scripts share one scope, so a `const said` inside a helper stands in front of the
function that words a position, and a parameter called `at` stands in front of
the one that makes a map position. Every one of them was harmless where it sat
and a `TypeError` waiting for whoever next reached for the real name inside that
function. The page is also strict now, which makes a mistyped name an error
rather than a new global; a test checks both, alongside the one already guarding
two top-level bindings of a single name.

**A right click on the map wiped the name you were typing into the form it had
just opened.** The block under the pointer is looked up after the form is up, and
the answer reopened the form to apply whatever preset matches that block — over
the top of anything typed in the meantime. What was typed is now put back.

**A slow block search could land on top of a later one.** The list under the box
is answered by the service, and an earlier question answered late drew the blocks
for a word that was no longer there. An answer to a question the box has stopped
asking is now dropped.

**A window near the right edge was lost when the browser was narrowed.** The
clamp that keeps a bar reachable ran when a window was dragged and never when the
screen changed size under it, so the only way back was reloading the page — which
is the exact failure the clamp exists to prevent.

**The block search could not be used from the keyboard at all.** It announces
itself as a combobox with a list under it, and the list answered only to the
pointer: no arrows, nothing saying which row was under the keyboard, and a
stylesheet rule for a highlighted row that nothing ever set. The arrows now walk
it in both directions and wrap at both ends, Enter takes the row they are on and
leaves a hand-typed pattern alone, and the box says which row it is on so a
screen reader can follow. The focus stays in the box throughout, which is what
keeps the next keystroke going to the search.

Three files now hold what they are named for. `blocks.js` had grown the marker
form's wiring and the colour feed alongside the block search; the wiring is in
`compose.js` with the rest of the form, the colours sit beside the icons in
`poll.js` doing the identical job, and the hover machinery that keeps a marker's
details up moved from the form to `players.js`, which is what draws the markers
it belongs to. Marker titles are also escaped against quotes rather than only
against angle brackets.

## 0.21.0

**Six of the interface's colours were defined as themselves.** `--accent`,
`--warn` and three of the surface greys each read `var(--accent): var(--accent)`,
which is invalid at computed-value time — so every one of the thirty-odd rules
using them inherited whatever was above it instead. What is focused, what is
being followed, what a refusal says and every raised panel had no colour of their
own. They are real values again, each measured against the surface it sits on.

**The page is three kinds of file rather than one of three thousand lines.**
`viewer/page.html` is what the map is, `viewer/style.css` is what it looks like,
and `viewer/*.js` is what it does — eleven scripts, each about one subject,
joined at compile time and served as one asset. Nothing about the page changed;
it is now possible to read. The scripts and the stylesheet are versioned by the
build and cached forever, so a browser fetches them once per release instead of
carrying them inside every page load.

**A block-name or palette file caught half-written was never read again.** Both
were written straight over the top of themselves by the mod, so the service could
read one mid-write; it then recorded the file as seen and moved on, and on a
server whose mod set had settled nothing would ever move that timestamp again.
The mod now writes every file beside itself and renames it into place, and the
service puts a file it could not read back to unseen so the next second tries.

**An edit that cleared a marker's name hung the form.** A marker with no name is
called "Marker" by the time it comes back, and an edit is known to have landed by
the marker reading as what was asked for — so the form waited out its full twenty
seconds and then reported a failure that had not happened. The form asks for the
name it will get.

**A tile could deadlock against an export.** Drawing the finest level held a read
lock on the world and then asked the world how many levels it had, and a second
read lock taken while holding one may wait forever behind a writer that arrived
in between — which on that lock is the watcher, every time the mod exports.

**Reloading a region cost a pass over every chunk in the world**, twice, because
a region's chunks were found by asking each chunk which region it was in. They
are named by arithmetic now: a region is a fixed square of chunk coordinates, and
the world's own bounds are a walk over a few hundred regions rather than over
millions of columns.

**One owner for every repeated question.** Writing a file beside itself and
renaming it into place was spelled out at five call sites and is now one; so are
reading a query, building a response, naming a stored file, and reading a region
header. `Wanted` and `Edit` were the same nine fields written twice and are one
type. `server.rs` was 1,931 lines doing five jobs and is 196 doing one — the rest
went to `routes`, `state`, `watch`, `feeds`, `apiport`, `http`, `urls`, `cache`,
`net` and `files`. There is a test that fails if a utility ever reaches back up
into the map service, which is the whole of what keeps them reusable.

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
