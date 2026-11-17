//! V2-29: **the notes service** — notes on their own API and in a file
//! of their own, as a service of v2's own beside `tickets`, `ui` and
//! `media` (V2-1: services that coexist, each reaching the others as a
//! client through the seam). The owner, 2026-11-02: *"I would like us to
//! move the docs onto a new docs api. same fe though."*
//!
//! A **note** is writing, not work: an id (8 characters of Crockford's
//! base32, TICKETS-35's form), the organization it is in, a title, a
//! markdown body, who made it and who wrote it last. The body is a Yjs CRDT
//! (V2-29's `pairing`, the owner: *"live co editting"*): every hand's change
//! is an update merged by the service and streamed to everyone on the
//! note, who is on it is said as they come and go, every change is kept
//! with who and when (replayed, squashed by permission), and the markdown is
//! derived from it. The editor's surface (`#/note/<id>`) writes to it.
//!
//! **Who may** is the gate's: a viewer reads, an editor writes, an admin
//! deletes, on the note's organization (`acme:`) or on `*` — the
//! method's grant asked of the running service's gate on that scope
//! ([`Seam::holds`]); the organizations a caller is in are the tickets
//! service's word (`orgs.list`, over the seam). This service never opens
//! another's file.
//!
//! | module | what |
//! |---|---|
//! | [`vocab`] | the typed parameters: [`NoteId`], [`NoteTitle`], [`NoteBody`], [`NoteQuery`]; the id's minting |
//! | [`events`] | [`Note`], [`HistoryRow`], [`Present`], [`NotesEvent`] |
//! | [`store`] | the file: `docs` and `doc_log`; get, list, rename, delete; the channel |
//! | [`crdt`] | the body, a Yjs CRDT: create, update, save (a diff merged), state, replay, history, squash, restore |
//! | [`presence`] | who is on a note: entries held by a watch, idling out |
//! | [`threads`] | V2-30: questions on a range, anchored in the CRDT, and their answers |
//! | [`slot`] | NOTES-4: a note's slot — the component drawn beside it, its props, its fills, the build under way |
//! | [`marks`] | NOTES-5: marks on a range — an underline, a link, a line of state — anchored as a question is, never writing the text |
//! | [`quota`] | the meter every write is reserved against (always allows until V2-31) |
//! | [`seam`] | the gate and `orgs.list` through the running service; [`seam::may`] for a client |
//! | [`activation`] | the methods, the one declaration |

pub mod vocab;
pub use vocab::{AgentName, BuildStep, ComponentName, SlotProps, SlotSay, VersionRef, NoteAwareness, NoteBody, NoteId, NoteQuery, NoteSession, NoteTitle, NoteUpdate, NoteVector, ThreadId, ThreadText, MarkId, MarkLabel, MarkLink};
pub mod events;
pub use events::{Anchor, Author, Building, Fill, Note, NotesEvent, HistoryRow, Present, Reply, Resolution, Slot, Thread, WatchEnd, Mark};
pub mod store;
pub use store::{Change, NotesStore, StoreError};
pub mod crdt;
pub mod presence;
pub mod quota;
pub mod threads;
pub mod slot;
mod slot_calls;
pub mod marks;
mod caller;
pub use caller::{Caller, NotesRequest};
pub mod seam;
pub use seam::{Components, Held, Seam};
pub mod activation;
pub use activation::{NotesActivation, NotesClient};
mod streams;
mod watch;

/// The service's file when `--notes-db-path` names none: `notes.db` beside
/// the tickets file, never the same file.
pub const NOTES_DB_FILE: &str = "notes.db";
