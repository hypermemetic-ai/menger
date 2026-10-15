import type { RpcClient } from '../rpc';
import type { Author, Body, Kind, NoteId, NoteState, NotesEvent, Query, ShortcodeRef, TicketRef, Title } from './types';
/** Typed client interface for notes plugin */
export interface NotesClient {
    /** Write a note: dated, untitled, kept in one place; yields the note */
    add(body: Body, author?: Author | null): AsyncGenerator<NotesEvent>;
    /** Get one note by id */
    get(id: NoteId): AsyncGenerator<NotesEvent>;
    /** List notes newest first, by state, author and text; paged with after and limit */
    list(after?: NoteId | null, author?: Author | null, limit?: number | null, state?: NoteState | null, text?: Query | null): AsyncGenerator<NotesEvent>;
    /** Get plugin or method schema. Pass {"method": "name"} for a specific method. */
    schema(): Promise<unknown>;
    /** Make a note a ticket, titled by its first line unless given; yields the note */
    upgrade(epic: ShortcodeRef, id: NoteId, kind?: Kind | null, parent?: TicketRef | null, title?: Title | null): AsyncGenerator<NotesEvent>;
}
/** Create a typed notes client from an RPC client */
export declare function createNotesClient(rpc: RpcClient): NotesClient;
