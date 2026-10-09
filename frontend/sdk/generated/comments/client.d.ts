import type { RpcClient } from '../rpc';
import type { Anchor, CommentsEvent } from './types';
/** Typed client interface for comments plugin */
export interface CommentsClient {
    /** Leave a comment at an anchor (route, path, label, excerpt, and ticket when the element belongs to one); yields the comment */
    add(anchor: Anchor, body: string, author?: string | null): AsyncGenerator<CommentsEvent>;
    /** List comments, newest first, filtered by ticket, under (a ticket or shortcode's lineage), route, path (a prefix), state, author and text; paged with after + limit */
    list(after?: number | null, author?: string | null, limit?: number | null, path?: string | null, route?: string | null, state?: string | null, text?: string | null, ticket?: string | null, under?: string | null): AsyncGenerator<CommentsEvent>;
    /** Reopen a resolved comment; yields the comment, open */
    reopen(id: number): AsyncGenerator<CommentsEvent>;
    /** Resolve a comment; yields the comment, resolved */
    resolve(id: number): AsyncGenerator<CommentsEvent>;
    /** Get plugin or method schema. Pass {"method": "name"} for a specific method. */
    schema(): Promise<unknown>;
    /** Subscribe to comments: added / resolved / reopened, each with the comment, until disconnect; lagged { missed } where a slow subscriber lost changes */
    watch(): AsyncGenerator<CommentsEvent>;
}
/** Create a typed comments client from an RPC client */
export declare function createCommentsClient(rpc: RpcClient): CommentsClient;
