//! UI-6: **the page's socket** (wasm32) — v1's `WsTransport` (tag
//! v0.0.1, `frontend/src/session.rs`) over plexus-client's `ws-web`: one
//! socket, signed in by the session's cookie, checked every second and
//! opened again when it has gone (the reconnect loop), each reconnect
//! announced so the engine asks again for what it holds.

use super::{CallId, Sink, Wire, WireItem};
use futures::future::{AbortHandle, Abortable};
use futures::StreamExt;
use plexus_client::{MethodRef, StreamItem, Transport, WsTransport};
use serde_json::Value;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

/// The service the socket speaks to (plexus-client's namespace argument).
pub const SERVICE: &str = "menger";

/// How often the loop looks at the socket, in milliseconds.
pub const RECONNECT_MS: i32 = 1_000;

struct Inner {
    url: String,
    transport: RefCell<Option<Rc<WsTransport>>>,
    calls: RefCell<HashMap<u64, AbortHandle>>,
    next: Cell<u64>,
    connects: Cell<u32>,
    on_reconnect: RefCell<Vec<Rc<dyn Fn()>>>,
}

/// The one socket.
#[derive(Clone)]
pub struct WsWire(Rc<Inner>);

thread_local! {
    static METHODS: RefCell<HashMap<String, MethodRef>> = RefCell::new(HashMap::new());
}

/// `tickets.list` → its reference (interned: a page names few methods).
fn method_ref(method: &str) -> MethodRef {
    METHODS.with(|m| {
        if let Some(r) = m.borrow().get(method) {
            return *r;
        }
        let (path, name) = method.rsplit_once('.').unwrap_or(("", method));
        let path: &'static str = Box::leak(path.to_string().into_boxed_str());
        let name: &'static str = Box::leak(name.to_string().into_boxed_str());
        let ns: &'static str = path.rsplit('.').next().unwrap_or(path);
        let r = MethodRef::new(ns, name).at(path);
        m.borrow_mut().insert(method.to_string(), r);
        r
    })
}

/// Waits `ms` milliseconds.
pub async fn sleep(ms: i32) {
    let p = js_sys::Promise::new(&mut |resolve, _| {
        let _ = web_sys::window().expect("a window").set_timeout_with_callback_and_timeout_and_arguments_0(&resolve, ms);
    });
    let _ = wasm_bindgen_futures::JsFuture::from(p).await;
}

impl WsWire {
    /// The socket at `url`, not yet open: [`Self::run`] opens it.
    pub fn new(url: &str) -> Self {
        Self(Rc::new(Inner {
            url: url.into(),
            transport: RefCell::new(None),
            calls: RefCell::new(HashMap::new()),
            next: Cell::new(0),
            connects: Cell::new(0),
            on_reconnect: RefCell::new(Vec::new()),
        }))
    }

    /// Whether the socket is open.
    pub fn is_open(&self) -> bool {
        self.0.transport.borrow().as_ref().is_some_and(|t| !t.is_closed())
    }

    /// Called after every reconnect (not the first connect).
    pub fn on_reconnect(&self, f: Rc<dyn Fn()>) {
        self.0.on_reconnect.borrow_mut().push(f);
    }

    /// Opens the socket, then keeps it open: every second, a socket gone
    /// is opened again. Resolves once the first is open.
    pub async fn run(&self) {
        self.connect().await;
        let me = self.clone();
        wasm_bindgen_futures::spawn_local(async move {
            loop {
                sleep(RECONNECT_MS).await;
                if !me.is_open() {
                    *me.0.transport.borrow_mut() = None;
                    if me.try_connect().await {
                        let hooks: Vec<Rc<dyn Fn()>> = me.0.on_reconnect.borrow().clone();
                        for h in hooks {
                            h();
                        }
                    }
                }
            }
        });
    }

    async fn connect(&self) {
        while !self.try_connect().await {
            sleep(RECONNECT_MS).await;
        }
    }

    async fn try_connect(&self) -> bool {
        match WsTransport::connect(&self.0.url, SERVICE).await {
            Ok(t) => {
                *self.0.transport.borrow_mut() = Some(Rc::new(t));
                self.0.connects.set(self.0.connects.get() + 1);
                true
            }
            Err(e) => {
                web_sys::console::warn_1(&format!("the socket {}: {e}; again in 1 s", self.0.url).into());
                false
            }
        }
    }

    /// One call to its end: its data events, or the refusal / failure.
    pub async fn ask(&self, method: &str, params: Value) -> Result<Vec<Value>, String> {
        let (tx, rx) = futures::channel::oneshot::channel();
        let tx = RefCell::new(Some(tx));
        let got = Rc::new(RefCell::new(Vec::new()));
        let g = got.clone();
        self.open(method, params, Box::new(move |item| {
            let end = match item {
                WireItem::Data(v) => {
                    g.borrow_mut().push(v);
                    return;
                }
                WireItem::Done => Ok(std::mem::take(&mut *g.borrow_mut())),
                WireItem::Refused(m) | WireItem::Failed(m) => Err(m),
            };
            if let Some(tx) = tx.borrow_mut().take() {
                let _ = tx.send(end);
            }
        }));
        rx.await.unwrap_or_else(|_| Err("the call was dropped".into()))
    }
}

impl Wire for WsWire {
    fn open(&self, method: &str, params: Value, mut sink: Sink) -> CallId {
        let id = self.0.next.get() + 1;
        self.0.next.set(id);
        let Some(t) = self.0.transport.borrow().clone().filter(|t| !t.is_closed()) else {
            sink(WireItem::Failed("the socket is not open".into()));
            return CallId(id);
        };
        let mref = method_ref(method);
        let (handle, reg) = AbortHandle::new_pair();
        self.0.calls.borrow_mut().insert(id, handle);
        let inner = self.0.clone();
        let work = async move {
            let mut items = t.call(mref, params);
            while let Some(item) = items.next().await {
                let (wire, end) = match item {
                    Ok(StreamItem::Data { content, .. }) => (WireItem::Data(content), false),
                    Ok(StreamItem::Done { .. }) => (WireItem::Done, true),
                    Ok(StreamItem::Error { message, .. }) => (WireItem::Refused(message), true),
                    Ok(_) => continue,
                    Err(e) => (WireItem::Failed(e.to_string()), true),
                };
                sink(wire);
                if end {
                    break;
                }
            }
            inner.calls.borrow_mut().remove(&id);
        };
        wasm_bindgen_futures::spawn_local(async move {
            let _ = Abortable::new(work, reg).await;
        });
        CallId(id)
    }

    fn close(&self, call: CallId) {
        if let Some(h) = self.0.calls.borrow_mut().remove(&call.0) {
            h.abort();
        }
    }
}
