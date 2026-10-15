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
use crate::vocab::{AnchorPath, Author, Body, CommentId, Query, Route, TicketRef, Under};
#[cfg(feature = "server")]
use crate::store::{CommentChange, CommentQuery, Store};
#[cfg(feature = "server")]
use async_stream::stream;
#[cfg(feature = "server")]
use futures::Stream;
#[cfg(feature = "server")]
use crate::access::{Access, VerifiedUser};
#[cfg(feature = "server")]
use crate::store::Lens;
#[cfg(feature = "server")]
use plexus_core::plexus::{AuthContext, Reach};
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

/// IDENTITY-37: the ticket an app route is the page of — `#/ticket/X`'s
/// `X`, upper-cased — or `None`.
#[cfg(feature = "server")]
fn route_ticket(route: &str) -> Option<String> {
    let rest = route.trim().strip_prefix("#/ticket/")?;
    let id = rest.split(['?', '#']).next().unwrap_or(rest).trim();
    (!id.is_empty()).then(|| id.to_ascii_uppercase())
}

/// IDENTITY-37: the tickets a comment is on — its anchor's, and the one
/// whose page it was left on.
#[cfg(feature = "server")]
fn named(c: &crate::comments::Comment) -> Vec<String> {
    c.anchor.ticket.iter().cloned().chain(route_ticket(&c.anchor.route)).collect()
}

/// IDENTITY-21: whether a comment is within reach: its ticket admitted, or,
/// for a comment on the app, any reach at all. IDENTITY-37: and the ticket
/// its route is the page of, when it is one — a comment left on a ticket's
/// page is that ticket's to see, whatever its anchor names.
#[cfg(feature = "server")]
fn sees(store: &Store, caller: &VerifiedUser, c: &crate::comments::Comment) -> bool {
    let ids = named(c);
    ids.is_empty() || store.member(caller, &ids, &Lens::all()).into_iter().all(|ok| ok)
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
    typed_params,
    server_cfg = "feature = \"server\"",
    crate_path = "plexus_core",
    // AUTH-7: IDENTITY-21's roles as explicit grant lists. IDENTITY-18's G1
    // asked whether a viewer may comment; its recommendation (yes) stands:
    // `add` is a viewer's. Resolving and reopening are an editor's.
    roles(
        viewer = ["add", "list", "watch"],
        editor = [viewer, "resolve", "reopen"],
    )
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
    /// carries `added`. IDENTITY-37: a comment left on a ticket's page
    /// (`route` `#/ticket/X`) is `X`'s to see whatever its anchor names — the
    /// caller needs the role on `X` as well as on the anchor's ticket, and it
    /// is listed and watched only by those who can view both. The anchor is
    /// kept as given (`ticket` stays what the element belonged to).
    #[plexus_macros::method(
        group = "write",
        description = "Leave a comment at an anchor (route, path, label, excerpt, and ticket when the element belongs to one); yields the comment",
        params(
            anchor = "Where: {route: '#/ticket/DEMAND-2', path: '[data-tag=\"ticket DEMAND-1\"]', label: 'Parent chip', excerpt: 'DEMAND-1', ticket: 'DEMAND-2' (optional)}; version is set by the server",
            body = "What it says (markdown; not empty, at most 64 KiB)",
            author = "Who left it (optional; default owner)"
        ),
        example(note = "a note on a ticket's parent chip", params = r##"{"anchor": {"route": "#/ticket/DEMAND-2", "path": "[data-tag=\"ticket DEMAND-1\"]", "label": "Parent chip", "excerpt": "DEMAND-1", "ticket": "DEMAND-2"}, "body": "Show the parent's title here"}"##)
    )]
    pub async fn add(&self, auth: Option<&AuthContext>, reach: Reach, anchor: Anchor, body: Body, author: Option<Author>) -> impl Stream<Item = CommentsEvent> + Send + 'static {
        let store = self.store.clone();
        let admitted = self.access.verify_call(auth, &reach);
        stream! {
            let caller = match admitted {
                Ok(caller) => caller,
                Err(message) => {
                    yield CommentsEvent::Error { message };
                    return;
                }
            };
            let paged = route_ticket(&anchor.route);
            // IDENTITY-21: a comment on a ticket needs `comments.add` on it;
            // one on the app, held anywhere (the gate has said so).
            // IDENTITY-37: and on the ticket whose page it was left on.
            let named: Vec<&str> = anchor.ticket.as_deref().map(str::trim).into_iter().chain(paged.as_deref()).collect();
            let allowed = if named.is_empty() {
                caller.reach_filter().map(|_| ())
            } else {
                caller.require(&named, &store)
            };
            if let Err(message) = allowed {
                yield CommentsEvent::Error { message };
                return;
            }
            // Signed by the caller's principal; the name is theirs, not
            // what `author` says.
            let author = caller.name().map(str::to_string).or(author.map(String::from));
            yield answer(store.add_comment_as(anchor, body.into_string(), author, caller.principal_text()).await);
        }
    }

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
    /// else. From the cache.
    #[plexus_macros::method(
        group = "read",
        description = "List comments, newest first, filtered by ticket, under (a ticket or shortcode's lineage), route, path (a prefix), state, author and text; paged with after + limit",
        params(
            ticket = "Only comments on this ticket, e.g. DEMAND-2 (optional)",
            under = "Only comments whose ticket is in this one's lineage: a ticket (it and its descendants) or a shortcode (it and every shortcode hosted under it), e.g. FILTER (optional)",
            route = "Only comments on this app page, exactly, e.g. #/ticket/DEMAND-2 (optional)",
            path = "Only comments whose anchor's path begins with this (optional; through synapse pass it with -p — synapse expands a `path` flag as a file path)",
            state = "Only comments in this state: open | resolved (optional)",
            author = "Only comments by this author, exactly (optional)",
            text = "Only comments with this text in the body, the label or the excerpt, any ASCII case (optional)",
            intent = "Only comments of this intent: comment | changes (APP-23: a change request) (optional)",
            after = "Cursor: start strictly after (older than) this comment id (optional)",
            limit = "At most this many comments (default 500, at most 1000); the page event carries the next cursor (optional)"
        ),
        values(state = ["open", "resolved"], intent = ["comment", "changes"]),
        example(note = "open comments under FILTER that mention a title", params = r#"{"under": "FILTER", "state": "open", "text": "title"}"#)
    )]
    pub async fn list(
        &self, auth: Option<&AuthContext>, reach: Reach,
        ticket: Option<TicketRef>,
        under: Option<Under>,
        route: Option<Route>,
        path: Option<AnchorPath>,
        state: Option<CommentState>,
        author: Option<Author>,
        text: Option<Query>,
        intent: Option<CommentIntent>,
        after: Option<CommentId>,
        limit: Option<u32>,
    ) -> impl Stream<Item = CommentsEvent> + Send + 'static {
        let store = self.store.clone();
        let admitted = self.access.verify_call(auth, &reach);
        stream! {
            let caller = match admitted {
                Ok(caller) => caller,
                Err(message) => {
                    yield CommentsEvent::Error { message };
                    return;
                }
            };
            if let Err(message) = caller.read_filter() {
                yield CommentsEvent::Error { message };
                return;
            }
            let s = |v: Option<String>| v.filter(|v| !v.trim().is_empty());
            let q = CommentQuery {
                ticket: ticket.map(String::from),
                under: under.map(String::from),
                route: route.map(String::from),
                path: path.map(String::from),
                state,
                author: author.map(String::from),
                text: s(text.map(String::from)),
                intent,
            };
            let limit = crate::limits::page(limit, crate::limits::COMMENTS_PAGE_DEFAULT, crate::limits::COMMENTS_PAGE_MAX);
            // ACCESS-10: the comments in reach, judged as the page is cut.
            match store.comments(&caller, &q, after.map(CommentId::get), Some(limit)) {
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
        group = "write",
        description = "Resolve a comment; yields the comment, resolved",
        params(id = "The comment's id, e.g. 1")
    )]
    pub async fn resolve(&self, auth: Option<&AuthContext>, reach: Reach, id: CommentId) -> impl Stream<Item = CommentsEvent> + Send + 'static {
        let id = id.get();
        let store = self.store.clone();
        let admitted = self.access.verify_call(auth, &reach);
        stream! {
            let caller = match admitted {
                Ok(caller) => caller,
                Err(message) => {
                    yield CommentsEvent::Error { message };
                    return;
                }
            };
            // IDENTITY-21: the grant on the comment's ticket (held anywhere,
            // for a comment on the app).
            // ACCESS-10: a comment out of reach is no comment, as one not there.
            let allowed = match store.comment(&caller, id).map(|c| named(&c)) {
                Err(e) => Err(e.to_string()),
                Ok(ids) if ids.is_empty() => caller.reach_filter().map(|_| ()),
                Ok(ids) => caller.require(&ids.iter().map(String::as_str).collect::<Vec<_>>(), &store),
            };
            if let Err(message) = allowed {
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
        group = "write",
        description = "Reopen a resolved comment; yields the comment, open",
        params(id = "The comment's id, e.g. 1")
    )]
    pub async fn reopen(&self, auth: Option<&AuthContext>, reach: Reach, id: CommentId) -> impl Stream<Item = CommentsEvent> + Send + 'static {
        let id = id.get();
        let store = self.store.clone();
        let admitted = self.access.verify_call(auth, &reach);
        stream! {
            let caller = match admitted {
                Ok(caller) => caller,
                Err(message) => {
                    yield CommentsEvent::Error { message };
                    return;
                }
            };
            // IDENTITY-21: the grant on the comment's ticket (held anywhere,
            // for a comment on the app).
            // ACCESS-10: a comment out of reach is no comment, as one not there.
            let allowed = match store.comment(&caller, id).map(|c| named(&c)) {
                Err(e) => Err(e.to_string()),
                Ok(ids) if ids.is_empty() => caller.reach_filter().map(|_| ()),
                Ok(ids) => caller.require(&ids.iter().map(String::as_str).collect::<Vec<_>>(), &store),
            };
            if let Err(message) = allowed {
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
    /// ticket's write does is here — that is `tickets.watch`. IDENTITY-37:
    /// what it relays follows the caller's grants as they stand at each
    /// change, and a caller who can see nothing any more is told so in band
    /// and the stream ends — ACCESS-12: with one `watch_closed { reason,
    /// message }`, at once when a revoke or sign-out ends it.
    #[plexus_macros::method(
        group = "read",
        streaming,
        description = "Subscribe to comments: added / resolved / reopened, each with the comment, until disconnect; lagged { missed } where a slow subscriber lost changes"
    )]
    pub async fn watch(&self, auth: Option<&AuthContext>, reach: Reach) -> impl Stream<Item = CommentsEvent> + Send + 'static {
        let mut rx = self.store.subscribe_comments();
        // ACCESS-12: the caller followed — verified again when the grants
        // generation moves, a sign-out or the expiry comes, and only then.
        let opened = crate::access::Following::open(&self.access, &self.store, auth, &reach).and_then(|f| {
            f.user().read_filter()?;
            Ok(f)
        });
        let store = self.store.clone();
        stream! {
            let mut following = match opened {
                Ok(f) => f,
                Err(message) => {
                    yield CommentsEvent::Error { message };
                    return;
                }
            };
            loop {
                let next = tokio::select! {
                    biased;
                    () = following.changed() => None,
                    r = rx.recv() => Some(r),
                };
                // The caller as it stands, before anything is relayed:
                // refused, the watch ends saying why.
                if let Err(closed) = following.again() {
                    yield CommentsEvent::WatchClosed { reason: closed.reason, message: closed.message };
                    return;
                }
                let Some(next) = next else { continue };
                match next {
                    // IDENTITY-21: only comments the watcher can see, as
                    // its reach stands now.
                    Ok(CommentChange::Added(c) | CommentChange::Resolved(c) | CommentChange::Reopened(c))
                        if !sees(&store, following.user(), &c) => {}
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
