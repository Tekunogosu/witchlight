// Type declarations for the Witchlight page API.
//
// These describe what a plugin's script is handed. They exist so an editor can
// complete and check a plugin's `viewer.js`. Nothing here is compiled and
// nothing here ships. A plugin is written in plain JavaScript.
//
// Point an editor at this file by copying `jsconfig.json` from this directory
// into the plugin's own `plugin/` directory. See PLUGINS.md.
//
// The source of truth is `src/page/assets/plugins.js` in the service. When the
// two disagree, that file is right and this one is stale.

/** A Leaflet position. Latitude is Z and longitude is X. */
interface LatLng {
    lat: number;
    lng: number;
}

/** A Leaflet layer group, marker, or renderer. */
interface LeafletLayer {
    addTo(map: unknown): this;

    remove(): this;
}

/** A marker the page has drawn. */
interface WitchlightMark extends LeafletLayer {
    bindTooltip(text: string): this;

    bindPopup(content: HTMLElement | string): this;
}

/**
 * A layer of one plugin's own, in a pane of its own.
 *
 * `show` puts both the group and its renderer on the map. `hide` takes both
 * off. A shape drawn into a pane with no renderer is added without complaint
 * and never appears, which is why the renderer is here rather than left to the
 * plugin.
 */
interface WitchlightLayer {
    /** The pane's name, as `plugin-{id}-{pane}`. */
    pane: string;
    /** The group to add marks and shapes to. */
    group: LeafletLayer;
    /** The SVG surface shapes in this pane are drawn on. */
    renderer: LeafletLayer;

    show(): void;

    hide(): void;
}

/** A window of one plugin's own. */
interface WitchlightPanel {
    /** The window itself, already draggable and stacked. */
    panel: HTMLElement;
    /** The body to fill. */
    body: HTMLElement;
    /** Whether the window is open now. */
    readonly isOpen: boolean;

    /** Opens the window. `middle` centres it. */
    open(middle?: boolean): void;

    shut(): void;
}

/** A button in the column down the left edge. */
interface WitchlightButton {
    /** The bar the button sits in. */
    bar: HTMLElement;
    /** The anchor the click lands on. */
    anchor: HTMLElement;
}

/** One line in a popup. Every part is optional. */
interface WitchlightSaidRow {
    /** A colour for the swatch. A row without one keeps the swatch's space. */
    mark?: string;
    /** What the row says. */
    said?: string;
    /** A word for how much. */
    band?: string;
    /** A number. */
    note?: string | number;
}

/** What a popup says. */
interface WitchlightSaid {
    /** The heading. */
    name?: string;
    /**
     * Where it is. An `[x, z]` pair is written the way this reader has asked to
     * see positions. A string is written as given.
     */
    where?: [number, number] | string;
    /** Whose it is. */
    who?: string;
    /** The body. */
    rows?: WitchlightSaidRow[];
    /** A line under the heading. */
    note?: string;
}

/** Who is looking, as `/me` last answered. */
interface WitchlightViewer {
    uid: string;
    name: string;
    /** The groups this reader is in. */
    groups: number[];
}

/** A player on the live feed, as the mod described them. */
interface WitchlightPlayer {
    [field: string]: unknown;
}

/** A marker on the live feed. */
interface WitchlightMarker {
    [field: string]: unknown;
}

/** A land claim on the live feed. */
interface WitchlightClaim {
    [field: string]: unknown;
}

/**
 * What the live feed last said.
 *
 * Built once per beat and handed to every plugin. Frozen shallowly. The arrays
 * inside are the page's own and must not be written to.
 */
interface WitchlightLive {
    /** Every player this reader may see. */
    players: WitchlightPlayer[];
    /** How many are on. This is not the same as how many are listed. */
    online: number;
    /** Which of them share a group with whoever is looking. */
    grouped: WitchlightPlayer[];
    /** Every marker this reader may see. */
    markers: WitchlightMarker[];
    /** Every land claim this reader may see. */
    claims: WitchlightClaim[];
    /** The world's own clock, as the game last said it. */
    world: unknown | null;
    /** How tall the world is, in blocks. */
    height: number;
    /** How long the service asks the page to leave between beats, in ms. */
    every: number;
}

/** The world's edges, in blocks. */
interface WitchlightBounds {
    [edge: string]: number;
}

/** What a terrain change says. */
interface WitchlightTerrain {
    /** Rises whenever the ground is re-exported. */
    generation: number;
    /** The world's edges, in blocks. */
    bounds: WitchlightBounds;
}

/** One row, as `fetch` answers with it. */
interface WitchlightRow {
    /** The uid of whoever the row belongs to. Empty for a world-scoped plugin. */
    Owner: string;
    /** What that owner is called. Empty where the service has never been told. */
    OwnerName: string;

    /** Every column the plugin declared, under the name it declared. */
    [column: string]: string | number | boolean | null;
}

/** What one plugin may do to the page. */
interface WitchlightHandle {
    /** This plugin's own name, which is the id it registered with. */
    id: string;

    /**
     * Where a file this plugin shipped is served from.
     *
     * Takes a path under the plugin's own `assets/`, as
     * `asset('icons/mountains.svg')`. A plugin never writes its own id into a
     * path.
     */
    asset(path: string): string;

    /** A world position as Leaflet wants it. Latitude is Z. */
    at(x: number, z: number): LatLng;

    /**
     * A world position as the reader sees it.
     *
     * Positions are shown relative to spawn unless the reader asked for
     * absolute. Use this when writing a position into a label.
     */
    said(x: number, z: number): [number, number];

    /**
     * A position the reader typed, in the numbers the world uses.
     *
     * The inverse of `said`. Use this when reading a position a reader typed.
     */
    meant(x: number, z: number): [number, number];

    /** Who is looking. Null for a stranger. */
    me(): WitchlightViewer | null;

    /**
     * A layer of this plugin's own, in a pane of its own.
     *
     * `interactive: false` turns pointer events off for the whole pane, which
     * is what an overlay drawn over the ground wants. Asking for one pane name
     * twice throws.
     */
    layer(opts?: { pane?: string; z?: number; interactive?: boolean }): WitchlightLayer;

    /**
     * A mark on the map, in the shape the page's own marks take.
     *
     * `hover` follows the reader's own hover setting. Anything Leaflet can draw
     * is still available on the layer this is added to.
     */
    mark(opts: {
        x: number;
        z: number;
        icon?: string;
        label?: string;
        popup?: HTMLElement;
        pane?: string;
        hover?: boolean;
    }): WitchlightMark;

    /**
     * What a mark says when it is opened, in the shape the map's own popups
     * take. Answers an element, which is what `mark`'s own `popup` takes.
     */
    popup(said?: WitchlightSaid): HTMLElement;

    /**
     * A window of this plugin's own, built rather than found.
     *
     * `wide` and `high` are sizes in pixels. `onShut` is called when the window
     * closes by any route.
     */
    panel(opts?: {
        title?: string;
        wide?: number;
        high?: number;
        onShut?: () => void;
    }): WitchlightPanel;

    /**
     * A button down the left edge.
     *
     * `mark` is either one of the marks compiled into the service or a file
     * this plugin shipped. Anything with a slash in it is read as the plugin's
     * own.
     */
    button(opts: { mark?: string; label?: string; onPress?: () => void }): WitchlightButton;

    /**
     * This plugin's own rows, as this reader may see them.
     *
     * `ranges` bounds a column, as `{ x: [-1000, 1000] }`. Every column named
     * must have been declared ranged. What comes back is what the service
     * decided this reader may have. Rejects when the service refuses the ask.
     */
    fetch(ranges?: Record<string, [number, number]>): Promise<WitchlightRow[]>;

    /** This plugin's own styling, put on the page. */
    style(css: string): HTMLStyleElement;

    /** Which groups this reader shares this plugin's rows with. */
    sharedWith(): Promise<number[]>;

    /** Replaces that set. The whole of it, since it is one answer. */
    shareWith(groups: number[]): Promise<boolean>;

    /**
     * One row of this reader's own, taken away.
     *
     * The key is the values of the plugin's key columns, in the order it
     * declared them.
     */
    forget(key: string | Array<string | number>): Promise<boolean>;

    /**
     * Arms or puts down a click-mode of this plugin's own.
     *
     * Only one tool on the map may have the next click. Arming one puts the
     * others down. `drop` is called when something else takes the click.
     */
    tool(on: boolean, drop?: () => void): void;

    /** What block is under the pointer. Null where none. */
    pointing(): { x: number; z: number } | null;

    /**
     * Something this reader has set, kept for them and for this plugin alone.
     *
     * `kept(name)` reads. `kept(name, value)` writes. Values are JSON.
     */
    kept(name: string, value?: unknown): unknown;

    /**
     * This plugin's own state in the page's address.
     *
     * `linked(name)` reads. `linked(name, value)` writes. Writing nothing takes
     * it out again. A change from outside reaches the plugin as `onLink`.
     */
    linked(name: string, value?: string): string | undefined;

    /**
     * Says something to the reader, in the corner the map says things in.
     *
     * Called with nothing, it takes the message away again. `wrong` draws it in
     * the colour every window uses for something that did not work.
     */
    say(what?: string | null, wrong?: boolean): void;

    /**
     * A switch of this plugin's own, in the reader's display panel.
     *
     * `panel: 'access'` puts it in the accessibility window instead. What the
     * reader sets is remembered in this browser. Answers the switch's full
     * name. Asking for one name twice throws.
     */
    setting(
        name: string,
        opts?: {
            label?: string;
            on?: boolean;
            panel?: 'access';
            apply?: (on: boolean) => void;
        },
    ): string;

    /** Puts one of this plugin's own switches where the plugin says it is. */
    setSetting(name: string, on: boolean): void;

    /** What this reader has set, of the map's own switches. Read-only. */
    reads(name: string): boolean;

    /**
     * A key of this plugin's own, in the map's own table of them.
     *
     * `key` is a default and not a claim. A key the map already answers to is
     * left to the map, and the action arrives with no key. `offered` says
     * whether the key does anything right now. Answers the key's full name.
     * Asking for one name twice throws.
     */
    hotkey(
        name: string,
        opts?: {
            label?: string;
            key?: string;
            offered?: () => boolean;
            act?: () => void;
        },
    ): string;

    /**
     * Runs something on a beat, waiting for each answer before counting the
     * next. `ms` is the gap between one answer and the next question.
     */
    beat(fn: () => unknown, ms: number, what?: string): void;

    /** Watches something already running, and reports it if it fails. */
    started(promise: Promise<unknown>, what?: string): Promise<unknown>;
}

/**
 * What a plugin declares.
 *
 * Every hook is optional. A hook that throws is reported to the console and
 * leaves the map as it was.
 */
interface WitchlightPlugin {
    /** Called once, when the plugin registers. */
    start?(wl: WitchlightHandle): void;

    /** Called on every live beat, with what arrived. */
    onChange?(live: WitchlightLive, wl: WitchlightHandle): void;

    /** Called when the ground is re-exported. A far slower clock than the beat. */
    onTerrain?(terrain: WitchlightTerrain, wl: WitchlightHandle): void;

    /** Called when the address changed under the page. */
    onLink?(wl: WitchlightHandle): void;
}

interface Window {
    witchlight: {
        plugins: {
            /**
             * Registers a plugin.
             *
             * `id` must be the id the plugin registered with on the mod side,
             * which is its modid. Registering one id twice throws.
             */
            register(id: string, plugin: WitchlightPlugin): WitchlightHandle;
        };
    };
}

declare const window: Window;

/** Leaflet, as the page loaded it. */
declare const L: any;
