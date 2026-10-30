// Auto-generated typed client (Layer 2)
// Wraps RPC layer and unwraps PlexusStreamItem to domain types

import type { RpcClient } from '../rpc';
import { MethodStream, collectOne } from '../rpc';
import type { OrgId, OrgsEvent, Principal, SchemaResult } from './types';

/** Typed client interface for orgs plugin */
export interface OrgsClient {
  /**
   * Make an organization (admin on *), naming its first admin
   *
   * Makes the organization `name`, empty — lower-case letters, digits
   * and `-`, unique app-wide (among every name an organization has or
   * had). `admin` names its first admin, who holds admin on `name:`
   * from this grant. Needs admin on `*`. Yields `created { org, admin }`.
   * @requires `orgs.create` anywhere it is held
   */
  create(name: OrgId, admin?: Principal | null): MethodStream<'orgs.create', OrgsEvent>;
  /**
   * Delete an empty organization, and every grant on it
   *
   * Deletes the organization `name` — refused while it holds a shortcode
   * (a ticket, or a deleted ticket kept for its undo), and for the first
   * organization. Every grant on it goes with it. Needs admin on it or
   * on `*`. Yields `deleted { name, grants }`.
   * @requires `orgs.delete` anywhere it is held
   */
  delete(name: OrgId): MethodStream<'orgs.delete', OrgsEvent>;
  /**
   * The organizations you hold a role in
   *
   * The organizations the caller holds a role in — every one for a role
   * on `*` — in the order they were made: one `org
   * { name, aliases, roots, shortcodes, tickets }` each.
   * @requires `orgs.list` anywhere it is held
   */
  list(): MethodStream<'orgs.list', OrgsEvent>;
  /**
   * Rename an organization; its old name keeps resolving
   *
   * Renames the organization `name` to `to`. The old name stays an
   * alias: every address and grant scope written with it keeps
   * resolving; every grant on it is rewritten to the new name. Needs
   * admin on it (`name:`) or on `*`. Yields `renamed { from, org }`.
   * @requires `orgs.rename` anywhere it is held
   */
  rename(name: OrgId, to: OrgId): MethodStream<'orgs.rename', OrgsEvent>;
  /**
   * Get plugin or method schema. Pass {"method": "name"} for a specific method.
   * @requires `orgs.schema` anywhere it is held
   */
  schema(method?: string | null): Promise<SchemaResult>;
}

/** Typed client implementation for orgs plugin */
class OrgsClientImpl implements OrgsClient {
  private rpc: RpcClient;
  constructor(rpc: RpcClient) { this.rpc = rpc; }

  create(name: OrgId, admin?: Principal | null): MethodStream<'orgs.create', OrgsEvent> {
    const stream = this.rpc.call('orgs.create', { admin, name });
    return new MethodStream<'orgs.create', OrgsEvent>('orgs.create', stream);
  }

  delete(name: OrgId): MethodStream<'orgs.delete', OrgsEvent> {
    const stream = this.rpc.call('orgs.delete', { name });
    return new MethodStream<'orgs.delete', OrgsEvent>('orgs.delete', stream);
  }

  list(): MethodStream<'orgs.list', OrgsEvent> {
    const stream = this.rpc.call('orgs.list', {});
    return new MethodStream<'orgs.list', OrgsEvent>('orgs.list', stream);
  }

  rename(name: OrgId, to: OrgId): MethodStream<'orgs.rename', OrgsEvent> {
    const stream = this.rpc.call('orgs.rename', { name, to });
    return new MethodStream<'orgs.rename', OrgsEvent>('orgs.rename', stream);
  }

  async schema(method?: string | null): Promise<SchemaResult> {
    const stream = this.rpc.call('orgs.schema', { method });
    return collectOne<SchemaResult>(stream);
  }
}

/** Create a typed orgs client from an RPC client */
export function createOrgsClient(rpc: RpcClient): OrgsClient {
  return new OrgsClientImpl(rpc);
}

/** What each orgs method needs (AUTH-9): its grant, and the param that locates what it acts on (`null`: allowed wherever the grant is held). The gate decides, server-side. */
export const requires = {
  "orgs.create": { grant: "orgs.create", target: null },
  "orgs.delete": { grant: "orgs.delete", target: null },
  "orgs.list": { grant: "orgs.list", target: null },
  "orgs.rename": { grant: "orgs.rename", target: null },
  "orgs.schema": { grant: "orgs.schema", target: null },
} as const;