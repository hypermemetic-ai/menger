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
use crate::access::{Access, Caller};
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

/// IDENTITY-21: the lens a caller sees comments through — `None`: all.
/// AUTH-7: `under` where it holds the call's grant (`comments.list`,
/// `comments.watch`).
#[cfg(feature = "server")]
fn reach_lens(caller: &Caller) -> Result<Option<Lens>, String> {
    caller.reach_filter()?.map(|f| Lens::of(&f).map_err(|e| e.to_string())).transpose()
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
fn sees(store: &Store, reach: Option<&Lens>, c: &crate::comments::Comment) -> bool {
    let Some(lens) = reach else { return true };
    let ids = named(c);
    ids.is_empty() || store.member(&ids, lens).into_iter().all(|ok| ok)
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
    pub async fn add(&self, auth: Option<&AuthContext>, reach: Reach, anchor: Anchor, body: String, author: Option<String>) -> impl Stream<Item = CommentsEvent> + Send + 'static {
        let store = self.store.clone();
        let admitted = self.access.admit(auth, &reach);
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
            let author = caller.name.clone().or(author);
            yield answer(store.add_comment_as(anchor, body, author, caller.principal.clone()).await);
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
        ticket: Option<String>,
        under: Option<String>,
        route: Option<String>,
        path: Option<String>,
        state: Option<String>,
        author: Option<String>,
        text: Option<String>,
        intent: Option<String>,
        after: Option<u64>,
        limit: Option<u32>,
    ) -> impl Stream<Item = CommentsEvent> + Send + 'static {
        let store = self.store.clone();
        let admitted = self.access.admit(auth, &reach);
        stream! {
            let caller = match admitted {
                Ok(caller) => caller,
                Err(message) => {
                    yield CommentsEvent::Error { message };
                    return;
                }
            };
            let reach = match reach_lens(&caller) {
                Ok(r) => r,
                Err(message) => {
                    yield CommentsEvent::Error { message };
                    return;
                }
            };
            let state = match state.as_deref().map(str::trim).filter(|s| !s.is_empty()).map(str::parse::<CommentState>) {
                None => None,
                Some(Ok(s)) => Some(s),
                Some(Err(e)) => {
                    yield CommentsEvent::Error { message: e };
                    return;
                }
            };
            let intent = match intent.as_deref().map(str::trim).filter(|s| !s.is_empty()).map(str::parse::<crate::comments::CommentIntent>) {
                None => None,
                Some(Ok(i)) => Some(i),
                Some(Err(message)) => {
                    yield CommentsEvent::Error { message };
                    return;
                }
            };
            let q = CommentQuery { ticket, under, route, path, state, author, text, intent };
            let limit = crate::limits::page(limit, crate::limits::COMMENTS_PAGE_DEFAULT, crate::limits::COMMENTS_PAGE_MAX);
            match store.comments(&q, after, Some(limit)) {
                Ok(page) => {
                    for comment in page.comments {
                        if !sees(&store, reach.as_ref(), &comment) {
                            continue;
                        }
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
    pub async fn resolve(&self, auth: Option<&AuthContext>, reach: Reach, id: u64) -> impl Stream<Item = CommentsEvent> + Send + 'static {
        let store = self.store.clone();
        let admitted = self.access.admit(auth, &reach);
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
            let allowed = match store.comment(id).map(|c| named(&c)).filter(|n| !n.is_empty()) {
                Some(ids) => caller.require(&ids.iter().map(String::as_str).collect::<Vec<_>>(), &store),
                None => caller.reach_filter().map(|_| ()),
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
    pub async fn reopen(&self, auth: Option<&AuthContext>, reach: Reach, id: u64) -> impl Stream<Item = CommentsEvent> + Send + 'static {
        let store = self.store.clone();
        let admitted = self.access.admit(auth, &reach);
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
            let allowed = match store.comment(id).map(|c| named(&c)).filter(|n| !n.is_empty()) {
                Some(ids) => caller.require(&ids.iter().map(String::as_str).collect::<Vec<_>>(), &store),
                None => caller.reach_filter().map(|_| ()),
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
    /// and the stream ends.
    #[plexus_macros::method(
        group = "read",
        streaming,
        description = "Subscribe to comments: added / resolved / reopened, each with the comment, until disconnect; lagged { missed } where a slow subscriber lost changes"
    )]
    pub async fn watch(&self, auth: Option<&AuthContext>, reach: Reach) -> impl Stream<Item = CommentsEvent> + Send + 'static {
        let mut rx = self.store.subscribe_comments();
        // IDENTITY-19: admitted now and at every change, as tickets.watch.
        let access = self.access.clone();
        let who = auth.cloned();
        let store = self.store.clone();
        stream! {
            let (mut held, mut lens) = match access.admit(who.as_ref(), &reach).and_then(|c| Ok((c.reach_key()?, reach_lens(&c)?))) {
                Ok(r) => r,
                Err(message) => {
                    yield CommentsEvent::Error { message };
                    return;
                }
            };
            loop {
                let next = rx.recv().await;
                // AUTH-7: the gate's live reach — held nowhere any more, the
                // watch ends.
                let Some(now) = reach.current() else {
                    yield CommentsEvent::Error { message: "your access changed: you hold this nowhere any more".into() };
                    return;
                };
                let caller = match access.admit(who.as_ref(), &now) {
                    Ok(c) => c,
                    Err(message) => {
                        yield CommentsEvent::Error { message };
                        return;
                    }
                };
                // IDENTITY-37: the reach as the grants stand now.
                let key = match caller.reach_key() {
                    Ok(k) => k,
                    Err(why) => {
                        yield CommentsEvent::Error { message: format!("your access changed: {why}") };
                        return;
                    }
                };
                if key != held {
                    match reach_lens(&caller) {
                        Ok(r) => lens = r,
                        Err(why) => {
                            yield CommentsEvent::Error { message: format!("your access changed: {why}") };
                            return;
                        }
                    }
                    held = key;
                }
                match next {
                    // IDENTITY-21: only comments the watcher can see.
                    Ok(CommentChange::Added(c) | CommentChange::Resolved(c) | CommentChange::Reopened(c))
                        if !sees(&store, lens.as_ref(), &c) => {}
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
