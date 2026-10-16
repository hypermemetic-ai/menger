// Auto-generated typed client (Layer 2)
// Wraps RPC layer and unwraps PlexusStreamItem to domain types

import type { RpcClient } from '../../rpc';
import { extractData, collectOne } from '../../rpc';
import type { TicketRef } from '../../notes/types';
import type { CommitRef, TicketsEvent } from '../../tickets/types';

/** Typed client interface for tickets.commits plugin */
export interface TicketsCommitsClient {
  /**
   * Link commits to a ticket, or to many (ids; one write, all or none) — the commits that finished it; yields commits_linked { ticket, added } per ticket.

SDLC-9: records which commits finished a ticket. Each entry is a
commit reference — a hexadecimal object name of 7 to 64 characters
(stored lowercase), optionally prefixed by a repository and `@`
(`77df9a0`, `plexus-macros@4f7114a`) — compared as text: the server
has no repository, so link the form you want kept. Every entry not
already on the ticket is appended, in the order given. Yields
exactly one event: `CommitsLinked { ticket, added }` — `added` is
what was newly linked, empty when nothing was (every entry already
linked, or none given), which writes and broadcasts nothing —
`NotFound` if `id` names no ticket, or an in-band `Error` for a link
past 256 commits on the ticket; an entry that is not a reference is
`invalid_params` naming it, before the method runs. An error writes
nothing. Moves `updated_unix` and nothing else — not the status.
`watch` carries every `CommitsLinked` that added something. ACT-2:
`ids` instead of `id` links them to each, in one write, all or none,
one `CommitsLinked` per ticket (`cells_stream`).
   * @server-derived auth
   * @requiresAuth — resolver: self.access.verify
   * @requires `commits.link` on the scope of `id`
   */
  link(commits: CommitRef[], id?: TicketRef | null, ids?: TicketRef[] | null): AsyncGenerator<TicketsEvent>;
  /**
   * Get plugin or method schema. Pass {"method": "name"} for a specific method.
   * @requires `commits.schema` anywhere it is held
   */
  schema(): Promise<unknown>;
  /**
   * Unlink commits from a ticket; yields commits_unlinked { ticket, removed }.

SDLC-9: the inverse of `link` — every entry the ticket carries comes
off. The same references, compared the same way, all or nothing.
Yields exactly one event: `CommitsUnlinked { ticket, removed }` —
`removed` empty when none of them was linked, which writes and
broadcasts nothing — or `NotFound`; an entry that is not a reference
is `invalid_params` naming it. `watch` carries every
`CommitsUnlinked` that removed something.
   * @server-derived auth
   * @requiresAuth — resolver: self.access.verify
   * @requires `commits.unlink` on the scope of `id`
   */
  unlink(commits: CommitRef[], id: TicketRef): AsyncGenerator<TicketsEvent>;
}

/** Typed client implementation for tickets.commits plugin */
class TicketsCommitsClientImpl implements TicketsCommitsClient {
  private rpc: RpcClient;
  constructor(rpc: RpcClient) { this.rpc = rpc; }

  async *link(commits: CommitRef[], id?: TicketRef | null, ids?: TicketRef[] | null): AsyncGenerator<TicketsEvent> {
    const stream = this.rpc.call('tickets.commits.link', { commits, id, ids });
    yield* extractData<TicketsEvent>(stream);
  }

  async schema(): Promise<unknown> {
    const stream = this.rpc.call('tickets.commits.schema', {});
    return collectOne<unknown>(stream);
  }

  async *unlink(commits: CommitRef[], id: TicketRef): AsyncGenerator<TicketsEvent> {
    const stream = this.rpc.call('tickets.commits.unlink', { commits, id });
    yield* extractData<TicketsEvent>(stream);
  }
}

/** Create a typed tickets.commits client from an RPC client */
export function createTicketsCommitsClient(rpc: RpcClient): TicketsCommitsClient {
  return new TicketsCommitsClientImpl(rpc);
}

/** What each tickets.commits method needs (AUTH-9): its grant, and the param that locates what it acts on (`null`: allowed wherever the grant is held). The gate decides, server-side. */
export const requires = {
  "commits.link": { grant: "commits.link", target: "id" },
  "commits.schema": { grant: "commits.schema", target: null },
  "commits.unlink": { grant: "commits.unlink", target: "id" },
} as const;

/** The tickets.commits activation's default roles, each the grants it lists (AUTH-3). Defaults only: a server's roles are data an admin may change, and `admin` holds every grant. */
export const defaultRoles = {
  "editor": ["commits.link", "commits.unlink"],
} as const;