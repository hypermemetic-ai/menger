/** One note, as it is now (yielded by `add`, `get`, `upgrade`, and per
row by `list`). */
export interface NotesEventNote {
    type: 'note';
    at: number;
    author: string;
    body: string;
    id: number;
    principal: string | null;
    ticket: string | null;
    upgradedAt: number | null;
}
/** Where a `list` stands, once, last: `next` is the id to pass as the
next call's `after`, `null` when nothing more matches. */
export interface NotesEventPage {
    type: 'page';
    next: number | null;
}
/** In-band refusal, naming the problem; nothing was written. */
export interface NotesEventError {
    type: 'error';
    message: string;
}
/** What the `notes` activation streams. Every method returns
`impl Stream<Item = NotesEvent>`; a refusal is the in-band `Error`. */
export type NotesEvent = NotesEventNote | NotesEventPage | NotesEventError;
export declare function isNotesEventNote(e: NotesEvent): e is NotesEventNote;
export declare function isNotesEventPage(e: NotesEvent): e is NotesEventPage;
export declare function isNotesEventError(e: NotesEvent): e is NotesEventError;
