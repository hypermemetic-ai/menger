//! APP-24: `await_reply` as its stream — the caller and its reach, the two
//! channels subscribed, then `waiting`, then the store asked
//! ([`Store::reply_since`]) at once and again at every change heard, until
//! the first answer (`replied`), the timeout (`timed_out`) or the caller's
//! access ends. Kept beside the activation so its file keeps only its
//! methods (MENGER-29).
//!
//! **No gap, no duplicate.** The channels are subscribed at the call, before
//! the first ask: a write the cache held by then is in its answer; one
//! applied after is heard, and the ask after it finds it. Without `since`,
//! what was already there (the call's own second) is set aside first. The answer is a function of
//! what is kept, so a write seen twice answers once — and the stream ends
//! at its first `replied`.

use super::*;
use crate::store::{reply_key, Awaiting};
use std::collections::HashSet;
use tokio::sync::broadcast::error::RecvError;

/// What a call asks: the ticket, since when, which input, how long.
pub(super) struct Ask {
    pub(super) id: String,
    pub(super) since: Option<i64>,
    pub(super) input: Option<String>,
    pub(super) timeout_minutes: Option<u32>,
}

/// Why the wait woke.
enum Woke {
    Changed,
    Closed,
    Timeout,
}

/// APP-24: the stream. Everything before the first ask is done at the
/// call: the caller admitted, the ticket found, what is already there set
/// aside (without `since`), then the channels subscribed.
pub(super) fn await_reply_stream(
    store: Arc<Store>,
    access: Access,
    who: Option<AuthContext>,
    reach: Reach,
    ask: Ask,
) -> impl Stream<Item = TicketsEvent> + Send + 'static {
    let started = chrono::Utc::now().timestamp();
    let opened = access.verify_call(who.as_ref(), &reach).and_then(|c| view_lens(&c).map(|l| (c, l)));
    let prepared = opened.map(|(caller, lens)| {
        // The ticket as kept — or a deleted one's id.
        let held = store.ticket_id(&ask.id);
        let ticket = match held.clone().or_else(|| store.tombstone(&ask.id).map(|t| t.id)) {
            Some(t) if held.is_none() || sees_id(&store, lens.as_ref(), &t) => t,
            _ => return None,
        };
        let w = Awaiting {
            ticket,
            since: ask.since.unwrap_or(started),
            input: ask.input.clone().map(|i| i.trim().to_string()).filter(|i| !i.is_empty()),
            me: caller.principal_text(),
        };
        let before: HashSet<String> = match ask.since {
            Some(_) => HashSet::new(),
            None => store.replies_since(&w, |id| sees_id(&store, lens.as_ref(), id)).iter().map(reply_key).collect(),
        };
        Some((w, lens, before))
    });
    // Subscribed before the first ask.
    let mut tickets = store.subscribe();
    let mut comments = store.subscribe_comments();
    stream! {
        let (w, lens, before) = match prepared {
            Ok(Some(p)) => p,
            Ok(None) => {
                yield TicketsEvent::NotFound { id: ask.id.trim().to_ascii_uppercase() };
                return;
            }
            Err(message) => {
                yield TicketsEvent::Error { message };
                return;
            }
        };
        let ticket = w.ticket.clone();
        yield TicketsEvent::Waiting { ticket: ticket.clone(), queued: store.queue_entry(&ticket) };
        let deadline = ask.timeout_minutes.map(|m| tokio::time::Instant::now() + std::time::Duration::from_secs(u64::from(m) * 60));
        let sleep = tokio::time::sleep_until(deadline.unwrap_or_else(|| tokio::time::Instant::now() + std::time::Duration::from_secs(10 * 365 * 24 * 3600)));
        tokio::pin!(sleep);
        loop {
            let sees = |id: &str| sees_id(&store, lens.as_ref(), id);
            if let Some(reply) = store.replies_since(&w, sees).into_iter().find(|r| !before.contains(&reply_key(r))) {
                yield TicketsEvent::Replied { ticket: ticket.clone(), reply };
                return;
            }
            // Wait for a change, either channel; a lag asks again too.
            let woke = tokio::select! {
                r = tickets.recv() => if matches!(r, Err(RecvError::Closed)) { Woke::Closed } else { Woke::Changed },
                r = comments.recv() => if matches!(r, Err(RecvError::Closed)) { Woke::Closed } else { Woke::Changed },
                () = &mut sleep, if deadline.is_some() => Woke::Timeout,
            };
            match woke {
                Woke::Changed => {}
                Woke::Closed => return,
                Woke::Timeout => {
                    yield TicketsEvent::TimedOut { ticket: ticket.clone() };
                    return;
                }
            }
            // AUTH-7: the gate's live reach — held nowhere any more, it ends.
            if reach.current().is_none() {
                yield TicketsEvent::Error { message: "your access changed: you hold this nowhere any more".into() };
                return;
            }
        }
    }
}
