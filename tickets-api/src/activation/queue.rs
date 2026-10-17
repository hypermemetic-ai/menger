//! MENGER-39: the queue's methods as their streams — `queue`, `unqueue`,
//! `snooze`, `queued` — and `history` (MENGER-13's, moved here with the
//! queue's lines added after its transitions). The caller, its act and its
//! reach, then the store's answer as wire events. Kept beside the activation
//! so its file keeps only its methods (MENGER-29).

use super::*;
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
fn refused(store: &Store, caller: &VerifiedUser, id: String, e: StoreError) -> TicketsEvent {
    match e {
        StoreError::NotFound(_) => TicketsEvent::NotFound { id: Asked::stored(id) },
        // ORG-3: a name that fits several, each by its full address.
        e @ StoreError::Ambiguous { .. } => refusal(e),
        e => said(store, caller, e),
    }
}

/// What a queue write asks: `queue`, `unqueue` or `snooze`.
pub(super) enum Ask {
    Queue { urgency: Urgency, reason: Reason, input: Option<InputId> },
    Unqueue,
    Snooze { minutes: u32 },
}

/// MENGER-39: one queue write — the caller (the gate held the grant on
/// `id`), its act, the store's write; exactly one event.
pub(super) fn queue_stream(
    store: Arc<Store>,
    caller: VerifiedUser,
    id: String,
    ask: Ask,
    source: Option<CallerSource>,
) -> impl Stream<Item = TicketsEvent> + Send + 'static {
    stream! {
        // ACCESS-11: the editor's permit for `id`; the store writes it.
        let on = match permit::<crate::access::Editor>(&store, &caller, &id) {
            Ok(p) => p,
            Err(ev) => {
                yield ev;
                return;
            }
        };
        let act = match act_of(&caller, source, None) {
            Ok(a) => a,
            Err(message) => {
                yield TicketsEvent::Error { message };
                return;
            }
        };
        let answer = match ask {
            Ask::Queue { urgency, reason, input } => store.queue_by(on, urgency, &reason, input.as_deref(), act).await.map(wire),
            Ask::Unqueue => store
                .unqueue_by(on, act)
                .await
                .map(|c| c.map_or_else(|| TicketsEvent::NotQueued { id: id.trim().to_ascii_uppercase() }, wire)),
            Ask::Snooze { minutes } => store.snooze_by(on, minutes, act).await.map(wire),
        };
        match answer {
            Ok(ev) => yield ev,
            Err(e) => yield refused(&store, &caller, id, e),
        }
    }
}

/// APP-23: `request_changes` — the caller (the gate held the grant on `id`),
/// its act, the store's one write; exactly one event. The comment is signed
/// as `comments.add` signs: the caller's name, else `owner`.
pub(super) fn request_changes_stream(
    store: Arc<Store>,
    caller: VerifiedUser,
    id: String,
    body: String,
    source: Option<CallerSource>,
) -> impl Stream<Item = TicketsEvent> + Send + 'static {
    stream! {
        // ACCESS-11: the editor's permit for `id`; the store writes it.
        let on = match permit::<crate::access::Editor>(&store, &caller, &id) {
            Ok(p) => p,
            Err(ev) => {
                yield ev;
                return;
            }
        };
        let act = match act_of(&caller, source, None) {
            Ok(a) => a,
            Err(message) => {
                yield TicketsEvent::Error { message };
                return;
            }
        };
        match store.request_changes_by(on, body, caller.name().map(str::to_string), Some(caller.principal_text()), act).await {
            Ok((comment, QueueChange::Unqueued { id, entry, by, at_unix, .. })) => yield TicketsEvent::ChangesRequested { id, comment, entry, by, at_unix },
            Ok((_, other)) => yield wire(other),
            Err(e) => yield refused(&store, &caller, id, e),
        }
    }
}

/// MENGER-39: `queued` — the queue in the caller's reach, most urgent
/// first, then the oldest; a `queue_row` each, then one `queue_end`.
pub(super) fn queued_stream(store: Arc<Store>, caller: VerifiedUser, snoozed: bool) -> impl Stream<Item = TicketsEvent> + Send + 'static {
    stream! {
        if let Err(message) = caller.read_filter() {
            yield TicketsEvent::Error { message };
            return;
        }
        let now = chrono::Utc::now().timestamp();
        let rows = store.queued(&caller, snoozed, now);
        let (count, asleep) = (rows.len() as u64, rows.iter().filter(|(e, _)| e.snoozed_at(now)).count() as u64);
        for (entry, ticket) in rows {
            yield TicketsEvent::QueueRow { entry, ticket };
        }
        yield TicketsEvent::QueueEnd { count, snoozed: asleep, now_unix: now };
    }
}
