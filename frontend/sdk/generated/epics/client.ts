// Auto-generated typed client (Layer 2)
// Wraps RPC layer and unwraps PlexusStreamItem to domain types

import type { RpcClient } from '../rpc';
import { extractData, collectOne } from '../rpc';
import type { EpicName, EpicsEvent } from './types';
import type { OrgId } from '../orgs/types';

/** Typed client interface for epics plugin */
export interface EpicsClient {
  /**
   * Make an epic in the organization, empty (admin on it, or on *): its shortcodes start at 1. Move tickets in with tickets reshortcode --to org:epic:CODE; create in it with create --epic org:epic:CODE. Yields created { epic }.
   * @server-derived auth
   * @requiresAuth — resolver: self.access.verify
   * @requires `epics.create` anywhere it is held
   */
  create(name: EpicName, org?: OrgId | null): AsyncGenerator<EpicsEvent>;
  /**
   * The epic you are in, in the organization — your choice (switch), else its newest; yields current { epic }. A filtered read answers there unless its filter names epic (epic any: every one).
   * @server-derived auth
   * @requiresAuth — resolver: self.access.verify
   * @requires `epics.current` anywhere it is held
   */
  current(org?: OrgId | null): AsyncGenerator<EpicsEvent>;
  /**
   * Delete an empty epic (admin on its organization, or on *) — never the organization's first; whoever was in it is in the newest again. Yields deleted { org, name }.
   * @server-derived auth
   * @requiresAuth — resolver: self.access.verify
   * @requires `epics.delete` anywhere it is held
   */
  delete(name: EpicName, org?: OrgId | null): AsyncGenerator<EpicsEvent>;
  /**
   * One epic of the organization by name; `not_found` when it has none of that name.
   * @server-derived auth
   * @requiresAuth — resolver: self.access.verify
   * @requires `epics.get` anywhere it is held
   */
  get(name: EpicName, org?: OrgId | null): AsyncGenerator<EpicsEvent>;
  /**
   * The organization's epics, oldest first: one `epic { org, name, first, current, shortcodes, tickets, created_unix }` each.
   * @server-derived auth
   * @requiresAuth — resolver: self.access.verify
   * @requires `epics.list` anywhere it is held
   */
  list(org?: OrgId | null): AsyncGenerator<EpicsEvent>;
  /**
   * Rename an epic (admin on its organization, or on *): every address naming it moves to the new name. Yields renamed { from, epic }.
   * @server-derived auth
   * @requiresAuth — resolver: self.access.verify
   * @requires `epics.rename` anywhere it is held
   */
  rename(name: EpicName, to: EpicName, org?: OrgId | null): AsyncGenerator<EpicsEvent>;
  /**
   * Get plugin or method schema. Pass {"method": "name"} for a specific method.
   * @requires `epics.schema` anywhere it is held
   */
  schema(): Promise<unknown>;
  /**
   * Be in another epic of the organization (your own setting): short forms resolve there first, and filtered reads answer there. Yields current { epic }.
   * @server-derived auth
   * @requiresAuth — resolver: self.access.verify
   * @requires `epics.switch` anywhere it is held
   */
  switch(name: EpicName, org?: OrgId | null): AsyncGenerator<EpicsEvent>;
}

/** Typed client implementation for epics plugin */
class EpicsClientImpl implements EpicsClient {
  private rpc: RpcClient;
  constructor(rpc: RpcClient) { this.rpc = rpc; }

  async *create(name: EpicName, org?: OrgId | null): AsyncGenerator<EpicsEvent> {
    const stream = this.rpc.call('epics.create', { name, org });
    yield* extractData<EpicsEvent>(stream);
  }

  async *current(org?: OrgId | null): AsyncGenerator<EpicsEvent> {
    const stream = this.rpc.call('epics.current', { org });
    yield* extractData<EpicsEvent>(stream);
  }

  async *delete(name: EpicName, org?: OrgId | null): AsyncGenerator<EpicsEvent> {
    const stream = this.rpc.call('epics.delete', { name, org });
    yield* extractData<EpicsEvent>(stream);
  }

  async *get(name: EpicName, org?: OrgId | null): AsyncGenerator<EpicsEvent> {
    const stream = this.rpc.call('epics.get', { name, org });
    yield* extractData<EpicsEvent>(stream);
  }

  async *list(org?: OrgId | null): AsyncGenerator<EpicsEvent> {
    const stream = this.rpc.call('epics.list', { org });
    yield* extractData<EpicsEvent>(stream);
  }

  async *rename(name: EpicName, to: EpicName, org?: OrgId | null): AsyncGenerator<EpicsEvent> {
    const stream = this.rpc.call('epics.rename', { name, org, to });
    yield* extractData<EpicsEvent>(stream);
  }

  async schema(): Promise<unknown> {
    const stream = this.rpc.call('epics.schema', {});
    return collectOne<unknown>(stream);
  }

  async *switch(name: EpicName, org?: OrgId | null): AsyncGenerator<EpicsEvent> {
    const stream = this.rpc.call('epics.switch', { name, org });
    yield* extractData<EpicsEvent>(stream);
  }
}

/** Create a typed epics client from an RPC client */
export function createEpicsClient(rpc: RpcClient): EpicsClient {
  return new EpicsClientImpl(rpc);
}

/** What each epics method needs (AUTH-9): its grant, and the param that locates what it acts on (`null`: allowed wherever the grant is held). The gate decides, server-side. */
export const requires = {
  "epics.create": { grant: "epics.create", target: null },
  "epics.current": { grant: "epics.current", target: null },
  "epics.delete": { grant: "epics.delete", target: null },
  "epics.get": { grant: "epics.get", target: null },
  "epics.list": { grant: "epics.list", target: null },
  "epics.rename": { grant: "epics.rename", target: null },
  "epics.schema": { grant: "epics.schema", target: null },
  "epics.switch": { grant: "epics.switch", target: null },
} as const;