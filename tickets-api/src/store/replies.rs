//! APP-24: **what answers an agent waiting on a ticket** — the read behind
//! `tickets.await_reply`.
//!
//! Owner, 2026-10-15: *"when queued an agent should then listen to that
//! ticket for a response."* A response on the ticket is, since a moment:
//! an input **answered** (or, with one named, that input); a **change
//! request** (APP-23); the ticket **unqueued** by someone other than the
//! caller (a snooze, a queueing, a raise are not); the ticket **canceled**,
//! **archived** or **deleted**. [`Store::reply_since`] answers the first of
//! them — the earliest, then in that order — from the cache alone: the
//! inputs' histories, the queue's log, the transition log and the tombstones.
//!
//! It is a function of what is kept, not of what was heard, so the waiting
//! is gapless and answers once: the activation subscribes, then asks; each change heard (or a `lagged`) asks again; the first answer
//! ends the stream. A write applied before the first ask is in it; one
//! applied after is heard. Moments are seconds: without `since`, what was
//! already there when the call came (in its own second) is set aside by
//! [`reply_key`] — taken before the subscription, so nothing after the call
//! is. "Someone other than the caller" is told by
//! principal.

use super::{shortcodes, Reader, Seen, Store};
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
    /// The caller's principal: its own unqueues are not responses.
    pub me: Option<String>,
}

/// APP-24: what makes a response the same one when it is found again — an
/// answer by its input, value and moment, a change request or an unqueue by
/// its who and when, an ending by how.
pub fn reply_key(r: &Reply) -> String {
    match r {
        Reply::Answered { input, value, at_unix, .. } => format!("answered {input} {at_unix} {}", serde_json::to_string(value).unwrap_or_default()),
        Reply::ChangesRequested { by, at_unix, .. } => format!("changes {by} {at_unix}"),
        Reply::Unqueued { action, by, at_unix } => format!("unqueued {} {by} {at_unix}", action.as_str()),
        Reply::Ended { how, .. } => format!("ended {how:?}"),
        Reply::Archived { at_unix, .. } => format!("archived {at_unix}"),
    }
}

impl Store {
    /// APP-24: the first response on `w.ticket` at or after `w.since`, or
    /// `None` while there is none.
    pub fn reply_since(&self, who: &(impl Reader + ?Sized), w: &Awaiting) -> Option<Reply> {
        self.replies_since(who, w).into_iter().next()
    }

    /// APP-24: every response on `w.ticket` at or after `w.since`, the
    /// earliest first — at one moment, in the table's order (an answer, a
    /// change request, an unqueue, a cancel, a delete).
    pub fn replies_since(&self, who: &(impl Reader + ?Sized), w: &Awaiting) -> Vec<Reply> {
        // ACCESS-10: a live ticket out of `who`'s reach says nothing; a
        // deleted one was admitted by its tombstone at the call.
        if let Ok(seen) = Seen::of(who) {
            let cache = self.tickets.read().expect("store lock poisoned");
            if cache.get(&w.ticket).is_some() && !seen.sees(&cache).id(&w.ticket) {
                return Vec::new();
            }
        } else {
            return Vec::new();
        }
        // Its transitions, asked first (`history` takes the lock itself).
        let transitions = self.history_unscoped(&w.ticket).map(|h| h.own).unwrap_or_default();
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
        // The queue: a change request, or taken out by another.
        for r in cache.queue.records(id).iter().filter(|r| r.at_unix >= w.since) {
            match r.action {
                QueueAction::ChangesRequested => found.push(Reply::ChangesRequested { body: r.reason.clone().unwrap_or_default(), by: r.actor.clone(), at_unix: r.at_unix }),
                QueueAction::Unqueued | QueueAction::Closed if !mine(&r.actor) => found.push(Reply::Unqueued { action: r.action, by: r.actor.clone(), at_unix: r.at_unix }),
                _ => {}
            }
        }
        // Canceled.
        if let Some(t) = transitions.iter().cloned().find(|t| t.field == TransitionField::Status && t.to == "Canceled" && t.at_unix >= w.since) {
            found.push(Reply::Ended { how: Ending::Canceled, by: t.actor, at_unix: t.at_unix });
        }
        // ARCHIVE-2: archived.
        if let Some(t) = transitions.iter().find(|t| t.field == TransitionField::Archive && t.to == crate::types::ARCHIVE_ARCHIVED && t.at_unix >= w.since) {
            found.push(Reply::Archived { by: t.actor.clone(), at_unix: t.at_unix });
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
