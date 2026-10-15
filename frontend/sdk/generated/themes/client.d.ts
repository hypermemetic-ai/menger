import type { RpcClient } from '../rpc';
import type { Takeover, ThemeName, ThemeReach, ThemeScope, ThemesEvent } from './types';
import type { TicketRef } from '../notes/types';
/** Typed client interface for themes plugin */
export interface ThemesClient {
    /** Define (or redefine) a theme: its token overrides for light and dark; yields the theme */
    define(name: ThemeName, dark?: Record<string, unknown> | null, light?: Record<string, unknown> | null): AsyncGenerator<ThemesEvent>;
    /** Get one theme by name */
    get(name: ThemeName): AsyncGenerator<ThemesEvent>;
    /** Every theme, then what the site and each shortcode say */
    list(): AsyncGenerator<ThemesEvent>;
    /** Get plugin or method schema. Pass {"method": "name"} for a specific method. */
    schema(): Promise<unknown>;
    /** Set what the site or a shortcode says: the theme it adopts, its reach and takeover; yields the setting */
    set(scope: ThemeScope, reach?: ThemeReach | null, takeover?: Takeover | null, theme?: ThemeName | null): AsyncGenerator<ThemesEvent>;
    /** Who wears what for a ticket: each region's theme (site, ticket view, frame) and the level it came from */
    themeFor(id: TicketRef): AsyncGenerator<ThemesEvent>;
    /** Subscribe to themes: theme_changed (defined, a ```theme block written, orphaned or adopted), theme_removed (an unused orphan gone) and setting (site or shortcode), until disconnect; lagged { missed } where a slow subscriber lost changes */
    watch(): AsyncGenerator<ThemesEvent>;
}
/** Create a typed themes client from an RPC client */
export declare function createThemesClient(rpc: RpcClient): ThemesClient;
