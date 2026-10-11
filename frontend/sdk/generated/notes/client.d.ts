import type { RpcClient } from '../rpc';
import type { NotesEvent } from './types';
/** Typed client interface for notes plugin */
export interface NotesClient {
    /** Write a note: dated, untitled, kept in one place; yields the note */
    add(body: string, author?: string | null): AsyncGenerator<NotesEvent>;
    /** Get one note by id */
    get(id: number): AsyncGenerator<NotesEvent>;
    /** List notes newest first, by state, author and text; paged with after and limit */
    list(after?: number | null, author?: string | null, limit?: number | null, state?: string | null, text?: string | null): AsyncGenerator<NotesEvent>;
    /** Get plugin or method schema. Pass {"method": "name"} for a specific method. */
    schema(): Promise<unknown>;
    /** Make a note a ticket, titled by its first line unless given; yields the note */
    upgrade(epic: string, id: number, kind?: string | null, parent?: string | null, title?: string | null): AsyncGenerator<NotesEvent>;
}
/** Create a typed notes client from an RPC client */
export declare function createNotesClient(rpc: RpcClient): NotesClient;
