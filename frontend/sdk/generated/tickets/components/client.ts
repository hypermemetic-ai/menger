// Auto-generated typed client (Layer 2)
// Wraps RPC layer and unwraps PlexusStreamItem to domain types

import type { RpcClient } from '../../rpc';
import { MethodStream, collectOne } from '../../rpc';
import type { SchemaResult } from './types';
import type { TicketsEvent } from '../../tickets/types';

/** Typed client interface for tickets.components plugin */
export interface TicketsComponentsClient {
  /**
   * Every component in the registry, in name order: its source, its owner (null: an orphan), what references it, its docs (summary, typed attributes, slots, examples, help; null without <docs>) and its preview attributes.

COMP-3: the component registry. Yields one `Component` per component
a ticket defines or once defined, in name order — its `source` (the
` ```component <name> ` block's content), its `owner` (`null` for an
orphan: no ticket defines it any more, and something still uses it)
and its `references` (the tickets whose bodies hold `<name` as an
HTML tag, in the store's order, then the components whose sources
do). COMP-8: and its help — `docs`, the source's `<docs>` parsed
(summary, typed attributes, slots, examples, markdown help; `null`
without one) — and `preview`, the attributes on the block's opening
line. Nothing trails them — the stream's own `done` is the whole
answer, as for `nav groups`. From the cache alone; `tickets.watch`
keeps a client's copy current with `ComponentChanged` and
`ComponentRemoved`. IDENTITY-37: a caller who cannot view every
shortcode gets the components whose owner it can view — no orphan —
each with only the references it can view.
   * @requires `components.list` anywhere it is held
   */
  list(): MethodStream<'tickets.components.list', TicketsEvent>;
  /**
   * Get plugin or method schema. Pass {"method": "name"} for a specific method.
   * @requires `components.schema` anywhere it is held
   */
  schema(method?: string | null): Promise<SchemaResult>;
}

/** Typed client implementation for tickets.components plugin */
class TicketsComponentsClientImpl implements TicketsComponentsClient {
  private rpc: RpcClient;
  constructor(rpc: RpcClient) { this.rpc = rpc; }

  list(): MethodStream<'tickets.components.list', TicketsEvent> {
    const stream = this.rpc.call('tickets.components.list', {});
    return new MethodStream<'tickets.components.list', TicketsEvent>('tickets.components.list', stream);
  }

  async schema(method?: string | null): Promise<SchemaResult> {
    const stream = this.rpc.call('tickets.components.schema', { method });
    return collectOne<SchemaResult>(stream);
  }
}

/** Create a typed tickets.components client from an RPC client */
export function createTicketsComponentsClient(rpc: RpcClient): TicketsComponentsClient {
  return new TicketsComponentsClientImpl(rpc);
}

/** What each tickets.components method needs (AUTH-9): its grant, and the param that locates what it acts on (`null`: allowed wherever the grant is held). The gate decides, server-side. */
export const requires = {
  "components.list": { grant: "components.list", target: null },
  "components.schema": { grant: "components.schema", target: null },
} as const;

/** The tickets.components activation's default roles, each the grants it lists (AUTH-3). Defaults only: a server's roles are data an admin may change, and `admin` holds every grant. */
export const defaultRoles = {
  "editor": ["components.list"],
  "viewer": ["components.list"],
} as const;