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

use crate::comments::{Anchor, CommentsEvent};
#[cfg(feature = "server")]
use crate::comments::CommentState;
#[cfg(feature = "server")]
use crate::store::{CommentChange, CommentQuery, Store};
#[cfg(feature = "server")]
use async_stream::stream;
#[cfg(feature = "server")]
use futures::Stream;
#[cfg(feature = "server")]
use crate::access::Access;
#[cfg(feature = "server")]
use plexus_core::plexus::AuthContext;
#[cfg(feature = "server")]
use std::sync::Arc;

/// The activation. Server-only: it holds the store.
#[cfg(feature = "server")]
#[derive(Clone)]
pub struct Comments {
    store: Arc<Store>,
    /// IDENTITY-19: the rule every method asks first ([`crate::access`]).
    access: Access,
}

#[cfg(feature = "server")]
impl Comments {
    /// No login: every call admitted, as always.
    pub fn new(store: Arc<Store>) -> Self {
        Self::with_access(store, Access::open())
    }

    /// IDENTITY-19: behind [`Access`].
    pub fn with_access(store: Arc<Store>, access: Access) -> Self {
        Self { store, access }
    }
}

/// A write's answer: the comment as it is now, or the refusal in band.
#[cfg(feature = "server")]
fn answer(written: Result<crate::comments::Comment, crate::store::StoreError>) -> CommentsEvent {
    match written {
        Ok(comment) => comment.into(),
        Err(e) => CommentsEvent::Error { message: e.to_string() },
    }
}

// Not `#[cfg]`-gated: the client-only build needs it too (see activation.rs).
#[plexus_macros::activation(
    namespace = "comments",
    description = "Comments on the app and on ticket bodies — add, list (filtered), resolve, reopen, watch",
    client,
    server_cfg = "feature = \"server\"",
    crate_path = "plexus_core"
)]
impl Comments {
    /// Keeps a comment: `anchor` says where it was left — `route` (the app
    /// page, beginning `#/`), `path` (where on the page), `label` and
    /// `excerpt` (what was there, in words; the excerpt may be empty), and
    /// `ticket` when the element belongs to one, which must name a ticket
    /// and is kept with that ticket's `updated_unix` as `version` (set by
    /// the server). `author` is `owner` unless given; IDENTITY-14: a
    /// signed-in caller's comment is its identity's name, whatever `author`
    /// says (the description stays as it was, so the schema does). Yields
    /// exactly one event: `comment` — the comment as kept: its id, state `open`, the
    /// time (`at`, unix seconds), the author, its anchor and body — or an
    /// in-band `error` naming the problem (an empty body, a blank author, a
    /// malformed anchor, a ticket that is none), with nothing kept. `watch`
    /// carries `added`.
    #[plexus_macros::method(
        description = "Leave a comment at an anchor (route, path, label, excerpt, and ticket when the element belongs to one); yields the comment",
        params(
            anchor = "Where: {route: '#/ticket/DEMAND-2', path: '[data-tag=\"ticket DEMAND-1\"]', label: 'Parent chip', excerpt: 'DEMAND-1', ticket: 'DEMAND-2' (optional)}; version is set by the server",
            body = "What it says (markdown; not empty, at most 64 KiB)",
            author = "Who left it (optional; default owner)"
        )
    )]
    pub async fn add(&self, auth: Option<&AuthContext>, anchor: Anchor, body: String, author: Option<String>) -> impl Stream<Item = CommentsEvent> + Send + 'static {
        let store = self.store.clone();
        let admitted = self.access.admit(auth);
        let author = crate::session_activation::identity(auth).name().map(str::to_string).or(author);
        stream! {
            if let Err(message) = admitted {
                yield CommentsEvent::Error { message };
                return;
            }
            yield answer(store.add_comment(anchor, body, author).await);
        }
    }

    /// The comments matching every filter given, newest first — one
    /// `comment` event each — then exactly one `page { next }`: `next` is
    /// the id to pass as the next call's `after`, `null` when nothing more
    /// matches. `after` starts the page strictly after (older than) that
    /// comment; `limit` caps it; without `limit`, every match. The filters:
    /// `ticket` (the anchor's ticket), `under` (a ticket or a shortcode: the
    /// comment's ticket is in its lineage — itself, its ancestors, its
    /// shortcode and the shortcodes hosting it — as the filter language's
    /// `under`, read as the tree stands now), `route` (exactly), `path` (a
    /// prefix of the anchor's), `state` (`open` or `resolved`), `author`
    /// (exactly) and `text` (in the body, the label or the excerpt, whatever
    /// its ASCII case). A blank filter is none. An unknown `state`, or an
    /// `after` that names no comment, is one in-band `error` and nothing
    /// else. From the cache.
    #[plexus_macros::method(
        description = "List comments, newest first, filtered by ticket, under (a ticket or shortcode's lineage), route, path (a prefix), state, author and text; paged with after + limit",
        params(
            ticket = "Only comments on this ticket, e.g. DEMAND-2 (optional)",
            under = "Only comments whose ticket is in this one's lineage: a ticket (it and its descendants) or a shortcode (it and every shortcode hosted under it), e.g. FILTER (optional)",
            route = "Only comments on this app page, exactly, e.g. #/ticket/DEMAND-2 (optional)",
            path = "Only comments whose anchor's path begins with this (optional; through synapse pass it with -p — synapse expands a `path` flag as a file path)",
            state = "Only comments in this state: open | resolved (optional)",
            author = "Only comments by this author, exactly (optional)",
            text = "Only comments with this text in the body, the label or the excerpt, any ASCII case (optional)",
            after = "Cursor: start strictly after (older than) this comment id (optional)",
            limit = "At most this many comments; the page event carries the next cursor (optional)"
        )
    )]
    pub async fn list(
        &self, auth: Option<&AuthContext>,
        ticket: Option<String>,
        under: Option<String>,
        route: Option<String>,
        path: Option<String>,
        state: Option<String>,
        author: Option<String>,
        text: Option<String>,
        after: Option<u64>,
        limit: Option<u32>,
    ) -> impl Stream<Item = CommentsEvent> + Send + 'static {
        let store = self.store.clone();
        let admitted = self.access.admit(auth);
        stream! {
            if let Err(message) = admitted {
                yield CommentsEvent::Error { message };
                return;
            }
            let state = match state.as_deref().map(str::trim).filter(|s| !s.is_empty()).map(str::parse::<CommentState>) {
                None => None,
                Some(Ok(s)) => Some(s),
                Some(Err(e)) => {
                    yield CommentsEvent::Error { message: e };
                    return;
                }
            };
            let q = CommentQuery { ticket, under, route, path, state, author, text };
            match store.comments(&q, after, limit) {
                Ok(page) => {
                    for comment in page.comments {
                        yield comment.into();
                    }
                    yield CommentsEvent::Page { next: page.next };
                }
                Err(e) => yield CommentsEvent::Error { message: e.to_string() },
            }
        }
    }

    /// Resolves a comment. Yields exactly one event: `comment`, state
    /// `resolved` — as it was, if it already was, and then nothing is
    /// written or broadcast — or an in-band `error` for an id that names no
    /// comment. `watch` carries `resolved`.
    #[plexus_macros::method(
        description = "Resolve a comment; yields the comment, resolved",
        params(id = "The comment's id, e.g. 1")
    )]
    pub async fn resolve(&self, auth: Option<&AuthContext>, id: u64) -> impl Stream<Item = CommentsEvent> + Send + 'static {
        let store = self.store.clone();
        let admitted = self.access.admit(auth);
        stream! {
            if let Err(message) = admitted {
                yield CommentsEvent::Error { message };
                return;
            }
            yield answer(store.resolve_comment(id).await);
        }
    }

    /// Reopens a resolved comment. Yields exactly one event: `comment`,
    /// state `open` — as it was, if it already was — or an in-band `error`
    /// for an id that names no comment. `watch` carries `reopened`.
    #[plexus_macros::method(
        description = "Reopen a resolved comment; yields the comment, open",
        params(id = "The comment's id, e.g. 1")
    )]
    pub async fn reopen(&self, auth: Option<&AuthContext>, id: u64) -> impl Stream<Item = CommentsEvent> + Send + 'static {
        let store = self.store.clone();
        let admitted = self.access.admit(auth);
        stream! {
            if let Err(message) = admitted {
                yield CommentsEvent::Error { message };
                return;
            }
            yield answer(store.reopen_comment(id).await);
        }
    }

    /// Subscribes to the comments: `added`, `resolved` and `reopened`, each
    /// with the comment as the change left it, in the order the writes were
    /// acknowledged, until the client goes; `lagged { missed }` where a slow
    /// subscriber lost changes (`list` again to catch up). Nothing a
    /// ticket's write does is here — that is `tickets.watch`.
    #[plexus_macros::method(
        streaming,
        description = "Subscribe to comments: added / resolved / reopened, each with the comment, until disconnect; lagged { missed } where a slow subscriber lost changes"
    )]
    pub async fn watch(&self, auth: Option<&AuthContext>) -> impl Stream<Item = CommentsEvent> + Send + 'static {
        let mut rx = self.store.subscribe_comments();
        // IDENTITY-19: admitted now and at every change, as tickets.watch.
        let access = self.access.clone();
        let who = auth.cloned();
        stream! {
            if let Err(message) = access.admit(who.as_ref()) {
                yield CommentsEvent::Error { message };
                return;
            }
            loop {
                let next = rx.recv().await;
                if let Err(message) = access.admit(who.as_ref()) {
                    yield CommentsEvent::Error { message };
                    return;
                }
                match next {
                    Ok(CommentChange::Added(comment)) => yield CommentsEvent::Added { comment },
                    Ok(CommentChange::Resolved(comment)) => yield CommentsEvent::Resolved { comment },
                    Ok(CommentChange::Reopened(comment)) => yield CommentsEvent::Reopened { comment },
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(missed)) => {
                        yield CommentsEvent::Lagged { missed };
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
        }
    }
}
