import type { RpcClient } from '../../rpc';
import { MethodStream } from '../../rpc';
import type { SchemaResult } from './types';
import type { Body, Kind, TicketRef, Title } from '../../notes/types';
import type { Cursor, TicketsEvent } from '../../tickets/types';
/** Typed client interface for tickets.subtasks plugin */
export interface TicketsSubtasksClient {
    /**
     * Create a subtask of a ticket: the next free `<parent>.N` (from .1; a number is never reused), hidden from the nav's views; yields created { ticket }.
  
  TICKETS-28: one write, `tickets.create`'s rules and permits (editor
  on what is below the parent) with the naming done by the store: the
  id is the parent's with `.N` — the number past the highest the parent
  holds and past every one a delete or a rename took out — and
  `hidden` is true (TICKETS-27), so it is in no list, tree or count
  but its parent's children (`subtasks list`, `list --parent`). It is
  in the parent's shortcode, born as `create` births (not a scope
  or a doc: they root a shortcode). A parent that names no ticket is an
  in-band error and nothing is written. Yields exactly one event:
  `Created` or `Error`.
     * @server-derived auth
     * @requiresAuth — resolver: self.access.verify
     * @requires `subtasks.create` anywhere it is held
     */
    create(parent: TicketRef, title: Title, blockedBy?: TicketRef[] | null, body?: Body | null, kind?: Kind | null): MethodStream<'tickets.subtasks.create', TicketsEvent>;
    /**
     * A ticket's subtasks in `.N` order, as summaries (hidden ones included), then one page event.
  
  TICKETS-28: the children `tickets.subtasks.create` made — and any
  other child of the parent, as `list --parent` reads them — through the
  caller's reach. `after` and `limit` page as `list`'s do.
     * @server-derived auth
     * @requiresAuth — resolver: self.access.verify
     * @requires `subtasks.list` anywhere it is held
     */
    list(parent: TicketRef, after?: Cursor | null, limit?: number | null): MethodStream<'tickets.subtasks.list', TicketsEvent>;
    /**
     * Get plugin or method schema. Pass {"method": "name"} for a specific method.
     * @requires `subtasks.schema` anywhere it is held
     */
    schema(method?: string | null): Promise<SchemaResult>;
}
/** Create a typed tickets.subtasks client from an RPC client */
export declare function createTicketsSubtasksClient(rpc: RpcClient): TicketsSubtasksClient;
/** What each tickets.subtasks method needs (AUTH-9): its grant, and the param that locates what it acts on (`null`: allowed wherever the grant is held). The gate decides, server-side. */
export declare const requires: {
    readonly "subtasks.create": {
        readonly grant: "subtasks.create";
        readonly target: null;
    };
    readonly "subtasks.list": {
        readonly grant: "subtasks.list";
        readonly target: null;
    };
    readonly "subtasks.schema": {
        readonly grant: "subtasks.schema";
        readonly target: null;
    };
};
/** The tickets.subtasks activation's default roles, each the grants it lists (AUTH-3). Defaults only: a server's roles are data an admin may change, and `admin` holds every grant. */
export declare const defaultRoles: {
    readonly editor: readonly ["subtasks.list", "subtasks.create"];
    readonly viewer: readonly ["subtasks.list"];
};
