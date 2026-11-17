//! The `notes` activation — **the one declaration**, as `tickets`, `ui` and
//! `media` are: the `Activation` impl, dispatch, the schema and the
//! methods' bodies from this `impl`, and `NotesClient<T>` for a client of it
//! (the media service's `notes.may`, a test).
//!
//! **Under login**: a viewer reads (`get`, `list`, `state`, `history`,
//! `replay`, `present`, `watch`, `may`, `threads`, `waiting`, `marks`) and is present
//! (`here`), an editor writes (`create`, `update`, `save`, `rename`,
//! `restore`), asks and answers on a range (`ask`, `reply`, `resolve`) and
//! marks one (`mark`, `unmark`: NOTES-5, never writing the text), an admin
//! deletes and squashes (`delete`, `squash`: unlisted — admin holds every
//! grant; `notes.squash` is grantable to another role by the role table). The gate
//! admits a call when its grant is held somewhere; the method asks it again
//! on the note's organization (`acme:`) through the seam — a note
//! in an organization the caller does not read is `not_found`, one it
//! reads and may not write `refused { forbidden }`.

use crate::caller::{Caller, Callers, NotesRequest};
use crate::events::NotesEvent;
use crate::seam::Seam;
use crate::store::NotesStore;
use crate::streams::{many, once};
use crate::vocab::{AgentName, MarkId, MarkLabel, MarkLink, ThreadId, ThreadText, NoteAwareness, NoteBody, NoteId, NoteQuery, NoteSession, NoteTitle, NoteUpdate, NoteVector};
use futures::Stream;
use menger_tickets_api::address::OrgId;
use menger_tickets_api::members::Role;
use std::sync::Arc;

/// The activation: the store over `notes.db`, the seam to the running
/// service, and the resolver every caller is verified by.
#[derive(Clone)]
pub struct NotesActivation {
    store: Arc<NotesStore>,
    seam: Seam,
    callers: Callers,
}

impl NotesActivation {
    /// Over `store`. Inside a runtime, presence that idled out is swept
    /// every few seconds for as long as the store lives (and said `left`).
    pub fn new(store: Arc<NotesStore>) -> Self {
        if let Ok(rt) = tokio::runtime::Handle::try_current() {
            let weak = Arc::downgrade(&store);
            rt.spawn(async move {
                let mut every = tokio::time::interval(std::time::Duration::from_secs(5));
                loop {
                    every.tick().await;
                    match weak.upgrade() {
                        Some(store) => store.sweep(),
                        None => return,
                    }
                }
            });
        }
        Self { store, seam: Seam::default(), callers: Callers }
    }

    /// The seam the builder attaches its running service to, once built.
    pub fn seam(&self) -> &Seam {
        &self.seam
    }

    /// The store it keeps.
    pub fn store(&self) -> &Arc<NotesStore> {
        &self.store
    }
}

/// Notes: writing on its own, written by many hands at once — make one, read it, write it live (a Yjs CRDT), see who is on it, replay how it was built.
#[plexus_rpc::macros::activation(
    auth = "required",
    request = NotesRequest,
    typed_params,
    roles(
        viewer = ["get", "list", "state", "history", "replay", "present", "here", "watch", "may", "threads", "waiting", "marks"],
        editor = [viewer, "create", "update", "save", "rename", "restore", "ask", "reply", "resolve", "mark", "unmark"],
    )
)]
impl NotesActivation {
    /// Make a note in an organization you write in; yields created { note } (revision 1, its id 8 characters, opened at #/note/<id>).
    ///
    /// `org` names the organization (`acme`); without it, the first one you
    /// write notes in. Who may read and write the note is the roles
    /// held on its organization (`acme:`) or on `*`. Refused in band:
    /// `not_found { org }` for an organization you are not in, `refused {
    /// forbidden }` for one you only read. Editor.
    #[plexus_rpc::macros::method(
        group = "write",
        example(note = "a note, titled", params = r#"{"title": "The kiln, in practice", "body": "Fire twice a week."}"#)
    )]
    pub async fn create(
        &self,
        #[from_auth(self.callers.verify)] user: Caller,
        /// Its title, one line, e.g. The kiln, in practice.
        title: NoteTitle,
        /// Its markdown (optional: empty).
        body: Option<NoteBody>,
        /// The organization it is in, e.g. acme (optional: the first you write notes in).
        org: Option<OrgId>,
    ) -> impl Stream<Item = NotesEvent> + Send + 'static {
        let (store, seam) = (self.store.clone(), self.seam.clone());
        once(crate::streams::create(store, seam, user, title.into_string(), body.map(NoteBody::into_string).unwrap_or_default(), org.map(|o| o.as_str().to_string())))
    }

    /// A note whole; yields note { note } — its title, its body (the markdown its CRDT holds now), its revision, who made it and who wrote it last — or not_found.
    ///
    /// A note in an organization you do not read is `not_found`, as one
    /// there is not. `replay` reads it as it stood at a moment. Viewer.
    #[plexus_rpc::macros::method(group = "read", example(note = "a note by its id", params = r#"{"id": "7k3mq2xa"}"#))]
    pub async fn get(
        &self,
        #[from_auth(self.callers.verify)] user: Caller,
        /// Its id, e.g. 7k3mq2xa.
        id: NoteId,
    ) -> impl Stream<Item = NotesEvent> + Send + 'static {
        let (store, seam) = (self.store.clone(), self.seam.clone());
        once(crate::streams::get(store, seam, user, id.into_string()))
    }

    /// The notes you read, newest made first, without their bodies; yields rows { items: [note { note }] } then page { next, remaining }.
    ///
    /// `query` keeps those whose title holds every word of it, case-blind.
    /// A page is 50 rows unless `limit` says (at most 1,000); `after` is the
    /// last row's id of the page before (`page.next`). Viewer.
    #[plexus_rpc::macros::method(group = "read", example(note = "the notes about the kiln", params = r#"{"query": "kiln", "limit": 20}"#))]
    pub async fn list(
        &self,
        #[from_auth(self.callers.verify)] user: Caller,
        /// Words the title holds, e.g. kiln glaze (optional: every note).
        query: Option<NoteQuery>,
        /// The page's size, e.g. 50 (optional: 50; at most 1000).
        limit: Option<u32>,
        /// The id the page before ended at, as its page.next said (optional: the first page).
        after: Option<NoteId>,
    ) -> impl Stream<Item = NotesEvent> + Send + 'static {
        let (store, seam) = (self.store.clone(), self.seam.clone());
        many(crate::streams::list(store, seam, user, query.map(NoteQuery::into_string).filter(|q| !q.is_empty()), limit, after.map(NoteId::into_string)))
    }

    /// Write a note's body whole — merged with whatever else is being written; yields saved { id, revision, at, by } (stale: true when base was not the revision it found).
    ///
    /// For an agent or a script over synapse: the body given is diffed
    /// against the text now and made into the CRDT's own inserts and deletes,
    /// so it merges with live typing instead of overwriting it (a hand typing
    /// in a paragraph the body did not change keeps its words). `base` is the
    /// revision you read (`note.revision`). A body equal to the text writes
    /// nothing. The editor sends `update`s instead. Editor.
    #[plexus_rpc::macros::method(group = "write", example(note = "a body, read at revision 3", params = r##"{"id": "7k3mq2xa", "body": "# The kiln\n\nFire twice a week.\n", "base": 3}"##))]
    pub async fn save(
        &self,
        #[from_auth(self.callers.verify)] user: Caller,
        /// Its id, e.g. 7k3mq2xa.
        id: NoteId,
        /// The markdown, whole.
        body: NoteBody,
        /// The revision you read, e.g. 3 (optional: none said, never stale).
        base: Option<u32>,
    ) -> impl Stream<Item = NotesEvent> + Send + 'static {
        let (store, seam) = (self.store.clone(), self.seam.clone());
        once(crate::streams::save(store, seam, user, id.into_string(), body.into_string(), base))
    }

    /// Apply a Yjs update to a note's body — the editor's live typing; yields applied { id, revision }.
    ///
    /// `update` is a Yjs v1 update of the note's `body` text (base64),
    /// as the page's `ydoc` made it; it is merged into the note's CRDT,
    /// kept as a change with you and the moment, and said to everyone on the
    /// note (`watch`'s `updated`) — not back to `session`, when you name
    /// the presence it came from. Editor.
    #[plexus_rpc::macros::method(group = "write")]
    pub async fn update(
        &self,
        #[from_auth(self.callers.verify)] user: Caller,
        /// Its id, e.g. 7k3mq2xa.
        id: NoteId,
        /// The Yjs v1 update, base64.
        update: NoteUpdate,
        /// Your page's session on it, as your watch named it (optional).
        session: Option<NoteSession>,
    ) -> impl Stream<Item = NotesEvent> + Send + 'static {
        let (store, seam) = (self.store.clone(), self.seam.clone());
        once(crate::streams::update(store, seam, user, id.into_string(), update.into_string(), session.map(NoteSession::into_string)))
    }

    /// What your copy of a note's CRDT lacks; yields state { id, update, vector, revision } — the Yjs update to apply, and the note's own state vector.
    ///
    /// `vector` is your state vector (`Y.encodeStateVector`, base64); with
    /// none, the whole note. Send back what the note lacks of yours
    /// (`Y.encodeStateAsUpdate(ydoc, vector)`) as an `update`. Viewer.
    #[plexus_rpc::macros::method(group = "read", example(note = "the whole note", params = r#"{"id": "7k3mq2xa"}"#))]
    pub async fn state(
        &self,
        #[from_auth(self.callers.verify)] user: Caller,
        /// Its id, e.g. 7k3mq2xa.
        id: NoteId,
        /// Your state vector, base64 (optional: you hold nothing).
        vector: Option<NoteVector>,
    ) -> impl Stream<Item = NotesEvent> + Send + 'static {
        let (store, seam) = (self.store.clone(), self.seam.clone());
        once(crate::streams::state(store, seam, user, id.into_string(), vector.map(NoteVector::into_string)))
    }

    /// Every change of a note, newest first; yields change { change: { seq, kind, at, by, principal, bytes, title?, authors? } } each.
    ///
    /// Every change is kept with who and when until squashed: `create`,
    /// `update` (live typing), `save` (a whole body, merged), `rename`,
    /// `delete`, and `snapshot` (a squash: its `authors` say who wrote what
    /// in it). `at` is unix milliseconds. `before` pages back by `seq`. Viewer.
    #[plexus_rpc::macros::method(group = "read", example(note = "the last twenty", params = r#"{"id": "7k3mq2xa", "limit": 20}"#))]
    pub async fn history(
        &self,
        #[from_auth(self.callers.verify)] user: Caller,
        /// Its id, e.g. 7k3mq2xa.
        id: NoteId,
        /// How many, e.g. 20 (optional: up to 1000).
        limit: Option<u32>,
        /// Only changes before this seq, e.g. 120 (optional: from the newest).
        before: Option<u32>,
    ) -> impl Stream<Item = NotesEvent> + Send + 'static {
        let (store, seam) = (self.store.clone(), self.seam.clone());
        many(crate::streams::history(store, seam, user, id.into_string(), limit, before))
    }

    /// A note as it stood at a moment; yields note { note } — its title and body then, revision the change it stood at — or not_found { revision }.
    ///
    /// `seq` stands it after that change (`history`'s); `at` (unix
    /// milliseconds) after the last change made by then. A moment before the
    /// first change kept — squashed away — is `not_found`. With neither, now.
    /// Viewer.
    #[plexus_rpc::macros::method(group = "read", example(note = "after its tenth change", params = r#"{"id": "7k3mq2xa", "seq": 10}"#))]
    pub async fn replay(
        &self,
        #[from_auth(self.callers.verify)] user: Caller,
        /// Its id, e.g. 7k3mq2xa.
        id: NoteId,
        /// After this change, e.g. 10 (optional).
        seq: Option<u32>,
        /// At this moment, unix milliseconds, e.g. 1793608333000 (optional).
        at: Option<i64>,
    ) -> impl Stream<Item = NotesEvent> + Send + 'static {
        let (store, seam) = (self.store.clone(), self.seam.clone());
        once(crate::streams::replay(store, seam, user, id.into_string(), seq, at))
    }

    /// Collapse a note's changes up to a moment into one snapshot, keeping who wrote what; yields squashed { id, until, removed, at, by }, said on watch.
    ///
    /// The changes up to `until` (a `seq`) become one `snapshot` change whose
    /// `authors` say each writer's changes and bytes; later changes stay as
    /// they are. A replay before `until` is `not_found` after. Needs its own
    /// grant, `notes.squash`, on the note's organization — admin's by
    /// default, grantable to a role by the role table; refused typed
    /// without it. Admin.
    #[plexus_rpc::macros::method(group = "admin", example(note = "everything before change 200", params = r#"{"id": "7k3mq2xa", "until": 200}"#))]
    pub async fn squash(
        &self,
        #[from_auth(self.callers.verify)] user: Caller,
        /// Its id, e.g. 7k3mq2xa.
        id: NoteId,
        /// The last change to fold in, e.g. 200.
        until: u32,
    ) -> impl Stream<Item = NotesEvent> + Send + 'static {
        let (store, seam) = (self.store.clone(), self.seam.clone());
        once(crate::streams::squash(store, seam, user, id.into_string(), until))
    }

    /// Who is on a note now; yields present { present: { session, principal, name, agent, colour, client?, awareness?, at } } each.
    ///
    /// A connection is on it while its `watch { id, session }` is open; a
    /// writer without one (an agent's `save`) while it writes, idling out
    /// after 30 s. Viewer.
    #[plexus_rpc::macros::method(group = "read", example(note = "who is here", params = r#"{"id": "7k3mq2xa"}"#))]
    pub async fn present(
        &self,
        #[from_auth(self.callers.verify)] user: Caller,
        /// Its id, e.g. 7k3mq2xa.
        id: NoteId,
    ) -> impl Stream<Item = NotesEvent> + Send + 'static {
        let (store, seam) = (self.store.clone(), self.seam.clone());
        many(crate::streams::present(store, seam, user, id.into_string()))
    }

    /// Say where your caret is on a note; yields present { present }, said to everyone on it.
    ///
    /// `awareness` is your page's Yjs awareness update (base64) — your
    /// caret, your selection — and `client` your Yjs client id; `session`
    /// your page's, as your watch named it (one someone else holds is
    /// refused). Viewer.
    #[plexus_rpc::macros::method(group = "read")]
    pub async fn here(
        &self,
        #[from_auth(self.callers.verify)] user: Caller,
        /// Its id, e.g. 7k3mq2xa.
        id: NoteId,
        /// Your page's session on it, e.g. s-3f9a1c.
        session: NoteSession,
        /// Your Yjs client id (optional).
        client: Option<u64>,
        /// Your Yjs awareness update, base64 (optional).
        awareness: Option<NoteAwareness>,
    ) -> impl Stream<Item = NotesEvent> + Send + 'static {
        let (store, seam) = (self.store.clone(), self.seam.clone());
        once(crate::streams::here(store, seam, user, id.into_string(), session.into_string(), client, awareness.map(NoteAwareness::into_string)))
    }

    /// Retitle a note; yields renamed { id, title, revision, at, by }. Editor.
    #[plexus_rpc::macros::method(group = "write", example(note = "a new title", params = r#"{"id": "7k3mq2xa", "title": "The kiln, weekly"}"#))]
    pub async fn rename(
        &self,
        #[from_auth(self.callers.verify)] user: Caller,
        /// Its id, e.g. 7k3mq2xa.
        id: NoteId,
        /// The new title, one line.
        title: NoteTitle,
    ) -> impl Stream<Item = NotesEvent> + Send + 'static {
        let (store, seam) = (self.store.clone(), self.seam.clone());
        once(crate::streams::rename(store, seam, user, id.into_string(), title.into_string()))
    }

    /// Delete a note; yields deleted { id, at, by }. Its changes stay in the service's file. Admin.
    #[plexus_rpc::macros::method(group = "admin", example(note = "gone", params = r#"{"id": "7k3mq2xa"}"#))]
    pub async fn delete(
        &self,
        #[from_auth(self.callers.verify)] user: Caller,
        /// Its id, e.g. 7k3mq2xa.
        id: NoteId,
    ) -> impl Stream<Item = NotesEvent> + Send + 'static {
        let (store, seam) = (self.store.clone(), self.seam.clone());
        once(crate::streams::delete(store, seam, user, id.into_string()))
    }

    /// Put a note's body back as it stood after a change; yields saved { id, revision, at, by } — the restore is a change more, merged like a save. Editor.
    #[plexus_rpc::macros::method(group = "write", example(note = "back to change 2", params = r#"{"id": "7k3mq2xa", "seq": 2}"#))]
    pub async fn restore(
        &self,
        #[from_auth(self.callers.verify)] user: Caller,
        /// Its id, e.g. 7k3mq2xa.
        id: NoteId,
        /// The change to stand it at, e.g. 2 (`history`'s seq).
        seq: u32,
    ) -> impl Stream<Item = NotesEvent> + Send + 'static {
        let (store, seam) = (self.store.clone(), self.seam.clone());
        once(crate::streams::restore(store, seam, user, id.into_string(), seq))
    }

    /// Whether you hold a role on a note — viewer | editor | admin — as the gate says on its organization now; yields may { id, role, held }, or not_found for one you do not read.
    ///
    /// The question another service asks through the seam before it keeps
    /// something of a note's, as the caller (the service that keeps
    /// images asks `editor` before it keeps one for a note, `viewer`
    /// before it answers one). Nothing is written. Viewer.
    #[plexus_rpc::macros::method(group = "read", values(role = ["viewer", "editor", "admin"]), example(note = "may I write it?", params = r#"{"id": "7k3mq2xa", "role": "editor"}"#))]
    pub async fn may(
        &self,
        #[from_auth(self.callers.verify)] user: Caller,
        /// Its id, e.g. 7k3mq2xa.
        id: NoteId,
        /// The role asked: viewer | editor | admin.
        role: Role,
    ) -> impl Stream<Item = NotesEvent> + Send + 'static {
        let (store, seam) = (self.store.clone(), self.seam.clone());
        once(crate::streams::may(store, seam, user, id.into_string(), role))
    }

    /// Ask a question on a range of a note — it stays beside that text as the text changes; yields asked { thread }, said on watch.
    ///
    /// `from` and `to` are the range in characters of the text now (as
    /// `get`'s body counts them). The range is anchored in the note's
    /// CRDT, so it follows the text through every edit (`thread.from`,
    /// `thread.to` where it stands now; deleted whole, the thread is
    /// `detached`). `text` is the question (markdown: links, images).
    /// `to_whom` asks it of an agent (`builder`, or `agent` for any); such
    /// a question is listed by `waiting` and said to every watch of the
    /// organization's notes, so an agent picks it up, and may answer
    /// with `reply` or edit the note itself. Editor.
    #[plexus_rpc::macros::method(group = "write", example(note = "a question on characters 10 to 42", params = r#"{"id": "7k3mq2xa", "from": 10, "to": 42, "text": "Is this still true after the move?", "to_whom": "agent"}"#))]
    pub async fn ask(
        &self,
        #[from_auth(self.callers.verify)] user: Caller,
        /// Its id, e.g. 7k3mq2xa.
        id: NoteId,
        /// Where the range starts, in characters, e.g. 10.
        from: u32,
        /// Where it ends, e.g. 42.
        to: u32,
        /// The question, markdown.
        text: ThreadText,
        /// Whom it is asked of: an agent's name, or agent for any (optional: anyone reading).
        to_whom: Option<AgentName>,
    ) -> impl Stream<Item = NotesEvent> + Send + 'static {
        let (store, seam) = (self.store.clone(), self.seam.clone());
        once(crate::streams::ask(store, seam, user, id.into_string(), from, to, text.into_string(), to_whom.map(AgentName::into_string)))
    }

    /// Answer in a thread — words, links, images; yields replied { thread }, said on watch. Editor.
    #[plexus_rpc::macros::method(group = "write", example(note = "an answer", params = r#"{"id": "7k3mq2xa", "thread": "q4k9m2xa", "body": "Yes — see [the move](https://example.com)."}"#))]
    pub async fn reply(
        &self,
        #[from_auth(self.callers.verify)] user: Caller,
        /// The note's id, e.g. 7k3mq2xa.
        id: NoteId,
        /// The thread's id, as ask answered it.
        thread: ThreadId,
        /// The answer, markdown.
        body: ThreadText,
    ) -> impl Stream<Item = NotesEvent> + Send + 'static {
        let (store, seam) = (self.store.clone(), self.seam.clone());
        once(crate::streams::reply(store, seam, user, id.into_string(), thread.into_string(), body.into_string()))
    }

    /// Resolve a thread (it folds in the margin), or open it again; yields resolved { thread }, said on watch. Editor.
    #[plexus_rpc::macros::method(group = "write", example(note = "done", params = r#"{"id": "7k3mq2xa", "thread": "q4k9m2xa"}"#))]
    pub async fn resolve(
        &self,
        #[from_auth(self.callers.verify)] user: Caller,
        /// The note's id, e.g. 7k3mq2xa.
        id: NoteId,
        /// The thread's id.
        thread: ThreadId,
        /// Open it again instead (optional: false).
        open: Option<bool>,
    ) -> impl Stream<Item = NotesEvent> + Send + 'static {
        let (store, seam) = (self.store.clone(), self.seam.clone());
        once(crate::streams::resolve(store, seam, user, id.into_string(), thread.into_string(), open.unwrap_or(false)))
    }

    /// A note's threads, in the order their ranges stand in the text, the detached ones last; yields thread { thread } each.
    ///
    /// Each thread is whole: its anchor (two Yjs relative positions, for a
    /// page to resolve in its own copy), its range now in characters, the
    /// text it quoted, the question, whom it was asked of, its answers, and
    /// whether it is resolved. Viewer.
    #[plexus_rpc::macros::method(group = "read", example(note = "the threads", params = r#"{"id": "7k3mq2xa"}"#))]
    pub async fn threads(
        &self,
        #[from_auth(self.callers.verify)] user: Caller,
        /// Its id, e.g. 7k3mq2xa.
        id: NoteId,
    ) -> impl Stream<Item = NotesEvent> + Send + 'static {
        let (store, seam) = (self.store.clone(), self.seam.clone());
        many(crate::streams::threads(store, seam, user, id.into_string()))
    }

    /// The open questions asked of someone, in the notes you read, oldest first; yields thread { thread } each — an agent's queue.
    ///
    /// `to` keeps those asked of that name (`builder`, or `agent` for the
    /// ones asked of any agent). Viewer.
    #[plexus_rpc::macros::method(group = "read", example(note = "what any agent is asked", params = r#"{"to": "agent"}"#))]
    pub async fn waiting(
        &self,
        #[from_auth(self.callers.verify)] user: Caller,
        /// Asked of this name only, e.g. agent (optional: of anyone named).
        to: Option<AgentName>,
    ) -> impl Stream<Item = NotesEvent> + Send + 'static {
        let (store, seam) = (self.store.clone(), self.seam.clone());
        many(crate::streams::waiting(store, seam, user, to.map(AgentName::into_string)))
    }

    /// Mark a range of a note — an underline that follows its words, with a link, a line of state, or both; or change a mark; yields marked { mark }, said on watch. The note's text is not changed.
    ///
    /// A new mark names `from` and `to`, the range in characters of the
    /// text now (as `get`'s body counts them); it is anchored in the note's
    /// CRDT as a question's range is, so it follows its words through every
    /// edit (`mark.from`, `mark.to` where they stand now; every character
    /// deleted, it is `detached`, kept and drawn nowhere). With `mark`
    /// instead, that mark is changed: its `link` and `label` set as given,
    /// an empty one taken off. `link` is a ticket (`KILN-12`), an address of
    /// the app (`#/…`) or an `https` address; a click on the underline opens
    /// it. `label` is one line of state, drawn under the passage with who
    /// left it. Nothing here writes the note: its body and its revision stay
    /// as they are. Editor.
    #[plexus_rpc::macros::method(group = "write", example(note = "the first sentence, linked to the ticket filed from it", params = r#"{"id": "7k3mq2xa", "from": 0, "to": 40, "link": "KILN-12", "label": "filed KILN-12"}"#))]
    pub async fn mark(
        &self,
        #[from_auth(self.callers.verify)] user: Caller,
        /// The note's id, e.g. 7k3mq2xa.
        id: NoteId,
        /// Where a new mark's range starts, in characters, e.g. 0 (optional: with mark, none).
        from: Option<u32>,
        /// Where it ends, e.g. 40 (optional: with mark, none).
        to: Option<u32>,
        /// The mark to change, as mark answered it (optional: a new mark).
        mark: Option<MarkId>,
        /// What a click on it opens: a ticket, e.g. KILN-12, an address of the app (#/…) or an https address (optional; empty takes it off).
        link: Option<MarkLink>,
        /// One line of state, e.g. filed KILN-12 (optional; empty takes it off).
        label: Option<MarkLabel>,
    ) -> impl Stream<Item = NotesEvent> + Send + 'static {
        let (store, seam) = (self.store.clone(), self.seam.clone());
        once(crate::streams::mark(store, seam, user, id.into_string(), mark.map(MarkId::into_string), from, to, link.map(MarkLink::into_string), label.map(MarkLabel::into_string)))
    }

    /// Take a mark off a note; yields unmarked { id, mark }, said on watch as mark_removed. The note's text is not changed. Editor.
    #[plexus_rpc::macros::method(group = "write", example(note = "a mark taken off", params = r#"{"id": "7k3mq2xa", "mark": "m4k9q2xa"}"#))]
    pub async fn unmark(
        &self,
        #[from_auth(self.callers.verify)] user: Caller,
        /// The note's id, e.g. 7k3mq2xa.
        id: NoteId,
        /// The mark's id, as mark answered it.
        mark: MarkId,
    ) -> impl Stream<Item = NotesEvent> + Send + 'static {
        let (store, seam) = (self.store.clone(), self.seam.clone());
        once(crate::streams::unmark(store, seam, user, id.into_string(), mark.into_string()))
    }

    /// A note's marks, in the order their ranges stand in the text, the detached ones last; yields mark { mark } each — where each stands now and the words it holds.
    ///
    /// Each mark is whole: its anchor (two Yjs relative positions, for a page
    /// to resolve in its own copy), its range now in characters (`from`,
    /// `to`), the `words` it holds now and the `quote` it held when made,
    /// its link and label, who left it and when. Viewer.
    #[plexus_rpc::macros::method(group = "read", example(note = "the marks", params = r#"{"id": "7k3mq2xa"}"#))]
    pub async fn marks(
        &self,
        #[from_auth(self.callers.verify)] user: Caller,
        /// Its id, e.g. 7k3mq2xa.
        id: NoteId,
    ) -> impl Stream<Item = NotesEvent> + Send + 'static {
        let (store, seam) = (self.store.clone(), self.seam.clone());
        many(crate::streams::marks(store, seam, user, id.into_string()))
    }

    /// Follow notes: watching { id? } first, then created, updated { id, seq, update, by }, renamed, deleted, squashed, asked, replied, resolved, mark_changed, mark_removed — and, on one note, present and left, and with text the body as words — as they happen; open-ended, ends with watch_closed { reason, message }.
    ///
    /// With `id`, one note's: each `updated` carries the Yjs update to
    /// apply (the stream ends after its `deleted`), and who comes and goes
    /// (`present { present }`, `left { session, client? }`); with `session`
    /// as well, this watch is your presence on it while it is open, and your
    /// own updates (sent with that session) are not said back. Without `id`,
    /// every note's you read, `note` without its body. Held to you by the
    /// gate: asked again when the grants move, a sign-out or the expiry comes,
    /// and closed with a typed reason (`signed_out`, `expired`, `revoked`,
    /// `refused`); each write is asked of the gate on its organization as it
    /// stands then. `lagged { missed }` when it fell behind: read again
    /// (`state` with your vector). NOTES-5: `mark_changed { id, mark }` (a
    /// mark made or changed, whole) and `mark_removed { id, mark }`; with
    /// `text: true` (and `id`) the note's markdown as words — `text { id,
    /// revision, body, by, at }` once as the watch opens, then a second
    /// after the first change not yet said, holding every change since:
    /// at most one a second, the last change always said. Viewer.
    #[plexus_rpc::macros::method(group = "read")]
    pub async fn watch(
        &self,
        #[from_auth(self.callers.verify)] user: Caller,
        /// One note's id, e.g. 7k3mq2xa (optional: every note you read).
        id: Option<NoteId>,
        /// Your page's session on it, e.g. s-3f9a1c — you are present while the watch is open (optional; with id only).
        session: Option<NoteSession>,
        /// true: the note's markdown as words, text { id, revision, body, by, at }, as it is written (optional: false; with id only).
        text: Option<bool>,
    ) -> impl Stream<Item = NotesEvent> + Send + 'static {
        crate::watch::watch(self.store.clone(), self.seam.clone(), user, id.map(NoteId::into_string), session.map(NoteSession::into_string), text.unwrap_or(false))
    }

    /// Every principal's notes made and bytes written (admin of *): yields usage { principal, count, bytes } each — what quota.recount reads.
    ///
    /// V2-31: from the changes kept — `count` the notes a principal
    /// made, `bytes` every change they wrote (a squash's snapshot is
    /// nobody's). Admin of `*`.
    #[plexus_rpc::macros::method(group = "read", example(note = "every author", params = r#"{"everyone": true}"#))]
    pub async fn usage(
        &self,
        #[from_auth(self.callers.verify)] user: Caller,
        /// true: every principal (the only reading there is).
        everyone: Option<bool>,
        /// V2-70: by the day each change was kept — usage_day { principal, day, count, bytes } (what quota history is backfilled from).
        daily: Option<bool>,
    ) -> impl Stream<Item = NotesEvent> + Send + 'static {
        let _ = everyone;
        let (store, seam) = (self.store.clone(), self.seam.clone());
        many(crate::streams::usage(store, seam, user, daily.unwrap_or(false)))
    }
}
