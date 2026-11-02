//! What the `docs` service streams ([`DocsEvent`]) and what it says a
//! document is ([`Doc`], [`HistoryRow`], [`Present`]). Every method returns `impl
//! Stream<Item = DocsEvent>`; a refusal is in band.

use serde::{Deserialize, Serialize};

/// A document: its id, its organization, its title and markdown body, its
/// revision now (every write keeps one), who made it and who wrote it last.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Doc {
    /// Its id, 8 characters of Crockford's base32 (`7k3mq2xa`); the app opens it at `#/doc/<id>`.
    pub id: String,
    /// The organization it is in (`acme`): who may read and write it is the roles held there.
    pub org: String,
    pub title: String,
    /// The markdown, byte for byte — absent from a list's rows and a watch of every document.
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

impl Doc {
    /// The same document without its body — a list's row.
    pub fn summary(&self) -> Doc {
        Doc { body: None, ..self.clone() }
    }
}

/// One change of a document, as `history` lists it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct HistoryRow {
    pub id: String,
    /// Its place in the document's changes, from 1; `replay { seq }` stands the document there.
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

/// Someone on a document: a connection open on it, or a writer while it writes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Present {
    /// The document.
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

/// What the `docs` service streams.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DocsEvent {
    /// `create`'s answer, and a watch's word for a document made.
    Created { doc: Doc },
    /// `get`'s and `replay`'s answer: the document whole (as it stood then, for a replay).
    Doc { doc: Doc },
    /// A page of `list`'s rows at once: each item a `doc { doc }` without its body.
    Rows { items: Vec<DocsEvent> },
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
    /// update, base64), the document's own state vector (`vector`, base64), and
    /// its revision.
    State { id: String, update: String, vector: String, revision: u32 },
    /// A watch's word for every change of the body: the Yjs v1 update (base64) to
    /// apply, its `seq`, who and when, the presence `session` it came from when it
    /// said; `doc` the document as it is now (without its body on a watch of every
    /// document).
    Updated {
        id: String,
        seq: u32,
        update: String,
        at: i64,
        by: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        session: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        doc: Option<Doc>,
    },
    /// `rename`'s answer, and a watch's: the new title and the revision it made.
    Renamed {
        id: String,
        title: String,
        revision: u32,
        at: i64,
        by: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        doc: Option<Doc>,
    },
    /// `delete`'s answer, and a watch's: the document is gone (its changes are kept in the file).
    Deleted { id: String, at: i64, by: String },
    /// `squash`'s answer, and a watch's: the changes up to `until` are one
    /// snapshot now (`removed` rows fewer), who wrote what kept as its authors.
    Squashed { id: String, until: u32, removed: u32, at: i64, by: String },
    /// `history`'s rows, newest first.
    Change { change: HistoryRow },
    /// `present`'s rows, and a watch's word: someone is on the document, or moved their caret.
    Present { present: Present },
    /// A watch's word: someone left the document (`client` their Yjs client id, when said).
    Left {
        id: String,
        session: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        client: Option<u64>,
    },
    /// `may`'s answer: whether the caller holds `role` on the document `id`.
    May { id: String, role: menger_tickets_api::members::Role, held: bool },
    /// A watch is open: what it hears follows (`id` when it watches one document).
    Watching {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<String>,
    },
    /// A watch fell behind by `missed` writes: read again (`state`, `list`).
    Lagged { missed: u64 },
    /// A watch ended: `reason` is `signed_out`, `expired`, `revoked` or `refused`.
    WatchClosed { reason: WatchEnd, message: String },
    /// No such document, or none the caller may read (one answer); with
    /// `revision`, no such change kept (none, or squashed). `org` for an
    /// organization the caller is not in.
    NotFound {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        revision: Option<u32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        org: Option<String>,
    },
    /// The caller reads the document but may not do this to it (`forbidden`,
    /// `message` naming the grant and role it takes), or the meter refused the
    /// write (`quota`). Nothing was written.
    Refused { reason: String, message: String },
    /// V2-31: `usage { everyone }` — what one principal's changes keep:
    /// the documents they made and the bytes they wrote.
    Usage { principal: String, count: u64, bytes: u64 },
    /// Any other in-band refusal, naming the problem; nothing was written.
    Error { message: String },
}

impl plexus_client::FromClientError for DocsEvent {
    fn from_client_error(error: plexus_client::ClientError) -> Self {
        DocsEvent::Error { message: error.to_string() }
    }
}

impl DocsEvent {
    /// `not_found` for the document `id`.
    pub fn no_doc(id: &str) -> Self {
        DocsEvent::NotFound { id: Some(id.to_string()), revision: None, org: None }
    }
}
