//! UI-6: **the wire seam** — every Plexus call the engine makes goes
//! through one [`Wire`]: on wasm32 the page's one WebSocket ([`ws`],
//! v1's `WsTransport` over plexus-client's `ws-web`, with its 1 s reconnect
//! loop); in the native tests [`FakeWire`], which records what was opened
//! and lets the test feed each stream.
//!
//! Push-based: [`Wire::open`] hands a sink every item of the call's stream
//! in order, ending with [`WireItem::Done`] or [`WireItem::Failed`];
//! [`Wire::close`] unsubscribes. A component never holds a `Wire` — the
//! engine does.

use serde_json::Value;

/// A call's handle on the wire.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct CallId(pub u64);

/// One item of a call's stream.
#[derive(Debug, Clone, PartialEq)]
pub enum WireItem {
    /// A data event's content (`{"type": "summary", …}`).
    Data(Value),
    /// The stream ended normally.
    Done,
    /// The server refused the call (the gate's `forbidden`, …): its words.
    Refused(String),
    /// The socket dropped under the call; a reconnect re-opens what the
    /// engine still wants.
    Failed(String),
}

/// Where a call's items go.
pub type Sink = Box<dyn FnMut(WireItem)>;

/// The wire: the page's one socket.
pub trait Wire {
    /// Calls `method` (`tickets.list`) with `params`, its items into `sink`.
    fn open(&self, method: &str, params: Value, sink: Sink) -> CallId;
    /// Stops the call (an unsubscribe); its sink hears nothing more.
    fn close(&self, call: CallId);
}

impl<W: Wire + ?Sized> Wire for std::rc::Rc<W> {
    fn open(&self, method: &str, params: Value, sink: Sink) -> CallId {
        (**self).open(method, params, sink)
    }
    fn close(&self, call: CallId) {
        (**self).close(call)
    }
}

/// One call [`FakeWire`] saw.
#[derive(Debug, Clone, PartialEq)]
pub struct Opened {
    pub id: CallId,
    pub method: String,
    pub params: Value,
}

/// The tests' wire: records each open and close; the test feeds a call's
/// sink with [`FakeWire::feed`].
#[derive(Clone, Default)]
pub struct FakeWire(std::rc::Rc<std::cell::RefCell<Calls>>);

#[derive(Default)]
struct Calls {
    opened: Vec<Opened>,
    closed: Vec<CallId>,
    sinks: std::collections::BTreeMap<CallId, Sink>,
}

impl std::fmt::Debug for FakeWire {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let c = self.0.borrow();
        write!(f, "FakeWire({} opened, {} closed)", c.opened.len(), c.closed.len())
    }
}

impl FakeWire {
    pub fn new() -> Self {
        Self::default()
    }

    /// Every call opened, in order.
    pub fn opened(&self) -> Vec<Opened> {
        self.0.borrow().opened.clone()
    }

    /// The calls of `method` opened, in order.
    pub fn opened_of(&self, method: &str) -> Vec<Opened> {
        self.opened().into_iter().filter(|o| o.method == method).collect()
    }

    /// The latest call of `method`; panics naming it when none was opened.
    pub fn last_of(&self, method: &str) -> CallId {
        self.opened_of(method).last().unwrap_or_else(|| panic!("no call of {method} was opened")).id
    }

    /// Every call closed, in order.
    pub fn closed(&self) -> Vec<CallId> {
        self.0.borrow().closed.clone()
    }

    /// The calls open now.
    pub fn open_now(&self) -> Vec<Opened> {
        let closed = self.closed();
        self.opened().into_iter().filter(|o| !closed.contains(&o.id)).collect()
    }

    /// Hands `item` to `call`'s sink (nothing, once it is closed). The sink
    /// runs outside the wire's borrow, so it may open or close calls.
    pub fn feed(&self, call: CallId, item: WireItem) {
        let Some(mut sink) = self.0.borrow_mut().sinks.remove(&call) else { return };
        let ends = matches!(item, WireItem::Done | WireItem::Refused(_) | WireItem::Failed(_));
        sink(item);
        let mut c = self.0.borrow_mut();
        if !ends && !c.closed.contains(&call) {
            c.sinks.insert(call, sink);
        }
    }

    /// Hands each data event to `call`'s sink, then `Done`.
    pub fn answer(&self, call: CallId, events: impl IntoIterator<Item = Value>) {
        for e in events {
            self.feed(call, WireItem::Data(e));
        }
        self.feed(call, WireItem::Done);
    }

    /// Drops the socket: every open call hears `Failed`.
    pub fn drop_socket(&self) {
        let open: Vec<CallId> = self.0.borrow().sinks.keys().copied().collect();
        for id in open {
            self.feed(id, WireItem::Failed("the socket closed".into()));
        }
    }
}

impl Wire for FakeWire {
    fn open(&self, method: &str, params: Value, sink: Sink) -> CallId {
        let mut c = self.0.borrow_mut();
        let id = CallId(c.opened.len() as u64 + 1);
        c.opened.push(Opened { id, method: method.into(), params });
        c.sinks.insert(id, sink);
        id
    }
    fn close(&self, call: CallId) {
        let mut c = self.0.borrow_mut();
        c.sinks.remove(&call);
        if !c.closed.contains(&call) {
            c.closed.push(call);
        }
    }
}

/// The page's socket (wasm32): v1's `WsTransport` (tag v0.0.1,
/// `frontend/src/session.rs`) over plexus-client's `ws-web` — one socket,
/// signed in by the session's cookie, reconnecting on a 1 s loop and
/// re-opening every call the engine still holds.
#[cfg(target_arch = "wasm32")]
pub mod ws {
    /// The service the socket speaks to (plexus-client's namespace argument).
    pub const SERVICE: &str = "menger";
}
