//! **The meter** — what a service that stores or fetches holds to reserve
//! against a principal's allowance before it writes: [`Meter::reserve`],
//! then [`Meter::commit`] (what was actually written) or [`Meter::release`]
//! (nothing was), and [`Meter::credit`] when a deletion gives bytes back.
//!
//! A service holds a [`MeterSlot`] — empty, every reserve allowed
//! ([`Unmetered`]), until the builder puts the quota service's seam client
//! in it ([`crate::QuotaSeam`]). A refusal is a [`Refusal`], typed, and the
//! service writes nothing and says it in band.

use std::sync::{Arc, OnceLock};

/// Why a reserve was refused. Nothing was held.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Refusal {
    /// `used` (with what is reserved) + `asked` would pass `cap`.
    OverAllowance { source: String, used: u64, cap: u64, asked: u64 },
    /// As many reservations of `source` are open as are allowed at once
    /// (`max`; the free tier: 2 uploads at a time).
    TooManyAtOnce { source: String, open: u32, max: u32 },
    /// The data disk is `used_percent` full, past `full_percent`: free-tier
    /// writes wait until an admin makes room.
    BoxFull { used_percent: f64, full_percent: f64 },
    /// No such source: a new input registers one.
    UnknownSource { source: String },
    /// The quota service could not be asked, in words.
    Failed(String),
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Refusal::OverAllowance { source, used, cap, asked } => {
                write!(f, "over your allowance of {source}: {used} used of {cap}, and this asks {asked} more (an admin raises it with quota.set)")
            }
            Refusal::TooManyAtOnce { source, open, max } => write!(f, "{open} of {source} already in flight, at most {max} at once: wait for one to finish"),
            Refusal::BoxFull { used_percent, full_percent } => {
                write!(f, "the server's disk is {used_percent:.0}% full (writes stop at {full_percent:.0}%): nothing was written; an admin makes room")
            }
            Refusal::UnknownSource { source } => write!(f, "`{source}` is no quota source (the sources: {})", crate::sources::names()),
            Refusal::Failed(m) => write!(f, "the quota service: {m}"),
        }
    }
}

/// A reservation held: its ticket (`None` from [`Unmetered`], which holds
/// nothing), to commit or release.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Hold {
    pub ticket: Option<String>,
    pub remaining: Option<u64>,
}

/// The quota service, as a service writing for a principal sees it.
#[async_trait::async_trait]
pub trait Meter: Send + Sync {
    /// Reserve `amount` of `source` for `principal`, before the write.
    async fn reserve(&self, principal: &str, source: &str, amount: u64) -> Result<Hold, Refusal>;
    /// The write landed: `amount` of it counts (none: all reserved).
    async fn commit(&self, hold: Hold, amount: Option<u64>);
    /// The write did not land: nothing counts.
    async fn release(&self, hold: Hold);
    /// A deletion: `amount` of `source` back to `principal`.
    async fn credit(&self, principal: &str, source: &str, amount: u64);
}

/// Every reserve allowed, nothing counted — a service with no quota service
/// attached (a test's, or before the builder attaches it).
#[derive(Debug, Clone, Copy, Default)]
pub struct Unmetered;

#[async_trait::async_trait]
impl Meter for Unmetered {
    async fn reserve(&self, _: &str, source: &str, _: u64) -> Result<Hold, Refusal> {
        match crate::sources::source(source) {
            Some(_) => Ok(Hold::default()),
            None => Err(Refusal::UnknownSource { source: source.to_string() }),
        }
    }
    async fn commit(&self, _: Hold, _: Option<u64>) {}
    async fn release(&self, _: Hold) {}
    async fn credit(&self, _: &str, _: &str, _: u64) {}
}

/// Where a service's meter is, once the builder says; [`Unmetered`] until.
#[derive(Clone, Default)]
pub struct MeterSlot(Arc<OnceLock<Arc<dyn Meter>>>);

impl std::fmt::Debug for MeterSlot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MeterSlot").field("attached", &self.0.get().is_some()).finish()
    }
}

impl MeterSlot {
    /// The first attach holds.
    pub fn attach(&self, meter: Arc<dyn Meter>) {
        let _ = self.0.set(meter);
    }

    /// The meter: the attached one, or [`Unmetered`].
    pub fn get(&self) -> Arc<dyn Meter> {
        self.0.get().cloned().unwrap_or_else(|| Arc::new(Unmetered))
    }

    /// Reserves each `(source, amount)` for `principal`, all or none: one
    /// refused releases the ones already held.
    pub async fn reserve_all(&self, principal: &str, asks: &[(&str, u64)]) -> Result<Vec<Hold>, Refusal> {
        let meter = self.get();
        let mut held = Vec::new();
        for (source, amount) in asks {
            match meter.reserve(principal, source, *amount).await {
                Ok(h) => held.push(h),
                Err(r) => {
                    for h in held {
                        meter.release(h).await;
                    }
                    return Err(r);
                }
            }
        }
        Ok(held)
    }
}
