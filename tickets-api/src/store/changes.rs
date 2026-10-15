//! APP-23: **a change request** — the owner hands a queued ticket back to
//! whoever queued it, with a note saying what to change.
//!
//! Owner, 2026-10-15: *"the queue should give me the option to request
//! changes to the underlying ticket."* One write ([`Store::request_changes_by`]),
//! all or none: a comment on the ticket's conversation (APP-22's anchor —
//! `route "#/ticket/<ID>"`, `path "conversation"`) with intent `changes`,
//! and the ticket's queue entry taken out with action `changes_requested`,
//! its record naming the comment. Committed in one transaction, put into
//! the cache together, then broadcast: `comments.watch` hears `added`,
//! `tickets.watch` hears `unqueued`. The named input is not touched:
//! requesting changes is not an answer. A ticket that is not queued is
//! refused — there is no one to hand it back to (a comment is the way to
//! say something about it).

use super::history::Act;
use super::queue::{record_of, store as store_queue};
use super::{comments, lineage, QueueChange, Store, StoreError};
use crate::access::{Editor, Permit};
use crate::comments::{Anchor, Comment, CommentIntent, CommentState};
use crate::queue::QueueAction;
use std::sync::Arc;

/// The anchor of a comment on `ticket` as a whole — APP-22's Conversation.
pub fn conversation_anchor(ticket: &str) -> Anchor {
    Anchor {
        route: format!("#/ticket/{ticket}"),
        path: CONVERSATION_PATH.to_string(),
        label: "Conversation".to_string(),
        excerpt: String::new(),
        ticket: Some(ticket.to_string()),
        version: None,
    }
}

/// APP-22: the anchor path of a comment on a ticket as a whole.
pub const CONVERSATION_PATH: &str = "conversation";

impl Store {
    /// APP-23: requests changes on the queued ticket `id` — `body` kept as a
    /// comment on its conversation with intent `changes` (signed by `author`
    /// and `principal`, as `comments.add` signs), and its queue entry taken
    /// out (`changes_requested`, the record naming the comment), in one
    /// write. Answers the comment and the queue change. Refused, with
    /// nothing written: a ticket that is none, one not queued, a body
    /// `comments.add` would refuse.
    pub async fn request_changes_by(
        &self,
        on: Permit<Editor>,
        body: String,
        author: Option<String>,
        principal: Option<String>,
        act: Act,
    ) -> Result<(Comment, QueueChange), StoreError> {
        // ACCESS-11: of the ticket `on` is for.
        let id = self.resolve(on.id())?;
        let (mut anchor, author) =
            crate::comments::check(conversation_anchor(&id), &body, author.as_deref()).map_err(StoreError::CommentRefused)?;
        let serial = self.serial().await;
        let now = chrono::Utc::now().timestamp();
        let (comment, entry, record, key, under) = {
            let cache = self.tickets.read().expect("store lock poisoned");
            let row = cache.get(&id).ok_or_else(|| StoreError::NotFound(id.clone()))?;
            let entry = cache.queue.get(&id).cloned().ok_or_else(|| {
                StoreError::Invalid(format!(
                    "{id} is not queued — a change request hands a queued ticket back to whoever queued it; comment on it instead"
                ))
            })?;
            anchor.version = Some(row.updated_unix);
            let comment = Comment {
                id: crate::vocab::CommentId::new(cache.comments.next_id()),
                state: CommentState::Open,
                author,
                at: now,
                anchor,
                body,
                principal,
                intent: CommentIntent::Changes,
            };
            let mut record = record_of(cache.queue.next_id(), &entry, QueueAction::ChangesRequested, &act, now);
            record.comment = Some(comment.id.get());
            let key = (row.shortcode.clone(), row.seq);
            let under = lineage::touch_or_queued_before_with(&self.unders, &cache, &[(&key.0, key.1)]);
            (comment, entry, record, key, under)
        };
        let change = QueueChange::Unqueued { id: id.clone(), entry, action: QueueAction::ChangesRequested, by: act.actor, at_unix: now };
        let (stored, kept) = ((comment.clone(), record.clone()), (comment.clone(), record));
        let tickets = Arc::clone(&self.tickets);
        let shortcode = key.0.clone();
        self.write(
            serial,
            move |tx| {
                comments::store(tx, &stored.0)?;
                store_queue(tx, &stored.1.ticket, None, &stored.1)?;
                Ok(())
            },
            move |cache, _| {
                cache.log.touch(&key.0, key.1, kept.0.at);
                let ticket = kept.1.ticket.clone();
                cache.queue.put(&ticket, None, kept.1);
                cache.comments.put(kept.0);
            },
            move |(), (), tx| {
                tx.comment(super::CommentChange::Added(comment.clone()));
                tx.send(super::Change::Queue { shortcode, change: change.clone() });
                if let Some(c) = under.and_then(|u| u.after(&tickets)) {
                    tx.send(c);
                }
                (comment, change)
            },
        )
        .await
    }
}
