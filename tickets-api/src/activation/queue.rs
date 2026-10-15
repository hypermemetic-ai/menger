//! MENGER-39: the queue's methods as their streams — `queue`, `unqueue`,
//! `snooze`, `queued` — and `history` (MENGER-13's, moved here with the
//! queue's lines added after its transitions). The caller, its act and its
//! reach, then the store's answer as wire events. Kept beside the activation
//! so its file keeps only its methods (MENGER-29).

use super::*;
use crate::queue::Urgency;
use crate::store::QueueChange;

/// A queue write's answer as its wire event.
fn wire(change: QueueChange) -> TicketsEvent {
    match change {
        QueueChange::Queued { entry, ticket, action } => TicketsEvent::Queued { entry, ticket, action },
        QueueChange::Unqueued { id, entry, action, by, at_unix } => TicketsEvent::Unqueued { id, entry, action, by, at_unix },
        QueueChange::Snoozed { entry, ticket } => TicketsEvent::Snoozed { entry, ticket },
    }
}

/// A refused write as its one event: `not_found` for a ticket the store (or
/// the caller) does not hold, else the store's words.
fn refused(store: &Store, caller: &Caller, id: String, e: StoreError) -> TicketsEvent {
    match e {
        StoreError::NotFound(_) => TicketsEvent::NotFound { id },
        e => {
            let sight = sight_lens(caller).ok().flatten();
            TicketsEvent::Error { message: redacted(e, |id| sees_id(store, sight.as_ref(), id)) }
        }
    }
}

/// What a queue write asks: `queue`, `unqueue` or `snooze`.
pub(super) enum Ask {
    Queue { urgency: String, reason: String, input: Option<String> },
    Unqueue,
    Snooze { minutes: u32 },
}

/// MENGER-39: one queue write — the caller (the gate held the grant on
/// `id`), its act, the store's write; exactly one event.
pub(super) fn queue_stream(
    store: Arc<Store>,
    admitted: Result<Caller, String>,
    id: String,
    ask: Ask,
    source: Option<String>,
) -> impl Stream<Item = TicketsEvent> + Send + 'static {
    stream! {
        let (caller, act) = match admitted.and_then(|c| act_of(&c, source.as_deref(), None).map(|a| (c, a))) {
            Ok(x) => x,
            Err(message) => {
                yield TicketsEvent::Error { message };
                return;
            }
        };
        let answer = match ask {
            Ask::Queue { urgency, reason, input } => match urgency.parse::<Urgency>() {
                Ok(u) => store.queue_by(&id, u, &reason, input.as_deref(), act).await.map(wire),
                Err(message) => {
                    yield TicketsEvent::Error { message };
                    return;
                }
            },
            Ask::Unqueue => store
                .unqueue_by(&id, act)
                .await
                .map(|c| c.map_or_else(|| TicketsEvent::NotQueued { id: id.trim().to_ascii_uppercase() }, wire)),
            Ask::Snooze { minutes } => store.snooze_by(&id, minutes, act).await.map(wire),
        };
        match answer {
            Ok(ev) => yield ev,
            Err(e) => yield refused(&store, &caller, id, e),
        }
    }
}

/// MENGER-39: `queued` — the queue in the caller's reach, most urgent
/// first, then the oldest; a `queue_row` each, then one `queue_end`.
pub(super) fn queued_stream(store: Arc<Store>, admitted: Result<Caller, String>, snoozed: bool) -> impl Stream<Item = TicketsEvent> + Send + 'static {
    stream! {
        let reach = match admitted.and_then(|c| view_lens(&c)) {
            Ok(r) => r,
            Err(message) => {
                yield TicketsEvent::Error { message };
                return;
            }
        };
        let now = chrono::Utc::now().timestamp();
        let rows = store.queued(snoozed, now, |id| sees_id(&store, reach.as_ref(), id));
        let (count, asleep) = (rows.len() as u64, rows.iter().filter(|(e, _)| e.snoozed_at(now)).count() as u64);
        for (entry, ticket) in rows {
            yield TicketsEvent::QueueRow { entry, ticket };
        }
        yield TicketsEvent::QueueEnd { count, snoozed: asleep, now_unix: now };
    }
}
