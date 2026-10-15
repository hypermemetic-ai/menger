//! ACCESS-29 (ACCESS-4 M1): the socket's limits menger keeps itself —
//! calls open on one connection, and a connection left idle.
//!
//! Every Plexus call is a `menger.call` subscription for as long as its
//! stream runs (a unary answer for a moment, a `watch` for the session), so
//! the calls open on a connection are its subscriptions. [`rpc_module`] is
//! the hub's own module ([`plexus_shell::rpc_module`]) with `menger.call`
//! registered again over the same dispatch — the params read, the caller's
//! identity from the upgrade, `hub.route`, each item piped out — and two
//! things added, per connection (the transport's
//! [`plexus_transport::websocket::Connection`], AUTH-11):
//!
//! - **Calls open** ([`CALLS_PER_CONNECTION`], `--max-calls-per-connection`):
//!   past it, the call is accepted and answered with one error item,
//!   `error_kind: too_many_calls` with the `max`, recoverable — close a
//!   watch, or open another connection (the per-IP and per-principal
//!   connection quotas, AUTH-12, bound that). jsonrpsee's own 1,024 stays
//!   behind it.
//! - **Idle** ([`IDLE_SECS`], `--idle-timeout-secs`): a connection with no
//!   call open and none begun for that long is closed. An app holds its
//!   `watch` open, so it is never idle; a script's socket closes itself.
//!
//! What it cannot see — a connection that never makes a call, a peer gone
//! silent under an open watch (a WebSocket ping), and the message size
//! (jsonrpsee's 10 MB default) — is the transport's accept loop, beside
//! AUTH-11's liveness and AUTH-12's quotas: ACCESS-29's divergence note.
//! Over stdio or a plain HTTP request there is no connection, and nothing
//! is counted.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, Weak};
use std::time::{Duration, Instant};

use futures::StreamExt;
use jsonrpsee::RpcModule;
use plexus_core::plexus::{error_item, AuthContext, DynamicHub, PlexusContext, PlexusStreamItem, StreamMetadata};
use plexus_transport::websocket::Connection;
use serde_json::Value;

/// The calls one connection may have open at once: room for the app's
/// watches and windows and a frame's bridge, a quarter of jsonrpsee's 1,024.
pub const CALLS_PER_CONNECTION: usize = 256;

/// How long a connection may sit with no call open before it is closed:
/// fifteen minutes.
pub const IDLE_SECS: u64 = 900;

/// [`rpc_module`]'s limits (main's flags).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CallLimits {
    /// Calls open on one connection at once.
    pub per_connection: usize,
    /// A connection with no call open for this long is closed; `None`: never.
    pub idle: Option<Duration>,
}

impl Default for CallLimits {
    fn default() -> Self {
        Self { per_connection: CALLS_PER_CONNECTION, idle: Some(Duration::from_secs(IDLE_SECS)) }
    }
}

/// The error item a call past the limit gets (the module docs).
pub const TOO_MANY_CALLS: &str = "too_many_calls";

/// The hub's RPC module, `menger.call` held to `limits` (the module docs).
/// Needs a Tokio runtime when `limits.idle` is set: the idle sweep runs on it.
pub fn rpc_module(hub: Arc<DynamicHub>, limits: CallLimits) -> anyhow::Result<RpcModule<()>> {
    let mut module = plexus_shell::rpc_module(hub.clone())?;
    let ns: &'static str = Box::leak(hub.runtime_namespace().to_string().into_boxed_str());
    let call: &'static str = Box::leak(format!("{ns}.call").into_boxed_str());
    let unsub: &'static str = Box::leak(format!("{ns}.call_unsub").into_boxed_str());
    module.remove_method(call);
    module.remove_method(unsub);
    let open = Open::new(limits);
    if let Some(idle) = limits.idle {
        Open::sweep(Arc::downgrade(&open), idle);
    }
    module
        .register_subscription(call, call, unsub, move |params, pending, _ctx, ext| {
            let (hub, open) = (hub.clone(), open.clone());
            async move {
                let place = match ext.get::<Arc<Connection>>() {
                    Some(conn) => match open.admit(conn) {
                        Ok(place) => Some(place),
                        Err(max) => {
                            let sink = pending.accept().await?;
                            sink.send(serde_json::value::to_raw_value(&too_many(ns, max))?).await?;
                            return Ok(());
                        }
                    },
                    None => None,
                };
                // As plexus-core's own `{ns}.call`: the params, the caller
                // the upgrade proved, the route, each item piped out.
                let p: CallParams = params.parse()?;
                let auth = ext.get::<Arc<AuthContext>>().map(|a| a.as_ref());
                let result = hub.route(&p.method, p.params.unwrap_or_default(), auth).await;
                let sink = pending.accept().await?;
                match result {
                    // Until the stream ends or the caller lets go (an
                    // unsubscribe, the socket closed): a watch let go of
                    // gives its place back at once, not at its next event.
                    Ok(mut stream) => loop {
                        tokio::select! {
                            item = stream.next() => match item {
                                Some(item) => sink.send(serde_json::value::to_raw_value(&item)?).await?,
                                None => break,
                            },
                            _ = sink.closed() => break,
                        }
                    },
                    Err(e) => {
                        let _ = sink.send(serde_json::value::to_raw_value(&error_item(ns, &e))?).await;
                    }
                }
                drop(place);
                jsonrpsee::core::SubscriptionResult::Ok(())
            }
        })
        .map_err(|e| anyhow::anyhow!("rpc module build failed: {e}"))?;
    Ok(module)
}

/// `{ns}.call`'s params, as plexus-core reads them.
#[derive(serde::Deserialize)]
struct CallParams {
    method: String,
    #[serde(default)]
    params: Option<Value>,
}

/// The refusal past the limit: an error item, typed.
fn too_many(ns: &str, max: usize) -> Value {
    let item = PlexusStreamItem::Error {
        metadata: StreamMetadata::new(vec![ns.into()], PlexusContext::hash()),
        message: format!("too many calls open on this connection (at most {max}): close one (a watch) or open another connection"),
        code: Some(TOO_MANY_CALLS.into()),
        recoverable: true,
    };
    let mut v = serde_json::to_value(&item).unwrap_or(Value::Null);
    if let Some(o) = v.as_object_mut() {
        o.insert("error_kind".into(), TOO_MANY_CALLS.into());
        o.insert("max".into(), max.into());
    }
    v
}

/// The connections that have made a call: each one's calls open, and when
/// it last began or ended one. Keyed by the connection's address in memory,
/// held weakly: a connection gone is dropped at the next sweep (or when its
/// address is reused).
struct Open {
    limits: CallLimits,
    conns: Mutex<HashMap<usize, Entry>>,
}

struct Entry {
    conn: Weak<Connection>,
    open: usize,
    last: Instant,
}

/// One call's place on its connection, given back when the call ends.
struct Place {
    open: Arc<Open>,
    key: usize,
}

impl Open {
    fn new(limits: CallLimits) -> Arc<Self> {
        Arc::new(Self { limits, conns: Mutex::new(HashMap::new()) })
    }

    /// A place for a call on `conn`, or the limit it is at.
    fn admit(self: &Arc<Self>, conn: &Arc<Connection>) -> Result<Place, usize> {
        let key = Arc::as_ptr(conn) as usize;
        let weak = Arc::downgrade(conn);
        let mut conns = self.conns.lock().unwrap_or_else(|p| p.into_inner());
        let e = conns.entry(key).or_insert_with(|| Entry { conn: weak.clone(), open: 0, last: Instant::now() });
        if !e.conn.ptr_eq(&weak) {
            *e = Entry { conn: weak, open: 0, last: Instant::now() };
        }
        e.last = Instant::now();
        if e.open >= self.limits.per_connection {
            return Err(self.limits.per_connection);
        }
        e.open += 1;
        Ok(Place { open: self.clone(), key })
    }

    /// Closes every connection idle past `idle`, every `idle / 10` (1 s to
    /// a minute), until the module is gone.
    fn sweep(open: Weak<Self>, idle: Duration) {
        let every = (idle / 10).clamp(Duration::from_secs(1), Duration::from_secs(60));
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(every).await;
                let Some(open) = open.upgrade() else { return };
                open.close_idle(idle);
            }
        });
    }

    /// One sweep: the idle closed, the gone forgotten.
    fn close_idle(&self, idle: Duration) {
        let mut conns = self.conns.lock().unwrap_or_else(|p| p.into_inner());
        conns.retain(|_, e| {
            let Some(conn) = e.conn.upgrade() else { return false };
            if conn.is_closed() {
                return false;
            }
            if e.open == 0 && e.last.elapsed() >= idle {
                tracing::info!(peer = %conn.peer(), "closing socket: idle for {} s with no call open", e.last.elapsed().as_secs());
                conn.close();
                return false;
            }
            true
        });
    }

    #[cfg(test)]
    fn tracked(&self) -> usize {
        self.conns.lock().unwrap().len()
    }
}

impl Drop for Place {
    fn drop(&mut self) {
        let mut conns = self.open.conns.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(e) = conns.get_mut(&self.key) {
            e.open = e.open.saturating_sub(1);
            e.last = Instant::now();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_refusal_is_an_error_item_typed_with_its_max() {
        let v = too_many("menger", 256);
        assert_eq!((v["type"].as_str(), v["error_kind"].as_str(), v["max"].as_u64()), (Some("error"), Some(TOO_MANY_CALLS), Some(256)), "{v}");
        assert_eq!(v["recoverable"], true);
        assert!(v["message"].as_str().unwrap().contains("at most 256"));
    }

    #[test]
    fn the_defaults_are_the_named_ones() {
        let d = CallLimits::default();
        assert_eq!((d.per_connection, d.idle), (CALLS_PER_CONNECTION, Some(Duration::from_secs(IDLE_SECS))));
        let open = Open::new(d);
        assert_eq!(open.tracked(), 0);
    }
}
