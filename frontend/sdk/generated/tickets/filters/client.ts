// Auto-generated typed client (Layer 2)
// Wraps RPC layer and unwraps PlexusStreamItem to domain types

import type { RpcClient } from '../../rpc';
import { extractData, collectOne } from '../../rpc';
import type { FilterName, FilterSource } from './types';
import type { TicketsEvent } from '../../tickets/types';

/** Typed client interface for tickets.filters plugin */
export interface TicketsFiltersClient {
  /**
   * Read a filter without saving it: yields filter_checked with the filter as canonical text and how many tickets in reach it admits — or an in-band error naming the chunk and where it stands, a name that does not resolve, a cycle. Writes nothing.
   * @server-derived auth
   * @requiresAuth — resolver: self.access.verify
   * @requires `filters.check` anywhere it is held
   */
  check(filter: FilterSource): AsyncGenerator<TicketsEvent>;
  /**
   * Delete a saved filter by name; an in-band error when there is none. Yields filter_deleted. A filter that names it stops resolving.
   * @server-derived auth
   * @requiresAuth — resolver: self.access.verify
   * @requires `filters.delete` anywhere it is held
   */
  delete(name: FilterName): AsyncGenerator<TicketsEvent>;
  /**
   * One saved filter, by name (matched by its slug: `Open Builds` is `open-builds`), with its count; an in-band error when there is none.
   * @server-derived auth
   * @requiresAuth — resolver: self.access.verify
   * @requires `filters.get` anywhere it is held
   */
  get(name: FilterName): AsyncGenerator<TicketsEvent>;
  /**
   * Every saved filter of the caller's organization, by name: its filter, how many tickets in reach it admits, who saved it and when (`Needs you` among them until one is saved under that name).
   * @requires `filters.list` anywhere it is held
   */
  list(): AsyncGenerator<TicketsEvent>;
  /**
   * Give a saved filter another name, keeping its filter and who saved it; refused in band when the new name is another filter's. Yields filter_saved with `renamed_from`. A filter that names the old name stops resolving.
   * @server-derived auth
   * @requiresAuth — resolver: self.access.verify
   * @requires `filters.rename` anywhere it is held
   */
  rename(name: FilterName, to: FilterName): AsyncGenerator<TicketsEvent>;
  /**
   * Save a filter under a name in the caller's organization — new, or replacing the one of that name (who first saved it is kept). Refused in band, nothing written: a filter the language cannot read (the chunk and where it stands are named), one holding `text`, a `saved "…"` that names none or makes a cycle. Yields filter_saved with the row.
   * @server-derived auth
   * @requiresAuth — resolver: self.access.verify
   * @requires `filters.save` anywhere it is held
   */
  save(filter: FilterSource, name: FilterName): AsyncGenerator<TicketsEvent>;
  /**
   * Get plugin or method schema. Pass {"method": "name"} for a specific method.
   * @requires `filters.schema` anywhere it is held
   */
  schema(): Promise<unknown>;
}

/** Typed client implementation for tickets.filters plugin */
class TicketsFiltersClientImpl implements TicketsFiltersClient {
  private rpc: RpcClient;
  constructor(rpc: RpcClient) { this.rpc = rpc; }

  async *check(filter: FilterSource): AsyncGenerator<TicketsEvent> {
    const stream = this.rpc.call('tickets.filters.check', { filter });
    yield* extractData<TicketsEvent>(stream);
  }

  async *delete(name: FilterName): AsyncGenerator<TicketsEvent> {
    const stream = this.rpc.call('tickets.filters.delete', { name });
    yield* extractData<TicketsEvent>(stream);
  }

  async *get(name: FilterName): AsyncGenerator<TicketsEvent> {
    const stream = this.rpc.call('tickets.filters.get', { name });
    yield* extractData<TicketsEvent>(stream);
  }

  async *list(): AsyncGenerator<TicketsEvent> {
    const stream = this.rpc.call('tickets.filters.list', {});
    yield* extractData<TicketsEvent>(stream);
  }

  async *rename(name: FilterName, to: FilterName): AsyncGenerator<TicketsEvent> {
    const stream = this.rpc.call('tickets.filters.rename', { name, to });
    yield* extractData<TicketsEvent>(stream);
  }

  async *save(filter: FilterSource, name: FilterName): AsyncGenerator<TicketsEvent> {
    const stream = this.rpc.call('tickets.filters.save', { filter, name });
    yield* extractData<TicketsEvent>(stream);
  }

  async schema(): Promise<unknown> {
    const stream = this.rpc.call('tickets.filters.schema', {});
    return collectOne<unknown>(stream);
  }
}

/** Create a typed tickets.filters client from an RPC client */
export function createTicketsFiltersClient(rpc: RpcClient): TicketsFiltersClient {
  return new TicketsFiltersClientImpl(rpc);
}

/** What each tickets.filters method needs (AUTH-9): its grant, and the param that locates what it acts on (`null`: allowed wherever the grant is held). The gate decides, server-side. */
export const requires = {
  "filters.check": { grant: "filters.check", target: null },
  "filters.delete": { grant: "filters.delete", target: null },
  "filters.get": { grant: "filters.get", target: null },
  "filters.list": { grant: "filters.list", target: null },
  "filters.rename": { grant: "filters.rename", target: null },
  "filters.save": { grant: "filters.save", target: null },
  "filters.schema": { grant: "filters.schema", target: null },
} as const;

/** The tickets.filters activation's default roles, each the grants it lists (AUTH-3). Defaults only: a server's roles are data an admin may change, and `admin` holds every grant. */
export const defaultRoles = {
  "editor": ["filters.list", "filters.get", "filters.check", "filters.save", "filters.rename", "filters.delete"],
  "viewer": ["filters.list", "filters.get", "filters.check"],
} as const;