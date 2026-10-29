//! UI-6: **`live()`** — a component's collection bound to Plexus streams:
//! `{ rows, status }`.
//!
//! The snapshot method is called once (each data event's row is its one
//! object-valued field — `summary`, `ticket` — and an event with none, a
//! `page`, carries no row); the changes stream (opened first, with its own
//! `changes_params`) is folded by the declaration's `on`:
//!
//! - `upsert:<field>` inserts or replaces, by `key`, the item at
//!   `event.<field>` — only if it matches every key of the snapshot's
//!   params that names a field of the item (`parent`, `epic`, `status`;
//!   `summary` and `limit` name none), so a filtered collection ignores
//!   what is not its own, and an item that stops matching is removed;
//! - `remove:<field>` removes the id, or each id of the list, at
//!   `event.<field>`;
//! - `lagged` runs the snapshot again, `resyncing` meanwhile.
//!
//! `status` moves `pending → live`; `resyncing` on a `lagged`, or when the
//! socket drops until [`LiveCache::reconnected`] has asked again.
//!
//! [`LiveCache`] shares one subscription between every reader asking the
//! same snapshot, params, changes and changes params, and keeps it
//! [`KEEP_MS`] past its last reader ([`LiveCache::sweep`] closes what has
//! waited longer).

use crate::signals::{Runtime, Signal};
use crate::transport::{CallId, Wire, WireItem};
use serde_json::{json, Map, Value};
use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::rc::{Rc, Weak};

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
        match s.split_once(':') {
            Some(("upsert", f)) if !f.is_empty() => Ok(FoldRule::Upsert(f.into())),
            Some(("remove", f)) if !f.is_empty() => Ok(FoldRule::Remove(f.into())),
            _ => Err(format!("`{s}` is no fold: upsert:<field> or remove:<field>")),
        }
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
        json!({ "rows": self.rows, "status": self.status })
    }

    /// From [`Self::to_value`]'s shape.
    pub fn from_value(v: &Value) -> Option<Self> {
        Some(Self { rows: v.get("rows")?.as_array()?.clone(), status: serde_json::from_value(v.get("status")?.clone()).ok()? })
    }
}

/// A `live` declaration with its params evaluated: what a subscription is
/// keyed by (snapshot, params, changes, changes params) and how it folds.
#[derive(Debug, Clone, PartialEq)]
pub struct LiveSpec {
    pub snapshot: String,
    pub params: Value,
    /// UI-10: none — a one-shot read, no watch opened.
    pub changes: Option<String>,
    pub changes_params: Value,
    pub key: String,
    pub on: BTreeMap<String, FoldRule>,
}

impl LiveSpec {
    /// From its parts; an `on` rule that is no `upsert:`/`remove:` is refused.
    pub fn new(snapshot: &str, params: Value, changes: Option<&str>, changes_params: Value, key: &str, on: &[(&str, &str)]) -> Result<Self, String> {
        let on = on.iter().map(|(t, r)| Ok((t.to_string(), r.parse()?))).collect::<Result<_, String>>()?;
        let obj = |v: Value| if v.is_null() { json!({}) } else { v };
        Ok(Self { snapshot: snapshot.into(), params: obj(params), changes: changes.map(str::to_string), changes_params: obj(changes_params), key: key.into(), on })
    }

    fn cache_key(&self) -> String {
        format!("{}\u{0}{}\u{0}{}\u{0}{}", self.snapshot, self.params, self.changes.as_deref().unwrap_or(""), self.changes_params)
    }

    /// Whether `item` matches every snapshot param that names a field of it.
    pub fn admits(&self, item: &Value) -> bool {
        let (Some(params), Some(item)) = (self.params.as_object(), item.as_object()) else { return true };
        params.iter().all(|(k, v)| item.get(k).is_none_or(|have| have == v))
    }
}

/// The clock the cache keeps time by, and its timers (UI-10: a `live`'s
/// `debounce_ms`).
pub trait Clock {
    fn now_ms(&self) -> u64;
    /// Runs `f` once, `after_ms` from now.
    fn schedule(&self, after_ms: u64, f: Box<dyn FnOnce()>);
}

/// A timer of [`ManualClock`]: when it is due, its order, what it runs.
type Timer = (u64, u64, Box<dyn FnOnce()>);

/// The tests' clock: moves only when told, running each timer that comes
/// due on the way, in order.
#[derive(Clone, Default)]
pub struct ManualClock {
    now: Rc<Cell<u64>>,
    timers: Rc<RefCell<Vec<Timer>>>,
    seq: Rc<Cell<u64>>,
}

impl std::fmt::Debug for ManualClock {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ManualClock({} ms, {} timers)", self.now.get(), self.timers.borrow().len())
    }
}

impl ManualClock {
    pub fn new() -> Self {
        Self::default()
    }

    /// Moves `ms` on, running the timers that come due (a timer set by one
    /// that runs is run too, when due by the end).
    pub fn advance(&self, ms: u64) {
        let end = self.now.get() + ms;
        loop {
            let next = {
                let mut t = self.timers.borrow_mut();
                let i = t.iter().enumerate().filter(|(_, (due, _, _))| *due <= end).min_by_key(|(_, (due, seq, _))| (*due, *seq)).map(|(i, _)| i);
                i.map(|i| t.remove(i))
            };
            let Some((due, _, f)) = next else { break };
            self.now.set(due.max(self.now.get()));
            f();
        }
        self.now.set(end);
    }
}

impl Clock for ManualClock {
    fn now_ms(&self) -> u64 {
        self.now.get()
    }
    fn schedule(&self, after_ms: u64, f: Box<dyn FnOnce()>) {
        let seq = self.seq.get() + 1;
        self.seq.set(seq);
        self.timers.borrow_mut().push((self.now.get() + after_ms, seq, f));
    }
}

/// One kept subscription.
struct Sub {
    spec: LiveSpec,
    signal: Signal<Value>,
    rows: Vec<Value>,
    status: Status,
    readers: usize,
    released_at: u64,
    changes: Option<CallId>,
    snapshot: Option<CallId>,
    /// The snapshot's rows as they arrive.
    buffer: Vec<Value>,
}

impl Sub {
    fn publish(&self) {
        self.signal.set(Live { rows: self.rows.clone(), status: self.status }.to_value());
    }

    fn key_of(&self, item: &Value) -> Option<String> {
        match item.get(&self.spec.key)? {
            Value::String(s) => Some(s.clone()),
            other => Some(other.to_string()),
        }
    }

    fn position(&self, key: &str) -> Option<usize> {
        self.rows.iter().position(|r| self.key_of(r).as_deref() == Some(key))
    }

    /// Folds one change event; whether the rows moved.
    fn fold(&mut self, event: &Value) -> bool {
        let Some(kind) = event.get("type").and_then(Value::as_str) else { return false };
        match self.spec.on.get(kind).cloned() {
            Some(FoldRule::Upsert(field)) => {
                let Some(item) = event.get(&field).filter(|i| i.is_object()).cloned() else { return false };
                let Some(key) = self.key_of(&item) else { return false };
                let at = self.position(&key);
                match (self.spec.admits(&item), at) {
                    (true, Some(i)) => self.rows[i] = item,
                    (true, None) => self.rows.push(item),
                    (false, Some(i)) => {
                        self.rows.remove(i);
                    }
                    (false, None) => return false,
                }
                true
            }
            Some(FoldRule::Remove(field)) => {
                let ids: Vec<String> = match event.get(&field) {
                    Some(Value::Array(a)) => a.iter().map(|v| v.as_str().map(str::to_string).unwrap_or_else(|| v.to_string())).collect(),
                    Some(Value::String(s)) => vec![s.clone()],
                    Some(Value::Null) | None => return false,
                    Some(v) => vec![v.to_string()],
                };
                let before = self.rows.len();
                let key = self.spec.key.clone();
                self.rows.retain(|r| {
                    let k = match r.get(&key) {
                        Some(Value::String(s)) => s.clone(),
                        Some(v) => v.to_string(),
                        None => return true,
                    };
                    !ids.contains(&k)
                });
                self.rows.len() != before
            }
            None => false,
        }
    }
}

/// A snapshot event's row: its one object-valued field.
fn row_of(event: &Value) -> Option<Value> {
    let o = event.as_object()?;
    let mut objects = o.iter().filter(|(k, v)| k.as_str() != "type" && v.is_object());
    let (_, row) = objects.next()?;
    objects.next().is_none().then(|| row.clone())
}

/// Every `live` subscription of the page.
pub struct LiveCache<W: Wire, C: Clock> {
    rt: Runtime,
    wire: W,
    clock: C,
    subs: RefCell<BTreeMap<String, Rc<RefCell<Sub>>>>,
    me: Weak<Self>,
}

impl<W: Wire + Clone + 'static, C: Clock + Clone + 'static> LiveCache<W, C> {
    pub fn new(rt: &Runtime, wire: W, clock: C) -> Rc<Self> {
        Rc::new_cyclic(|me| Self { rt: rt.clone(), wire, clock, subs: RefCell::default(), me: me.clone() })
    }

    /// A reader of `spec`'s collection: the subscription kept for it, else
    /// a new one (changes opened, then the snapshot).
    pub fn acquire(self: &Rc<Self>, spec: &LiveSpec) -> Reader {
        let key = spec.cache_key();
        let existing = self.subs.borrow().get(&key).cloned();
        let sub = match existing {
            Some(sub) => sub,
            None => {
                let signal = self.rt.signal(Live { rows: Vec::new(), status: Status::Pending }.to_value());
                let sub = Rc::new(RefCell::new(Sub {
                    spec: spec.clone(),
                    signal,
                    rows: Vec::new(),
                    status: Status::Pending,
                    readers: 0,
                    released_at: 0,
                    changes: None,
                    snapshot: None,
                    buffer: Vec::new(),
                }));
                self.subs.borrow_mut().insert(key.clone(), sub.clone());
                self.open_changes(&key, &sub);
                self.open_snapshot(&key, &sub);
                sub
            }
        };
        let signal = {
            let mut s = sub.borrow_mut();
            s.readers += 1;
            s.signal.clone()
        };
        let me = self.me.clone();
        Reader {
            signal,
            release: Some(Box::new(move || {
                if let Some(cache) = me.upgrade() {
                    cache.release(&key);
                }
            })),
        }
    }

    fn release(&self, key: &str) {
        if let Some(sub) = self.subs.borrow().get(key) {
            let mut s = sub.borrow_mut();
            s.readers = s.readers.saturating_sub(1);
            if s.readers == 0 {
                s.released_at = self.clock.now_ms();
            }
        }
    }

    fn open_changes(&self, key: &str, sub: &Rc<RefCell<Sub>>) {
        let (method, params) = {
            let s = sub.borrow();
            (s.spec.changes.clone(), s.spec.changes_params.clone())
        };
        let Some(method) = method else { todo!("UI-10: a live without changes") };
        let (me, k) = (self.me.clone(), key.to_string());
        let id = self.wire.open(&method, params, Box::new(move |item| {
            if let Some(cache) = me.upgrade() {
                cache.heard_change(&k, item);
            }
        }));
        sub.borrow_mut().changes = Some(id);
    }

    fn open_snapshot(&self, key: &str, sub: &Rc<RefCell<Sub>>) {
        let (method, params) = {
            let mut s = sub.borrow_mut();
            s.buffer.clear();
            (s.spec.snapshot.clone(), s.spec.params.clone())
        };
        let (me, k) = (self.me.clone(), key.to_string());
        let id = self.wire.open(&method, params, Box::new(move |item| {
            if let Some(cache) = me.upgrade() {
                cache.heard_snapshot(&k, item);
            }
        }));
        let mut s = sub.borrow_mut();
        // A snapshot that answered at once (a fake, a cache) is done already.
        if s.snapshot.is_none() && matches!(s.status, Status::Live) {
            return;
        }
        s.snapshot = Some(id);
    }

    fn sub(&self, key: &str) -> Option<Rc<RefCell<Sub>>> {
        self.subs.borrow().get(key).cloned()
    }

    fn heard_snapshot(&self, key: &str, item: WireItem) {
        let Some(sub) = self.sub(key) else { return };
        let mut s = sub.borrow_mut();
        match item {
            WireItem::Data(event) => {
                if let Some(row) = row_of(&event) {
                    s.buffer.push(row);
                }
                return;
            }
            WireItem::Done => {
                s.rows = std::mem::take(&mut s.buffer);
                s.status = Status::Live;
            }
            WireItem::Refused(_) => s.status = Status::Refused,
            WireItem::Failed(_) => s.status = Status::Resyncing,
        }
        s.snapshot = None;
        drop(s);
        sub.borrow().publish();
    }

    fn heard_change(&self, key: &str, item: WireItem) {
        let Some(sub) = self.sub(key) else { return };
        match item {
            WireItem::Data(event) if event.get("type").and_then(Value::as_str) == Some("lagged") => {
                sub.borrow_mut().status = Status::Resyncing;
                sub.borrow().publish();
                self.open_snapshot(key, &sub);
            }
            WireItem::Data(event) => {
                let moved = sub.borrow_mut().fold(&event);
                if moved {
                    sub.borrow().publish();
                }
            }
            WireItem::Done => sub.borrow_mut().changes = None,
            WireItem::Refused(_) => {
                let mut s = sub.borrow_mut();
                s.changes = None;
                s.status = Status::Refused;
                drop(s);
                sub.borrow().publish();
            }
            WireItem::Failed(_) => {
                let mut s = sub.borrow_mut();
                s.changes = None;
                s.status = Status::Resyncing;
                drop(s);
                sub.borrow().publish();
            }
        }
    }

    /// Closes every subscription with no reader for longer than [`KEEP_MS`].
    pub fn sweep(&self) {
        let now = self.clock.now_ms();
        let gone: Vec<(String, Rc<RefCell<Sub>>)> = self
            .subs
            .borrow()
            .iter()
            .filter(|(_, s)| {
                let s = s.borrow();
                s.readers == 0 && now.saturating_sub(s.released_at) > KEEP_MS
            })
            .map(|(k, s)| (k.clone(), s.clone()))
            .collect();
        for (k, sub) in gone {
            self.subs.borrow_mut().remove(&k);
            let (changes, snapshot) = {
                let mut s = sub.borrow_mut();
                s.status = Status::Closed;
                (s.changes.take(), s.snapshot.take())
            };
            for id in [changes, snapshot].into_iter().flatten() {
                self.wire.close(id);
            }
        }
    }

    /// The socket came back: every kept subscription `resyncing`, its
    /// changes re-opened and its snapshot asked again.
    pub fn reconnected(&self) {
        let subs: Vec<(String, Rc<RefCell<Sub>>)> = self.subs.borrow().iter().map(|(k, s)| (k.clone(), s.clone())).collect();
        for (k, sub) in subs {
            let (changes, snapshot) = {
                let mut s = sub.borrow_mut();
                s.status = Status::Resyncing;
                (s.changes.take(), s.snapshot.take())
            };
            for id in [changes, snapshot].into_iter().flatten() {
                self.wire.close(id);
            }
            sub.borrow().publish();
            self.open_changes(&k, &sub);
            self.open_snapshot(&k, &sub);
        }
    }

    /// How many subscriptions are kept (read or waiting out their 5 s).
    pub fn kept(&self) -> usize {
        self.subs.borrow().len()
    }
}

/// UI-10: a `live` as declared, before its params are known: the methods,
/// the key, the folds, and how long its params must be quiet before it
/// runs again.
#[derive(Debug, Clone, PartialEq)]
pub struct LiveQuery {
    pub snapshot: String,
    pub changes: Option<String>,
    pub key: String,
    pub on: Vec<(String, String)>,
    pub debounce_ms: Option<u64>,
}

impl<W: Wire + Clone + 'static, C: Clock + Clone + 'static> LiveCache<W, C> {
    /// UI-10: a collection whose params are `params()` — `(params,
    /// changes_params)`, read in a binding: when a signal it read changes,
    /// the snapshot runs again (after `debounce_ms` of quiet when given),
    /// the old rows kept, `resyncing`, until the new snapshot lands.
    pub fn follow(self: &Rc<Self>, query: LiveQuery, params: impl FnMut() -> (Value, Value) + 'static) -> Follow {
        let _ = (query, params);
        todo!("UI-10: LiveCache::follow")
    }
}

/// UI-10: a collection that follows its params; dropping it stops that and
/// releases its reader.
pub struct Follow {
    signal: Signal<Value>,
    stop: Option<Box<dyn FnOnce()>>,
}

impl Follow {
    /// The collection as a signal of `{ rows, status }` — the same signal
    /// whatever the params become.
    pub fn signal(&self) -> Signal<Value> {
        self.signal.clone()
    }

    /// The collection now, untracked.
    pub fn live(&self) -> Live {
        Live::from_value(&self.signal.peek()).expect("a live collection's value")
    }
}

impl Drop for Follow {
    fn drop(&mut self) {
        if let Some(stop) = self.stop.take() {
            stop();
        }
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

/// A `live` declaration's params resolved against the props: `{ "expr" }`
/// values evaluated (a prop by name, or a Rhai expression over the props).
pub fn resolve(params: &BTreeMap<String, Value>, props: &Map<String, Value>) -> Value {
    let mut out = Map::new();
    for (k, v) in params {
        let v = match v.as_object().and_then(|o| o.get("expr")).and_then(Value::as_str) {
            Some(e) => match props.get(e) {
                Some(p) => p.clone(),
                None => {
                    let mut scope = rhai::Scope::new();
                    for (pk, pv) in props {
                        scope.push_constant_dynamic(pk.clone(), crate::tree::expr::to_dynamic(pv));
                    }
                    crate::tree::expr::engine()
                        .eval_expression_with_scope::<rhai::Dynamic>(&mut scope, e)
                        .map(|d| crate::tree::expr::to_value(&d))
                        .unwrap_or(Value::Null)
                }
            },
            None => v.clone(),
        };
        out.insert(k.clone(), v);
    }
    Value::Object(out)
}

#[cfg(test)]
mod tests;
