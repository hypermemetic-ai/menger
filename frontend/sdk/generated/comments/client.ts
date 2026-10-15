// Auto-generated typed client (Layer 2)
// Wraps RPC layer and unwraps PlexusStreamItem to domain types

import type { RpcClient } from '../rpc';
import { extractData, collectOne } from '../rpc';
import type { AnchorPath, CommentsEvent, Route, Under } from './types';
import type { Author, Body, Query, TicketRef } from '../notes/types';
import type { Anchor, CommentId, CommentIntent, CommentState } from '../tickets/types';

/** Typed client interface for comments plugin */
export interface CommentsClient {
  /** Leave a comment at an anchor (route, path, label, excerpt, and ticket when the element belongs to one); yields the comment.

Keeps a comment: `anchor` says where it was left — `route` (the app
page, beginning `#/`), `path` (where on the page), `label` and
`excerpt` (what was there, in words; the excerpt may be empty), and
`ticket` when the element belongs to one, which must name a ticket
and is kept with that ticket's `updated_unix` as `version` (set by
the server). ACCESS-19: the comment is signed by the caller — its
principal kept, its name the author (the principal when it has no
name; `owner` with login off); no call names its author. Yields
exactly one event: `comment` — the comment as kept: its id, state
`open`, the time (`at`, unix seconds), the author, its anchor and
body — or an in-band `error` naming the problem (an empty body, a
malformed anchor, a ticket that is none), with nothing kept. `watch`
carries `added`. IDENTITY-37: a comment left on a ticket's page
(`route` `#/ticket/X`) is `X`'s to see whatever its anchor names — the
caller needs the role on `X` as well as on the anchor's ticket, and it
is listed and watched only by those who can view both. The anchor is
kept as given (`ticket` stays what the element belonged to).
ACCESS-19: a comment on the app (naming no ticket) is seen only by
its author and the admins of `*`. */
  add(anchor: Anchor, body: Body): AsyncGenerator<CommentsEvent>;
  /** List comments, newest first, filtered by ticket, under (a ticket or shortcode's lineage), route, path (a prefix), state, author and text; paged with after + limit.

The comments matching every filter given, newest first — one
`comment` event each — then exactly one `page { next }`: `next` is
the id to pass as the next call's `after`, `null` when nothing more
matches. `after` starts the page strictly after (older than) that
comment; `limit` caps it (ACCESS-29: 500 unless given, at most 1,000,
[`crate::limits`]). The filters:
`ticket` (the anchor's ticket), `under` (a ticket or a shortcode: the
comment's ticket is in its lineage — itself, its ancestors, its
shortcode and the shortcodes hosting it — as the filter language's
`under`, read as the tree stands now), `route` (exactly), `path` (a
prefix of the anchor's), `state` (`open` or `resolved`), `author`
(exactly) and `text` (in the body, the label or the excerpt, whatever
its ASCII case) and `intent` (APP-23: `comment` or `changes`). A blank filter is none. An unknown `state`, or an
`after` that names no comment, is one in-band `error` and nothing
else. Only the comments the caller sees (ACCESS-10; a comment on the
app, its author's and the admins' of `*`). From the cache. */
  list(after?: CommentId | null, author?: Author | null, intent?: CommentIntent | null, limit?: number | null, path?: AnchorPath | null, route?: Route | null, state?: CommentState | null, text?: Query | null, ticket?: TicketRef | null, under?: Under | null): AsyncGenerator<CommentsEvent>;
  /** Reopen a resolved comment; yields the comment, open.

Yields exactly one event: `comment`, state `open` — as it was, if it
already was — or an in-band `error` for an id that names no comment.
A comment the caller does not see is answered as one that is not
there, naming nothing (ACCESS-3 #7). `watch` carries `reopened`. */
  reopen(id: CommentId): AsyncGenerator<CommentsEvent>;
  /** Resolve a comment; yields the comment, resolved.

Yields exactly one event: `comment`, state `resolved` — as it was,
if it already was, and then nothing is written or broadcast — or an
in-band `error` for an id that names no comment. A comment the
caller does not see is answered as one that is not there, naming
nothing (ACCESS-3 #7). `watch` carries `resolved`. */
  resolve(id: CommentId): AsyncGenerator<CommentsEvent>;
  /** Get plugin or method schema. Pass {"method": "name"} for a specific method. */
  schema(): Promise<unknown>;
  /** Subscribe to comments: added / resolved / reopened, each with the comment, until disconnect; lagged { missed } where a slow subscriber lost changes.

`added`, `resolved` and `reopened`, each with the comment as the
change left it, in the order the writes were acknowledged, until the
client goes; `lagged { missed }` where a slow subscriber lost changes
(`list` again to catch up). Nothing a ticket's write does is here —
that is `tickets.watch`. IDENTITY-37: what it relays follows the
caller's grants as they stand at each change, and a caller who can
see nothing any more is told so in band and the stream ends —
ACCESS-12: with one `watch_closed { reason, message }`, at once when
a revoke or sign-out ends it. */
  watch(): AsyncGenerator<CommentsEvent>;
}

/** Typed client implementation for comments plugin */
class CommentsClientImpl implements CommentsClient {
  private rpc: RpcClient;
  constructor(rpc: RpcClient) { this.rpc = rpc; }

  async *add(anchor: Anchor, body: Body): AsyncGenerator<CommentsEvent> {
    const stream = this.rpc.call('comments.add', { anchor, body });
    yield* extractData<CommentsEvent>(stream);
  }

  async *list(after?: CommentId | null, author?: Author | null, intent?: CommentIntent | null, limit?: number | null, path?: AnchorPath | null, route?: Route | null, state?: CommentState | null, text?: Query | null, ticket?: TicketRef | null, under?: Under | null): AsyncGenerator<CommentsEvent> {
    const stream = this.rpc.call('comments.list', { after, author, intent, limit, path, route, state, text, ticket, under });
    yield* extractData<CommentsEvent>(stream);
  }

  async *reopen(id: CommentId): AsyncGenerator<CommentsEvent> {
    const stream = this.rpc.call('comments.reopen', { id });
    yield* extractData<CommentsEvent>(stream);
  }

  async *resolve(id: CommentId): AsyncGenerator<CommentsEvent> {
    const stream = this.rpc.call('comments.resolve', { id });
    yield* extractData<CommentsEvent>(stream);
  }

  async schema(): Promise<unknown> {
    const stream = this.rpc.call('comments.schema', {});
    return collectOne<unknown>(stream);
  }

  async *watch(): AsyncGenerator<CommentsEvent> {
    const stream = this.rpc.call('comments.watch', {});
    yield* extractData<CommentsEvent>(stream);
  }
}

/** Create a typed comments client from an RPC client */
export function createCommentsClient(rpc: RpcClient): CommentsClient {
  return new CommentsClientImpl(rpc);
}