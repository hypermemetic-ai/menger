// Auto-generated typed client (Layer 2)
// Wraps RPC layer and unwraps PlexusStreamItem to domain types

import type { RpcClient } from '../rpc';
import { extractData, collectOne } from '../rpc';
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

/** Typed client implementation for notes plugin */
class NotesClientImpl implements NotesClient {
  private rpc: RpcClient;
  constructor(rpc: RpcClient) { this.rpc = rpc; }

  async *add(body: Body, author?: Author | null): AsyncGenerator<NotesEvent> {
    const stream = this.rpc.call('notes.add', { author, body });
    yield* extractData<NotesEvent>(stream);
  }

  async *get(id: NoteId): AsyncGenerator<NotesEvent> {
    const stream = this.rpc.call('notes.get', { id });
    yield* extractData<NotesEvent>(stream);
  }

  async *list(after?: NoteId | null, author?: Author | null, limit?: number | null, state?: NoteState | null, text?: Query | null): AsyncGenerator<NotesEvent> {
    const stream = this.rpc.call('notes.list', { after, author, limit, state, text });
    yield* extractData<NotesEvent>(stream);
  }

  async schema(): Promise<unknown> {
    const stream = this.rpc.call('notes.schema', {});
    return collectOne<unknown>(stream);
  }

  async *upgrade(epic: ShortcodeRef, id: NoteId, kind?: Kind | null, parent?: TicketRef | null, title?: Title | null): AsyncGenerator<NotesEvent> {
    const stream = this.rpc.call('notes.upgrade', { epic, id, kind, parent, title });
    yield* extractData<NotesEvent>(stream);
  }
}

/** Create a typed notes client from an RPC client */
export function createNotesClient(rpc: RpcClient): NotesClient {
  return new NotesClientImpl(rpc);
}