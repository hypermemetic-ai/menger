// Auto-generated typed client (Layer 2)
// Wraps RPC layer and unwraps PlexusStreamItem to domain types

import type { RpcClient } from '../../rpc';
import { extractData, collectOne } from '../../rpc';
import type { RevisionField } from './types';
import type { TicketRef } from '../../notes/types';
import type { CallerSource, Message, TicketsEvent } from '../../tickets/types';

/** Typed client interface for tickets.revisions plugin */
export interface TicketsRevisionsClient {
  /**
   * What changed between two revisions: field by field, the frontmatter key by key, the body as a line diff with words within a changed line; yields revision_diff { diff }.

TICKETS-7: `from` → `to` — `to` omitted, the latest; `from` omitted,
the revision before `to`. Each field that differs with its value in
each; the body's frontmatter key by key; the body's line diff in hunks
(three lines of context; a line changed in place carries its words),
of its source — or, `text: true`, of the text a reader sees (no
markup, no scripts or styles, no frontmatter). At most 4,000 lines of
hunks (`truncated` says so).
   * @server-derived auth
   * @requiresAuth — resolver: self.access.verify
   * @requires `revisions.diff` anywhere it is held
   */
  diff(id: TicketRef, from?: number | null, text?: boolean | null, to?: number | null): AsyncGenerator<TicketsEvent>;
  /**
   * The ticket exactly as it was at a revision; yields revision_at { revision, ticket }.

TICKETS-7: every field a revision keeps (title, body, status, kind,
severity, parent, blockers, labels, commits) as it was then; the rest
(its id, address, children, placement) as it is now. A body that uses
a component or a theme renders with today's definitions. `NotFound`
for a ticket out of reach; an `Error` for a revision it does not have.
   * @server-derived auth
   * @requiresAuth — resolver: self.access.verify
   * @requires `revisions.get` anywhere it is held
   */
  get(id: TicketRef, rev: number): AsyncGenerator<TicketsEvent>;
  /**
   * A ticket's revisions, newest first: rev, when, who, from where, why, what changed, the body's hash, size and +/- lines; then revisions_end { count, latest, next }.

TICKETS-7: yields one `Revision` per revision of the ticket, newest
first — at most `limit` (100; at most 500), below `before` when
given — then `RevisionsEnd { count, latest, next }` (`next`: the
`before` of the next page, `null` at the oldest). A ticket older than
revisions has none until it is first changed; that write keeps its
version before (`source: baseline`) and its own. Transitions stay in
`history`. `NotFound` for a ticket out of the caller's reach. From
storage, not the cache.
   * @server-derived auth
   * @requiresAuth — resolver: self.access.verify
   * @requires `revisions.list` anywhere it is held
   */
  list(id: TicketRef, before?: number | null, limit?: number | null): AsyncGenerator<TicketsEvent>;
  /**
   * Put a revision's fields back — all it keeps but its commits, or those named — as one update, itself a new revision; yields revision_restored { ticket, from_rev, changed, rev }.

TICKETS-7: writes the chosen fields of revision `rev` (default: title,
body, status, kind, severity, parent, blocked_by, labels — each that
differs from now) through `update`'s rules: a parent that no longer
exists, a blocker cycle, a duplicate input id, a component another
ticket defines are refused, naming why, and nothing is written. The
revision it keeps says what it restored, from which revision, and
`message`. Nothing differing writes nothing (`changed: []`, `rev:
null`). A deleted ticket is undeleted first; commits are never
restored; a parent is moved, never cleared.
   * @server-derived auth
   * @requiresAuth — resolver: self.access.verify
   * @requires `revisions.restore` on the scope of `id`
   */
  restore(id: TicketRef, rev: number, fields?: RevisionField[] | null, message?: Message | null, source?: CallerSource | null): AsyncGenerator<TicketsEvent>;
  /**
   * Get plugin or method schema. Pass {"method": "name"} for a specific method.
   * @requires `revisions.schema` anywhere it is held
   */
  schema(): Promise<unknown>;
}

/** Typed client implementation for tickets.revisions plugin */
class TicketsRevisionsClientImpl implements TicketsRevisionsClient {
  private rpc: RpcClient;
  constructor(rpc: RpcClient) { this.rpc = rpc; }

  async *diff(id: TicketRef, from?: number | null, text?: boolean | null, to?: number | null): AsyncGenerator<TicketsEvent> {
    const stream = this.rpc.call('tickets.revisions.diff', { from, id, text, to });
    yield* extractData<TicketsEvent>(stream);
  }

  async *get(id: TicketRef, rev: number): AsyncGenerator<TicketsEvent> {
    const stream = this.rpc.call('tickets.revisions.get', { id, rev });
    yield* extractData<TicketsEvent>(stream);
  }

  async *list(id: TicketRef, before?: number | null, limit?: number | null): AsyncGenerator<TicketsEvent> {
    const stream = this.rpc.call('tickets.revisions.list', { before, id, limit });
    yield* extractData<TicketsEvent>(stream);
  }

  async *restore(id: TicketRef, rev: number, fields?: RevisionField[] | null, message?: Message | null, source?: CallerSource | null): AsyncGenerator<TicketsEvent> {
    const stream = this.rpc.call('tickets.revisions.restore', { fields, id, message, rev, source });
    yield* extractData<TicketsEvent>(stream);
  }

  async schema(): Promise<unknown> {
    const stream = this.rpc.call('tickets.revisions.schema', {});
    return collectOne<unknown>(stream);
  }
}

/** Create a typed tickets.revisions client from an RPC client */
export function createTicketsRevisionsClient(rpc: RpcClient): TicketsRevisionsClient {
  return new TicketsRevisionsClientImpl(rpc);
}

/** What each tickets.revisions method needs (AUTH-9): its grant, and the param that locates what it acts on (`null`: allowed wherever the grant is held). The gate decides, server-side. */
export const requires = {
  "revisions.diff": { grant: "revisions.diff", target: null },
  "revisions.get": { grant: "revisions.get", target: null },
  "revisions.list": { grant: "revisions.list", target: null },
  "revisions.restore": { grant: "revisions.restore", target: "id" },
  "revisions.schema": { grant: "revisions.schema", target: null },
} as const;

/** The tickets.revisions activation's default roles, each the grants it lists (AUTH-3). Defaults only: a server's roles are data an admin may change, and `admin` holds every grant. */
export const defaultRoles = {
  "editor": ["revisions.list", "revisions.get", "revisions.diff", "revisions.restore"],
  "viewer": ["revisions.list", "revisions.get", "revisions.diff"],
} as const;