// Auto-generated typed client (Layer 2)
// Wraps RPC layer and unwraps PlexusStreamItem to domain types

import type { RpcClient } from '../rpc';
import { extractData, collectOne } from '../rpc';
import type { Author, Body, Kind, NoteId, NoteState, NotesEvent, Query, ShortcodeRef, TicketRef, Title } from './types';

/** Typed client interface for notes plugin */
export interface NotesClient {
  /**
   * Write a note: dated, untitled, kept in one place; yields the note
   *
   * Keeps a note: its `body` as written (not blank, at most 64 KiB) and
   * its author — `owner` unless `author` is given; a signed-in caller's
   * note is its identity's name, whatever `author` says, and signed with
   * its principal. Nothing titles it and it is on no shortcode. Yields
   * exactly one event: `note` — the note as kept: a new id, `at` (unix
   * seconds), the author, the body, `ticket: null` — or an in-band
   * `error` naming the problem, with nothing kept.
   * @requires `notes.add` anywhere it is held
   */
  add(body: Body, author?: Author | null): AsyncGenerator<NotesEvent>;
  /**
   * Get one note by id
   *
   * One note, as it is now. Yields exactly one event: `note`, or an
   * in-band `error` for an id that names no note.
   * @requires `notes.get` anywhere it is held
   */
  get(id: NoteId): AsyncGenerator<NotesEvent>;
  /**
   * List notes newest first, by state, author and text; paged with after and limit
   *
   * The notes matching every filter given, newest first — one `note`
   * event each — then exactly one `page { next }`: `next` is the id to
   * pass as the next call's `after`, `null` when nothing more matches.
   * `state` is `open` (not yet a ticket) or `upgraded`; `author` matches
   * exactly; `text` is a substring of the body, whatever its ASCII case.
   * `after` starts strictly after (older than) that note; `limit` caps the
   * page; without it, every match. An unknown `state`, or an `after` that
   * names no note, is one in-band `error` and nothing else. From the cache.
   * @requires `notes.list` anywhere it is held
   */
  list(after?: NoteId | null, author?: Author | null, limit?: number | null, state?: NoteState | null, text?: Query | null): AsyncGenerator<NotesEvent>;
  /**
   * Get plugin or method schema. Pass {"method": "name"} for a specific method.
   * @requires `notes.schema` anywhere it is held
   */
  schema(): Promise<unknown>;
  /**
   * Make a note a ticket, titled by its first line unless given; yields the note
   *
   * Makes a note a ticket, in one write: the ticket born as
   * `tickets.create` makes one — in `epic`, titled `title` or else the
   * note's first line with words (markdown marks dropped, at most 80
   * characters, cut at a word), its body the note's text and a line
   * saying when it was noted and which note, `kind` (default build) and
   * `parent` as `tickets.create` takes them — and the note marked with
   * the ticket's id and `upgraded_at`. Yields exactly one event: `note`,
   * the note as it is now (its `ticket` set; `tickets.watch` carries the
   * ticket's birth as any create's) — or one in-band `error`: a note that
   * is none or is already a ticket (naming it), or whatever
   * `tickets.create` refuses (a bad epic, a parent that is none, …), and
   * then the note stays open and no id is spent. Under login it needs
   * `tickets.create` where the ticket lands, as `tickets.create` does.
   * @requires `notes.upgrade` anywhere it is held
   */
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

/** What each notes method needs (AUTH-9): its grant, and the param that locates what it acts on (`null`: allowed wherever the grant is held). The gate decides, server-side. */
export const requires = {
  "notes.add": { grant: "notes.add", target: null },
  "notes.get": { grant: "notes.get", target: null },
  "notes.list": { grant: "notes.list", target: null },
  "notes.schema": { grant: "notes.schema", target: null },
  "notes.upgrade": { grant: "notes.upgrade", target: null },
} as const;

/** The notes activation's default roles, each the grants it lists (AUTH-3). Defaults only: a server's roles are data an admin may change, and `admin` holds every grant. */
export const defaultRoles = {
  "editor": ["notes.add", "notes.list", "notes.get", "notes.upgrade"],
  "viewer": ["notes.add", "notes.list", "notes.get"],
} as const;