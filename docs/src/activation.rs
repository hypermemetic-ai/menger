//! The `docs` activation — **the one declaration**, as `tickets`, `ui` and
//! `media` are: the `Activation` impl, dispatch, the schema and the
//! methods' bodies from this `impl`, and `DocsClient<T>` for a client of it
//! (the media service's `docs.may`, a test).
//!
//! **Under login**: a viewer reads (`get`, `list`, `state`, `history`,
//! `replay`, `present`, `watch`, `may`) and is present (`here`), an editor
//! writes (`create`, `update`, `save`, `rename`, `restore`), an admin
//! deletes and squashes (`delete`, `squash`: unlisted — admin holds every
//! grant; `docs.squash` is grantable to another role by the role table). The gate
//! admits a call when its grant is held somewhere; the method asks it again
//! on the document's organization (`acme:`) through the seam — a document
//! in an organization the caller does not read is `not_found`, one it
//! reads and may not write `refused { forbidden }`.

use crate::caller::{Caller, Callers, DocsRequest};
use crate::events::DocsEvent;
use crate::seam::Seam;
use crate::store::DocsStore;
use crate::vocab::{DocAwareness, DocBody, DocId, DocQuery, DocSession, DocTitle, DocUpdate, DocVector};
use futures::Stream;
use menger_tickets_api::address::OrgId;
use menger_tickets_api::members::Role;
use std::sync::Arc;

/// The activation: the store over `docs.db`, the seam to the running
/// service, and the resolver every caller is verified by.
#[derive(Clone)]
pub struct DocsActivation {
    store: Arc<DocsStore>,
    seam: Seam,
    callers: Callers,
}

impl DocsActivation {
    /// Over `store`. Inside a runtime, presence that idled out is swept
    /// every few seconds for as long as the store lives (and said `left`).
    pub fn new(store: Arc<DocsStore>) -> Self {
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
    pub fn store(&self) -> &Arc<DocsStore> {
        &self.store
    }
}

fn once(ev: impl std::future::Future<Output = DocsEvent> + Send + 'static) -> impl Stream<Item = DocsEvent> + Send + 'static {
    futures::stream::once(ev)
}

fn many(evs: impl std::future::Future<Output = Vec<DocsEvent>> + Send + 'static) -> impl Stream<Item = DocsEvent> + Send + 'static {
    use futures::StreamExt;
    futures::stream::once(evs).flat_map(futures::stream::iter)
}

/// Documents: writing on its own, written by many hands at once — make one, read it, write it live (a Yjs CRDT), see who is on it, replay how it was built.
#[plexus_macros::activation(
    auth = "required",
    request = DocsRequest,
    typed_params,
    roles(
        viewer = ["get", "list", "state", "history", "replay", "present", "here", "watch", "may"],
        editor = [viewer, "create", "update", "save", "rename", "restore"],
    )
)]
impl DocsActivation {
    /// Make a document in an organization you write in; yields created { doc } (revision 1, its id 8 characters, opened at #/doc/<id>).
    ///
    /// `org` names the organization (`acme`); without it, the first one you
    /// write documents in. Who may read and write the document is the roles
    /// held on its organization (`acme:`) or on `*`. Refused in band:
    /// `not_found { org }` for an organization you are not in, `refused {
    /// forbidden }` for one you only read. Editor.
    #[plexus_macros::method(
        group = "write",
        example(note = "a document, titled", params = r#"{"title": "The kiln, in practice", "body": "Fire twice a week."}"#)
    )]
    pub async fn create(
        &self,
        #[from_auth(self.callers.verify)] user: Caller,
        /// Its title, one line, e.g. The kiln, in practice.
        title: DocTitle,
        /// Its markdown (optional: empty).
        body: Option<DocBody>,
        /// The organization it is in, e.g. acme (optional: the first you write documents in).
        org: Option<OrgId>,
    ) -> impl Stream<Item = DocsEvent> + Send + 'static {
        let (store, seam) = (self.store.clone(), self.seam.clone());
        once(crate::streams::create(store, seam, user, title.into_string(), body.map(DocBody::into_string).unwrap_or_default(), org.map(|o| o.as_str().to_string())))
    }

    /// A document whole; yields doc { doc } — its title, its body (the markdown its CRDT holds now), its revision, who made it and who wrote it last — or not_found.
    ///
    /// A document in an organization you do not read is `not_found`, as one
    /// there is not. `replay` reads it as it stood at a moment. Viewer.
    #[plexus_macros::method(group = "read", example(note = "a document by its id", params = r#"{"id": "7k3mq2xa"}"#))]
    pub async fn get(
        &self,
        #[from_auth(self.callers.verify)] user: Caller,
        /// Its id, e.g. 7k3mq2xa.
        id: DocId,
    ) -> impl Stream<Item = DocsEvent> + Send + 'static {
        let (store, seam) = (self.store.clone(), self.seam.clone());
        once(crate::streams::get(store, seam, user, id.into_string()))
    }

    /// The documents you read, newest made first, without their bodies; yields rows { items: [doc { doc }] } then page { next, remaining }.
    ///
    /// `query` keeps those whose title holds every word of it, case-blind.
    /// A page is 50 rows unless `limit` says (at most 1,000); `after` is the
    /// last row's id of the page before (`page.next`). Viewer.
    #[plexus_macros::method(group = "read", example(note = "the documents about the kiln", params = r#"{"query": "kiln", "limit": 20}"#))]
    pub async fn list(
        &self,
        #[from_auth(self.callers.verify)] user: Caller,
        /// Words the title holds, e.g. kiln glaze (optional: every document).
        query: Option<DocQuery>,
        /// The page's size, e.g. 50 (optional: 50; at most 1000).
        limit: Option<u32>,
        /// The id the page before ended at, as its page.next said (optional: the first page).
        after: Option<DocId>,
    ) -> impl Stream<Item = DocsEvent> + Send + 'static {
        let (store, seam) = (self.store.clone(), self.seam.clone());
        many(crate::streams::list(store, seam, user, query.map(DocQuery::into_string).filter(|q| !q.is_empty()), limit, after.map(DocId::into_string)))
    }

    /// Write a document's body whole — merged with whatever else is being written; yields saved { id, revision, at, by } (stale: true when base was not the revision it found).
    ///
    /// For an agent or a script over synapse: the body given is diffed
    /// against the text now and made into the CRDT's own inserts and deletes,
    /// so it merges with live typing instead of overwriting it (a hand typing
    /// in a paragraph the body did not change keeps its words). `base` is the
    /// revision you read (`doc.revision`). A body equal to the text writes
    /// nothing. The editor sends `update`s instead. Editor.
    #[plexus_macros::method(group = "write", example(note = "a body, read at revision 3", params = r##"{"id": "7k3mq2xa", "body": "# The kiln\n\nFire twice a week.\n", "base": 3}"##))]
    pub async fn save(
        &self,
        #[from_auth(self.callers.verify)] user: Caller,
        /// Its id, e.g. 7k3mq2xa.
        id: DocId,
        /// The markdown, whole.
        body: DocBody,
        /// The revision you read, e.g. 3 (optional: none said, never stale).
        base: Option<u32>,
    ) -> impl Stream<Item = DocsEvent> + Send + 'static {
        let (store, seam) = (self.store.clone(), self.seam.clone());
        once(crate::streams::save(store, seam, user, id.into_string(), body.into_string(), base))
    }

    /// Apply a Yjs update to a document's body — the editor's live typing; yields applied { id, revision }.
    ///
    /// `update` is a Yjs v1 update of the document's `body` text (base64),
    /// as the page's `ydoc` made it; it is merged into the document's CRDT,
    /// kept as a change with you and the moment, and said to everyone on the
    /// document (`watch`'s `updated`) — not back to `session`, when you name
    /// the presence it came from. Editor.
    #[plexus_macros::method(group = "write")]
    pub async fn update(
        &self,
        #[from_auth(self.callers.verify)] user: Caller,
        /// Its id, e.g. 7k3mq2xa.
        id: DocId,
        /// The Yjs v1 update, base64.
        update: DocUpdate,
        /// Your page's session on it, as your watch named it (optional).
        session: Option<DocSession>,
    ) -> impl Stream<Item = DocsEvent> + Send + 'static {
        let (store, seam) = (self.store.clone(), self.seam.clone());
        once(crate::streams::update(store, seam, user, id.into_string(), update.into_string(), session.map(DocSession::into_string)))
    }

    /// What your copy of a document's CRDT lacks; yields state { id, update, vector, revision } — the Yjs update to apply, and the document's own state vector.
    ///
    /// `vector` is your state vector (`Y.encodeStateVector`, base64); with
    /// none, the whole document. Send back what the document lacks of yours
    /// (`Y.encodeStateAsUpdate(ydoc, vector)`) as an `update`. Viewer.
    #[plexus_macros::method(group = "read", example(note = "the whole document", params = r#"{"id": "7k3mq2xa"}"#))]
    pub async fn state(
        &self,
        #[from_auth(self.callers.verify)] user: Caller,
        /// Its id, e.g. 7k3mq2xa.
        id: DocId,
        /// Your state vector, base64 (optional: you hold nothing).
        vector: Option<DocVector>,
    ) -> impl Stream<Item = DocsEvent> + Send + 'static {
        let (store, seam) = (self.store.clone(), self.seam.clone());
        once(crate::streams::state(store, seam, user, id.into_string(), vector.map(DocVector::into_string)))
    }

    /// Every change of a document, newest first; yields change { change: { seq, kind, at, by, principal, bytes, title?, authors? } } each.
    ///
    /// Every change is kept with who and when until squashed: `create`,
    /// `update` (live typing), `save` (a whole body, merged), `rename`,
    /// `delete`, and `snapshot` (a squash: its `authors` say who wrote what
    /// in it). `at` is unix milliseconds. `before` pages back by `seq`. Viewer.
    #[plexus_macros::method(group = "read", example(note = "the last twenty", params = r#"{"id": "7k3mq2xa", "limit": 20}"#))]
    pub async fn history(
        &self,
        #[from_auth(self.callers.verify)] user: Caller,
        /// Its id, e.g. 7k3mq2xa.
        id: DocId,
        /// How many, e.g. 20 (optional: up to 1000).
        limit: Option<u32>,
        /// Only changes before this seq, e.g. 120 (optional: from the newest).
        before: Option<u32>,
    ) -> impl Stream<Item = DocsEvent> + Send + 'static {
        let (store, seam) = (self.store.clone(), self.seam.clone());
        many(crate::streams::history(store, seam, user, id.into_string(), limit, before))
    }

    /// A document as it stood at a moment; yields doc { doc } — its title and body then, revision the change it stood at — or not_found { revision }.
    ///
    /// `seq` stands it after that change (`history`'s); `at` (unix
    /// milliseconds) after the last change made by then. A moment before the
    /// first change kept — squashed away — is `not_found`. With neither, now.
    /// Viewer.
    #[plexus_macros::method(group = "read", example(note = "after its tenth change", params = r#"{"id": "7k3mq2xa", "seq": 10}"#))]
    pub async fn replay(
        &self,
        #[from_auth(self.callers.verify)] user: Caller,
        /// Its id, e.g. 7k3mq2xa.
        id: DocId,
        /// After this change, e.g. 10 (optional).
        seq: Option<u32>,
        /// At this moment, unix milliseconds, e.g. 1793608333000 (optional).
        at: Option<i64>,
    ) -> impl Stream<Item = DocsEvent> + Send + 'static {
        let (store, seam) = (self.store.clone(), self.seam.clone());
        once(crate::streams::replay(store, seam, user, id.into_string(), seq, at))
    }

    /// Collapse a document's changes up to a moment into one snapshot, keeping who wrote what; yields squashed { id, until, removed, at, by }, said on watch.
    ///
    /// The changes up to `until` (a `seq`) become one `snapshot` change whose
    /// `authors` say each writer's changes and bytes; later changes stay as
    /// they are. A replay before `until` is `not_found` after. Needs its own
    /// grant, `docs.squash`, on the document's organization — admin's by
    /// default, grantable to a role by the role table; refused typed
    /// without it. Admin.
    #[plexus_macros::method(group = "admin", example(note = "everything before change 200", params = r#"{"id": "7k3mq2xa", "until": 200}"#))]
    pub async fn squash(
        &self,
        #[from_auth(self.callers.verify)] user: Caller,
        /// Its id, e.g. 7k3mq2xa.
        id: DocId,
        /// The last change to fold in, e.g. 200.
        until: u32,
    ) -> impl Stream<Item = DocsEvent> + Send + 'static {
        let (store, seam) = (self.store.clone(), self.seam.clone());
        once(crate::streams::squash(store, seam, user, id.into_string(), until))
    }

    /// Who is on a document now; yields present { present: { session, principal, name, agent, colour, client?, awareness?, at } } each.
    ///
    /// A connection is on it while its `watch { id, session }` is open; a
    /// writer without one (an agent's `save`) while it writes, idling out
    /// after 30 s. Viewer.
    #[plexus_macros::method(group = "read", example(note = "who is here", params = r#"{"id": "7k3mq2xa"}"#))]
    pub async fn present(
        &self,
        #[from_auth(self.callers.verify)] user: Caller,
        /// Its id, e.g. 7k3mq2xa.
        id: DocId,
    ) -> impl Stream<Item = DocsEvent> + Send + 'static {
        let (store, seam) = (self.store.clone(), self.seam.clone());
        many(crate::streams::present(store, seam, user, id.into_string()))
    }

    /// Say where your caret is on a document; yields present { present }, said to everyone on it.
    ///
    /// `awareness` is your page's Yjs awareness update (base64) — your
    /// caret, your selection — and `client` your Yjs client id; `session`
    /// your page's, as your watch named it (one someone else holds is
    /// refused). Viewer.
    #[plexus_macros::method(group = "read")]
    pub async fn here(
        &self,
        #[from_auth(self.callers.verify)] user: Caller,
        /// Its id, e.g. 7k3mq2xa.
        id: DocId,
        /// Your page's session on it, e.g. s-3f9a1c.
        session: DocSession,
        /// Your Yjs client id (optional).
        client: Option<u64>,
        /// Your Yjs awareness update, base64 (optional).
        awareness: Option<DocAwareness>,
    ) -> impl Stream<Item = DocsEvent> + Send + 'static {
        let (store, seam) = (self.store.clone(), self.seam.clone());
        once(crate::streams::here(store, seam, user, id.into_string(), session.into_string(), client, awareness.map(DocAwareness::into_string)))
    }

    /// Retitle a document; yields renamed { id, title, revision, at, by }. Editor.
    #[plexus_macros::method(group = "write", example(note = "a new title", params = r#"{"id": "7k3mq2xa", "title": "The kiln, weekly"}"#))]
    pub async fn rename(
        &self,
        #[from_auth(self.callers.verify)] user: Caller,
        /// Its id, e.g. 7k3mq2xa.
        id: DocId,
        /// The new title, one line.
        title: DocTitle,
    ) -> impl Stream<Item = DocsEvent> + Send + 'static {
        let (store, seam) = (self.store.clone(), self.seam.clone());
        once(crate::streams::rename(store, seam, user, id.into_string(), title.into_string()))
    }

    /// Delete a document; yields deleted { id, at, by }. Its changes stay in the service's file. Admin.
    #[plexus_macros::method(group = "admin", example(note = "gone", params = r#"{"id": "7k3mq2xa"}"#))]
    pub async fn delete(
        &self,
        #[from_auth(self.callers.verify)] user: Caller,
        /// Its id, e.g. 7k3mq2xa.
        id: DocId,
    ) -> impl Stream<Item = DocsEvent> + Send + 'static {
        let (store, seam) = (self.store.clone(), self.seam.clone());
        once(crate::streams::delete(store, seam, user, id.into_string()))
    }

    /// Put a document's body back as it stood after a change; yields saved { id, revision, at, by } — the restore is a change more, merged like a save. Editor.
    #[plexus_macros::method(group = "write", example(note = "back to change 2", params = r#"{"id": "7k3mq2xa", "seq": 2}"#))]
    pub async fn restore(
        &self,
        #[from_auth(self.callers.verify)] user: Caller,
        /// Its id, e.g. 7k3mq2xa.
        id: DocId,
        /// The change to stand it at, e.g. 2 (`history`'s seq).
        seq: u32,
    ) -> impl Stream<Item = DocsEvent> + Send + 'static {
        let (store, seam) = (self.store.clone(), self.seam.clone());
        once(crate::streams::restore(store, seam, user, id.into_string(), seq))
    }

    /// Whether you hold a role on a document — viewer | editor | admin — as the gate says on its organization now; yields may { id, role, held }, or not_found for one you do not read.
    ///
    /// The question another service asks through the seam before it keeps
    /// something of a document's, as the caller (the service that keeps
    /// images asks `editor` before it keeps one for a document, `viewer`
    /// before it answers one). Nothing is written. Viewer.
    #[plexus_macros::method(group = "read", values(role = ["viewer", "editor", "admin"]), example(note = "may I write it?", params = r#"{"id": "7k3mq2xa", "role": "editor"}"#))]
    pub async fn may(
        &self,
        #[from_auth(self.callers.verify)] user: Caller,
        /// Its id, e.g. 7k3mq2xa.
        id: DocId,
        /// The role asked: viewer | editor | admin.
        role: Role,
    ) -> impl Stream<Item = DocsEvent> + Send + 'static {
        let (store, seam) = (self.store.clone(), self.seam.clone());
        once(crate::streams::may(store, seam, user, id.into_string(), role))
    }

    /// Follow documents: watching { id? } first, then created, updated { id, seq, update, by }, renamed, deleted, squashed — and, on one document, present and left — as they happen; open-ended, ends with watch_closed { reason, message }.
    ///
    /// With `id`, one document's: each `updated` carries the Yjs update to
    /// apply (the stream ends after its `deleted`), and who comes and goes
    /// (`present { present }`, `left { session, client? }`); with `session`
    /// as well, this watch is your presence on it while it is open, and your
    /// own updates (sent with that session) are not said back. Without `id`,
    /// every document's you read, `doc` without its body. Held to you by the
    /// gate: asked again when the grants move, a sign-out or the expiry comes,
    /// and closed with a typed reason (`signed_out`, `expired`, `revoked`,
    /// `refused`); each write is asked of the gate on its organization as it
    /// stands then. `lagged { missed }` when it fell behind: read again
    /// (`state` with your vector). Viewer.
    #[plexus_macros::method(group = "read", streaming)]
    pub async fn watch(
        &self,
        #[from_auth(self.callers.verify)] user: Caller,
        /// One document's id, e.g. 7k3mq2xa (optional: every document you read).
        id: Option<DocId>,
        /// Your page's session on it, e.g. s-3f9a1c — you are present while the watch is open (optional; with id only).
        session: Option<DocSession>,
    ) -> impl Stream<Item = DocsEvent> + Send + 'static {
        crate::watch::watch(self.store.clone(), self.seam.clone(), user, id.map(DocId::into_string), session.map(DocSession::into_string))
    }
}
