//! NOTE-2: the `notes` activation — **the one declaration**, as `tickets`
//! and `comments` are (MFE-11).
//!
//! | build | what the macro emits from it |
//! |---|---|
//! | `server` (default; the menger binary) | the `Activation` impl, dispatch, the schema, these bodies |
//! | `default-features = false` (the browser app, wasm32) | `NotesClient<T>` — one typed method per `#[method]`, over [`crate::notes`]' types |
//!
//! Notes are kept by the store, behind the service-wide cache
//! ([`crate::store`]'s `notes`); this activation holds the same
//! `Arc<Store>` as `tickets`, so `upgrade` makes its ticket through the
//! store's own birth of one.
//!
//! **Under login** (IDENTITY-19, AUTH-7): a note is on no shortcode, so a
//! note is any member's — `add`, `list` and `get` are a viewer's, held
//! anywhere, as a comment on the app is. `upgrade` is an editor's, and makes
//! a ticket: it asks what `tickets.create` asks, where the ticket lands —
//! the parent, else the shortcode's root, else `*` for a new shortcode.

use crate::notes::{NoteState, NotesEvent};
// ACCESS-8: every parameter a type (MACRO-12's `typed_params` holds it).
use crate::types::Kind;
use crate::vocab::{Author, Body, NoteId, Query, ShortcodeRef, TicketRef, Title};
#[cfg(feature = "server")]
use crate::access::Access;
#[cfg(feature = "server")]
use crate::store::{NewTicket, NoteQuery, Store, StoreError};
#[cfg(feature = "server")]
use async_stream::stream;
#[cfg(feature = "server")]
use futures::Stream;
#[cfg(feature = "server")]
use plexus_core::plexus::{AuthContext, Grant, Reach};
#[cfg(feature = "server")]
use std::sync::Arc;

/// The activation. Server-only: it holds the store.
#[cfg(feature = "server")]
#[derive(Clone)]
pub struct NotesActivation {
    store: Arc<Store>,
    /// IDENTITY-19: the rule every method asks first ([`crate::access`]).
    access: Access,
}

#[cfg(feature = "server")]
impl NotesActivation {
    /// ACCESS-24: a test's, behind [`Access::for_tests`].
    #[cfg(any(test, feature = "test-support"))]
    pub fn new(store: Arc<Store>) -> Self {
        let access = Access::for_tests();
        access.attach(Arc::clone(&store));
        Self::with_access(store, access)
    }

    /// IDENTITY-19: behind [`Access`].
    pub fn with_access(store: Arc<Store>, access: Access) -> Self {
        Self { store, access }
    }
}

/// A note as an event, or the store's refusal in band.
#[cfg(feature = "server")]
fn answer(note: Result<crate::notes::Note, StoreError>) -> NotesEvent {
    match note {
        Ok(n) => n.into(),
        Err(e) => NotesEvent::Error { message: e.to_string() },
    }
}

// Not `#[cfg]`-gated: the client-only build needs it too (see activation.rs).
#[plexus_macros::activation(
    description = "Notes: dated, untitled thoughts in one place; add, list, get, upgrade to a ticket",
    typed_params,
    // AUTH-7: a member's notes are any member's to read and write; making
    // one a ticket is an editor's (and needs tickets.create where it lands).
    roles(
        viewer = ["add", "list", "get"],
        editor = [viewer, "upgrade"],
    )
)]
impl NotesActivation {
    /// Keeps a note: its `body` as written (not blank, at most 64 KiB) and
    /// its author — `owner` unless `author` is given; a signed-in caller's
    /// note is its identity's name, whatever `author` says, and signed with
    /// its principal. Nothing titles it and it is on no shortcode. Yields
    /// exactly one event: `note` — the note as kept: a new id, `at` (unix
    /// seconds), the author, the body, `ticket: null` — or an in-band
    /// `error` naming the problem, with nothing kept.
    #[plexus_macros::method(
        group = "write",
        description = "Write a note: dated, untitled, kept in one place; yields the note",
        params(
            body = "What it says (markdown; not blank, at most 64 KiB)",
            author = "Who wrote it (optional; default owner; a signed-in caller's own name wins)"
        ),
        example(note = "a thought to keep", params = r#"{"body": "the palette should remember its last query"}"#)
    )]
    pub async fn add(&self, auth: Option<&AuthContext>, reach: Reach, body: Body, author: Option<Author>) -> impl Stream<Item = NotesEvent> + Send + 'static {
        let store = self.store.clone();
        let admitted = self.access.verify_call(auth, &reach);
        stream! {
            let caller = match admitted.and_then(|c| c.reach_filter().map(|_| c)) {
                Ok(caller) => caller,
                Err(message) => {
                    yield NotesEvent::Error { message };
                    return;
                }
            };
            let author = caller.name().map(str::to_string).or(author.map(String::from));
            yield answer(store.add_note_as(body.into_string(), author, Some(caller.principal_text())).await);
        }
    }

    /// The notes matching every filter given, newest first — one `note`
    /// event each — then exactly one `page { next }`: `next` is the id to
    /// pass as the next call's `after`, `null` when nothing more matches.
    /// `state` is `open` (not yet a ticket) or `upgraded`; `author` matches
    /// exactly; `text` is a substring of the body, whatever its ASCII case.
    /// `after` starts strictly after (older than) that note; `limit` caps the
    /// page; without it, every match. An unknown `state`, or an `after` that
    /// names no note, is one in-band `error` and nothing else. From the cache.
    #[plexus_macros::method(
        group = "read",
        description = "List notes newest first, by state, author and text; paged with after and limit",
        params(
            state = "Only notes in this state: open (not yet a ticket) | upgraded (optional)",
            author = "Only notes by this author, exactly (optional)",
            text = "Only notes with this text in the body, any ASCII case (optional)",
            after = "Cursor: start strictly after (older than) this note id (optional)",
            limit = "At most this many notes; the page event carries the next cursor (optional)"
        ),
        values(state = ["open", "upgraded"]),
        example(note = "the notes not yet made tickets", params = r#"{"state": "open"}"#)
    )]
    pub async fn list(
        &self, auth: Option<&AuthContext>, reach: Reach,
        state: Option<NoteState>,
        author: Option<Author>,
        text: Option<Query>,
        after: Option<NoteId>,
        limit: Option<u32>,
    ) -> impl Stream<Item = NotesEvent> + Send + 'static {
        let store = self.store.clone();
        let admitted = self.access.verify_call(auth, &reach);
        stream! {
            if let Err(message) = admitted.and_then(|c| c.reach_filter().map(|_| ())) {
                yield NotesEvent::Error { message };
                return;
            }
            let (author, text) = (author.map(String::from), text.map(String::from).filter(|t| !t.trim().is_empty()));
            match store.notes(&NoteQuery { state, author, text }, after.map(NoteId::get), limit) {
                Ok(page) => {
                    for note in page.notes {
                        yield note.into();
                    }
                    yield NotesEvent::Page { next: page.next };
                }
                Err(e) => yield NotesEvent::Error { message: e.to_string() },
            }
        }
    }

    /// One note, as it is now. Yields exactly one event: `note`, or an
    /// in-band `error` for an id that names no note.
    #[plexus_macros::method(
        group = "read",
        description = "Get one note by id",
        params(id = "The note's id, e.g. 7"),
        example(note = "note 7", params = r#"{"id": 7}"#)
    )]
    pub async fn get(&self, auth: Option<&AuthContext>, reach: Reach, id: NoteId) -> impl Stream<Item = NotesEvent> + Send + 'static {
        let id = id.get();
        let store = self.store.clone();
        let admitted = self.access.verify_call(auth, &reach);
        stream! {
            if let Err(message) = admitted.and_then(|c| c.reach_filter().map(|_| ())) {
                yield NotesEvent::Error { message };
                return;
            }
            yield answer(store.note(id));
        }
    }

    /// Makes a note a ticket, in one write: the ticket born as
    /// `tickets.create` makes one — in `epic`, titled `title` or else the
    /// note's first line with words (markdown marks dropped, at most 80
    /// characters, cut at a word), its body the note's text and a line
    /// saying when it was noted and which note, `kind` (default build) and
    /// `parent` as `tickets.create` takes them — and the note marked with
    /// the ticket's id and `upgraded_at`. Yields exactly one event: `note`,
    /// the note as it is now (its `ticket` set; `tickets.watch` carries the
    /// ticket's birth as any create's) — or one in-band `error`: a note that
    /// is none or is already a ticket (naming it), or whatever
    /// `tickets.create` refuses (a bad epic, a parent that is none, …), and
    /// then the note stays open and no id is spent. Under login it needs
    /// `tickets.create` where the ticket lands, as `tickets.create` does.
    #[plexus_macros::method(
        group = "write",
        description = "Make a note a ticket, titled by its first line unless given; yields the note",
        params(
            id = "The note's id, e.g. 7",
            epic = "The ticket's shortcode, e.g. MENGER",
            title = "The ticket's title (optional; default: the note's first line with words)",
            kind = "scope | execution | build | gap | spike | doc (optional; default build)",
            parent = "The ticket's parent, as tickets.create takes it (optional; default: the shortcode's scope root)"
        ),
        values(kind = ["scope", "execution", "build", "gap", "spike", "doc"]),
        example(note = "note 7 as a build under MENGER-2", params = r#"{"id": 7, "epic": "MENGER", "parent": "MENGER-2"}"#)
    )]
    pub async fn upgrade(
        &self, auth: Option<&AuthContext>, reach: Reach,
        id: NoteId,
        epic: ShortcodeRef,
        title: Option<Title>,
        kind: Option<Kind>,
        parent: Option<TicketRef>,
    ) -> impl Stream<Item = NotesEvent> + Send + 'static {
        let (id, epic, parent) = (id.get(), epic.into_string(), parent.map(String::from));
        let store = self.store.clone();
        let admitted = self.access.verify_call(auth, &reach);
        stream! {
            let caller = match admitted {
                Ok(caller) => caller,
                Err(message) => {
                    yield NotesEvent::Error { message };
                    return;
                }
            };
            // tickets.create's own question, where the ticket lands: the
            // parent, else the shortcode's root; a new shortcode needs `*`.
            // And this method's grant there too.
            let anchor = match parent.as_deref().map(str::trim).filter(|p| !p.is_empty()) {
                Some(p) => Some(p.to_string()),
                // ORG-3: the shortcode by any short form of its address.
                None => match store.root_of(epic.trim()) {
                    Ok(root) => root,
                    Err(e) => {
                        yield NotesEvent::Error { message: e.to_string() };
                        return;
                    }
                },
            };
            if let Some(own) = caller.grant().cloned() {
                let create = Grant::new("tickets.create");
                let allowed = match &anchor {
                    Some(a) => caller.check(&create, a).and_then(|()| caller.check(&own, a)),
                    None => caller
                        .check(&create, "*")
                        .and_then(|()| caller.check(&own, "*"))
                        .map_err(|_| format!("a new shortcode {} needs tickets.create on *", epic.trim().to_ascii_uppercase())),
                };
                if let Err(message) = allowed {
                    yield NotesEvent::Error { message };
                    return;
                }
            }
            // ACCESS-11: the proofs a birth takes, as tickets.create's.
            let at = match crate::activation::filing_text(&store, &caller, &epic, parent.as_deref().map(str::trim).filter(|p| !p.is_empty())) {
                Ok(at) => at,
                Err(message) => {
                    yield NotesEvent::Error { message };
                    return;
                }
            };
            let note = match store.note(id) {
                Ok(n) => n,
                Err(e) => {
                    yield NotesEvent::Error { message: e.to_string() };
                    return;
                }
            };
            let kind = kind.unwrap_or(Kind::Build);
            let (derived, body) = Store::note_ticket_text(&note);
            let title = title.map(|t| t.trim().to_string()).unwrap_or(derived);
            let new = NewTicket {
                epic,
                title,
                kind,
                severity: None,
                blocked_by: Vec::new(),
                parent: parent.map(|p| p.trim().to_string()).filter(|p| !p.is_empty()),
                path: None,
                body,
            };
            yield answer(store.upgrade_note(at, id, new).await.map(|(_, note)| note));
        }
    }
}
