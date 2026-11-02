//! ACCESS-29 (ACCESS-4 M1): the socket's limits menger keeps itself —
//! calls open on one connection, and a connection left idle.
//!
//! Every Plexus call is a `menger.call` subscription for as long as its
//! stream runs (a unary answer for a moment, a `watch` for the session), so
//! the calls open on a connection are its subscriptions. [`rpc_module`] is
//! the service's own module ([`plexus_webapp::rpc_module`]) with `menger.call`
//! registered again over the same dispatch — the params read, the caller's
//! identity from the upgrade, `hub.route`, each item piped out by
//! plexus-core's own pipe (PERF-7: encoded once into its frame) — and two
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
//! - **Writes per principal** (V2-31, [`menger_quota::Buckets`],
//!   `--max-writes-per-second`, 20 by default with a burst of twice that —
//!   the owner's free tier): a write (a method an editor holds and a viewer
//!   does not, as the activations declare their roles) past the
//!   principal's bucket (an admin of `*` has none) is answered with one error item, `error_kind:
//!   too_many_writes` with `retry_after_ms`, recoverable; nothing is
//!   written.
//!
//! What it cannot see is the transport's (PERF-20, plexus-transport's own
//! socket loop): a connection that never makes a call is closed by the
//! transport's idle limit, which `--idle-timeout-secs` sets too (RPC-5); a
//! peer gone silent under an open watch is found by its pings; a message
//! over 10 MiB is refused there — beside AUTH-11's liveness and AUTH-12's
//! quotas: ACCESS-29's divergence note.
//! Over stdio or a plain HTTP request there is no connection, and nothing
//! is counted.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, OnceLock, Weak};
use std::time::{Duration, Instant};

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

/// V2-31: the connections one signed-in person may hold open (the owner's
/// free tier: 4 — a browser tab or two), `--max-connections-per-principal`.
pub const CONNECTIONS_PER_PRINCIPAL: u32 = 4;

/// V2-31: the connections one agent's key (`key:` principal) may hold open —
/// several agents' sessions share a key (`builder`) —
/// `--max-connections-per-agent`.
pub const CONNECTIONS_PER_AGENT: u32 = 64;

/// The error item a connection past its principal's limit gets, before it
/// is closed.
pub const TOO_MANY_CONNECTIONS: &str = "too_many_connections";

/// V2-31: the writes one principal may make a second, sustained (the
/// owner's free tier: about 20); twice that at once after a pause.
pub const WRITES_PER_SECOND: u32 = 20;

/// [`rpc_module`]'s limits (main's flags).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CallLimits {
    /// Calls open on one connection at once.
    pub per_connection: usize,
    /// A connection with no call open for this long is closed; `None`: never.
    pub idle: Option<Duration>,
    /// V2-31: writes one principal may make a second, sustained (0: no limit).
    pub writes_per_second: u32,
    /// V2-31: connections one person may hold (0: no limit).
    pub per_principal: u32,
    /// V2-31: connections one agent's key may hold (0: no limit).
    pub per_agent: u32,
}

impl Default for CallLimits {
    fn default() -> Self {
        Self { per_connection: CALLS_PER_CONNECTION, idle: Some(Duration::from_secs(IDLE_SECS)), writes_per_second: WRITES_PER_SECOND, per_principal: CONNECTIONS_PER_PRINCIPAL, per_agent: CONNECTIONS_PER_AGENT }
    }
}

/// The error item a write past the principal's rate gets.
pub const TOO_MANY_WRITES: &str = "too_many_writes";

/// V2-31: the methods that write — an editor's and not a viewer's, as the
/// activations declare their roles (children under their whole path).
fn writes_of(hub: &DynamicHub) -> HashSet<String> {
    let roles = hub.declared_roles();
    let viewer: HashSet<&String> = roles.get("viewer").map(|v| v.iter().collect()).unwrap_or_default();
    roles.get("editor").map(|e| e.iter().filter(|g| !viewer.contains(g)).cloned().collect()).unwrap_or_default()
}

/// The error item a call past the limit gets (the module docs).
pub const TOO_MANY_CALLS: &str = "too_many_calls";

/// The hub's RPC module, `menger.call` held to `limits` (the module docs).
/// Needs a Tokio runtime when `limits.idle` is set: the idle sweep runs on it.
pub fn rpc_module(hub: Arc<DynamicHub>, limits: CallLimits) -> anyhow::Result<RpcModule<()>> {
    let mut module = plexus_webapp::rpc_module(hub.clone())?;
    let ns: &'static str = Box::leak(hub.runtime_namespace().to_string().into_boxed_str());
    let call: &'static str = Box::leak(format!("{ns}.call").into_boxed_str());
    let unsub: &'static str = Box::leak(format!("{ns}.call_unsub").into_boxed_str());
    module.remove_method(call);
    module.remove_method(unsub);
    let open = Open::new(limits);
    // V2-31: each principal's write bucket, and which methods write (read
    // from the roles at the first call: the hub is whole by then).
    let rate = f64::from(limits.writes_per_second);
    let buckets = Arc::new(menger_quota::Buckets::new(rate, rate * 2.0));
    let writes: Arc<OnceLock<HashSet<String>>> = Arc::new(OnceLock::new());
    if let Some(idle) = limits.idle {
        Open::sweep(Arc::downgrade(&open), idle);
    }
    module
        .register_subscription(call, call, unsub, move |params, pending, _ctx, ext| {
            let (hub, open, buckets, writes) = (hub.clone(), open.clone(), buckets.clone(), writes.clone());
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
                // ACCESS-13: with login off, the anonymous caller.
                let anonymous = AuthContext::anonymous();
                let auth = caller(&hub, ext.get::<Arc<AuthContext>>().map(|a| a.as_ref()), &anonymous);
                // V2-31: a connection past its principal's limit is answered
                // and closed — a person's 4, an agent key's 64, an admin of
                // `*` none (the transport's own per-principal quota is one
                // number for everyone, so main hands it no limit and the
                // split is made here, at the connection's first call).
                if let (Some(conn), Some(a)) = (ext.get::<Arc<Connection>>(), auth) {
                    if let Some(principal) = a.principal().map(|p| p.to_string()) {
                        let limit = limit_for(&principal, &open.limits);
                        if let Err(max) = open.seat(&principal, conn, limit, || admin_everywhere(&hub, a)) {
                            let sink = pending.accept().await?;
                            sink.send(serde_json::value::to_raw_value(&crowded(ns, max, &principal))?).await?;
                            drop(place);
                            // A moment for the refusal to reach the peer, then the socket goes.
                            let conn = conn.clone();
                            tokio::spawn(async move {
                                tokio::time::sleep(Duration::from_millis(250)).await;
                                conn.close();
                            });
                            return Ok(());
                        }
                    }
                }
                // V2-31: a write past the principal's rate is answered, not run.
                if let Some(principal) = auth.and_then(|a| a.principal()).map(|p| p.to_string()) {
                    if writes.get_or_init(|| writes_of(&hub)).contains(&p.method) {
                        let exempt = || auth.is_some_and(|a| admin_everywhere(&hub, a));
                        if let Err(wait) = buckets.take(&principal, Instant::now()).or_else(|w| if exempt() { Ok(()) } else { Err(w) }) {
                            let sink = pending.accept().await?;
                            sink.send(serde_json::value::to_raw_value(&too_fast(ns, wait, buckets.rate()))?).await?;
                            drop(place);
                            return Ok(());
                        }
                    }
                }
                let result = hub.route(&p.method, p.params.unwrap_or_default(), auth).await;
                let sink = pending.accept().await?;
                let piped = match result {
                    // PERF-7: plexus-core's pipe, as its own `{ns}.call`
                    // pipes — each item encoded once, straight into the frame
                    // the socket writes. Until the stream ends or the caller
                    // lets go (an unsubscribe, the socket closed): a watch let
                    // go of gives its place back at once, not at its next event.
                    Ok(stream) => plexus_core::plexus::wire::pipe(sink, stream, &ext).await,
                    Err(e) => {
                        let _ = sink.send(serde_json::value::to_raw_value(&error_item(ns, &e))?).await;
                        Ok(())
                    }
                };
                drop(place);
                piped
            }
        })
        .map_err(|e| anyhow::anyhow!("rpc module build failed: {e}"))?;
    Ok(module)
}

/// ACCESS-13: the identity a call carries into `hub` — the one the upgrade
/// proved, else, on a hub with no gate (login off), `anonymous`. Every
/// method on the frame resolves its caller from an identity
/// (`#[from_auth(self.access.verify)]`), which the dispatch refuses to run
/// without one; with login off the resolver admits anyone, so the
/// transports menger serves hand it the anonymous identity — what "no
/// identity" always meant there. With login on (a gate) nothing is added:
/// no identity is the anonymous view, which the gate decides. ACCESS-24
/// removes open mode, and this with it.
pub fn caller<'a>(hub: &DynamicHub, proved: Option<&'a AuthContext>, anonymous: &'a AuthContext) -> Option<&'a AuthContext> {
    proved.or_else(|| hub.gate().is_none().then_some(anonymous))
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
        // plexus-core 0.6.2 (POLYGLOT-5): a typed error carries fields; this one has none.
        fields: Default::default(),
    };
    let mut v = serde_json::to_value(&item).unwrap_or(Value::Null);
    if let Some(o) = v.as_object_mut() {
        o.insert("error_kind".into(), TOO_MANY_CALLS.into());
        o.insert("max".into(), max.into());
    }
    v
}

/// V2-31: an admin of `*` (the operator among them) writes at any rate, as
/// it stores past any cap — asked of the gate only once its bucket is empty.
fn admin_everywhere(hub: &DynamicHub, caller: &AuthContext) -> bool {
    hub.gate().is_none_or(|g| g.held_on(caller, &plexus_auth_core::grants::Grant::new(menger_quota::seam::ADMIN_GRANT)).iter().any(|s| s.is_everywhere()))
}

/// V2-31: the connections `principal` may hold: an agent's key (a `key:`
/// principal) the agents' limit, anyone else a person's.
pub fn limit_for(principal: &str, limits: &CallLimits) -> u32 {
    if principal.starts_with("key:") {
        limits.per_agent
    } else {
        limits.per_principal
    }
}

/// The refusal past a principal's connections, before the connection closes.
fn crowded(ns: &str, max: u32, principal: &str) -> Value {
    let item = PlexusStreamItem::Error {
        metadata: StreamMetadata::new(vec![ns.into()], PlexusContext::hash()),
        message: format!("too many connections as {principal} (at most {max}): close one; this one closes"),
        code: Some(TOO_MANY_CONNECTIONS.into()),
        recoverable: true,
        fields: Default::default(),
    };
    let mut v = serde_json::to_value(&item).unwrap_or(Value::Null);
    if let Some(o) = v.as_object_mut() {
        o.insert("error_kind".into(), TOO_MANY_CONNECTIONS.into());
        o.insert("max".into(), max.into());
    }
    v
}

/// The refusal past the write rate: an error item, typed, with how long to wait.
fn too_fast(ns: &str, wait: Duration, rate: f64) -> Value {
    let ms = wait.as_millis().max(1) as u64;
    let item = PlexusStreamItem::Error {
        metadata: StreamMetadata::new(vec![ns.into()], PlexusContext::hash()),
        message: format!("too many writes ({rate:.0} a second sustained): try again in {ms} ms; nothing was written"),
        code: Some(TOO_MANY_WRITES.into()),
        recoverable: true,
        fields: Default::default(),
    };
    let mut v = serde_json::to_value(&item).unwrap_or(Value::Null);
    if let Some(o) = v.as_object_mut() {
        o.insert("error_kind".into(), TOO_MANY_WRITES.into());
        o.insert("retry_after_ms".into(), ms.into());
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
    /// V2-31: each principal's open connections (seated at their first call).
    seats: Mutex<HashMap<String, Vec<Weak<Connection>>>>,
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
        Arc::new(Self { limits, conns: Mutex::new(HashMap::new()), seats: Mutex::new(HashMap::new()) })
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

    /// V2-31: `conn` seated as one of `principal`'s connections — already, or
    /// now if fewer than `limit` (0: no limit) are open, or `exempt` says so
    /// (asked only when the limit is reached); else the limit it is at.
    fn seat(&self, principal: &str, conn: &Arc<Connection>, limit: u32, exempt: impl FnOnce() -> bool) -> Result<(), u32> {
        let mut seats = self.seats.lock().unwrap_or_else(|p| p.into_inner());
        let held = seats.entry(principal.to_string()).or_default();
        held.retain(|w| w.upgrade().is_some_and(|c| !c.is_closed()));
        let me = Arc::downgrade(conn);
        if held.iter().any(|w| w.ptr_eq(&me)) {
            return Ok(());
        }
        if limit > 0 && held.len() as u32 >= limit && !exempt() {
            return Err(limit);
        }
        held.push(me);
        if seats.len() > 4096 {
            seats.retain(|_, v| v.iter().any(|w| w.upgrade().is_some_and(|c| !c.is_closed())));
        }
        Ok(())
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
    fn a_person_and_an_agents_key_have_their_own_limits() {
        let d = CallLimits::default();
        assert_eq!(limit_for("idp:c952acd3", &d), 4);
        assert_eq!(limit_for("oidc:workos/user_01", &d), 4);
        assert_eq!(limit_for("key:54a9677a", &d), 64);
        let v = crowded("menger", 4, "idp:x");
        assert_eq!((v["error_kind"].as_str(), v["max"].as_u64()), (Some(TOO_MANY_CONNECTIONS), Some(4)), "{v}");
    }

    #[test]
    fn a_write_too_fast_is_an_error_item_with_its_wait() {
        let v = too_fast("menger", Duration::from_millis(50), 20.0);
        assert_eq!((v["type"].as_str(), v["error_kind"].as_str(), v["retry_after_ms"].as_u64()), (Some("error"), Some(TOO_MANY_WRITES), Some(50)), "{v}");
        assert_eq!(v["recoverable"], true);
    }

    #[test]
    fn the_defaults_are_the_named_ones() {
        let d = CallLimits::default();
        assert_eq!(d.writes_per_second, WRITES_PER_SECOND);
        assert_eq!((d.per_principal, d.per_agent), (CONNECTIONS_PER_PRINCIPAL, CONNECTIONS_PER_AGENT));
        assert_eq!((d.per_connection, d.idle), (CALLS_PER_CONNECTION, Some(Duration::from_secs(IDLE_SECS))));
        let open = Open::new(d);
        assert_eq!(open.tracked(), 0);
    }
}
