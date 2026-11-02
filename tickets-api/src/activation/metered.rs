//! V2-31: **a write held against the author's allowance** — the quota
//! service's meter, asked through the seam before a write lands
//! (`quota.reserve`), the hold committed with what was written or released
//! when nothing was. A refusal is the write's one event, typed
//! (`over_allowance`, `box_full`), and nothing is written.
//!
//! What a ticket's write counts (V2-31's follow-up): `tickets.count`, one
//! per ticket created, and `tickets.bytes` — **what the store keeps, not
//! every write** ([`Store::revision_usage`]'s rule): a body's content the
//! first time it is kept (as a revision; a copy of one kept already costs
//! nothing), and the current body of each ticket, its last writer's. A write
//! reserves the delta: a new content's bytes, plus the new body's size —
//! less the old one's when it was the writer's own, else the old body's
//! writer is credited it back. A write that leaves the body as it was
//! counts nothing.

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

/// `create`'s write: one `tickets.count` and what its body adds to what is
/// kept held, then the birth, the holds settled by it.
pub(super) async fn born(access: &Access, user: &VerifiedUser, store: &Store, at: crate::store::Filing, new: NewTicket, act: Act) -> Result<crate::types::Ticket, Failed> {
    let charge = store.body_charge(None, &new.body).await?;
    let len = new.body.len() as u64;
    let held = hold(access, user, &[("tickets.count", 1), ("tickets.bytes", if charge.kept { len } else { 2 * len })]).await?;
    Ok(held.settle(store.create_by(at, new, act).await, |_, _| None).await?)
}

/// A body write held: the holds, and who gets the old body's bytes back
/// once it lands.
pub(super) struct Rewrite {
    held: Held,
    credit: Option<(String, u64)>,
}

/// `amend`'s and `update`'s body (none: nothing to count) on the ticket
/// `id`, held against `user`'s allowance by what it adds to what is kept.
pub(super) async fn rewrite(access: &Access, user: &VerifiedUser, store: &Store, id: &str, body: Option<&str>) -> Result<Rewrite, Failed> {
    let Some(body) = body else { return Ok(Rewrite { held: hold(access, user, &[]).await?, credit: None }) };
    let (me, len) = (user.principal_text(), body.len() as u64);
    let c = store.body_charge(Some(id), body).await?;
    let content = if c.kept { 0 } else { len };
    let (ask, credit) = match c.writer {
        Some(w) if w == me => (content + len.saturating_sub(c.old), Some((w, c.old.saturating_sub(len)))),
        Some(w) => (content + len, Some((w, c.old))),
        None => (content + len, None),
    };
    Ok(Rewrite { held: hold(access, user, &[("tickets.bytes", ask)]).await?, credit: credit.filter(|(_, n)| *n > 0) })
}

impl Rewrite {
    /// The write's outcome settles it: landed with its body changed, all
    /// held counts and the old body's writer gets its bytes back; landed
    /// with the body as it was, nothing counts; refused, all is released.
    pub(super) async fn settle<T>(self, access: &Access, r: Result<T, StoreError>, body_moved: impl Fn(&T) -> bool) -> Result<T, StoreError> {
        let moved = r.as_ref().is_ok_and(&body_moved);
        let r = self.held.settle(r, |_, _| if moved { None } else { Some(0) }).await;
        if let (true, Some((who, n))) = (moved, self.credit) {
            access.meter().get().credit(&who, "tickets.bytes", n).await;
        }
        r
    }
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
