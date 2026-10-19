//! COMMENT-8: **a reply** — a comment that answers another, kept as a
//! comment of its own with `reply_to` naming its thread's root.
//!
//! Owner, 2026-10-18: the builder answers a comment from synapse and the
//! owner sees the answer under it. One write ([`Store::reply_comment_as`]),
//! all or none: the reply takes its root's anchor whole — so it is on the
//! same ticket and page, and seen by exactly who sees the root — and a
//! reply to a resolved root reopens it, in the same transaction. A reply
//! to a reply is a reply to the same root: threads are one level deep.
//! Committed, put into the cache, then broadcast: `comments.watch` hears
//! `added` (the reply, `reply_to` set) and, when it moved, `reopened`.

use super::{comments, CommentChange, OnComment, Store, StoreError};
use crate::access::Viewer;
use crate::comments::{Comment, CommentIntent, CommentState};
use crate::vocab::CommentId;
use std::sync::Arc;

impl Store {
    /// COMMENT-8: keeps `body` as a reply to the comment `to` (signed by
    /// `author` and `principal`, as `comments.add` signs) — on `to`'s root
    /// when `to` is itself a reply, its anchor that root's — and reopens the
    /// root when it is resolved, in one write. Answers the reply. Refused,
    /// with nothing written: an id naming no comment, a body `add` would
    /// refuse, a permit that is not for the root's ticket.
    pub async fn reply_comment_as(
        &self,
        at: impl Into<OnComment<Viewer>>,
        to: u64,
        body: String,
        author: Option<String>,
        principal: Option<String>,
    ) -> Result<Comment, StoreError> {
        let at = at.into();
        let serial = self.serial().await;
        let (reply, reopened, on, under) = {
            let cache = self.tickets.read().expect("store lock poisoned");
            let missing = || StoreError::CommentNotFound { id: to, newest: cache.comments.newest() };
            let first = cache.comments.get(to).ok_or_else(missing)?;
            let root = match first.reply_to {
                Some(r) => cache.comments.get(r.get()).ok_or_else(missing)?,
                None => first,
            };
            at.check(&cache, root.anchor.ticket.as_deref())?;
            let (anchor, author) = crate::comments::check(root.anchor.clone(), &body, author.as_deref()).map_err(StoreError::CommentRefused)?;
            let reply = Comment {
                id: CommentId::new(cache.comments.next_id()),
                state: CommentState::Open,
                author,
                at: chrono::Utc::now().timestamp(),
                // The root's, version included: what the thread is about.
                anchor: crate::comments::Anchor { version: root.anchor.version, ..anchor },
                body,
                principal,
                intent: CommentIntent::Comment,
                reply_to: Some(root.id),
            };
            let reopened = (root.state == CommentState::Resolved).then(|| Comment { state: CommentState::Open, ..root.clone() });
            // FILTER-30: a reply on a ticket touches it, as a comment does.
            let on = reply.anchor.ticket.as_deref().and_then(|t| cache.at.get(t)).cloned();
            let under = on.as_ref().and_then(|k| self.touch_before(&cache, &[(&k.shortcode, k.seq)]));
            (reply, reopened, on, under)
        };
        let (stored, kept) = ((reply.clone(), reopened.clone()), (reply.clone(), reopened.clone()));
        let tickets = Arc::clone(&self.tickets);
        self.write(
            serial,
            move |tx| {
                comments::store(tx, &stored.0)?;
                if let Some(root) = &stored.1 {
                    comments::store(tx, root)?;
                }
                Ok(())
            },
            move |cache, _| {
                if let Some(k) = &on {
                    cache.log.touch(&k.shortcode, k.seq, kept.0.at);
                }
                cache.comments.put(kept.0);
                if let Some(root) = kept.1 {
                    cache.comments.put(root);
                }
            },
            move |(), (), tx| {
                tx.comment(CommentChange::Added(reply.clone()));
                if let Some(root) = reopened {
                    tx.comment(CommentChange::Reopened(root));
                }
                if let Some(c) = under.and_then(|u| u.after(&tickets)) {
                    tx.send(c);
                }
                reply
            },
        )
        .await
    }
}
