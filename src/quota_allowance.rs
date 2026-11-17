//! V2-31: **the media service's allowance is the quota service.** EDITOR-10
//! put one call site behind a trait ([`menger_media::Allowance`]): every
//! byte put or uploaded (`media.bytes`), every link read (`media.fetch.calls`,
//! `media.fetch.bytes`) and every byte `/media/<hash>` serves
//! (`media.egress.bytes`) is reserved before and committed or released
//! after. [`Metered`] answers it with the quota service's meter — the seam
//! client the builder attaches ([`menger_quota::MeterSlot`]) — so a refusal
//! is the quota service's, with its numbers: `over_allowance` (used, cap,
//! asked; `max` what is left), `too_many_at_once` (two uploads in flight)
//! or `box_full` (the disk past its full line).

use menger_media::allowance::{Denied, Reservation};
use menger_quota::{Hold, MeterSlot, Refusal};

/// A refusal's reason, as a typed `refused` says it.
fn reason_of(r: &Refusal) -> &'static str {
    match r {
        Refusal::OverAllowance { .. } => "over_allowance",
        Refusal::TooManyAtOnce { .. } => "too_many_at_once",
        Refusal::BoxFull { .. } => "box_full",
        _ => "over_allowance",
    }
}

/// The quota service's meter, as the media service asks it.
#[derive(Clone, Debug)]
pub struct Metered(pub MeterSlot);

/// A refusal as the media service says it: why, in words with the numbers,
/// and what is left.
pub fn denied(r: Refusal) -> Denied {
    let remaining = match &r {
        Refusal::OverAllowance { used, cap, .. } => Some(cap.saturating_sub(*used)),
        _ => None,
    };
    let reason = reason_of(&r);
    Denied { remaining, message: format!("{reason}: {r}") }
}

#[async_trait::async_trait]
impl menger_media::Allowance for Metered {
    async fn reserve(&self, principal: &str, source: &str, amount: u64) -> Result<Reservation, Denied> {
        match self.0.get().reserve(principal, source, amount).await {
            Ok(Hold { ticket, .. }) => Ok(Reservation {
                // Unmetered (no quota service attached) holds nothing: an id of its own.
                id: ticket.unwrap_or_else(|| format!("unmetered-{}", uuid_like())),
                principal: principal.to_string(),
                source: source.to_string(),
                amount,
            }),
            Err(r) => Err(denied(r)),
        }
    }

    async fn commit(&self, id: &str, amount: Option<u64>) {
        if !id.starts_with("unmetered-") {
            self.0.get().commit(Hold { ticket: Some(id.to_string()), remaining: None }, amount).await;
        }
    }

    async fn release(&self, id: &str) {
        if !id.starts_with("unmetered-") {
            self.0.get().release(Hold { ticket: Some(id.to_string()), remaining: None }).await;
        }
    }
}

/// V2-31: **the notes service's meter is the quota service** too (V2-29's
/// `menger_notes::Quota`, called inside the store's blocking work):
/// `notes.bytes` for every change a principal writes, `notes.count` for every
/// note made, each reserved before the change is kept and committed
/// after. Its calls are synchronous; the quota service's are asked on a
/// thread of their own, the runtime's handle blocking on them.
#[derive(Clone, Debug)]
pub struct NotesMetered(pub MeterSlot);

impl NotesMetered {
    fn wait<F: std::future::Future + Send>(f: F) -> Option<F::Output>
    where
        F::Output: Send,
    {
        let h = tokio::runtime::Handle::try_current().ok()?;
        std::thread::scope(|s| s.spawn(move || h.block_on(f)).join().ok())
    }

    fn hold(&self, author: &str, source: &str, amount: u64) -> Result<menger_notes::quota::Reserved, String> {
        let meter = self.0.get();
        match Self::wait(meter.reserve(author, source, amount)) {
            Some(Ok(h)) => Ok(menger_notes::quota::Reserved { author: author.to_string(), bytes: amount, hold: h.ticket }),
            Some(Err(r)) => Err(r.to_string()),
            // No runtime to ask on (a test's store): nothing is held.
            None => Ok(menger_notes::quota::Reserved { author: author.to_string(), bytes: amount, hold: None }),
        }
    }
}

impl menger_notes::quota::Quota for NotesMetered {
    fn reserve(&self, author: &str, bytes: u64) -> Result<menger_notes::quota::Reserved, String> {
        self.hold(author, "notes.bytes", bytes)
    }
    fn reserve_count(&self, author: &str) -> Result<menger_notes::quota::Reserved, String> {
        self.hold(author, "notes.count", 1)
    }
    /// V2-39: `questions.count`, one per question asked; refused with its reason.
    fn reserve_question(&self, author: &str) -> Result<menger_notes::quota::Reserved, menger_notes::quota::Refused> {
        let meter = self.0.get();
        match Self::wait(meter.reserve(author, "questions.count", 1)) {
            Some(Ok(h)) => Ok(menger_notes::quota::Reserved { author: author.to_string(), bytes: 1, hold: h.ticket }),
            Some(Err(r)) => Err(menger_notes::quota::Refused { reason: reason_of(&r).to_string(), message: format!("{}: {r}", reason_of(&r)) }),
            None => Ok(menger_notes::quota::Reserved { author: author.to_string(), bytes: 1, hold: None }),
        }
    }
    fn release(&self, reserved: menger_notes::quota::Reserved) {
        if let Some(t) = reserved.hold {
            let meter = self.0.get();
            Self::wait(meter.release(Hold { ticket: Some(t), remaining: None }));
        }
    }
    fn commit(&self, reserved: menger_notes::quota::Reserved) {
        if let Some(t) = reserved.hold {
            let meter = self.0.get();
            Self::wait(meter.commit(Hold { ticket: Some(t), remaining: None }, None));
        }
    }
    /// A squash: what `author` keeps shrank (credited back) or, their share
    /// of the snapshot past what was folded, grew (counted, never refused:
    /// the squash is done; past the cap, the recount says so).
    fn moved(&self, author: &str, delta: i64) {
        let meter = self.0.get();
        if delta < 0 {
            Self::wait(meter.credit(author, "notes.bytes", delta.unsigned_abs()));
        } else if let Ok(h) = self.hold(author, "notes.bytes", delta as u64) {
            if let Some(t) = h.hold {
                Self::wait(meter.commit(Hold { ticket: Some(t), remaining: None }, None));
            }
        }
    }
}

/// CODE-2: **the code service's allowance is the quota service** too
/// (`menger_code::Allowance`): the bytes a fetch adds to the store are
/// reserved for the caller on `code.bytes` before anything is kept, then
/// committed or released; a file that goes — its commit dropped for room,
/// or forgotten — is credited to whoever it was reserved for.
#[derive(Clone, Debug)]
pub struct CodeMetered(pub MeterSlot);

#[async_trait::async_trait]
impl menger_code::Allowance for CodeMetered {
    async fn reserve(&self, principal: &str, amount: u64) -> Result<String, menger_code::Denied> {
        match self.0.get().reserve(principal, menger_code::allowance::SOURCE, amount).await {
            Ok(Hold { ticket, .. }) => Ok(ticket.unwrap_or_else(|| format!("unmetered-{}", uuid_like()))),
            Err(r) => {
                let d = denied(r.clone());
                Err(menger_code::Denied { reason: reason_of(&r).to_string(), message: d.message, remaining: d.remaining })
            }
        }
    }

    async fn commit(&self, id: &str, amount: u64) {
        if !id.starts_with("unmetered-") {
            self.0.get().commit(Hold { ticket: Some(id.to_string()), remaining: None }, Some(amount)).await;
        }
    }

    async fn release(&self, id: &str) {
        if !id.starts_with("unmetered-") {
            self.0.get().release(Hold { ticket: Some(id.to_string()), remaining: None }).await;
        }
    }

    async fn credit(&self, principal: &str, amount: u64) {
        self.0.get().credit(principal, menger_code::allowance::SOURCE, amount).await;
    }
}

/// LOGS-2: **the logs service's allowance is the quota service** too
/// (`menger_logs::Allowance`): the bytes an `append` keeps — each entry's
/// JSON and its tags — are reserved for the writer on `logs.bytes` before
/// anything is kept, then committed or released. An entry is never deleted
/// but with its log, and no method deletes a log yet: nothing is credited.
#[derive(Clone, Debug)]
pub struct LogsMetered(pub MeterSlot);

#[async_trait::async_trait]
impl menger_logs::Allowance for LogsMetered {
    async fn reserve(&self, principal: &str, amount: u64) -> Result<String, menger_logs::Denied> {
        match self.0.get().reserve(principal, menger_logs::allowance::SOURCE, amount).await {
            Ok(Hold { ticket, .. }) => Ok(ticket.unwrap_or_else(|| format!("unmetered-{}", uuid_like()))),
            Err(r) => Err(menger_logs::Denied { reason: reason_of(&r).to_string(), message: denied(r).message }),
        }
    }

    async fn commit(&self, id: &str, amount: u64) {
        if !id.starts_with("unmetered-") {
            self.0.get().commit(Hold { ticket: Some(id.to_string()), remaining: None }, Some(amount)).await;
        }
    }

    async fn release(&self, id: &str) {
        if !id.starts_with("unmetered-") {
            self.0.get().release(Hold { ticket: Some(id.to_string()), remaining: None }).await;
        }
    }
}

fn uuid_like() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(0);
    format!("{:x}-{:x}", std::process::id(), N.fetch_add(1, Ordering::Relaxed))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_refusal_keeps_its_numbers() {
        let d = denied(Refusal::OverAllowance { source: "media.bytes".into(), used: 1_900_000_000, cap: 2_000_000_000, asked: 300_000_000 });
        assert_eq!(d.remaining, Some(100_000_000));
        assert!(d.message.starts_with("over_allowance: ") && d.message.contains("1.9 GB used of 2 GB") && d.message.contains("300 MB more"), "{}", d.message);
        assert!(denied(Refusal::BoxFull { used_percent: 90.0, full_percent: 85.0 }).message.starts_with("box_full: "));
        assert!(denied(Refusal::TooManyAtOnce { source: "media.bytes".into(), open: 2, max: 2 }).message.starts_with("too_many_at_once: "));
    }
}
