//! ACCESS-19: what the `comments` methods do, beside the activation (its
//! `impl` keeps only the methods — MENGER-29, ACCESS-13's frame). Each
//! takes the [`VerifiedUser`] the dispatch resolved; nothing here admits by
//! hand.

use crate::access::{Access, Editor, Following, VerifiedUser, Viewer};
use crate::comments::{Anchor, Comment, CommentsEvent};
use crate::store::{CommentChange, CommentQuery, OnComment, Store, StoreError};
use async_stream::stream;
use futures::Stream;
use std::sync::Arc;

/// IDENTITY-37: the ticket an app route is the page of — `#/ticket/X`'s
/// `X`, upper-cased — or `None`.
fn route_ticket(route: &str) -> Option<String> {
    let rest = route.trim().strip_prefix("#/ticket/")?;
    let id = rest.split(['?', '#']).next().unwrap_or(rest).trim();
    (!id.is_empty()).then(|| id.to_ascii_uppercase())
}

/// IDENTITY-37: the tickets a comment is on — its anchor's, and the one
/// whose page it was left on. None: a comment on the app.
fn named(anchor: &Anchor) -> Vec<String> {
    anchor.ticket.as_deref().map(str::trim).filter(|t| !t.is_empty()).map(str::to_string).into_iter().chain(route_ticket(&anchor.route)).collect()
}

/// A write's answer: the comment as it is now, or the refusal in band.
fn answer(written: Result<Comment, StoreError>) -> CommentsEvent {
    match written {
        Ok(comment) => comment.into(),
        Err(e) => CommentsEvent::Error { message: e.to_string() },
    }
}

/// `comments.add`: the caller needs `comments.add` on each ticket the
/// comment is on (IDENTITY-21, IDENTITY-37), or — a comment on the app —
/// held anywhere. ACCESS-19 (ACCESS-3 #9): signed by the caller — its
/// principal kept, its name the author (its principal when it has no name;
/// with login off, the store's `owner`) — never named by the call.
pub(super) async fn add(store: Arc<Store>, user: VerifiedUser, anchor: Anchor, body: String) -> CommentsEvent {
    let on = named(&anchor);
    let on: Vec<&str> = on.iter().map(String::as_str).collect();
    let allowed = match on.is_empty() {
        true => user.reach_filter().map(|_| ()),
        false => user.require(&on, &store),
    };
    if let Err(message) = allowed {
        return CommentsEvent::Error { message };
    }
    let author = user.name().map(str::to_string).or_else(|| user.principal_text());
    // ACCESS-11: a viewer's permit on each ticket it is on.
    let permits = OnComment::new(crate::activation::comment_permits::<Viewer>(&store, &user, &on));
    answer(store.add_comment_as(permits, anchor, body, author, user.principal_text()).await)
}

/// IDENTITY-21 / ACCESS-11: what resolving or reopening the comment `id`
/// takes — the grant on the comment's tickets (held anywhere, for a comment
/// on the app), as an editor's permit on each. ACCESS-10 / ACCESS-19
/// (ACCESS-3 #7): a comment out of reach — on a hidden ticket, or another's
/// comment on the app to anyone but an admin of `*` — is answered as one
/// that is not there, naming nothing.
fn editors_of(store: &Store, user: &VerifiedUser, id: u64) -> Result<OnComment<Editor>, String> {
    let on = store.comment(user, id).map(|c| named(&c.anchor)).map_err(|e| e.to_string())?;
    let on: Vec<&str> = on.iter().map(String::as_str).collect();
    match on.is_empty() {
        true => user.reach_filter().map(|_| ())?,
        false => user.require(&on, store)?,
    }
    Ok(OnComment::new(crate::activation::comment_permits(store, user, &on)))
}

/// `comments.resolve` (`open: false`) and `comments.reopen` (`open: true`).
pub(super) async fn settle(store: Arc<Store>, user: VerifiedUser, id: u64, open: bool) -> CommentsEvent {
    let on = match editors_of(&store, &user, id) {
        Ok(on) => on,
        Err(message) => return CommentsEvent::Error { message },
    };
    answer(match open {
        true => store.reopen_comment(on, id).await,
        false => store.resolve_comment(on, id).await,
    })
}

/// `comments.list`: ACCESS-10 — the comments in reach, judged as the page
/// is cut (a short page never stands for a hidden one).
pub(super) fn list_stream(store: Arc<Store>, user: VerifiedUser, q: CommentQuery, after: Option<u64>, limit: Option<u32>) -> impl Stream<Item = CommentsEvent> + Send + 'static {
    stream! {
        if let Err(message) = user.read_filter() {
            yield CommentsEvent::Error { message };
            return;
        }
        let limit = crate::limits::page(limit, crate::limits::COMMENTS_PAGE_DEFAULT, crate::limits::COMMENTS_PAGE_MAX);
        match store.comments(&user, &q, after, Some(limit)) {
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

/// `comments.watch`: ACCESS-12 — the caller followed (verified again when
/// the grants generation moves, a sign-out or the expiry comes), each
/// change relayed only when the caller sees its comment as it stands then.
pub(super) fn watch_stream(access: Access, store: Arc<Store>, user: VerifiedUser) -> impl Stream<Item = CommentsEvent> + Send + 'static {
    let mut rx = store.subscribe_comments();
    let opened = user.read_filter().and_then(|_| Following::of(&access, &store, user));
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
            // The caller as it stands, before anything is relayed: refused,
            // the watch ends saying why.
            if let Err(closed) = following.again() {
                yield CommentsEvent::WatchClosed { reason: closed.reason, message: closed.message };
                return;
            }
            let Some(next) = next else { continue };
            match next {
                Ok(CommentChange::Added(c) | CommentChange::Resolved(c) | CommentChange::Reopened(c))
                    if !store.sees_comment(following.user(), &c) => {}
                Ok(CommentChange::Added(comment)) => yield CommentsEvent::Added { comment },
                Ok(CommentChange::Resolved(comment)) => yield CommentsEvent::Resolved { comment },
                Ok(CommentChange::Reopened(comment)) => yield CommentsEvent::Reopened { comment },
                Err(tokio::sync::broadcast::error::RecvError::Lagged(missed)) => yield CommentsEvent::Lagged { missed },
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
    }
}
