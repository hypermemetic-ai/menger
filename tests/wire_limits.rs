//! ACCESS-29 (ACCESS-4 M1): the socket's limits over the real protocol —
//! the calls one connection may have open (`menger::calls`), a connection
//! idle with none open closed, and AUTH-12's connection quotas as main sets
//! them (`--max-connections-per-ip` …).


/// ACCESS-24: the test provider — every server runs `key`, every client is its `--admin`.
mod support;
use jsonrpsee::core::client::{Subscription, SubscriptionClientT};
use jsonrpsee::core::params::ObjectParams;
use jsonrpsee::ws_client::{WsClient, WsClientBuilder};
use menger::calls::{CallLimits, TOO_MANY_CALLS};
use plexus_transport::{ConnectionQuotas, TransportServer};
use serde_json::{json, Value};
use std::time::Duration;

async fn free_port() -> u16 {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    listener.local_addr().unwrap().port()
}

/// menger on a free port, its `menger.call` held to `limits`, its sockets to
/// `quotas`.
async fn start(limits: CallLimits, quotas: ConnectionQuotas) -> u16 {
    let dir = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
    let login = support::Login::new();
    let hub = support::hub(&dir.path().join("t.db"), &login).await;
    let port = free_port().await;
    let gate = login.gate(port);
    tokio::spawn(async move {
        let _keep = dir;
        TransportServer::builder(hub, move |hub| menger::calls::rpc_module(hub, limits))
            .with_websocket(port)
            .with_session_validator(gate)
            .anonymous_view()
            .credential_cookies(["access_token"])
            .with_connection_quotas(quotas)
            .build()
            .await
            .unwrap()
            .serve()
            .await
            .unwrap();
    });
    for _ in 0..100 {
        if tokio::net::TcpStream::connect(("127.0.0.1", port)).await.is_ok() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    port
}

async fn connect(port: u16) -> Result<WsClient, String> {
    WsClientBuilder::default().set_headers(support::admin_headers(port)).build(format!("ws://127.0.0.1:{port}")).await.map_err(|e| e.to_string())
}

async fn call(c: &WsClient, method: &str, params: Value) -> Subscription<Value> {
    let mut p = ObjectParams::new();
    p.insert("method", method).unwrap();
    p.insert("params", params).unwrap();
    c.subscribe("menger.call", p, "menger.call_unsub").await.unwrap()
}

/// A call's first item.
async fn first(sub: &mut Subscription<Value>) -> Value {
    tokio::time::timeout(Duration::from_secs(5), sub.next()).await.expect("an item").expect("open").unwrap()
}

#[tokio::test]
async fn a_call_past_the_connection_s_limit_is_refused_typed_and_a_closed_watch_gives_its_place_back() {
    let port = start(CallLimits { per_connection: 3, idle: None }, ConnectionQuotas::default()).await;
    let c = connect(port).await.unwrap();
    let mut watches = Vec::new();
    for _ in 0..3 {
        watches.push(call(&c, "tickets.watch", json!({})).await);
    }
    // The three are open (a watch says nothing until something changes).
    tokio::time::sleep(Duration::from_millis(100)).await;
    let mut over = call(&c, "tickets.health", json!({})).await;
    let item = first(&mut over).await;
    assert_eq!((item["type"].as_str(), item["error_kind"].as_str(), item["max"].as_u64()), (Some("error"), Some(TOO_MANY_CALLS), Some(3)), "{item}");
    assert_eq!(item["recoverable"], true);
    // Another connection has its own.
    let other = connect(port).await.unwrap();
    let mut health = call(&other, "tickets.health", json!({})).await;
    assert_eq!(first(&mut health).await["type"], "data", "the dispatch is the hub's");
    // Let one watch go: its place comes back at once, not at its next event.
    watches.pop().unwrap().unsubscribe().await.unwrap();
    let mut again = None;
    for _ in 0..50 {
        let mut h = call(&c, "tickets.health", json!({})).await;
        let item = first(&mut h).await;
        if item["type"] == "data" {
            again = Some(item);
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(again.is_some(), "the unsubscribed watch gave its place back");
    // An answered call holds no place: many in turn, all answered.
    for _ in 0..10 {
        let mut h = call(&c, "tickets.health", json!({})).await;
        assert_eq!(first(&mut h).await["type"], "data");
    }
}

#[tokio::test]
async fn a_connection_idle_with_no_call_open_is_closed_and_an_open_watch_keeps_it() {
    let port = start(CallLimits { per_connection: 16, idle: Some(Duration::from_secs(2)) }, ConnectionQuotas::default()).await;
    let idle = connect(port).await.unwrap();
    let mut h = call(&idle, "tickets.health", json!({})).await;
    assert_eq!(first(&mut h).await["type"], "data");
    drop(h);
    let watching = connect(port).await.unwrap();
    let _watch = call(&watching, "tickets.watch", json!({})).await;
    tokio::time::sleep(Duration::from_secs(5)).await;
    assert!(!idle.is_connected(), "idle past 2 s with nothing open: closed by the server");
    assert!(watching.is_connected(), "a watch open is not idle");
    let mut h = call(&watching, "tickets.health", json!({})).await;
    assert_eq!(first(&mut h).await["type"], "data");
}

#[tokio::test]
async fn the_connection_quotas_main_sets_refuse_past_them() {
    // AUTH-12's quotas, as `--max-connections-per-ip` sets them: without a
    // validator, every socket is its IP's.
    let quotas = ConnectionQuotas { max_per_ip: 2, ..ConnectionQuotas::default() };
    let port = start(CallLimits::default(), quotas).await;
    let a = connect(port).await.unwrap();
    let b = connect(port).await.unwrap();
    let refused = connect(port).await.unwrap_err();
    assert!(refused.contains("429"), "{refused}");
    drop(a);
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(connect(port).await.is_ok(), "a closed socket gives its place back");
    drop(b);
}
