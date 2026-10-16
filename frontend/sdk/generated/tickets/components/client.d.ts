import type { RpcClient } from '../../rpc';
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
    list(): AsyncGenerator<TicketsEvent>;
    /**
     * Get plugin or method schema. Pass {"method": "name"} for a specific method.
     * @requires `components.schema` anywhere it is held
     */
    schema(): Promise<unknown>;
}
/** Create a typed tickets.components client from an RPC client */
export declare function createTicketsComponentsClient(rpc: RpcClient): TicketsComponentsClient;
/** What each tickets.components method needs (AUTH-9): its grant, and the param that locates what it acts on (`null`: allowed wherever the grant is held). The gate decides, server-side. */
export declare const requires: {
    readonly "components.list": {
        readonly grant: "components.list";
        readonly target: null;
    };
    readonly "components.schema": {
        readonly grant: "components.schema";
        readonly target: null;
    };
};
/** The tickets.components activation's default roles, each the grants it lists (AUTH-3). Defaults only: a server's roles are data an admin may change, and `admin` holds every grant. */
export declare const defaultRoles: {
    readonly editor: readonly ["components.list"];
    readonly viewer: readonly ["components.list"];
};
