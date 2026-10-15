//! Two reads as their streams, beside the activation so its file keeps only
//! its methods (MENGER-29): `blockers`, and `history` — MENGER-13's, with
//! MENGER-39's queue records after its transitions.

use super::*;

/// `blockers` — the ticket's blockers in its reach, each resolved or
/// dangling.
pub(super) fn blockers_stream(store: Arc<Store>, admitted: Result<Caller, String>, id: String) -> impl Stream<Item = TicketsEvent> + Send + 'static {
    stream! {
        let caller = match admitted {
            Ok(caller) => caller,
            Err(message) => {
                yield TicketsEvent::Error { message };
                return;
            }
        };
        if caller.require(&[id.as_str()], &store).is_err() {
            yield TicketsEvent::NotFound { id };
            return;
        }
        let reach = match view_lens(&caller) {
            Ok(r) => r,
            Err(message) => {
                yield TicketsEvent::Error { message };
                return;
            }
        };
        match store.blockers(&id).await {
            Ok(refs) => {
                for r in refs {
                    yield match r {
                        BlockerRef::Resolved(ticket) if sees_id(&store, reach.as_ref(), &ticket.id) => TicketsEvent::Blocker { ticket },
                        BlockerRef::Resolved(ticket) => TicketsEvent::DanglingBlocker { id: ticket.id },
                        BlockerRef::Dangling(id) => TicketsEvent::DanglingBlocker { id },
                    };
                }
            }
            Err(StoreError::NotFound(_)) => yield TicketsEvent::NotFound { id },
            Err(e) => yield TicketsEvent::Error { message: e.to_string() },
        }
    }
}

/// MENGER-13: `history` — the ticket's transitions, those they caused where
/// the caller can see, MENGER-39: its queue history, then `history_end`.
pub(super) fn history_stream(store: Arc<Store>, admitted: Result<Caller, String>, id: String) -> impl Stream<Item = TicketsEvent> + Send + 'static {
    stream! {
        let caller = match admitted {
            Ok(caller) => caller,
            Err(message) => {
                yield TicketsEvent::Error { message };
                return;
            }
        };
        if caller.require(&[id.as_str()], &store).is_err() {
            yield TicketsEvent::NotFound { id };
            return;
        }
        let reach = match view_lens(&caller) {
            Ok(r) => r,
            Err(message) => {
                yield TicketsEvent::Error { message };
                return;
            }
        };
        match store.history(&id) {
            Ok(h) => {
                for transition in h.own {
                    yield TicketsEvent::Transition { transition };
                }
                // IDENTITY-21: what it caused, where the caller can see.
                for transition in h.caused.into_iter().filter(|t| sees_id(&store, reach.as_ref(), &t.ticket)) {
                    yield TicketsEvent::Transition { transition };
                }
                // MENGER-39: who queued it, why, at what urgency.
                for record in store.queue_history(&id) {
                    yield TicketsEvent::QueueRecord { record };
                }
                let restorable = h.restorable.into_iter().filter(|t| sees_id(&store, reach.as_ref(), t)).collect();
                let (status_since_unix, status_since_exact) = (h.since.map(|s| s.0), h.since.map(|s| s.1));
                yield TicketsEvent::HistoryEnd { began_unix: h.began, restorable, status_since_unix, status_since_exact, deletions: h.deletions };
            }
            Err(StoreError::NotFound(_)) => yield TicketsEvent::NotFound { id },
            Err(e) => yield TicketsEvent::Error { message: e.to_string() },
        }
    }
}
