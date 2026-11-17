//! **Every kept entry counts against someone's allowance** (V2-31's rule,
//! the code service's pattern): the quota service is another service's,
//! reached over the seam; this is the logs service's one call site for it,
//! a trait whose answer the service obeys.
//!
//! - before an `append` is kept, `reserve(principal, bytes)` for the caller
//!   — each entry's JSON as it was given, and its tags', once per entry;
//! - `commit` once kept, `release` when not (a refusal, a race lost).
//!
//! An entry is never edited or deleted but with its log, and no method
//! deletes a log yet, so nothing is credited back. menger's builder answers
//! it with the quota service's meter (`src/quota_allowance.rs`, the source
//! [`SOURCE`]); a test with [`Unlimited`] or a fake.

use async_trait::async_trait;

/// The quota source the logs service counts: bytes of entries kept, for good.
pub const SOURCE: &str = "logs.bytes";

/// Why an allowance refused: its reason as a `refused` says it
/// (`over_allowance`, `too_many_at_once`, `box_full`), in words with the numbers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Denied {
    pub reason: String,
    pub message: String,
}

/// The allowance the logs service asks.
#[async_trait]
pub trait Allowance: Send + Sync + std::fmt::Debug {
    /// Holds `amount` bytes for `principal`, or refuses; the hold's id.
    async fn reserve(&self, principal: &str, amount: u64) -> Result<String, Denied>;
    /// The hold `id` is used: `amount` of it counts.
    async fn commit(&self, id: &str, amount: u64);
    /// The hold `id` is not used: it is given back.
    async fn release(&self, id: &str);
}

/// Allows everything, counts nothing — a test's, or a service with no quota
/// service attached.
#[derive(Debug, Default, Clone, Copy)]
pub struct Unlimited;

#[async_trait]
impl Allowance for Unlimited {
    async fn reserve(&self, _: &str, _: u64) -> Result<String, Denied> {
        Ok(String::from("unlimited"))
    }
    async fn commit(&self, _: &str, _: u64) {}
    async fn release(&self, _: &str) {}
}
