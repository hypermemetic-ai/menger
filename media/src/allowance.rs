//! EDITOR-10: **every byte counts against someone's allowance.** The owner,
//! 2026-11-02: "we should have per user allowances for just about every
//! data source input. and be able to tabulate their total storage usage.
//! use that to measure and prevent them from exceeding their cap." The
//! allowance itself is another service's (V2-31, the quota service, reached
//! over the seam); this is the media service's one call site for it, a
//! trait whose answer the service obeys:
//!
//! - before a file is taken — `put`, `begin` — `reserve(principal,
//!   "media.bytes", size)` for whoever uploads it (a ticket's or a doc's
//!   file counts against its uploader, never the ticket);
//! - before a link is read — `unfurl`, `fetch` — `media.fetch.calls` (one)
//!   and, once the bytes are in hand, `media.fetch.bytes`;
//! - then `commit` once the bytes are kept, or `release` when they are not
//!   (a refusal, a duplicate, an upload abandoned and swept).
//!
//! Until V2-31 lands the service runs on [`Unlimited`], which allows
//! everything; wiring the real one is [`crate::MediaActivation::with_allowance`].

use async_trait::async_trait;

/// The sources the media service counts.
pub mod source {
    /// Bytes kept: a file put or uploaded.
    pub const BYTES: &str = "media.bytes";
    /// A link read by the server: an unfurl or a fetch.
    pub const FETCH_CALLS: &str = "media.fetch.calls";
    /// The bytes a fetch read and kept.
    pub const FETCH_BYTES: &str = "media.fetch.bytes";
    /// The bytes `/media/<hash>` serves (or signs an address for).
    pub const EGRESS_BYTES: &str = "media.egress.bytes";
}

/// Amounts held for a principal on a source until committed or released.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reservation {
    /// The allowance's id for it — kept with an upload, so a sweep releases it.
    pub id: String,
    pub principal: String,
    pub source: String,
    pub amount: u64,
}

/// Why an allowance refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Denied {
    /// What is left on the source, when the allowance says.
    pub remaining: Option<u64>,
    pub message: String,
}

/// The allowance the media service asks.
#[async_trait]
pub trait Allowance: Send + Sync + std::fmt::Debug {
    /// Holds `amount` of `source` for `principal`, or refuses.
    async fn reserve(&self, principal: &str, source: &str, amount: u64) -> Result<Reservation, Denied>;
    /// The reservation `id` is used: it counts — `amount` of it, what was
    /// actually kept (none: all it held). V2-31: a put reserves a bound
    /// (base64's), a fetch the most one may read; the kept size is what counts.
    async fn commit(&self, id: &str, amount: Option<u64>);
    /// The reservation `id` is not used: it is given back.
    async fn release(&self, id: &str);
}

/// Allows everything, keeps nothing — until the quota service (V2-31).
#[derive(Debug, Default, Clone, Copy)]
pub struct Unlimited;

#[async_trait]
impl Allowance for Unlimited {
    async fn reserve(&self, principal: &str, source: &str, amount: u64) -> Result<Reservation, Denied> {
        Ok(Reservation { id: uuid::Uuid::new_v4().simple().to_string(), principal: principal.into(), source: source.into(), amount })
    }
    async fn commit(&self, _id: &str, _amount: Option<u64>) {}
    async fn release(&self, _id: &str) {}
}

impl From<Denied> for crate::MediaEvent {
    fn from(d: Denied) -> Self {
        crate::MediaEvent::Refused { reason: "over_allowance".into(), message: format!("{} (nothing was kept)", d.message), bytes: None, max: d.remaining }
    }
}
