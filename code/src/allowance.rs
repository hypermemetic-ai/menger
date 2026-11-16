//! **Every kept byte counts against someone's allowance** (V2-31's rule,
//! the media service's pattern): the quota service is another service's,
//! reached over the seam; this is the code service's one call site for it,
//! a trait whose answer the service obeys.
//!
//! - before a fetched commit is kept, `reserve(principal, new)` for the
//!   caller who fetched it — the bytes new to the store, each file once;
//! - `commit` once kept, `release` when not (a refusal, the store full);
//! - `credit(principal, bytes)` when a file goes (the commit that named it
//!   dropped for room, or forgotten), to whoever it was reserved for.
//!
//! menger's builder answers it with the quota service's meter
//! (`src/quota_allowance.rs`, the source [`SOURCE`]); a test with
//! [`Unlimited`] or a fake.

use async_trait::async_trait;

/// The quota source the code service counts: bytes kept, for good.
pub const SOURCE: &str = "code.bytes";

/// Why an allowance refused: its reason as a `refused` says it
/// (`over_allowance`, `too_many_at_once`, `box_full`), in words with the numbers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Denied {
    pub reason: String,
    pub message: String,
    /// What is left, when the allowance says.
    pub remaining: Option<u64>,
}

/// The allowance the code service asks.
#[async_trait]
pub trait Allowance: Send + Sync + std::fmt::Debug {
    /// Holds `amount` bytes for `principal`, or refuses; the hold's id.
    async fn reserve(&self, principal: &str, amount: u64) -> Result<String, Denied>;
    /// The hold `id` is used: `amount` of it counts.
    async fn commit(&self, id: &str, amount: u64);
    /// The hold `id` is not used: it is given back.
    async fn release(&self, id: &str);
    /// `amount` bytes `principal` was charged for went: given back.
    async fn credit(&self, principal: &str, amount: u64);
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
    async fn credit(&self, _: &str, _: u64) {}
}
