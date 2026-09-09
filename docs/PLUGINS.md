# Writing a Witchlight plugin

A plugin adds something of its own to the map. It stores rows the game produced
and draws them on the web map. The ore heatmap is one: it takes the prospecting
readings the game already keeps and draws the field between them.

A plugin is an ordinary Vintage Story mod. Somebody installs it by dropping it
in their Mods folder. Nothing else is installed and nothing is configured.

## What a plugin is made of

A plugin spans three parts.

The **mod** runs inside the game server. It declares what its rows look like,
collects them, and hands them to Witchlight. It is C# and it is the only part
that touches the game.

The **service** stores those rows and answers for them. A plugin writes no part
of this. The service creates the plugin's table, decides who may read a row, and
serves the plugin's files.

The **script** runs on the web map and draws. It is plain JavaScript. It asks
the service for the rows this reader may see and puts them on the map.

The service never runs a plugin's code. A plugin is a name, a declared shape for
its rows, and files it shipped. A plugin cannot make the map wrong, because the
tiles are drawn without asking whether a plugin exists.

## Laying out the mod

A plugin keeps what the map serves in a `plugin/` directory inside the mod:

```
WitchlightHeatmap/
  modinfo.json
  HeatmapSystem.cs
  plugin/
    viewer.js          the script the page runs
    scripts/           more script, joined ahead of viewer.js in name order
      10-ore.js
      20-surface.js
    assets/            files the page fetches, such as icons
      icons/prospect.svg
```

The contents of `plugin/` become the plugin's directory beside the map. The mod
copies them there the first time the plugin registers, and again whenever the
mod is newer than what was copied. Nothing else in the mod is copied.

Everything under `scripts/` is read in name order and joined ahead of
`viewer.js`, which comes last so its top-level code can use whatever the rest of
the plugin declared. Number the files to fix their order. The whole is wrapped
in a scope of its own, so two plugins that each declare `const draw` do not
collide, and no individual file has an address of its own.

## Declaring the mod

Name the mod in `modinfo.json` and depend on Witchlight:

```json
{
  "type": "code",
  "modid": "witchlightheatmap",
  "name": "Witchlight Ore Heatmap",
  "version": "0.3.0",
  "side": "server",
  "requiredOnClient": false,
  "dependencies": {
    "witchlight": "0.52.0"
  }
}
```

The modid is the plugin's id. The map keys the plugin's store, its addresses,
its files and its sharing by that name.

Reference Witchlight in the `.csproj` at compile time only:

```xml
<Reference Include="Witchlight">
  <HintPath>$(WitchlightRepo)/Witchlight/bin/$(Configuration)/Witchlight.dll</HintPath>
  <Private>false</Private>
</Reference>
```

`Private=false` keeps the DLL out of the plugin's output. A mod that ships a
second copy of an assembly carrying ModSystems breaks both mods, because the
game loads every mod assembly into one context keyed by name.

The build emits `Witchlight.xml` beside the DLL. An editor reads it for the
documentation on every method a plugin calls.

To run without Witchlight installed, guard with
`api.ModLoader.IsModEnabled("witchlight")`. Put the guard in a different method
from the use. A local of a type from an absent mod fails at method entry whether
or not a guard precedes it.

## Declaring what the rows look like

Register once, from the plugin's own `Start`:

```csharp
var wl = api.ModLoader.GetModSystem<WitchlightSystem>();
var plugin = await wl.Plugins.Register(Mod, new PluginShape()
    .Column("x", PluginKind.Int)
    .Column("y", PluginKind.Int)
    .Column("z", PluginKind.Int)
    .Column("code", PluginKind.Text)
    .Column("amount", PluginKind.Real)
    .KeyedBy("x", "y", "z")
    .RangedBy("x", "z")
    .SeenBy(PluginScope.Owner));
```

`Register` answers a handle, or null when the service would not take the shape.
A null answer is not fatal. A plugin whose rows are not being kept should log
that and go on running.

**Columns** hold one of four types: `Int`, `Real`, `Text`, `Bool`. A name is
lowercase letters, digits, `_` and `-`. A shape may have 32 columns at most. The
service adds `owner_uid` itself and a plugin must not declare it.

**`KeyedBy`** names the columns that identify a row. A row written again with
the same key replaces the one already there. A shape needs a key.

**`RangedBy`** names the columns a reader may ask ranges of. The service builds
an index over them and refuses a range asked of any other column. A plugin
holding a world's worth of rows should range the columns its page filters on.

**`SeenBy`** decides who may see a row. `Owner` gives each row to one player,
who may share it with a group. `World` gives every row to everybody, like the
terrain.

### Changing a shape later

Registering again with the same shape costs a lookup.

Registering with a **column added** keeps the rows already there. The new column
holds null on every existing row. The service copies the database before it
changes anything.

**Any other change is refused** and the rows are left alone. That covers a
dropped column, a changed type, a moved key, and a changed scope. Rebuilding the
table would mean deciding which rows to keep, and only the plugin can decide
that.

To make one of those changes, read the rows out, register under a new name, and
write them back. `Query` reads the map port with no session, so it answers an
owner-scoped plugin with an empty list. An owner-scoped plugin that must migrate
should read its database directly at
`<map data>/plugins/<id>/data.sqlite`, or keep enough in the game's own save to
rebuild from.

## Storing rows

```csharp
plugin.Store(new { x = 4820, y = 61, z = -1190, code = "ore-quartz", amount = 1.4 }, byPlayer);
plugin.StoreMany(rows, byPlayer);
```

Both queue the rows and return at once, so a plugin may call them wherever it
finds something worth keeping. The queue goes out on the next drain, in batches
of 500 and one owner per post.

The player decides who the row belongs to. Pass the player rather than a uid, so
the uid and the name cannot be given as a mismatched pair. A world-scoped plugin
passes null.

A field in the row that matches no declared column is ignored. A declared column
with no field in the row is stored as null. A value is read as the type the
column declared, so a number arriving as a string is stored as the number, and a
value that will not read as its type is stored as null.

Rows that do not land go back on the front of the queue and are sent again. A
restarting service is a reason to wait rather than to throw rows away.

## Drawing on the map

The script registers itself and declares what it answers to:

```js
window.witchlight.plugins.register('witchlightheatmap', {
    start(wl) {
        wl.style(MY_STYLE);
        const layer = wl.layer({pane: 'surface', z: 360, interactive: false});
        layer.show();
    },

    onChange(live, wl) {
    },
});
```

The id must be the same one the mod registered with. Registering an id twice
throws.

Four hooks, all optional:

| Hook | When |
| --- | --- |
| `start(wl)` | Once, when the plugin registers. |
| `onChange(live, wl)` | On every live beat, with what arrived. |
| `onTerrain(terrain, wl)` | When the ground is re-exported. A far slower clock. |
| `onLink(wl)` | When the address changed, by a pasted link or the Back button. |

A hook that throws is reported to the console and leaves the map as it was. A
plugin that fails to start does not stop the map.

`onChange` runs about once a second and almost every call says nothing about any
one plugin. Compare what arrived against what was last drawn before rebuilding
anything. A panel rebuilt on every beat replaces its own controls while somebody
is using them.

### What the script is handed

`wl` is this plugin's own handle. Everything a plugin may touch is on it. It is
the page's own internals under names that will not move, which is the point of
it: a plugin that only ever touches this object goes on working when the page is
rebuilt.

Full signatures are in [`witchlight.d.ts`](witchlight.d.ts).

**Drawing.** `layer` makes a pane of this plugin's own, with a renderer, so a
shape drawn into it appears. `mark` puts a mark on the map in the shape the
page's own marks take. `popup` builds what a mark says when it is opened, with
the map's own furniture around it. `style` puts this plugin's own CSS on the
page. The map's own colours, spacing and fonts are available to it, so a plugin
that uses them follows the reader's settings without knowing they exist.

**Positions.** `at` converts a world position to what Leaflet wants. `said`
writes a position the way this reader has asked to see it, and `meant` reads one
back. Positions are shown relative to spawn unless the reader asked for
absolute, so a plugin that does its own conversion drifts from the markers
around it.

**Furniture.** `panel` builds a window with the same bar, heading and manners
every other window has. `button` puts a button in the column down the left edge.
`setting` adds a switch to the reader's display panel, which is where they look
to turn a layer off. `hotkey` adds a key to the map's own table of them, so it
is listed in the reminder and rebound in the account window with the rest.

**Rows.** `fetch` asks for this plugin's rows, optionally bounded:
`wl.fetch({x: [-1000, 1000], z: [-1000, 1000]})`. `forget` takes one of this
reader's rows away. `sharedWith` and `shareWith` read and set which groups this
reader shares with.

**State.** `kept` holds something this reader set, for this plugin alone, in
this browser. `linked` holds this plugin's state in the page's address, so a
copied link carries what the reader was looking at.

**The rest.** `say` writes a line in the corner the map says things in, which is
the one place always visible. `tool` arms a click-mode, so only one tool on the
map has the next click. `beat` and `started` run work on the page's own clock,
so a plugin's polling is named and reported like everything else. `me` says who
is looking. `reads` says what the reader has set of the map's own switches.
`asset` gives the address of a file this plugin shipped. `pointing` gives the
block under the pointer.

### What comes back from `fetch`

An array of rows. Each carries `Owner`, `OwnerName`, and every column the plugin
declared:

```json
[
  {"Owner": "abc123", "OwnerName": "Wren", "x": 4820, "y": 61, "z": -1190,
   "code": "ore-quartz", "amount": 1.4}
]
```

A value is read back as the type the plugin declared. A column added to a table
with existing rows reads null on those rows.

## Who may see a row

The service decides. It works out whose rows to answer with from the reader's
session and from who has shared with which group, and never from anything the
page or the plugin sent.

A plugin is never handed rows to filter. A browser cannot be asked to hide what
it already holds, so what arrives at the page is already only what may be drawn.
A plugin that decided who may read a row would be a plugin that can give away
where somebody found gold.

A stranger reading an owner-scoped plugin gets an empty list.

## Editor support

Copy [`witchlight.d.ts`](witchlight.d.ts) and [`jsconfig.json`](jsconfig.json)
into the plugin's `plugin/` directory, beside `viewer.js`. An editor then
completes and checks the handle.

Neither file is compiled and neither ships. The plugin stays plain JavaScript.
`witchlight.d.ts` describes `src/page/assets/plugins.js` in the service, and
that file is the source of truth when the two disagree.

For the C# half, the reference to `Witchlight.dll` carries `Witchlight.xml`
beside it, and an editor reads the documentation on every method from there.

## The addresses

A plugin written in C# needs none of this. `WitchlightPlugins` speaks all of it.
A plugin written another way, or one debugging what it sent, can use these.

Registering and storing happen on the service's **API channel**, which is bound
to loopback and needs the token from `api.json` beside the map. Reading happens
on the **map port**, under the reader's session.

| Address | Port | What it does |
| --- | --- | --- |
| `POST /plugins/register/{id}` | API | Registers a plugin. The body is the shape. |
| `POST /plugins/data/{id}` | API | Stores rows: `{"Owner": …, "OwnerName": …, "Rows": […]}`. |
| `GET /plugins` | Map | Lists the ids that have registered. |
| `GET /data/{id}` | Map | The rows this reader may see. Ranges go in the query, as `?x=-1000..1000`. |
| `GET /data/{id}/shares` | Map | The groups this reader shares with. |
| `PUT /data/{id}/shares` | Map | Replaces that set. The body is an array of group ids. |
| `DELETE /data/{id}/{key}` | Map | Takes away one of this reader's rows. The key is the key columns' values, comma separated. |
| `GET /plugins/{id}/viewer.js` | Map | The plugin's script, joined and wrapped. |
| `GET /plugins/{id}/assets/{path}` | Map | A file the plugin shipped. |

A shape is JSON:

```json
{
  "columns": {"x": "int", "z": "int", "code": "text"},
  "key": ["x", "z"],
  "ranged": ["x", "z"],
  "scope": "owner"
}
```

Sharing and deleting need a session. Reading without one answers an
owner-scoped plugin with an empty list.

## When something does not work

**The plugin draws nothing and the console says it never registered.** The
script loaded and did not call `register`, or it threw before reaching it.

**The console says the script could not be loaded.** The plugin's files were not
copied. The server log says why, under `nothing to draw with`. The usual cause
is no `plugin/` directory in the mod.

**The server log says the plugin was refused.** The shape was rejected, and the
log gives the reason. A refused registration keeps no rows.

**The server log says rows were held back.** The service did not take them. They
are still queued and go out again.

**The map draws nothing for one reader and everything for another.** That is
sharing working. An owner-scoped row is seen by its owner and by the groups they
shared it with.

**A range comes back as an error.** The column was not declared ranged. Add it
to `RangedBy` and register again, which is a shape change the service accepts
only as part of a wider one.

## Where things live

| Path | What |
| --- | --- |
| `<map data>/plugins/<id>/data.sqlite` | The plugin's own rows. |
| `<map data>/plugins/<id>/viewer.js` | The script, as copied from the mod. |
| `<map data>/plugins/<id>/scripts/` | The rest of the script. |
| `<map data>/plugins/<id>/assets/` | The files the page fetches. |
| `<map data>/plugins/<id>/data.shape<fingerprint>.bak` | A copy kept before a shape changed. |

Each plugin has a database of its own. Uninstalling a plugin is deleting its
folder. The map's own database is never touched by a plugin, so a map stays
readable by a build that has never heard of one.

## A worked example

[`witchlight-heatmap`](https://github.com/Tekunogosu/witchlight-heatmap) is a
complete plugin: an owner-scoped shape, a collector reading the game's
prospecting data, a panel, a layer drawn under the marks, sharing, hotkeys, and
state kept in the address.
