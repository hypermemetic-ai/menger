// Auto-generated typed client (Layer 2)
// Wraps RPC layer and unwraps PlexusStreamItem to domain types

import type { RpcClient } from '../rpc';
import { extractData, collectOne } from '../rpc';
import type { CommentsEvent } from './types';
import type { Anchor } from '../tickets/types';

/** Typed client interface for comments plugin */
export interface CommentsClient {
  /** Leave a comment at an anchor (route, path, label, excerpt, and ticket when the element belongs to one); yields the comment */
  add(anchor: Anchor, body: string, author?: string | null): AsyncGenerator<CommentsEvent>;
  /** List comments, newest first, filtered by ticket, under (a ticket or shortcode's lineage), route, path (a prefix), state, author and text; paged with after + limit */
  list(after?: number | null, author?: string | null, intent?: string | null, limit?: number | null, path?: string | null, route?: string | null, state?: string | null, text?: string | null, ticket?: string | null, under?: string | null): AsyncGenerator<CommentsEvent>;
  /** Reopen a resolved comment; yields the comment, open */
  reopen(id: number): AsyncGenerator<CommentsEvent>;
  /** Resolve a comment; yields the comment, resolved */
  resolve(id: number): AsyncGenerator<CommentsEvent>;
  /** Get plugin or method schema. Pass {"method": "name"} for a specific method. */
  schema(): Promise<unknown>;
  /** Subscribe to comments: added / resolved / reopened, each with the comment, until disconnect; lagged { missed } where a slow subscriber lost changes */
  watch(): AsyncGenerator<CommentsEvent>;
}

/** Typed client implementation for comments plugin */
class CommentsClientImpl implements CommentsClient {
  private rpc: RpcClient;
  constructor(rpc: RpcClient) { this.rpc = rpc; }

  async *add(anchor: Anchor, body: string, author?: string | null): AsyncGenerator<CommentsEvent> {
    const stream = this.rpc.call('comments.add', { anchor, author, body });
    yield* extractData<CommentsEvent>(stream);
  }

  async *list(after?: number | null, author?: string | null, intent?: string | null, limit?: number | null, path?: string | null, route?: string | null, state?: string | null, text?: string | null, ticket?: string | null, under?: string | null): AsyncGenerator<CommentsEvent> {
    const stream = this.rpc.call('comments.list', { after, author, intent, limit, path, route, state, text, ticket, under });
    yield* extractData<CommentsEvent>(stream);
  }

  async *reopen(id: number): AsyncGenerator<CommentsEvent> {
    const stream = this.rpc.call('comments.reopen', { id });
    yield* extractData<CommentsEvent>(stream);
  }

  async *resolve(id: number): AsyncGenerator<CommentsEvent> {
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