//! APP-24: **what answers an agent waiting on a ticket** — the read behind
//! `tickets.await_reply`.
//!
//! Owner, 2026-10-15: *"when queued an agent should then listen to that
//! ticket for a response."* A response on the ticket is, since a moment:
//! an input **answered** (or, with one named, that input); a **change
//! request** (APP-23: a comment with intent `changes`); a **comment** by
//! someone other than the caller; the ticket **unqueued** by someone other
//! than the caller (a snooze, a queueing, a raise are not); the ticket
//! **canceled** or **deleted**. [`Store::reply_since`] answers the first of
//! them — the earliest, then in that order — from the cache alone: the
//! inputs' histories, the comments, the queue's log, the transition log and
//! the tombstones.
//!
//! It is a function of what is kept, not of what was heard, so the waiting
//! is gapless and answers once: the activation subscribes to both channels,
//! then asks; each change heard (or a `lagged`) asks again; the first answer
//! ends the stream. A write applied before the first ask is in it; one
//! applied after is heard. Moments are seconds: without `since`, what was
//! already there when the call came (in its own second) is set aside by
//! [`reply_key`] — taken before the subscription, so nothing after the call
//! is. "Someone other than the caller" is told by
//! principal: with login off every caller is the same `anonymous`, and
//! nothing is set aside.

use super::{shortcodes, Store};
use crate::comments::{Comment, CommentIntent};
use crate::queue::{Ending, QueueAction, Reply};
use crate::types::{InputAction, TransitionField};

/// What a wait asks of [`Store::reply_since`].
#[derive(Debug, Clone, Default)]
pub struct Awaiting {
    /// The ticket, by its key (or a deleted one's id).
    pub ticket: String,
    /// Responses at or after this moment, unix seconds.
    pub since: i64,
    /// Only this input's answer is an `answered` (the rest still count).
    pub input: Option<String>,
    /// The caller's principal: its own comments and unqueues are not
    /// responses. `None` (login off): nothing is set aside.
    pub me: Option<String>,
}

/// APP-24: what makes a response the same one when it is found again — a
/// comment by its id (its state may move), an answer by its input, value and
/// moment, an unqueue by its action, who and when, an ending by how.
pub fn reply_key(r: &Reply) -> String {
    match r {
        Reply::Answered { input, value, at_unix, .. } => format!("answered {input} {at_unix} {}", serde_json::to_string(value).unwrap_or_default()),
        Reply::ChangesRequested { comment } | Reply::Commented { comment } => format!("comment {}", comment.id),
        Reply::Unqueued { action, by, at_unix } => format!("unqueued {} {by} {at_unix}", action.as_str()),
        Reply::Ended { how, .. } => format!("ended {how:?}"),
    }
}

/// Whether the comment `c` is on `ticket` — its anchor's ticket, or the
/// ticket whose page it was left on (IDENTITY-37).
pub fn comment_on(c: &Comment, ticket: &str) -> bool {
    let paged = c.anchor.route.trim().strip_prefix("#/ticket/").map(|r| r.split(['?', '#']).next().unwrap_or(r).trim());
    c.anchor.ticket.as_deref().is_some_and(|t| t.eq_ignore_ascii_case(ticket)) || paged.is_some_and(|p| p.eq_ignore_ascii_case(ticket))
}

impl Store {
    /// APP-24: the first response on `w.ticket` at or after `w.since`, or
    /// `None` while there is none. `sees` says which tickets the caller may
    /// see: a comment naming one it may not is not heard.
    pub fn reply_since(&self, w: &Awaiting, sees: impl Fn(&str) -> bool) -> Option<Reply> {
        self.replies_since(w, sees).into_iter().next()
    }

    /// APP-24: every response on `w.ticket` at or after `w.since`, the
    /// earliest first — at one moment, in the table's order (an answer, a
    /// change request, a comment, an unqueue, a cancel, a delete).
    pub fn replies_since(&self, w: &Awaiting, sees: impl Fn(&str) -> bool) -> Vec<Reply> {
        // Its transitions, asked first (`history` takes the lock itself).
        let transitions = self.history(&w.ticket).map(|h| h.own).unwrap_or_default();
        let cache = self.tickets.read().expect("store lock poisoned");
        let id = w.ticket.as_str();
        let mine = |who: &str| w.me.as_deref().is_some_and(|me| me == who);
        let mut found: Vec<Reply> = Vec::new();
        // Answers.
        for input in cache.inputs.of(id) {
            if w.input.as_deref().is_some_and(|only| only != input.id.as_str()) {
                continue;
            }
            if let Some(h) = input.history.iter().find(|h| h.action == InputAction::Answered && h.at_unix >= w.since) {
                if let Some(value) = h.value.clone() {
                    found.push(Reply::Answered { input: input.id.to_string(), value, rationale: h.rationale.clone(), source: h.source, at_unix: h.at_unix });
                }
            }
        }
        // Comments: a change request, or another's comment.
        for c in cache.comments.since(w.since) {
            if !comment_on(c, id) {
                continue;
            }
            let named: Vec<&str> = c.anchor.ticket.iter().map(String::as_str).collect();
            if !named.iter().all(|t| sees(t)) {
                continue;
            }
            match c.intent {
                CommentIntent::Changes => found.push(Reply::ChangesRequested { comment: c.clone() }),
                CommentIntent::Comment if !c.principal.as_deref().is_some_and(mine) => found.push(Reply::Commented { comment: c.clone() }),
                CommentIntent::Comment => {}
            }
        }
        // The queue: taken out by another (a change request is its comment's).
        for r in cache.queue.records(id).iter().filter(|r| r.at_unix >= w.since) {
            if matches!(r.action, QueueAction::Unqueued | QueueAction::Closed) && !mine(&r.actor) {
                found.push(Reply::Unqueued { action: r.action, by: r.actor.clone(), at_unix: r.at_unix });
            }
        }
        // Canceled.
        if let Some(t) = transitions.into_iter().find(|t| t.field == TransitionField::Status && t.to == "Canceled" && t.at_unix >= w.since) {
            found.push(Reply::Ended { how: Ending::Canceled, by: t.actor, at_unix: t.at_unix });
        }
        // Deleted.
        let key = match cache.find_ticket(id, shortcodes::OrgReach::All) {
            shortcodes::Found::One(key) => key,
            _ => crate::address::folded(id),
        };
        if let Some(t) = cache.tombstones.get(&key).filter(|t| t.at_unix >= w.since) {
            found.push(Reply::Ended { how: Ending::Deleted, by: t.by.clone(), at_unix: t.at_unix });
        }
        // The earliest first; at one moment, in the order above.
        let mut found: Vec<(usize, Reply)> = found.into_iter().enumerate().collect();
        found.sort_by_key(|(i, r)| (r.at_unix(), *i));
        found.into_iter().map(|(_, r)| r).collect()
    }
}
