//! TICKETS-9: `assign` as its stream — the tickets (`id`, or `ids`), the
//! caller's editor permit on each, every principal to add asked whether it
//! reads each ticket ([`Access::reads_ticket`]; one that cannot is refused,
//! naming why, before anything is written), then the store's one write.
//! `update_many { assign }` asks the same of what it selects
//! ([`unreadable`]). Kept beside the activation so its file keeps only its
//! methods (MENGER-29).

use super::*;
use crate::store::Change;
use crate::vocab::Principal;

/// The refusal for the principals of `add` that cannot read the tickets
/// `keys` names — each with why — or `None` when every one reads every one.
pub(super) fn unreadable(store: &Store, access: &Access, keys: &[String], add: &[Principal]) -> Option<String> {
    let mut why: Vec<String> = Vec::new();
    for key in keys {
        for p in add.iter().filter(|p| !access.reads_ticket(p.as_str(), key, store)) {
            why.push(format!(
                "{p} cannot read {key} — an assignee reads the ticket: no grant of theirs (nor a team's) reaches it; grant them viewer there first"
            ));
        }
    }
    (!why.is_empty()).then(|| format!("{}; nothing written", why.join(" · ")))
}

/// TICKETS-9: `assign` — one ticket (the gate held the grant on it; one
/// event, `not_found` for an id that names none) or many (the caller's
/// grant on every one asked first, one missing refusing the write), every
/// principal added asked whether it reads each, then one write, all or
/// none: one `assignees_changed` per ticket, in the order named.
pub(super) fn assign_stream(
    store: Arc<Store>,
    access: Access,
    caller: VerifiedUser,
    id: Option<TicketRef>,
    ids: Option<Vec<TicketRef>>,
    add: Vec<Principal>,
    remove: Vec<Principal>,
    source: Option<CallerSource>,
    message: Option<Message>,
) -> impl Stream<Item = TicketsEvent> + Send + 'static {
    stream! {
        if add.is_empty() && remove.is_empty() {
            yield TicketsEvent::Error { message: "name who: add, remove — at least one principal".into() };
            return;
        }
        let act = match act_of(&caller, source, message) {
            Ok(a) => a,
            Err(message) => {
                yield TicketsEvent::Error { message };
                return;
            }
        };
        let on = match (id, ids) {
            (Some(id), None) => match permit::<Editor>(&store, &caller, &id) {
                Ok(p) => vec![p],
                Err(ev) => {
                    yield ev;
                    return;
                }
            },
            (None, Some(ids)) if !ids.is_empty() => match permits::<Editor>(&store, &caller, &texts(ids)) {
                Ok(on) => on,
                Err(message) => {
                    yield TicketsEvent::Error { message };
                    return;
                }
            },
            _ => {
                yield TicketsEvent::Error { message: "name the tickets: id, or ids — one of them".into() };
                return;
            }
        };
        let keys: Vec<String> = on.iter().map(|p| p.id().to_string()).collect();
        if let Some(message) = unreadable(&store, &access, &keys, &add) {
            yield TicketsEvent::Error { message };
            return;
        }
        match store.assign_by(on, add, remove, act).await {
            Ok(changes) => {
                for change in changes {
                    if let Change::AssigneesChanged { ticket, added, removed } = change {
                        yield TicketsEvent::AssigneesChanged { ticket, added, removed };
                    }
                }
            }
            Err(StoreError::NotFound(id)) => yield TicketsEvent::NotFound { id: Asked::stored(id.as_str()) },
            Err(e) => yield said(&store, &caller, e),
        }
    }
}
