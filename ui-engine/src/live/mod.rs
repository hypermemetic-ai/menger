//! UI-6: **`live()`** — a component's collection bound to Plexus streams:
//! `{ rows, status }`.
//!
//! The snapshot method is called once (each data event's row is its one
//! object-valued field — `summary`, `ticket` — and an event with none, a
//! `page`, carries no row); the changes stream is folded by the
//! declaration's `on`: `upsert:<field>` inserts or replaces the item under
//! `key` found at `event.<field>`, `remove:<field>` removes by the id (or
//! each id of a list) at `event.<field>`; `lagged` runs the snapshot again,
//! `resyncing` meanwhile. `status` moves `pending → live`, and `resyncing`
//! on a `lagged` or a reconnect.
//!
//! [`LiveCache`] shares one subscription between every reader asking the
//! same snapshot, params and changes, and keeps it [`KEEP_MS`] past its
//! last reader ([`LiveCache::sweep`] closes what has waited longer).

use crate::signals::{Runtime, Signal};
use crate::transport::Wire;
use serde_json::Value;
use std::cell::Cell;
use std::collections::BTreeMap;
use std::rc::Rc;

/// How long a subscription outlives its last reader, in milliseconds.
pub const KEEP_MS: u64 = 5_000;

/// Where a collection stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Pending,
    Live,
    Resyncing,
    Closed,
    Error,
    Refused,
}

/// One `on` rule.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FoldRule {
    /// `upsert:<field>`.
    Upsert(String),
    /// `remove:<field>`.
    Remove(String),
}

impl std::str::FromStr for FoldRule {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, String> {
        let _ = s;
        todo!("UI-6: FoldRule::from_str")
    }
}

/// A collection as a component reads it.
#[derive(Debug, Clone, PartialEq)]
pub struct Live {
    pub rows: Vec<Value>,
    pub status: Status,
}

impl Live {
    /// `{ "rows": […], "status": "live" }` — what an expression reads.
    pub fn to_value(&self) -> Value {
        todo!("UI-6: Live::to_value")
    }

    /// From [`Self::to_value`]'s shape.
    pub fn from_value(v: &Value) -> Option<Self> {
        let _ = v;
        todo!("UI-6: Live::from_value")
    }
}

/// A `live` declaration with its params evaluated: what a subscription is
/// keyed by (snapshot, params, changes) and how it folds.
#[derive(Debug, Clone, PartialEq)]
pub struct LiveSpec {
    pub snapshot: String,
    pub params: Value,
    pub changes: String,
    pub key: String,
    pub on: BTreeMap<String, FoldRule>,
}

impl LiveSpec {
    /// From its parts; an `on` rule that is no `upsert:`/`remove:` is refused.
    pub fn new(snapshot: &str, params: Value, changes: &str, key: &str, on: &[(&str, &str)]) -> Result<Self, String> {
        let _ = (snapshot, params, changes, key, on);
        todo!("UI-6: LiveSpec::new")
    }
}

/// The clock the cache keeps time by.
pub trait Clock {
    fn now_ms(&self) -> u64;
}

/// The tests' clock: moves only when told.
#[derive(Debug, Clone, Default)]
pub struct ManualClock(Rc<Cell<u64>>);

impl ManualClock {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn advance(&self, ms: u64) {
        self.0.set(self.0.get() + ms);
    }
}

impl Clock for ManualClock {
    fn now_ms(&self) -> u64 {
        self.0.get()
    }
}

/// Every `live` subscription of the page.
pub struct LiveCache<W: Wire, C: Clock> {
    rt: Runtime,
    wire: W,
    clock: C,
}

impl<W: Wire + Clone + 'static, C: Clock + Clone + 'static> LiveCache<W, C> {
    pub fn new(rt: &Runtime, wire: W, clock: C) -> Rc<Self> {
        Rc::new(Self { rt: rt.clone(), wire, clock })
    }

    /// A reader of `spec`'s collection: the subscription kept for it, else
    /// a new one (changes opened, then the snapshot).
    pub fn acquire(self: &Rc<Self>, spec: &LiveSpec) -> Reader {
        let _ = (spec, &self.rt, &self.wire, &self.clock);
        todo!("UI-6: LiveCache::acquire")
    }

    /// Closes every subscription with no reader for longer than [`KEEP_MS`].
    pub fn sweep(&self) {
        todo!("UI-6: LiveCache::sweep")
    }

    /// The socket came back: every kept subscription `resyncing`, its
    /// changes re-opened and its snapshot asked again.
    pub fn reconnected(&self) {
        todo!("UI-6: LiveCache::reconnected")
    }

    /// How many subscriptions are kept (read or waiting out their 5 s).
    pub fn kept(&self) -> usize {
        todo!("UI-6: LiveCache::kept")
    }
}

/// One reader of a collection; dropping it releases the subscription
/// (kept [`KEEP_MS`] more).
pub struct Reader {
    signal: Signal<Value>,
    release: Option<Box<dyn FnOnce()>>,
}

impl Reader {
    /// The collection as a signal of `{ rows, status }` (what the tree binds).
    pub fn signal(&self) -> Signal<Value> {
        self.signal.clone()
    }

    /// The collection now, untracked.
    pub fn live(&self) -> Live {
        Live::from_value(&self.signal.peek()).expect("a live collection's value")
    }
}

impl Drop for Reader {
    fn drop(&mut self) {
        if let Some(release) = self.release.take() {
            release();
        }
    }
}

#[cfg(test)]
mod tests;
