import type { RpcClient } from '../rpc';
import type { Takeover, ThemeName, ThemeReach, ThemeScope, ThemesEvent } from './types';
import type { TicketRef } from '../notes/types';
/** Typed client interface for themes plugin */
export interface ThemesClient {
    /**
     * Define (or redefine) a theme: its token overrides for light and dark; yields the theme
     *
     * THEME-3: the authoring path is a ticket's ` ```theme name
     * [extends=parent] ` block (`tickets.create` / `amend` / `update`
     * register it, owned by that ticket); `define` stays for admin and
     * tests — an ownerless theme, refused for a name a block owns.
     *
     * Defines the theme `name` — its token overrides for `light` and for
     * `dark`, each a map from a token (`--accent`, or `accent`) to its
     * value, replacing whatever it overrode before (THEME-8: a theme is a
     * name and a map over today's tokens; `default` overrides nothing and
     * cannot be defined). A token not in the contract, or a value that
     * could escape its declaration, is refused, naming each; nothing is
     * kept. Yields exactly one event: `theme`, as kept — or an in-band
     * `error`. `watch` carries it too. Under login: an editor on `*`.
     * @requires `themes.define` anywhere it is held
     */
    define(name: ThemeName, dark?: Record<string, unknown> | null, light?: Record<string, unknown> | null): AsyncGenerator<ThemesEvent>;
    /**
     * Get one theme by name
     *
     * One theme, as it is now: `theme`, or an in-band `error` for a name
     * that is none (naming the ones there are).
     * @requires `themes.get` anywhere it is held
     */
    get(name: ThemeName): AsyncGenerator<ThemesEvent>;
    /**
     * Every theme, then what the site and each shortcode say
     *
     * Every theme, `default` first then by name — one `theme` event each —
     * then what the site and every shortcode say: one `setting` each, the
     * site first. From the cache.
     * @requires `themes.list` anywhere it is held
     */
    list(): AsyncGenerator<ThemesEvent>;
    /**
     * Get plugin or method schema. Pass {"method": "name"} for a specific method.
     * @requires `themes.schema` anywhere it is held
     */
    schema(): Promise<unknown>;
    /**
     * Set what the site or a shortcode says: the theme it adopts, its reach and takeover; yields the setting
     *
     * What `scope` says — `site` (everyone's site theme; admin on `*`) or
     * a shortcode (an editor there) — replacing what it said: the `theme`
     * it adopts (none or `default`: none), its `reach` (`frame` — only the
     * frames of its tickets; `ticket`, the default — its tickets' views
     * and frames; `site` — and the site while one of its tickets is open),
     * and `takeover` (`off`: its tickets ignore the site's theme). The
     * site takes neither a reach nor `takeover: off`. Saying nothing
     * clears it. Yields exactly one event: `setting`, as kept — or an
     * in-band `error` (an undefined theme, a shortcode that is none).
     * `watch` carries it too.
     * @requires `themes.set` anywhere it is held
     */
    set(scope: ThemeScope, reach?: ThemeReach | null, takeover?: Takeover | null, theme?: ThemeName | null): AsyncGenerator<ThemesEvent>;
    /**
     * Who wears what for a ticket: each region's theme (site, ticket view, frame) and the level it came from
     *
     * Who wears what for the ticket `id`: one `regions` event — the site,
     * the ticket view and the frame, each its theme and the level it came
     * from (`none`, `site`, `shortcode` with `from`, `ticket`, `frame`) —
     * with what every level said (`levels`: the site's theme, the nearest
     * shortcode's setting, the ticket's frontmatter and its frame's). The
     * reader's own choices are the app's, outside this. An in-band `error`
     * for a ticket that is none.
     * @requires `themes.theme_for` on the scope of `id`
     */
    themeFor(id: TicketRef): AsyncGenerator<ThemesEvent>;
    /**
     * Subscribe to themes: theme_changed (defined, a ```theme block written, orphaned or adopted), theme_removed (an unused orphan gone) and setting (site or shortcode), until disconnect; lagged { missed } where a slow subscriber lost changes
     *
     * Subscribes to the themes: `theme` when one is defined and `setting`
     * when the site or a shortcode says something new, in the order the
     * writes were acknowledged, until the client goes; `lagged { missed }`
     * where a slow subscriber lost changes (`list` again). A ticket's
     * frontmatter moving is `tickets.watch`'s `amended`.
     * @requires `themes.watch` anywhere it is held
     */
    watch(): AsyncGenerator<ThemesEvent>;
}
/** Create a typed themes client from an RPC client */
export declare function createThemesClient(rpc: RpcClient): ThemesClient;
/** What each themes method needs (AUTH-9): its grant, and the param that locates what it acts on (`null`: allowed wherever the grant is held). The gate decides, server-side. */
export declare const requires: {
    readonly "themes.define": {
        readonly grant: "themes.define";
        readonly target: null;
    };
    readonly "themes.get": {
        readonly grant: "themes.get";
        readonly target: null;
    };
    readonly "themes.list": {
        readonly grant: "themes.list";
        readonly target: null;
    };
    readonly "themes.schema": {
        readonly grant: "themes.schema";
        readonly target: null;
    };
    readonly "themes.set": {
        readonly grant: "themes.set";
        readonly target: null;
    };
    readonly "themes.theme_for": {
        readonly grant: "themes.theme_for";
        readonly target: "id";
    };
    readonly "themes.watch": {
        readonly grant: "themes.watch";
        readonly target: null;
    };
};
