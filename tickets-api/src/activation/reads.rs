//! Three reads as their streams, beside the activation so its file keeps
//! only its methods (MENGER-29): `get`, `blockers`, and `history` —
//! MENGER-13's, with MENGER-39's queue records after its transitions.

use super::*;

/// `get` — one ticket by any address, in the caller's reach: `ticket` (ACT-6:
/// with `moved_from` when an old id named it), a deleted one's tombstone
/// (ACT-5), or `not_found`.
pub(super) fn get_stream(store: Arc<Store>, admitted: Result<VerifiedUser, String>, id: String) -> impl Stream<Item = TicketsEvent> + Send + 'static {
    stream! {
        let caller = match admitted {
            Ok(caller) => caller,
            Err(message) => {
                yield TicketsEvent::Error { message };
                return;
            }
        };
        // ORG-4: a short form read in the caller's organizations.
        let named = named_in(&store, &caller, id.clone());
        // ACCESS-10: a ticket out of reach is not found, as one not there.
        match store.get(&caller, &named).await {
            Ok(ticket) => yield TicketsEvent::Ticket { ticket },
            // ACT-5: a deleted ticket answers its tombstone.
            Err(StoreError::NotFound(_)) => yield store.tombstone(&caller, &id).map_or(TicketsEvent::NotFound { id: Asked::stored(id.as_str()) }, |t| TicketsEvent::Deleted { ids: vec![t.id.clone()], tombstones: vec![t] }),
            Err(e) => yield veiled(&caller, e),
        }
    }
}

/// `blockers` — the ticket's blockers in its reach, each resolved or
/// dangling.
pub(super) fn blockers_stream(store: Arc<Store>, admitted: Result<VerifiedUser, String>, id: String) -> impl Stream<Item = TicketsEvent> + Send + 'static {
    stream! {
        let caller = match admitted {
            Ok(caller) => caller,
            Err(message) => {
                yield TicketsEvent::Error { message };
                return;
            }
        };
        // ACCESS-10: the blockers in reach, a hidden one not named.
        match store.blockers(&caller, &id).await {
            Ok(refs) => {
                for r in refs {
                    yield match r {
                        BlockerRef::Resolved(ticket) => TicketsEvent::Blocker { ticket },
                        BlockerRef::Dangling(id) => TicketsEvent::DanglingBlocker { id },
                    };
                }
            }
            Err(StoreError::NotFound(_)) => yield TicketsEvent::NotFound { id: Asked::stored(id.as_str()) },
            Err(e) => yield veiled(&caller, e),
        }
    }
}

/// MENGER-13: `history` — the ticket's transitions, those they caused where
/// the caller can see, MENGER-39: its queue history, then `history_end`.
pub(super) fn history_stream(store: Arc<Store>, admitted: Result<VerifiedUser, String>, id: String) -> impl Stream<Item = TicketsEvent> + Send + 'static {
    stream! {
        let caller = match admitted {
            Ok(caller) => caller,
            Err(message) => {
                yield TicketsEvent::Error { message };
                return;
            }
        };
        // ACCESS-10: what it caused and what a restore puts back, in reach.
        match store.history(&caller, &id) {
            Ok(h) => {
                for transition in h.own {
                    yield TicketsEvent::Transition { transition };
                }
                for transition in h.caused {
                    yield TicketsEvent::Transition { transition };
                }
                // MENGER-39: who queued it, why, at what urgency.
                for record in store.queue_history(&caller, &id) {
                    yield TicketsEvent::QueueRecord { record };
                }
                let (status_since_unix, status_since_exact) = (h.since.map(|s| s.0), h.since.map(|s| s.1));
                yield TicketsEvent::HistoryEnd { began_unix: h.began, restorable: h.restorable, status_since_unix, status_since_exact, deletions: h.deletions };
            }
            Err(StoreError::NotFound(_)) => yield TicketsEvent::NotFound { id: Asked::stored(id.as_str()) },
            Err(e) => yield veiled(&caller, e),
        }
    }
}
