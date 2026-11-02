//! V2-31: **a write held against the author's allowance** — the quota
//! service's meter, asked through the seam before a write lands
//! (`quota.reserve`), the hold committed with what was written or released
//! when nothing was. A refusal is the write's one event, typed
//! (`over_allowance`, `box_full`), and nothing is written.
//!
//! What a ticket's write counts: `create` one `tickets.count` and its
//! body's bytes as `tickets.bytes`; `amend` and `update` a new body's bytes
//! (each kept as a revision), counted only when the body changed.

use super::*;
use menger_quota::{Hold, Meter, Refusal};

/// The holds a write took, to settle after it.
pub(super) struct Held {
    meter: Arc<dyn Meter>,
    holds: Vec<(&'static str, Hold)>,
}

/// The refusal as the write's one event.
pub(super) fn refused(r: Refusal) -> TicketsEvent {
    let message = format!("{r} — nothing was written");
    match r {
        Refusal::OverAllowance { source, used, cap, asked } => TicketsEvent::OverAllowance { source, used, cap, asked, message },
        Refusal::BoxFull { used_percent, full_percent } => TicketsEvent::BoxFull { used_percent, full_percent, message },
        _ => TicketsEvent::Error { message },
    }
}

/// Reserves each `(source, amount)` for `user` (an amount of nothing is not
/// asked), all or none; refused, the write's one event.
pub(super) async fn hold(access: &Access, user: &VerifiedUser, asks: &[(&'static str, u64)]) -> Result<Held, Failed> {
    let asks: Vec<(&'static str, u64)> = asks.iter().copied().filter(|(_, n)| *n > 0).collect();
    let slot = access.meter();
    let pairs: Vec<(&str, u64)> = asks.iter().map(|(s, n)| (*s, *n)).collect();
    match slot.reserve_all(&user.principal_text(), &pairs).await {
        Ok(holds) => Ok(Held { meter: slot.get(), holds: asks.iter().map(|(s, _)| *s).zip(holds).collect() }),
        Err(r) => Err(Failed::Said(refused(r))),
    }
}

impl Held {
    /// The write's outcome settles the holds: landed, each committed with
    /// what `amount` says of it (none: all it held); refused, each released.
    pub(super) async fn settle<T>(self, r: Result<T, StoreError>, amount: impl Fn(&T, &str) -> Option<u64>) -> Result<T, StoreError> {
        match &r {
            Ok(t) => {
                for (source, h) in self.holds {
                    self.meter.commit(h, amount(t, source)).await;
                }
            }
            Err(_) => {
                for (_, h) in self.holds {
                    self.meter.release(h).await;
                }
            }
        }
        r
    }
}

/// `create`'s write: one `tickets.count` and the body's bytes held, then the
/// birth, the holds settled by it.
pub(super) async fn born(access: &Access, user: &VerifiedUser, store: &Store, at: crate::store::Filing, new: NewTicket, act: Act) -> Result<crate::types::Ticket, Failed> {
    let held = hold(access, user, &[("tickets.count", 1), ("tickets.bytes", new.body.len() as u64)]).await?;
    Ok(held.settle(store.create_by(at, new, act).await, |_, _| None).await?)
}

/// `create`'s parameters as the ticket to file.
pub(super) fn new_ticket(
    epic: String,
    title: Title,
    kind: Option<Kind>,
    severity: Option<Severity>,
    blocked_by: Option<Vec<TicketRef>>,
    parent: Option<TicketRef>,
    path: Option<MengerPath>,
    body: Option<Body>,
) -> NewTicket {
    let (title, kind, blocked_by) = (title.into_string(), kind.unwrap_or(Kind::Build), blocked_by.unwrap_or_default());
    let (parent, path, body) = (parent.map(String::from), path.map(String::from), body.map(String::from).unwrap_or_default());
    NewTicket { epic, title, kind, severity, blocked_by, parent, path, body }
}
