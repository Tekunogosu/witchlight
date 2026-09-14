// What a plugin's script is handed.
//
// A plugin draws on the map and never in it. The tiles are the service's alone,
// drawn without ever asking whether a plugin exists, so nothing here can make
// the map itself wrong — what a plugin adds is an overlay above the ground, and
// a reader who turns it off gets the same map back exactly.
//
// Everything below is the page's own internals under names that will not move.
// That is the whole point of it: the alternative is what other map services do,
// where an addon reaches into whatever the app happens to be built out of and
// breaks whenever the app is rebuilt. A plugin that only ever touches what is on
// this object is a plugin that keeps working.
//
// Loaded before `gatherCorner`, because a bar that hangs in the tool column has
// to exist before the column is gathered — see the end of `poll.js`.

/** Every plugin that has registered, by id, in the order they did. */
const plugins = new Map();

/** How many buttons each plugin has made, so no two of them share an id. */
const pluginButtons = new Map();

/** Where a plugin's own kept things live, beside the map's own settings. */
const PLUGIN_KEPT = 'witchlight.plugins';

/** The most one plugin may keep, so a plugin in a loop cannot fill the browser. */
const MOST_KEPT = 64;

/**
 * Something a reader set, kept for one plugin and read back.
 *
 * In the browser rather than on the account, which is where the map keeps the
 * rest of what one screen is set to. Read and written whole each time: it is a
 * handful of small values, and holding a copy would be a second thing to keep in
 * step with what another tab has written.
 */
function pluginKept(id, name, value) {
  let held = {};
  try {
    held = JSON.parse(localStorage.getItem(PLUGIN_KEPT) || '{}') || {};
  } catch (error) {
    /* a browser that will not say is a browser this starts empty */
  }
  // Not `mine`: that is the page's own account preferences, and a local of that
  // name shadows it for everything below.
  const ours = (held[id] && typeof held[id] === 'object') ? held[id] : {};

  if (value === undefined) return ours[name];

  if (value === null) delete ours[name];
  else ours[name] = value;

  // Bounded, because what is written here came from a plugin rather than from
  // this page, and a plugin writing a new name every beat would grow this file
  // without end.
  const names = Object.keys(ours);
  while (names.length > MOST_KEPT) delete ours[names.shift()];

  held[id] = ours;
  try {
    localStorage.setItem(PLUGIN_KEPT, JSON.stringify(held));
  } catch (error) {
    console.warn(`witchlight: ${id} could not keep ${name}`, error);
  }
  return value;
}

/**
 * What each plugin is currently saying to the reader, by id.
 *
 * Kept per plugin rather than as one line, so that two plugins with something to
 * report do not overwrite each other — and so that a plugin clearing its own
 * message cannot clear somebody else's.
 */
const pluginSaying = new Map();

/** Writes the corner's plugin line from what every plugin is saying. */
function drawPluginSaid() {
  const line = document.getElementById('plugin-said');
  if (!line) return;
  line.textContent = '';
  for (const [, { what, wrong }] of pluginSaying) {
    const word = document.createElement('span');
    word.textContent = what;
    if (wrong) word.className = 'wrong';
    line.append(word);
  }
  line.classList.toggle('saying', pluginSaying.size > 0);
}

/**
 * How wide a plugin's own pane may sit, and where.
 *
 * Between the claims and the markers: a plugin draws ground rather than things
 * standing on it, and a reading of the ground belongs under the pins somebody
 * placed. Each plugin gets a pane of its own so that one plugin's drawing cannot
 * disturb another's, and the number rises with each so the order they registered
 * in is the order they stack.
 */
const PLUGIN_PANE_FLOOR = 360;

/** Panes this page has already made, so two plugins cannot claim one name. */
const pluginPanes = new Set(['grid', 'claims', 'heatmap', 'markerPane', 'overlayPane']);

/**
 * What one plugin may do to the page.
 *
 * Built fresh for each, closing over its own id so that nothing it is handed can
 * name another plugin's rows, files or furniture. A plugin cannot reach another
 * plugin through this object, which is not a security boundary — everything on
 * this page shares one scope and always will — but is enough that a plugin
 * cannot do it *by accident*.
 */
function pluginHandle(id) {
  const own = `/plugins/${id}/assets`;

  return {
    id,

    /**
     * Where a file this plugin shipped is served from.
     *
     * `asset('icons/mountains.svg')` rather than the whole address, so a plugin
     * never writes its own id into a path and a plugin renamed is a plugin that
     * still works.
     */
    asset: (path) => `${own}/${String(path).replace(/^\/+/, '')}`,

    /**
     * A world position as Leaflet wants it. Latitude is Z.
     *
     * Handed over rather than left to be worked out, because the page has said
     * this in one place since the map was written and a plugin doing its own
     * conversion is a plugin whose overlay drifts from the markers on top of it.
     */
    at,

    /**
     * A world position as the reader sees it, and back again.
     *
     * These two are a pair and are the reader's own setting: coordinates are
     * shown relative to spawn unless they asked for absolute. A plugin writing
     * a position into a label wants `said`; one reading a position a reader
     * typed wants `meant`. Getting them the wrong way round puts a thing a
     * spawn away from where it belongs, which is why they are here rather than
     * left to be reinvented.
     */
    said,
    meant,

    /** Who is looking, as `/me` last answered. Null for a stranger. */
    me: () => (viewer ? { uid: viewer.Uid, name: viewer.Name, groups: viewer.Groups || [] } : null),

    /**
     * A layer of this plugin's own, in a pane of its own.
     *
     * `interactive: false` turns pointer events off for the whole pane, which is
     * what an overlay drawn over the ground wants: a tile that swallowed a
     * click would take it from the marker underneath.
     */
    layer({ pane, z, interactive = false } = {}) {
      const name = pane ? `plugin-${id}-${pane}` : `plugin-${id}`;
      if (pluginPanes.has(name)) {
        throw new Error(`witchlight: ${id} asked for the pane ${name} twice`);
      }
      pluginPanes.add(name);

      map.createPane(name);
      const made = map.getPane(name);
      made.style.zIndex = String(z ?? PLUGIN_PANE_FLOOR + pluginPanes.size);
      if (!interactive) made.style.pointerEvents = 'none';

      // A renderer of this pane's own, and the reason for it is not obvious: a
      // shape drawn into a pane nothing renders into is added without complaint
      // and never appears. Leaflet draws vectors through one SVG surface, which
      // it makes on the overlay pane unless it is told otherwise — so a
      // rectangle handed `pane` alone is a rectangle in a pane with nothing to
      // draw it. Marks do not need this, being ordinary elements; shapes do,
      // and a plugin should not have to know which is which.
      const renderer = L.svg({ pane: name });
      const group = L.layerGroup([], { pane: name, renderer });
      return {
        pane: name,
        group,
        renderer,
        show: () => {
          renderer.addTo(map);
          group.addTo(map);
        },
        hide: () => {
          group.remove();
          renderer.remove();
        },
      };
    },

    /**
     * A mark on the map, in the shape the page's own already take.
     *
     * The same `L.divIcon` a player and a waypoint are drawn with, so a plugin's
     * marks sit among them rather than beside them looking like something else.
     * Anything Leaflet can draw is still available on the layer this is added
     * to; this is the common case made short, not the only way through.
     */
    /**
     * What a mark says when it is opened, in the shape the map's own popups take.
     *
     * A plugin that built its own came out looking like something else on the
     * same map — different type, different spacing, a heading that was not a
     * heading — because the classes that make a popup look like this one are the
     * page's and a plugin cannot see them. So the shape is offered rather than
     * described: a plugin says what it wants said and gets the map's own
     * furniture around it.
     *
     * `name` is the heading. `where` is a position, which is written the way
     * this reader has asked to see positions. `who` is whose it is. `rows` is
     * the body: each is `{ mark, said, band, note }`, and every part is
     * optional — a row of just `said` is a line of text, and one with all four
     * is a swatch, a name, a word for how much, and a number.
     *
     * Answers an element, which is what `mark`'s own `popup` takes.
     */
    popup({ name, where, who, rows = [], note } = {}) {
      const box = document.createElement('div');
      box.className = 'plugin-said';

      if (name) {
        const heading = document.createElement('b');
        heading.className = 'said-name';
        heading.textContent = String(name);
        box.append(heading);
      }

      if (note) {
        const line = document.createElement('span');
        line.className = 'said-may';
        line.textContent = String(note);
        box.append(line);
      }

      if (rows.length > 0) {
        const table = document.createElement('div');
        table.className = 'plugin-said-rows';
        for (const line of rows) {
          const swatch = document.createElement('span');
          swatch.className = 'plugin-said-mark';
          // Kept even where a row names no colour, so that the names in a list
          // of rows line up whether or not every one of them has a swatch.
          if (line.mark) swatch.style.background = String(line.mark);
          else swatch.classList.add('plain');

          const what = document.createElement('span');
          what.className = 'plugin-said-what';
          what.textContent = String(line.said ?? '');

          const band = document.createElement('span');
          band.className = 'plugin-said-band';
          band.textContent = line.band === undefined ? '' : String(line.band);

          const number = document.createElement('span');
          number.className = 'plugin-said-note';
          number.textContent = line.note === undefined ? '' : String(line.note);

          table.append(swatch, what, band, number);
        }
        box.append(table);
      }

      // The footer is the map's own: where it is on the left, whose it is on the
      // right, under a rule. Left off entirely where a plugin gave neither,
      // rather than drawn as an empty strip.
      if (where !== undefined || who) {
        const foot = document.createElement('div');
        foot.className = 'said-foot';
        const spot = document.createElement('span');
        spot.className = 'said-where';
        if (Array.isArray(where)) {
          // The page's own `said`, which is why nothing here may be called that:
          // a local of the same name would shadow the one this needs.
          const [x, z] = said(where[0], where[1]);
          spot.textContent = `${x}, ${z}`;
        } else {
          spot.textContent = where === undefined ? '' : String(where);
        }
        const by = document.createElement('span');
        by.className = 'said-who';
        by.textContent = who ? String(who) : '';
        foot.append(spot, by);
        box.append(foot);
      }

      return box;
    },

    /**
     * Puts this plugin's own markers in the map's marker list.
     *
     * They are drawn on the map and listed in the marker window beside the
     * game's own, under the same all/public/private tabs, the same search, the
     * same ordering and the same per-marker "show on map" box — which is the
     * whole point of putting them there rather than drawing a second set.
     *
     * Nothing is stored. These live in the page for as long as the plugin says
     * so: no waypoint is made, the game is never told, and closing the page
     * takes them with it. A plugin that wants them to outlive a reload keeps
     * them in its own rows and says them again on the next load.
     *
     * Each marker takes `Key` (this plugin's own name for it, unique among this
     * plugin's markers), `Title`, `X`, `Y`, `Z`, and optionally `Icon`,
     * `Color`, `Owner` and `Private`. Calling this replaces everything this
     * plugin last said; calling it with an empty list takes them all away.
     */
    places(markers, onEdit) {
      if (typeof onEdit === 'function') pluginPlaceEdits.set(id, onEdit);
      const given = Array.isArray(markers) ? markers : [];
      pluginPlaces.set(id, given.map((place, nth) => ({
        Key: String(place.Key ?? nth),
        Title: String(place.Title ?? ''),
        X: Number(place.X) || 0,
        Y: Number(place.Y) || 0,
        Z: Number(place.Z) || 0,
        Icon: place.Icon ? String(place.Icon) : '',
        Color: colourOf(place.Color),
        Owner: place.Owner ? String(place.Owner) : '',
        Private: Boolean(place.Private),
        // What the popup says under the heading, as markup the plugin has
        // already made safe. The same trust `popup` on this handle takes.
        Said: place.Said ? String(place.Said) : '',
      })));
      // Drawn from what is held rather than from what arrived, since the poll
      // is what usually calls this and the poll is not what changed.
      redrawPlaces();
    },

    /**
     * A mark drawn the way the map draws its own markers.
     *
     * The same picture a waypoint of that colour and picture would be, from the
     * one function that draws all of them, so a plugin's marks and the map's
     * cannot come to differ. `picture` is a name the service offers under
     * `/icons`; one it does not have is drawn as the diamond that stands in for
     * a missing picture everywhere else.
     */
    markFor: (picture, colour) => markFor(picture, colourOf(colour)),

    /**
     * Asks the reader for a colour and a picture, in the map's own picker.
     *
     * The same swatches the marker form offers, over the same palette the game
     * sent and the same pictures the service has — so a mod that adds a colour
     * to the game's picker adds it here too, and a plugin does not ship a
     * palette of its own to fall out of step with it.
     *
     * Nothing is stored. This asks a question and answers it; what is done with
     * the answer, and where it is kept, is the plugin's own. A marker is not
     * made and the map's own markers are not touched.
     *
     * Answers `{colour, picture}` when the reader chooses, and null when they
     * close the window instead — a dismissal is an answer of "no change" rather
     * than a failure, so it resolves rather than rejects.
     */
    pick({ colour = '#ffffff', picture = 'circle', title = 'Pick a mark' } = {}) {
      return new Promise(answerWith => {
        const held = { colour: colourOf(colour), picture };

        // A window of the plugin's own, built the way its panels are, so this
        // wears the map's furniture and answers Escape like everything else.
        const asked = document.createElement('div');
        asked.className = 'window';
        asked.id = `plugin-${id}-pick`;

        const titleBar = document.createElement('div');
        titleBar.className = 'bar';
        const heading = document.createElement('h2');
        heading.textContent = title;
        const shut = document.createElement('button');
        shut.className = 'shut';
        shut.type = 'button';
        shut.title = 'Close';
        shut.setAttribute('aria-label', 'Close');
        shut.append(chromeMark('x'));
        titleBar.append(heading, shut);

        const body = document.createElement('div');
        body.className = 'plugin-body scroll';

        const { colours, pictures } = pickerFields(held);
        body.append(colours, pictures);

        const take = document.createElement('button');
        take.type = 'button';
        take.className = 'word go';
        take.textContent = 'Use this';
        body.append(take);

        asked.append(titleBar, body);
        document.body.append(asked);
        dragBy(asked);
        growBy(asked);
        sizeWindow(asked, 320, 380);

        // Answered once, whichever way the window goes. `shutting` is what runs
        // on the close button, on Escape and on anything else that puts a
        // window away, so the promise is settled from there rather than from
        // the button alone.
        let answered = false;
        const finish = answer => {
          if (answered) return;
          answered = true;
          shutting.delete(asked);
          shutWindow(asked);
          asked.remove();
          answerWith(answer);
        };

        shutting.set(asked, () => finish(null));
        take.addEventListener('click', () => finish({ ...held }));

        openWindow(asked, true);
      });
    },

    mark({ x, z, icon, label, popup, pane, hover = true }) {
      // Built rather than written into an attribute. A `url()` in markup is a
      // string the browser resolves against the document, so one that is empty
      // — a plugin that named no icon, or named one that came back undefined —
      // resolves to the page itself and is fetched as `file:///` when the page
      // was not served over http. Quoted for the same reason: a path with a
      // bracket or a space in it is not a url() somebody wrote by hand.
      //
      // An element rather than a path is taken as the mark itself, which is what
      // `markFor` on this handle answers with: a plugin asking for one of the
      // map's own marks hands it straight back rather than unpicking it into a
      // url this would put together again.
      let mark;
      if (icon instanceof Element) {
        mark = icon;
      } else {
        mark = document.createElement('span');
        mark.className = icon ? 'plugin-mark' : 'plugin-mark plain';
        if (icon) {
          const url = `url("${String(icon).replace(/["\\]/g, '\\$&')}")`;
          mark.style.webkitMaskImage = url;
          mark.style.maskImage = url;
        }
      }
      const drawn = L.marker(at(x, z), {
        icon: L.divIcon({ className: 'plugin-pin', iconSize: [0, 0], html: mark.outerHTML }),
        ...(pane ? { pane } : {}),
      });
      if (label) drawn.bindTooltip(String(label));
      if (popup) drawn.bindPopup(popup);
      // Into the reader's own hover setting, the way every other thing on the
      // map is. A plugin's mark that wired its own would open a second box
      // beside a marker's and ignore a reader who had asked for neither.
      if (popup && hover) hoverOpens(drawn);
      return drawn;
    },

    /**
     * A window of this plugin's own, built rather than found.
     *
     * A plugin cannot edit `page.html`, so a plugin that had to find its panel
     * by id could never have one. This builds the same shape every other window
     * on the page has — a bar, a heading, a way out — and hands back the body to
     * fill, already draggable, resizable, stacked and answering Escape.
     */
    panel({ title, wide, high, onShut } = {}) {
      const panel = document.createElement('div');
      panel.className = 'window';
      panel.id = `plugin-${id}-panel`;

      const titleBar = document.createElement('div');
      titleBar.className = 'bar';
      const heading = document.createElement('h2');
      heading.textContent = title || id;
      const shut = document.createElement('button');
      shut.className = 'shut';
      shut.type = 'button';
      shut.title = 'Close';
      shut.setAttribute('aria-label', 'Close');
      shut.append(chromeMark('x'));
      titleBar.append(heading, shut);

      // The body is the scrolling box, so a plugin's bar stays put while its
      // contents scroll under it — the same arrangement every other window on
      // the page has, and a plugin gets it without asking.
      const body = document.createElement('div');
      body.className = 'plugin-body scroll';

      panel.append(titleBar, body);
      document.body.append(panel);

      // The same manners every other window has, asked for here rather than in
      // `buildWindows` — a plugin's panel does not exist when that runs.
      dragBy(panel);
      if (wide || high) growBy(panel);
      if (wide || high) sizeWindow(panel, wide, high);
      if (onShut) shutting.set(panel, onShut);

      return {
        panel,
        body,
        open: (middle = true) => openWindow(panel, middle),
        shut: () => shutWindow(panel),
        get isOpen() {
          return panel.classList.contains('open');
        },
      };
    },

    /**
     * A button down the left edge, in the bar shape the map's own take.
     *
     * `mark` is either one of the marks compiled into the service or a file this
     * plugin shipped — anything with a slash in it is read as the plugin's own,
     * since a compiled mark is only ever a name.
     */
    button({ mark, label, onPress }) {
      // Numbered after the first, because a plugin may have more than one button
      // and an id is supposed to name one element. Kept bare for the first so
      // that a plugin with a single button has the id its name suggests, and so
      // that the column's own spacing rule — which matches on the prefix — goes
      // on reading every one of them.
      const made = pluginButtons.get(id) || 0;
      pluginButtons.set(id, made + 1);
      const tools = cornerButton(
        made === 0 ? `plugin-${id}` : `plugin-${id}-${made + 1}`,
        'puzzle-piece',
        label || id,
      );
      const anchor = tools.querySelector('a');

      if (mark && String(mark).includes('/')) {
        const url = `${own}/${String(mark).replace(/^\/+/, '')}`;
        const drawn = anchor.querySelector('.chrome');
        if (drawn) {
          const quoted = `url("${url.replace(/["\\]/g, '\\$&')}")`;
          drawn.style.webkitMaskImage = quoted;
          drawn.style.maskImage = quoted;
        }
      } else if (mark) {
        const drawn = anchor.querySelector('.chrome');
        if (drawn) drawn.className = `chrome masked mark-${mark}`;
      }

      if (onPress) anchor.addEventListener('click', () => onPress());
      return { bar: tools, anchor };
    },

    /**
     * This plugin's own rows, as this reader may see them.
     *
     * The address is built here so a plugin never writes one, and the ranges are
     * spelled the way the service reads them. What comes back is what the
     * service decided this reader may have — a plugin is never handed rows to
     * filter, because a browser cannot be asked to hide what it already holds.
     */
    async fetch(ranges) {
      const query = ranges
        ? '?' + Object.entries(ranges)
            .map(([column, [low, high]]) => `${column}=${low}..${high}`)
            .join('&')
        : '';
      const answer = await fetch(`/data/${id}${query}`, { cache: 'no-store' });
      if (!answer.ok) throw new Error(await answer.text());
      return answer.json();
    },

    /**
     * A plugin's own styling, put on the page.
     *
     * A plugin builds its panel from script and cannot add a rule to the map's
     * own stylesheet, so it says here what its own furniture looks like. Given
     * as text rather than as a file to fetch, because it is small and because a
     * panel that appears before its styling arrives is a panel that jumps.
     *
     * Everything the map's own stylesheet defines is available to it — the
     * colours, the spacing, the fonts — so a plugin that uses those looks like
     * part of the map rather than like something bolted to it, and follows the
     * reader's own settings without having to know they exist.
     */
    style(css) {
      const sheet = document.createElement('style');
      sheet.dataset.plugin = id;
      sheet.textContent = String(css);
      document.head.append(sheet);
      return sheet;
    },

    /**
     * Which groups this reader shares this plugin's rows with.
     *
     * Kept by the service rather than by the plugin, and enforced there: a
     * plugin that decided who may read a row is a plugin that can give away
     * where somebody found gold. What a plugin does with this is draw the
     * checkboxes.
     */
    async sharedWith() {
      const answer = await fetch(`/data/${id}/shares`, { cache: 'no-store' });
      return answer.ok ? answer.json() : [];
    },

    /** Replaces that set. The whole of it, since it is one answer. */
    async shareWith(groups) {
      const answer = await fetch(`/data/${id}/shares`, {
        method: 'PUT',
        body: JSON.stringify(groups),
      });
      return answer.ok;
    },

    /** One row of this reader's own, taken away. */
    async forget(key) {
      const named = Array.isArray(key) ? key.join(',') : String(key);
      const answer = await fetch(`/data/${id}/${named}`, { method: 'DELETE' });
      return answer.ok;
    },

    /**
     * Arms or puts down a click-mode of this plugin's own.
     *
     * The map has three tools that want the next click — the block picker, the
     * marker placer, and now this — and only one of them may have it. Told here,
     * arming one puts the others down; a plugin that wired its own listener
     * instead ended up with two cursors lit and the click going to whichever was
     * asked for first.
     *
     * `drop` is called when something else takes the click, and is where the
     * plugin un-arms its own button.
     */
    tool(on, drop) {
      armPluginTool(id, Boolean(on), typeof drop === 'function' ? drop : () => {});
    },

    /** What block is under the pointer, as the map last read it. Null where none. */
    pointing: () => (pointer ? { x: Math.round(pointer.lng), z: Math.round(pointer.lat) } : null),

    /**
     * Something this reader has set, kept for them and for this plugin alone.
     *
     * Namespaced, bounded and beside the map's own settings rather than in a
     * corner of `localStorage` a plugin picked for itself — so two plugins
     * cannot collide, and what is kept goes when the reader clears the map's
     * settings rather than outliving it.
     *
     * `kept(name)` reads and `kept(name, value)` writes. Values are JSON: a
     * number, a string, a boolean, or something small made of those.
     */
    kept(name, value) {
      return pluginKept(id, String(name), value);
    },

    /**
     * This plugin's own state in the page's address, kept and read back.
     *
     * What a reader copies out of the address bar should carry what they were
     * looking at and not only where they were looking — which layer of a
     * plugin's was shown, what it was filtered to. Namespaced by plugin id, so
     * two plugins cannot claim one word.
     *
     * `linked(name)` reads, `linked(name, value)` writes, and writing nothing
     * takes it out again. A change to the address from outside — a pasted link,
     * the Back button — reaches the plugin as `onLink`.
     */
    linked(name, value) {
      const key = `${id}.${name}`;
      if (value === undefined) return readAddressExtras().get(key);
      keepInAddress(key, value);
      return value;
    },

    /**
     * Says something to the reader, in the corner the map says things in.
     *
     * A plugin's own window is usually shut, so a plugin that reported trouble
     * there reported it to nobody. This is the one line on the page that is
     * always visible.
     *
     * Called with nothing, it takes the message away again. `wrong` draws it in
     * the colour every window on this page uses for something that did not
     * work. Keep it to a few words: it is a line in a corner, not a log.
     */
    say(what, wrong = false) {
      if (what === undefined || what === null || what === '') pluginSaying.delete(id);
      else pluginSaying.set(id, { what: String(what), wrong: Boolean(wrong) });
      drawPluginSaid();
    },

    /**
     * A switch of this plugin's own, in the reader's display panel.
     *
     * Where a reader looks to turn a layer off is the panel behind the cog, not
     * a control inside one plugin's window — so an overlay that could only be
     * switched from its own panel was an overlay half of them never found. What
     * they set is remembered in this browser with the rest.
     *
     * `panel: 'access'` puts it in the accessibility window instead, which is
     * where something about how the map is read belongs rather than what it
     * shows.
     */
    setting(name, { label, on, panel, apply } = {}) {
      return addPluginSetting(id, String(name), { label, on, panel, apply });
    },

    /**
     * Puts one of this plugin's own switches where the plugin says it is.
     *
     * A plugin usually offers more than one way to turn a thing on — a button in
     * the column, a key, the switch itself — and the switch has to follow all of
     * them or it is a box that disagrees with the map beside it. `setSetting` is
     * what keeps the box, what is remembered and what is applied in step, so
     * this is that, with the plugin's own name filled in.
     */
    setSetting(name, on) {
      setSetting(`${id}:${name}`, Boolean(on));
    },

    /**
     * What this reader has set, of the map's own switches.
     *
     * Read-only, and by name: a plugin that wants to follow the reader's choice
     * about coordinates or hover can ask, and a plugin cannot reach in and
     * change a switch that is not its own.
     */
    reads: (name) => Boolean(settings[name] && settings[name].on),

    /**
     * A key of this plugin's own, in the map's own table of them.
     *
     * Pressed, listed in the reminder under the map, and rebound in the account
     * window by the same code that does all three for the map's own keys — a
     * plugin's key is not a second key system beside the reader's, it is one
     * more row in theirs, and what they rebind it to is kept with the rest.
     *
     * `key` is a default, not a claim. A key the map already answers to is left
     * to the map: a plugin installed on Tuesday must not quietly take over a
     * press a reader has been using since Monday. Such an action arrives with no
     * key and the account window is where it is given one.
     *
     * `offered` says whether the key does anything right now — the map's own
     * keys answer with the button they stand for, so a key is silent while its
     * button is not on the page. Return a plain `true` where there is no button
     * to point at. Left out, the key works whenever the plugin is loaded.
     */
    hotkey(name, { label, key, offered, act } = {}) {
      return addPluginHotkey(id, String(name), { label, key, offered, act });
    },

    /**
     * Work on a clock, counted the way the page's own is.
     *
     * A plugin using `setInterval` is a plugin whose polling nobody can see and
     * which goes on running after its own error. These are the page's own — see
     * `work.js` — so a plugin's work is reported and named like everything else.
     */
    beat: (fn, ms, what) => beat(fn, ms, what || `${id}'s poll`),
    started: (promise, what) => started(promise, what || `${id}'s work`),
  };
}

/**
 * The markers each plugin is putting in the list, by plugin.
 *
 * A plugin's markers are the page's to draw and nobody's to keep: they are held
 * here for as long as the plugin says so and are gone when the page is closed.
 * Nothing is sent to the game and nothing reaches the service, which is what
 * separates one of these from a waypoint — a waypoint is a thing in somebody's
 * world, and one of these is a thing on a map of it.
 *
 * Keyed by plugin so that one plugin replacing its markers leaves every other
 * plugin's alone, and so a plugin that stops saying anything takes only its own
 * with it.
 */
const pluginPlaces = new Map();

/**
 * What each plugin asked to be told when one of its markers is changed, by
 * plugin. A plugin that named nothing is not offered the edit.
 */
const pluginPlaceEdits = new Map();

/**
 * Hands one changed marker back to the plugin that put it there.
 *
 * The page does not keep the change: a plugin's markers are drawn from what the
 * plugin last said, so an edit that the plugin does not act on is an edit that
 * disappears on the next draw. That is the honest behaviour — what a marker
 * says is the plugin's record to change, and this is the asking.
 *
 * The key handed back is the plugin's own, without the `plugin:{id}:` the page
 * put in front of it, since the plugin never saw that.
 */
function changedPluginPlace(place, changes) {
  const wanted = pluginPlaceEdits.get(place.Plugin);
  if (!wanted) return;
  const ownKey = String(place.Key).slice(`plugin:${place.Plugin}:`.length);
  try {
    wanted({ ...changes, Key: ownKey });
  } catch (error) {
    console.warn(`witchlight: ${place.Plugin} failed on a marker edit`, error);
  }
}

/**
 * Every plugin's markers, as one list, in the shape a waypoint takes.
 *
 * `Key` is written here rather than taken from the plugin: the map, the list and
 * the hidden set all address a marker by it, and two plugins each numbering
 * their markers from one would otherwise be two markers claiming one row. The
 * plugin's own id is what makes it unambiguous.
 *
 * `Private` is carried through as the plugin set it, so a plugin's marker sits
 * under the same all/public/private tabs every other marker does. `OwnerUid` is
 * deliberately absent: `mayEdit` reads it to decide whether to offer the game's
 * own edit, and a marker the game has never heard of has no edit to offer.
 */
function pluginMarkers() {
  const all = [];
  for (const [id, places] of pluginPlaces) {
    for (const place of places) {
      all.push({
        ...place,
        Key: `plugin:${id}:${place.Key}`,
        Plugin: id,
      });
    }
  }
  return all;
}

/**
 * What a plugin shuts down with, by the panel it belongs to.
 *
 * `shutWindow` knows what the page's own windows have to forget when they close;
 * a plugin's panel is not one of those and says for itself.
 */
const shutting = new Map();

/**
 * A plugin, registered.
 *
 * Called by a plugin's own script, which the service serves and the page loads
 * before it starts. Everything the plugin is given is on the handle it is
 * handed; nothing here reaches back into it.
 */
function registerPlugin(id, plugin) {
  if (!id || typeof id !== 'string') throw new Error('witchlight: a plugin needs a name');
  if (plugins.has(id)) throw new Error(`witchlight: ${id} has registered twice`);

  const handle = pluginHandle(id);
  plugins.set(id, { plugin, handle });

  try {
    if (typeof plugin.start === 'function') plugin.start(handle);
  } catch (error) {
    // One plugin that throws on start is one plugin that does not draw. The map
    // is the service's and goes on without it.
    console.error(`witchlight: ${id} failed to start`, error);
  }

  return handle;
}

/**
 * What the live feed last said, as a plugin is shown it.
 *
 * Built once per beat and handed to every plugin, rather than each of them
 * fetching `/live` again: the page has just read all of this, and a plugin
 * asking for it a second time doubles the traffic to say what the page already
 * knows. Frozen shallowly so that one plugin cannot hand the next a changed
 * copy — the arrays inside are the page's own and are not to be written to.
 */
function livePassed(live) {
  return Object.freeze({
    /** Every player the reader may see, as the mod described them. */
    players: live.Players || [],
    /** How many are on, which is not the same as how many are listed. */
    online: Number.isFinite(live.Online) ? live.Online : (live.Players || []).length,
    /** Which of them share a group with whoever is looking. */
    grouped: live.Grouped || [],
    /** Every marker this reader may see. */
    markers: live.Waypoints || [],
    /** Every land claim this reader may see. */
    claims: live.Claims || [],
    /** The world's own clock, as the game last said it. */
    world: live.World || null,
    /** How tall the world is, in blocks. */
    height: live.Height,
    /** How long the service asks the page to leave between beats, in ms. */
    every: LIVE_BEAT,
  });
}

/**
 * Tells every plugin the live feed moved, and what it said.
 *
 * Called where the page takes a live reading, so a plugin hears about a change
 * on the same beat the markers do rather than on a clock of its own — and is
 * handed what arrived, so it does not have to ask again to find out.
 */
function pluginsChanged(live) {
  // Not `said`: that is the page's own coordinate helper, and a local of that
  // name shadows it for everything below.
  const reading = livePassed(live || {});
  for (const [id, { plugin, handle }] of plugins) {
    if (typeof plugin.onChange !== 'function') continue;
    try {
      plugin.onChange(reading, handle);
    } catch (error) {
      console.error(`witchlight: ${id} failed on a change`, error);
    }
  }
}

/**
 * Tells every plugin the address changed under it.
 *
 * Somebody pasted a link, or pressed Back. What a plugin kept in the hash may
 * now say something different, and the plugin is the only thing that knows what
 * to do about that.
 */
function pluginsRelinked() {
  for (const [id, { plugin, handle }] of plugins) {
    if (typeof plugin.onLink !== 'function') continue;
    try {
      plugin.onLink(handle);
    } catch (error) {
      console.error(`witchlight: ${id} failed on a new address`, error);
    }
  }
}

/**
 * Tells every plugin the terrain itself changed.
 *
 * A different thing from the live beat, and on a far slower clock: the live feed
 * is who moved, and this is the ground being re-exported. A plugin drawing
 * something worked out from the terrain had no way to hear about this at all.
 */
function pluginsRedrew(info) {
  for (const [id, { plugin, handle }] of plugins) {
    if (typeof plugin.onTerrain !== 'function') continue;
    try {
      plugin.onTerrain(
        Object.freeze({
          /** Rises whenever the ground is re-exported. */
          generation: info.generation,
          /** The world's edges, in blocks. */
          bounds: Object.freeze({ ...info.bounds }),
        }),
        handle,
      );
    } catch (error) {
      console.error(`witchlight: ${id} failed on a terrain change`, error);
    }
  }
}

window.witchlight.plugins = { register: registerPlugin };

/**
 * Fetches every registered plugin's script and runs it.
 *
 * The page cannot know at build time which plugins a server has: a plugin
 * registers while the server runs, and the page is compiled into the binary. So
 * the list is asked for and each script is loaded from the address the service
 * serves it at.
 *
 * Loaded as a classic script through a tag rather than evaluated from text,
 * because a tag gives a plugin's own errors a file and a line number in the
 * console. A plugin that will not load leaves the map exactly as it was.
 *
 * Every step says what it did. A plugin drawing nothing is the failure this has
 * had three times over, and each time it was silent from the page's side — see
 * the loading notes in `viewer.rs`.
 */
async function loadPlugins() {
  let names;
  try {
    const answer = await fetch('/plugins', { cache: 'no-store' });
    if (!answer.ok) throw new Error(`the service answered ${answer.status}`);
    names = await answer.json();
  } catch (error) {
    console.error('witchlight: could not ask which plugins there are', error);
    return;
  }

  if (!Array.isArray(names) || names.length === 0) {
    console.info('witchlight: no plugins are registered');
    return;
  }

  console.info(`witchlight: loading ${names.length} plugin(s): ${names.join(', ')}`);
  await Promise.all(names.map((id) => new Promise((done) => {
    const tag = document.createElement('script');
    tag.src = `/plugins/${encodeURIComponent(id)}/viewer.js`;
    tag.async = false;
    tag.addEventListener('load', () => {
      // Loading is not registering: a script that ran and never called
      // `register` is a plugin that will draw nothing, and saying so here is
      // what turns that from a blank map into one line in the console.
      if (!plugins.has(id)) {
        console.warn(`witchlight: ${id} loaded but never registered`);
      }
      done();
    });
    tag.addEventListener('error', () => {
      console.error(`witchlight: ${id}'s script could not be loaded from ${tag.src}`);
      done();
    });
    document.head.append(tag);
  })));
}
