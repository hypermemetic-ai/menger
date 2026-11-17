//! What the `notes` service streams ([`NotesEvent`]) and what it says a
//! note is ([`Note`], [`HistoryRow`], [`Present`]). Every method returns `impl
//! Stream<Item = NotesEvent>`; a refusal is in band.

use serde::{Deserialize, Serialize};

/// A note: its id, its organization, its title and markdown body, its
/// revision now (every write keeps one), who made it and who wrote it last.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Note {
    /// Its id, 8 characters of Crockford's base32 (`7k3mq2xa`); the app opens it at `#/note/<id>`.
    pub id: String,
    /// The organization it is in (`acme`): who may read and write it is the roles held there.
    pub org: String,
    pub title: String,
    /// The markdown, byte for byte — absent from a list's rows and a watch of every note.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
    /// The body's size, in bytes.
    pub bytes: u64,
    /// The revision it is at: the `seq` of its last change (1 when made, one more with every change).
    pub revision: u32,
    /// When it was made (unix seconds), and by whom.
    pub created_at: i64,
    pub created_by: String,
    /// When it was last written (unix seconds), and by whom.
    pub at: i64,
    pub by: String,
}

impl Note {
    /// The same note without its body — a list's row.
    pub fn summary(&self) -> Note {
        Note { body: None, ..self.clone() }
    }
}

/// One change of a note, as `history` lists it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct HistoryRow {
    pub id: String,
    /// Its place in the note's changes, from 1; `replay { seq }` stands the note there.
    pub seq: u32,
    /// What it was: `create`, `update` (a live edit), `save` (a whole body, merged), `rename`, `delete`, or `snapshot` (a squash of every change up to it).
    pub kind: String,
    /// When, in unix milliseconds.
    pub at: i64,
    /// Who: their name, and their principal.
    pub by: String,
    pub principal: String,
    /// The update's size, in bytes.
    pub bytes: u64,
    /// The title it set (`create`, `rename`, `snapshot`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// A snapshot's: who wrote what in the changes it holds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub authors: Option<Vec<Author>>,
}

/// Who wrote what in a squashed run of changes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Author {
    pub by: String,
    pub principal: String,
    /// How many changes, and their bytes.
    pub changes: u32,
    pub bytes: u64,
    /// The first and the last, in unix milliseconds.
    pub first: i64,
    pub last: i64,
}

/// V2-31: a snapshot's `bytes` shared among its `authors` by what each
/// wrote (their `bytes`): each the floor of their part, what is left one
/// byte apiece to the largest writers first (then by principal) — so the
/// shares sum to the snapshot's bytes, the same every time it is asked. No
/// writing at all: the first author holds it. What `notes.bytes` charges
/// each author of a squash, and what `notes.usage` counts.
pub fn shares(bytes: u64, authors: &[Author]) -> Vec<(String, u64)> {
    let total: u128 = authors.iter().map(|a| a.bytes as u128).sum();
    if authors.is_empty() {
        return Vec::new();
    }
    if total == 0 {
        let mut out: Vec<(String, u64)> = authors.iter().map(|a| (a.principal.clone(), 0)).collect();
        out[0].1 = bytes;
        return out;
    }
    let mut out: Vec<(String, u64)> = authors.iter().map(|a| (a.principal.clone(), ((bytes as u128 * a.bytes as u128) / total) as u64)).collect();
    let mut left = bytes - out.iter().map(|(_, n)| n).sum::<u64>();
    let mut order: Vec<usize> = (0..authors.len()).collect();
    order.sort_by(|&a, &b| authors[b].bytes.cmp(&authors[a].bytes).then_with(|| authors[a].principal.cmp(&authors[b].principal)));
    for i in order.into_iter().cycle() {
        if left == 0 {
            break;
        }
        out[i].1 += 1;
        left -= 1;
    }
    out
}

/// Someone on a note: a connection open on it, or a writer while it writes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Present {
    /// The note.
    pub id: String,
    /// The connection's own id (the page's, as its watch said it).
    pub session: String,
    pub principal: String,
    /// The name they signed in as.
    pub name: String,
    /// An agent's key, not a person.
    pub agent: bool,
    /// Their colour, for their caret and their name (#rrggbb).
    pub colour: String,
    /// Their Yjs client id, when their page said it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client: Option<u64>,
    /// Their caret and selection: a Yjs awareness update (base64), when their page said one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub awareness: Option<String>,
    /// Their last word, in unix milliseconds.
    pub at: i64,
}

/// V2-30: a question on a range of a note, and its thread.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Thread {
    /// Its id, 8 characters as a note's.
    pub thread: String,
    /// The note it is on.
    pub id: String,
    /// The range as the note's CRDT holds it: two Yjs relative positions (base64), which follow the text.
    pub anchor: Anchor,
    /// The range now, in characters of the text — absent when it is detached.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub to: Option<u32>,
    /// The text it was on was deleted whole: the thread is kept, listed at the end.
    pub detached: bool,
    /// The text the range held when it was asked.
    pub quote: String,
    /// The question, markdown.
    pub question: String,
    /// Whom it is asked of — an agent's name, `agent` for any (absent: anyone reading).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub to_whom: Option<String>,
    pub by: String,
    pub principal: String,
    /// When, unix milliseconds.
    pub at: i64,
    /// When it was resolved and by whom — absent while it is open.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolved: Option<Resolution>,
    /// The answers, in order.
    pub replies: Vec<Reply>,
}

/// A thread's range as two Yjs relative positions, base64 (`Y.decodeRelativePosition`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Anchor {
    pub from: String,
    pub to: String,
}

/// An answer in a thread.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Reply {
    /// Its place in the thread, from 1.
    pub n: u32,
    /// Markdown: words, links, images (as a body shows them).
    pub body: String,
    pub by: String,
    pub principal: String,
    /// When, unix milliseconds.
    pub at: i64,
}

/// When a thread was resolved, and by whom.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Resolution {
    pub at: i64,
    pub by: String,
}

/// NOTES-4: a note's slot whole — what fills it now, its props, every fill
/// it had and the build under way — as every answer and every watch event
/// about it carries it (a page's `live()` replaces its row whole).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Slot {
    /// The note.
    pub id: String,
    /// What fills it now — absent: nothing (never filled, or emptied).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fill: Option<Fill>,
    /// What the component in it is given beside the note's id: a JSON object (`{}` when none).
    pub props: serde_json::Map<String, serde_json::Value>,
    /// Every fill it had, the newest first (`fill`, when there is one, is among them).
    pub fills: Vec<Fill>,
    /// A build under way, said by the agent with `notes.building` — absent when none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub building: Option<Building>,
}

/// NOTES-4: one fill of a slot: a component of a kept version, who and when.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Fill {
    /// Which fill of the slot it is, from 1.
    pub n: u32,
    /// The component, as its version's manifest names it.
    pub component: String,
    /// The version it is of: its hash, 64 hex digits — what the page pins its mark to.
    pub version: String,
    /// One line saying what this fill does.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub say: Option<String>,
    pub by: String,
    pub principal: String,
    /// When, unix milliseconds.
    pub at: i64,
}

/// NOTES-4: a build under way in a slot — kept in memory only: a fill or an
/// empty ends it, so do ten minutes without a word, and a restart.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Building {
    /// The component being built.
    pub component: String,
    /// The last step reached: `heard`, `written` or `compiled` (absent: none yet).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub step: Option<crate::vocab::BuildStep>,
    /// The agent's line — with `failed`, the refusal (the compiler's line).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub say: Option<String>,
    /// The build failed: the step after `step` is where.
    pub failed: bool,
    pub by: String,
    pub principal: String,
    /// When it began, and its last word — unix milliseconds.
    pub started: i64,
    pub at: i64,
}

/// Why an open `watch` ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum WatchEnd {
    /// Its session was signed out.
    SignedOut,
    /// Its sign-in's expiry passed.
    Expired,
    /// Its grants changed: it holds the watch's grant nowhere any more.
    Revoked,
    /// Verified again, it was refused for another reason (named in `message`).
    Refused,
}

/// What the `notes` service streams.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum NotesEvent {
    /// `create`'s answer, and a watch's word for a note made.
    Created { note: Note },
    /// `get`'s and `replay`'s answer: the note whole (as it stood then, for a replay).
    Note { note: Note },
    /// A page of `list`'s rows at once: each item a `note { note }` without its body.
    Rows { items: Vec<NotesEvent> },
    /// Where a `list` walk stands, the last data event of the call: `next` is the
    /// cursor to pass as the next call's `after` (`null` when nothing more
    /// matches); `remaining` counts the matches past this page.
    Page { next: Option<String>, remaining: u64 },
    /// `save`'s and `restore`'s answer: the body merged in as the CRDT's own
    /// edits — the revision it made (none new when it changed nothing), when and
    /// by whom; `stale` when `base` was not the revision it found.
    Saved {
        id: String,
        revision: u32,
        at: i64,
        by: String,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        stale: bool,
    },
    /// `update`'s answer: the update kept as change `revision`.
    Applied { id: String, revision: u32 },
    /// `state`'s answer: what the caller's state vector lacks (`update`, a Yjs v1
    /// update, base64), the note's own state vector (`vector`, base64), and
    /// its revision.
    State { id: String, update: String, vector: String, revision: u32 },
    /// A watch's word for every change of the body: the Yjs v1 update (base64) to
    /// apply, its `seq`, who and when, the presence `session` it came from when it
    /// said; `note` the note as it is now (without its body on a watch of every
    /// note).
    Updated {
        id: String,
        seq: u32,
        update: String,
        at: i64,
        by: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        session: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        note: Option<Note>,
    },
    /// `rename`'s answer, and a watch's: the new title and the revision it made.
    Renamed {
        id: String,
        title: String,
        revision: u32,
        at: i64,
        by: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        note: Option<Note>,
    },
    /// `delete`'s answer, and a watch's: the note is gone (its changes are kept in the file).
    Deleted { id: String, at: i64, by: String },
    /// `squash`'s answer, and a watch's: the changes up to `until` are one
    /// snapshot now (`removed` rows fewer), who wrote what kept as its authors.
    Squashed { id: String, until: u32, removed: u32, at: i64, by: String },
    /// `history`'s rows, newest first.
    Change { change: HistoryRow },
    /// `present`'s rows, and a watch's word: someone is on the note, or moved their caret.
    Present { present: Present },
    /// A watch's word: someone left the note (`client` their Yjs client id, when said).
    Left {
        id: String,
        session: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        client: Option<u64>,
    },
    /// V2-30: `threads`' and `waiting`'s rows, a thread whole.
    Thread { thread: Thread },
    /// V2-30: `ask`'s answer, and a watch's: a question asked on a range.
    Asked { thread: Thread },
    /// V2-30: `reply`'s answer, and a watch's: the thread with its new answer.
    Replied { thread: Thread },
    /// V2-30: `resolve`'s answer, and a watch's: the thread folded (or opened again: `resolved` absent).
    Resolved { thread: Thread },
    /// `may`'s answer: whether the caller holds `role` on the note `id`.
    May { id: String, role: menger_tickets_api::members::Role, held: bool },
    /// A watch is open: what it hears follows (`id` when it watches one note).
    Watching {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<String>,
    },
    /// A watch fell behind by `missed` writes: read again (`state`, `list`).
    Lagged { missed: u64 },
    /// A watch ended: `reason` is `signed_out`, `expired`, `revoked` or `refused`.
    WatchClosed { reason: WatchEnd, message: String },
    /// No such note, or none the caller may read (one answer); with
    /// `revision`, no such change kept (none, or squashed). `org` for an
    /// organization the caller is not in.
    NotFound {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        revision: Option<u32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        org: Option<String>,
        /// V2-30: no such thread on the note.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        thread: Option<String>,
    },
    /// The caller reads the note but may not do this to it (`forbidden`,
    /// `message` naming the grant and role it takes), or the meter refused the
    /// write (`quota`). Nothing was written.
    Refused { reason: String, message: String },
    /// NOTES-4: `slot`'s answer: the note's slot whole.
    Slot { slot: Slot },
    /// NOTES-4: `fill`'s answer: the slot with its new fill.
    Filled { slot: Slot },
    /// NOTES-4: `empty`'s answer: the slot with nothing in it.
    Emptied { slot: Slot },
    /// NOTES-4: `building`'s answer: the slot with its build under way.
    Building { slot: Slot },
    /// NOTES-4: `props`' answer: the slot with its props merged.
    Props { slot: Slot },
    /// NOTES-4: a watch's word: the slot was filled or emptied — `slot` whole.
    SlotChanged { id: String, slot: Slot },
    /// NOTES-4: a watch's word: a build began, moved a step, failed or ended (`building` absent) — `slot` whole.
    BuildingChanged {
        id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        building: Option<Building>,
        slot: Slot,
    },
    /// NOTES-4: a watch's word: the slot's props, merged by `by` — `slot` whole.
    PropsChanged { id: String, props: serde_json::Map<String, serde_json::Value>, by: String, slot: Slot },
    /// V2-31: `usage { everyone }` — what one principal's changes keep:
    /// the notes they made and the bytes they wrote.
    Usage { principal: String, count: u64, bytes: u64 },
    /// V2-70: `usage { everyone, daily }` — what one principal's changes
    /// kept on one UTC day (`day`: days since 1970-01-01).
    UsageDay { principal: String, day: i64, count: u64, bytes: u64 },
    /// Any other in-band refusal, naming the problem; nothing was written.
    Error { message: String },
}

impl plexus_rpc::client::FromClientError for NotesEvent {
    fn from_client_error(error: plexus_rpc::client::ClientError) -> Self {
        NotesEvent::Error { message: error.to_string() }
    }
}

impl NotesEvent {
    /// `not_found` for the note `id`.
    pub fn no_doc(id: &str) -> Self {
        NotesEvent::NotFound { id: Some(id.to_string()), revision: None, org: None, thread: None }
    }
}
