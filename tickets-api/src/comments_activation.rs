//! COMMENT-2: the `comments` activation — **the one declaration**, as the
//! `tickets` activation is (MFE-11).
//!
//! | build | what the macro emits from it |
//! |---|---|
//! | `server` (default; the menger binary) | the `Activation` impl, dispatch, the schema, these bodies |
//! | `default-features = false` (the browser app, wasm32) | `CommentsClient<T>` — one typed method per `#[method]`, over [`crate::comments`]' types |
//!
//! The comments are kept by the store, behind the service-wide cache
//! ([`crate::store`]'s `comments`); this activation holds the same
//! `Arc<Store>` as `tickets`, so a comment's anchor is checked against the
//! tickets it names and `under` reads the tickets' lineage.

use crate::comments::{Anchor, CommentIntent, CommentState, CommentsEvent};
// ACCESS-8: every parameter a type (MACRO-12's `typed_params` holds it).
use crate::vocab::{AnchorPath, Author, Body, CommentId, Message, Query, Reason, Route, TicketRef, Under};
#[cfg(feature = "server")]
use crate::access::{Access, MengerRequest, VerifiedUser};
#[cfg(feature = "server")]
use crate::store::{CommentQuery, Store};
#[cfg(feature = "server")]
use futures::Stream;
#[cfg(feature = "server")]
use std::sync::Arc;

// ACCESS-19: the methods' logic, beside the activation.
#[cfg(feature = "server")]
mod failures;
#[cfg(feature = "server")]
mod streams;
#[cfg(feature = "server")]
use streams::*;

/// The activation. Server-only: it holds the store.
#[cfg(feature = "server")]
#[derive(Clone)]
pub struct CommentsActivation {
    store: Arc<Store>,
    /// IDENTITY-19: the rule every method's caller is verified by
    /// ([`crate::access`]).
    access: Access,
    /// COMMENT-9: the comments the app could not keep, as it reported them.
    failures: failures::Failures,
}

#[cfg(feature = "server")]
impl CommentsActivation {
    /// ACCESS-24: a test's, behind [`Access::for_tests`].
    #[cfg(any(test, feature = "test-support"))]
    pub fn new(store: Arc<Store>) -> Self {
        let access = Access::for_tests();
        access.attach(Arc::clone(&store));
        Self::with_access(store, access)
    }

    /// IDENTITY-19: behind [`Access`].
    pub fn with_access(store: Arc<Store>, access: Access) -> Self {
        Self { store, access, failures: Default::default() }
    }
}

// Not `#[cfg]`-gated: the client-only build needs it too (see activation.rs).
// AUTH-7: IDENTITY-21's roles as explicit grant lists. IDENTITY-18's G1
// asked whether a viewer may comment; its recommendation (yes) stands: `add`
// is a viewer's. Resolving and reopening are an editor's.
/// Comments on the app and on ticket bodies — add, list (filtered), resolve, reopen, watch.
#[plexus_macros::activation(
    auth = "required",
    request = MengerRequest,
    typed_params,
    roles(
        viewer = ["add", "list", "watch", "report", "failures"],
        editor = [viewer, "resolve", "reopen"],
    )
)]
impl CommentsActivation {
    /// Leave a comment at an anchor (route, path, label, excerpt, and ticket when the element belongs to one); yields the comment.
    ///
    /// Keeps a comment: `anchor` says where it was left — `route` (the app
    /// page, beginning `#/`), `path` (where on the page), `label` and
    /// `excerpt` (what was there, in words; the excerpt may be empty), and
    /// `ticket` when the element belongs to one, which must name a ticket
    /// and is kept with that ticket's `updated_unix` as `version` (set by
    /// the server). ACCESS-19: the comment is signed by the caller — its
    /// principal kept, its name the author (the principal when it has no
    /// name; `owner` on one kept before ACCESS-24, when login could be off);
    /// no call names its author. Yields
    /// exactly one event: `comment` — the comment as kept: its id, state
    /// `open`, the time (`at`, unix seconds), the author, its anchor and
    /// body — or an in-band `error` naming the problem (an empty body, a
    /// malformed anchor, a ticket that is none), with nothing kept. `watch`
    /// carries `added`. IDENTITY-37: a comment left on a ticket's page
    /// (`route` `#/ticket/X`) is `X`'s to see whatever its anchor names — the
    /// caller needs the role on `X` as well as on the anchor's ticket, and it
    /// is listed and watched only by those who can view both. The anchor is
    /// kept as given (`ticket` stays what the element belonged to).
    /// ACCESS-19: a comment on the app (naming no ticket) is seen only by
    /// its author and the admins of `*`. COMMENT-8: with `reply_to`, a reply
    /// to that comment — no anchor (one given is refused): it takes its
    /// thread's root's, and so its ticket and who sees it; a reply to a
    /// reply answers the same root; a resolved root is reopened in the same
    /// write (`watch` carries `added`, then `reopened`). The answer carries
    /// `reply_to`, the root's id.
    #[plexus_macros::method(
        group = "write",
        example(note = "a note on a ticket's parent chip", params = r##"{"anchor": {"route": "#/ticket/DEMAND-2", "path": "[data-tag=\"ticket DEMAND-1\"]", "label": "Parent chip", "excerpt": "DEMAND-1", "ticket": "DEMAND-2"}, "body": "Show the parent's title here"}"##)
    )]
    pub async fn add(
        &self,
        #[from_auth(self.access.verify)] user: VerifiedUser,
        /// Where: {route: '#/ticket/DEMAND-2', path: '[data-tag="ticket DEMAND-1"]', label: 'Parent chip', excerpt: 'DEMAND-1', ticket: 'DEMAND-2' (optional)}; version is set by the server. Required unless reply_to is given.
        anchor: Option<Anchor>,
        /// What it says (markdown; not empty, at most 64 KiB).
        body: Body,
        /// COMMENT-8: the comment this answers, e.g. 29 — the reply takes its thread's anchor (optional).
        reply_to: Option<CommentId>,
    ) -> impl Stream<Item = CommentsEvent> + Send + 'static {
        let store = self.store.clone();
        futures::stream::once(async move { add(store, user, anchor, body.into_string(), reply_to.map(CommentId::get)).await })
    }

    /// List comments, newest first, filtered by ticket, under (a ticket or shortcode's lineage), route, path (a prefix), state, author and text; paged with after + limit.
    ///
    /// The comments matching every filter given, newest first — one
    /// `comment` event each — then exactly one `page { next }`: `next` is
    /// the id to pass as the next call's `after`, `null` when nothing more
    /// matches. `after` starts the page strictly after (older than) that
    /// comment; `limit` caps it (ACCESS-29: 500 unless given, at most 1,000,
    /// [`crate::limits`]). The filters:
    /// `ticket` (the anchor's ticket), `under` (a ticket or a shortcode: the
    /// comment's ticket is in its lineage — itself, its ancestors, its
    /// shortcode and the shortcodes hosting it — as the filter language's
    /// `under`, read as the tree stands now), `route` (exactly), `path` (a
    /// prefix of the anchor's), `state` (`open` or `resolved`), `author`
    /// (exactly) and `text` (in the body, the label or the excerpt, whatever
    /// its ASCII case) and `intent` (APP-23: `comment` or `changes`). A blank filter is none. An unknown `state`, or an
    /// `after` that names no comment, is one in-band `error` and nothing
    /// else. Only the comments the caller sees (ACCESS-10; a comment on the
    /// app, its author's and the admins' of `*`). From the cache.
    #[plexus_macros::method(
        group = "read",
        values(state = ["open", "resolved"], intent = ["comment", "changes"]),
        example(note = "open comments under FILTER that mention a title", params = r#"{"under": "FILTER", "state": "open", "text": "title"}"#)
    )]
    pub async fn list(
        &self,
        #[from_auth(self.access.verify)] user: VerifiedUser,
        /// Only comments on this ticket, e.g. DEMAND-2 (optional).
        ticket: Option<TicketRef>,
        /// Only comments whose ticket is in this one's lineage: a ticket (it and its descendants) or a shortcode (it and every shortcode hosted under it), e.g. FILTER (optional).
        under: Option<Under>,
        /// Only comments on this app page, exactly, e.g. #/ticket/DEMAND-2 (optional).
        route: Option<Route>,
        /// Only comments whose anchor's path begins with this (optional; through synapse pass it with -p — synapse expands a `path` flag as a file path).
        path: Option<AnchorPath>,
        /// Only comments in this state: open | resolved (optional).
        state: Option<CommentState>,
        /// Only comments by this author, exactly (optional).
        author: Option<Author>,
        /// Only comments with this text in the body, the label or the excerpt, any ASCII case (optional).
        text: Option<Query>,
        /// Only comments of this intent: comment | changes (APP-23: a change request) (optional).
        intent: Option<CommentIntent>,
        /// COMMENT-8: only this comment's thread — its root and the root's replies, oldest last as every page (optional).
        thread: Option<CommentId>,
        /// Cursor: start strictly after (older than) this comment id (optional).
        after: Option<CommentId>,
        /// At most this many comments (default 500, at most 1000); the page event carries the next cursor (optional).
        limit: Option<u32>,
    ) -> impl Stream<Item = CommentsEvent> + Send + 'static {
        let (ticket, under, route, path, author) = (ticket.map(String::from), under.map(String::from), route.map(String::from), path.map(String::from), author.map(String::from));
        let text = text.map(String::from).filter(|v| !v.trim().is_empty());
        let q = CommentQuery { ticket, under, route, path, state, author, text, intent, thread: thread.map(CommentId::get) };
        list_stream(self.store.clone(), user, q, after.map(CommentId::get), limit)
    }

    /// Resolve a comment; yields the comment, resolved.
    ///
    /// Yields exactly one event: `comment`, state `resolved` — as it was,
    /// if it already was, and then nothing is written or broadcast — or an
    /// in-band `error` for an id that names no comment. A comment the
    /// caller does not see is answered as one that is not there, naming
    /// nothing (ACCESS-3 #7). `watch` carries `resolved`.
    #[plexus_macros::method(group = "write")]
    pub async fn resolve(
        &self,
        #[from_auth(self.access.verify)] user: VerifiedUser,
        /// The comment's id, e.g. 1.
        id: CommentId,
    ) -> impl Stream<Item = CommentsEvent> + Send + 'static {
        let store = self.store.clone();
        futures::stream::once(async move { settle(store, user, id.get(), false).await })
    }

    /// Reopen a resolved comment; yields the comment, open.
    ///
    /// Yields exactly one event: `comment`, state `open` — as it was, if it
    /// already was — or an in-band `error` for an id that names no comment.
    /// A comment the caller does not see is answered as one that is not
    /// there, naming nothing (ACCESS-3 #7). `watch` carries `reopened`.
    #[plexus_macros::method(group = "write")]
    pub async fn reopen(
        &self,
        #[from_auth(self.access.verify)] user: VerifiedUser,
        /// The comment's id, e.g. 1.
        id: CommentId,
    ) -> impl Stream<Item = CommentsEvent> + Send + 'static {
        let store = self.store.clone();
        futures::stream::once(async move { settle(store, user, id.get(), true).await })
    }

    /// COMMENT-9: report a comment the app could not keep; yields `reported { failure }`.
    ///
    /// The app's word that a comment was not kept — the socket down, no
    /// answer, a refusal — with the draft, so whoever can see where it was
    /// being left reads it with `failures`. Kept in memory (the newest 200;
    /// a restart forgets them) and logged; never a comment.
    #[plexus_macros::method(group = "write")]
    pub async fn report(
        &self,
        #[from_auth(self.access.verify)] user: VerifiedUser,
        /// Where the comment was being left, as `add`'s anchor.
        anchor: Anchor,
        /// What it said (cut to 64 KiB).
        body: Body,
        /// Why it was not kept, in the app's words.
        reason: Reason,
        /// When it failed, unix seconds (optional: now).
        failed_at: Option<i64>,
        /// The browser, as it names itself (optional).
        agent: Option<Message>,
    ) -> impl Stream<Item = CommentsEvent> + Send + 'static {
        let r = failures::Report { anchor, body: body.into_string(), reason: reason.to_string(), failed_at, agent: agent.and_then(Message::given) };
        report_stream(self.failures.clone(), user, r)
    }

    /// COMMENT-9: the comments the app could not keep, newest first — `failure { failure }` each.
    ///
    /// The caller's own reports and those whose comment it would see had it
    /// been kept; at most `limit` (default 50, at most 200).
    #[plexus_macros::method(group = "read")]
    pub async fn failures(
        &self,
        #[from_auth(self.access.verify)] user: VerifiedUser,
        /// At most this many (default 50, at most 200) (optional).
        limit: Option<u32>,
    ) -> impl Stream<Item = CommentsEvent> + Send + 'static {
        failures_stream(self.failures.clone(), self.store.clone(), user, limit)
    }

    /// Subscribe to comments: added / resolved / reopened, each with the comment, until disconnect; lagged { missed } where a slow subscriber lost changes.
    ///
    /// `added`, `resolved` and `reopened`, each with the comment as the
    /// change left it, in the order the writes were acknowledged, until the
    /// client goes; `lagged { missed }` where a slow subscriber lost changes
    /// (`list` again to catch up). Nothing a ticket's write does is here —
    /// that is `tickets.watch`. IDENTITY-37: what it relays follows the
    /// caller's grants as they stand at each change, and a caller who can
    /// see nothing any more is told so in band and the stream ends —
    /// ACCESS-12: with one `watch_closed { reason, message }`, at once when
    /// a revoke or sign-out ends it.
    #[plexus_macros::method(group = "read", streaming)]
    pub async fn watch(&self, #[from_auth(self.access.verify)] user: VerifiedUser) -> impl Stream<Item = CommentsEvent> + Send + 'static {
        watch_stream(self.access.clone(), self.store.clone(), user)
    }
}
