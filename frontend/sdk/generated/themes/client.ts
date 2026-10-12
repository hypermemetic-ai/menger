// Auto-generated typed client (Layer 2)
// Wraps RPC layer and unwraps PlexusStreamItem to domain types

import type { RpcClient } from '../rpc';
import { extractData, collectOne } from '../rpc';
import type { ThemesEvent } from './types';

/** Typed client interface for themes plugin */
export interface ThemesClient {
  /** Define (or redefine) a theme: its token overrides for light and dark; yields the theme */
  define(name: string, dark?: Record<string, unknown> | null, light?: Record<string, unknown> | null): AsyncGenerator<ThemesEvent>;
  /** Get one theme by name */
  get(name: string): AsyncGenerator<ThemesEvent>;
  /** Every theme, then what the site and each shortcode say */
  list(): AsyncGenerator<ThemesEvent>;
  /** Get plugin or method schema. Pass {"method": "name"} for a specific method. */
  schema(): Promise<unknown>;
  /** Set what the site or a shortcode says: the theme it adopts, its reach and takeover; yields the setting */
  set(scope: string, reach?: string | null, takeover?: string | null, theme?: string | null): AsyncGenerator<ThemesEvent>;
  /** Who wears what for a ticket: each region's theme (site, ticket view, frame) and the level it came from */
  themeFor(id: string): AsyncGenerator<ThemesEvent>;
  /** Subscribe to themes: theme (defined) and setting (site or shortcode), until disconnect; lagged { missed } where a slow subscriber lost changes */
  watch(): AsyncGenerator<ThemesEvent>;
}

/** Typed client implementation for themes plugin */
class ThemesClientImpl implements ThemesClient {
  private rpc: RpcClient;
  constructor(rpc: RpcClient) { this.rpc = rpc; }

  async *define(name: string, dark?: Record<string, unknown> | null, light?: Record<string, unknown> | null): AsyncGenerator<ThemesEvent> {
    const stream = this.rpc.call('themes.define', { dark, light, name });
    yield* extractData<ThemesEvent>(stream);
  }

  async *get(name: string): AsyncGenerator<ThemesEvent> {
    const stream = this.rpc.call('themes.get', { name });
    yield* extractData<ThemesEvent>(stream);
  }

  async *list(): AsyncGenerator<ThemesEvent> {
    const stream = this.rpc.call('themes.list', {});
    yield* extractData<ThemesEvent>(stream);
  }

  async schema(): Promise<unknown> {
    const stream = this.rpc.call('themes.schema', {});
    return collectOne<unknown>(stream);
  }

  async *set(scope: string, reach?: string | null, takeover?: string | null, theme?: string | null): AsyncGenerator<ThemesEvent> {
    const stream = this.rpc.call('themes.set', { reach, scope, takeover, theme });
    yield* extractData<ThemesEvent>(stream);
  }

  async *themeFor(id: string): AsyncGenerator<ThemesEvent> {
    const stream = this.rpc.call('themes.theme_for', { id });
    yield* extractData<ThemesEvent>(stream);
  }

  async *watch(): AsyncGenerator<ThemesEvent> {
    const stream = this.rpc.call('themes.watch', {});
    yield* extractData<ThemesEvent>(stream);
  }
}

/** Create a typed themes client from an RPC client */
export function createThemesClient(rpc: RpcClient): ThemesClient {
  return new ThemesClientImpl(rpc);
}