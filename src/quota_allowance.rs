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
    let reason = match &r {
        Refusal::OverAllowance { .. } => "over_allowance",
        Refusal::TooManyAtOnce { .. } => "too_many_at_once",
        Refusal::BoxFull { .. } => "box_full",
        _ => "over_allowance",
    };
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

/// V2-31: **the docs service's meter is the quota service** too (V2-29's
/// `menger_docs::Quota`, called inside the store's blocking work):
/// `docs.bytes` for every change a principal writes, `docs.count` for every
/// document made, each reserved before the change is kept and committed
/// after. Its calls are synchronous; the quota service's are asked on a
/// thread of their own, the runtime's handle blocking on them.
#[derive(Clone, Debug)]
pub struct DocsMetered(pub MeterSlot);

impl DocsMetered {
    fn wait<F: std::future::Future + Send>(f: F) -> Option<F::Output>
    where
        F::Output: Send,
    {
        let h = tokio::runtime::Handle::try_current().ok()?;
        std::thread::scope(|s| s.spawn(move || h.block_on(f)).join().ok())
    }

    fn hold(&self, author: &str, source: &str, amount: u64) -> Result<menger_docs::quota::Reserved, String> {
        let meter = self.0.get();
        match Self::wait(meter.reserve(author, source, amount)) {
            Some(Ok(h)) => Ok(menger_docs::quota::Reserved { author: author.to_string(), bytes: amount, hold: h.ticket }),
            Some(Err(r)) => Err(r.to_string()),
            // No runtime to ask on (a test's store): nothing is held.
            None => Ok(menger_docs::quota::Reserved { author: author.to_string(), bytes: amount, hold: None }),
        }
    }
}

impl menger_docs::quota::Quota for DocsMetered {
    fn reserve(&self, author: &str, bytes: u64) -> Result<menger_docs::quota::Reserved, String> {
        self.hold(author, "docs.bytes", bytes)
    }
    fn reserve_count(&self, author: &str) -> Result<menger_docs::quota::Reserved, String> {
        self.hold(author, "docs.count", 1)
    }
    fn commit(&self, reserved: menger_docs::quota::Reserved) {
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
            Self::wait(meter.credit(author, "docs.bytes", delta.unsigned_abs()));
        } else if let Ok(h) = self.hold(author, "docs.bytes", delta as u64) {
            if let Some(t) = h.hold {
                Self::wait(meter.commit(Hold { ticket: Some(t), remaining: None }, None));
            }
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
