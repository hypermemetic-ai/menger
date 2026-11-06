//! TICKETS-6: `tickets.freeze` / `tickets.unfreeze` over the real wire,
//! login on — real WebSocket clients each proving its own Ed25519 key at
//! the upgrade, the `menger.call` envelope (wire_assign.rs's harness).
//!
//! The admin freezes THEME-1 and `watch` carries `frozen_changed`; a create
//! under it is refused `frozen { scope }` naming the unfreeze command; a
//! create in THEME under THEME-4 is refused (no new THEME numbers); a new
//! shortcode hosted under THEME-4 is born; `frozen` filters; an editor may
//! neither freeze nor unfreeze; unfrozen, a create succeeds.

#[path = "support/ports.rs"]
mod ports;
use ed25519_dalek::SigningKey;
use jsonrpsee::core::client::{Subscription, SubscriptionClientT};
use jsonrpsee::core::params::ObjectParams;
use jsonrpsee::ws_client::{HeaderMap, HeaderValue, WsClient, WsClientBuilder};
use menger::build_rpc_offering;
use menger::login::{self, Provider};
use menger::sessions::{Gate, Sessions};
use menger_tickets_api::access::Access;
use plexus_auth_core::adapters::{key_principal, sign_key_proof};
use plexus_core::plexus::DynamicHub;
use plexus_transport::TransportServer;
use serde_json::{json, Value};
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Arc;
use std::time::Duration;

/// The operator's key (`--admin`): admin on `*`.
const BOSS: [u8; 32] = [0xb0; 32];
/// ada, an editor of THEME.
const ADA: [u8; 32] = [0xad; 32];
fn principal(seed: &[u8; 32]) -> String {
    key_principal(SigningKey::from_bytes(seed).verifying_key().as_bytes()).to_string()
}

fn now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as i64
}

fn nonce() -> [u8; 32] {
    static N: AtomicU8 = AtomicU8::new(1);
    let mut n = [0u8; 32];
    n[0] = N.fetch_add(1, Ordering::Relaxed);
    n[1] = std::process::id() as u8;
    n[2..10].copy_from_slice(&std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos().to_le_bytes()[..8]);
    n
}

async fn free_port() -> u16 {
    ports::free_port()
}

/// menger as `main` wires it with `--auth-provider key --require-login
/// --admin key:<boss>`.
async fn start() -> u16 {
    let port = free_port().await;
    let providers: Vec<Provider> = vec!["key".parse().unwrap()];
    let dir = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
    let sessions = Arc::new(Sessions::new(3600, &menger::login_names()));
    let access = Access::with_login(menger::session_table(&sessions), vec![principal(&BOSS)]).minting(menger::session_table(&sessions));
    let service = build_rpc_offering(&dir.path().join("t.db"), None, Default::default(), Vec::new(), access.clone()).await.unwrap();
    let carriers = login::boundary_at(&providers, &login::socket_urls(port, None)).unwrap();
    let gate = Arc::new(Gate::new(sessions, carriers).noticing(menger::MengerAccess(access)));
    let conv = |arc: Arc<DynamicHub>| DynamicHub::arc_into_rpc_module(arc).map_err(|e| anyhow::anyhow!("{e}"));
    tokio::spawn(async move {
        let _keep = dir;
        TransportServer::builder(service, conv)
            .with_websocket(port)
            .with_session_validator(gate)
            .anonymous_view()
            .credential_cookies(["access_token"])
            .build()
            .await
            .unwrap()
            .serve()
            .await
            .unwrap();
    });
    for _ in 0..100 {
        if tokio::net::TcpStream::connect(("127.0.0.1", port)).await.is_ok() {
            return port;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("server never came up on {port}");
}

/// A connection as the key `seed`.
async fn as_key(port: u16, seed: &[u8; 32], agent: &str) -> WsClient {
    let proof = sign_key_proof(seed, &format!("ws://127.0.0.1:{port}"), agent, &nonce(), now());
    let mut map = HeaderMap::new();
    map.insert("authorization", HeaderValue::from_str(&format!("Bearer {proof}")).unwrap());
    WsClientBuilder::default().set_headers(map).build(format!("ws://127.0.0.1:{port}")).await.expect("admitted")
}

async fn subscribe(c: &WsClient, method: &str, params: Value) -> Subscription<Value> {
    let mut p = ObjectParams::new();
    p.insert("method", method).unwrap();
    p.insert("params", params).unwrap();
    c.subscribe("menger.call", p, "menger.call_unsub").await.unwrap()
}

/// A finite call's data items — and an error item, last, if there is one.
async fn call(c: &WsClient, method: &str, params: Value) -> Vec<Value> {
    let mut s = subscribe(c, method, params).await;
    let mut out = Vec::new();
    while let Some(Ok(item)) = s.next().await {
        match item["type"].as_str() {
            // PERF-15: a page's `rows` read as the per-row events it carries.
            Some("data") => match item["content"].clone() {
                c if c["type"] == "rows" => out.extend(c["items"].as_array().cloned().unwrap_or_default()),
                c => out.push(c),
            },
            Some("error") => {
                out.push(item);
                break;
            }
            _ => break,
        }
    }
    out
}

async fn one(c: &WsClient, method: &str, params: Value) -> Value {
    let mut evs = call(c, method, params).await;
    assert_eq!(evs.len(), 1, "{method}: {evs:?}");
    evs.remove(0)
}

#[tokio::test]
async fn freeze_refuses_growth_and_unfreeze_lets_it_grow() {
    let port = start().await;
    let boss = as_key(port, &BOSS, "boss").await;
    for (title, extra) in [("the theme", json!({ "kind": "scope" })), ("two", json!({})), ("three", json!({})), ("four", json!({ "kind": "execution" }))] {
        let mut p = json!({ "epic": "THEME", "title": title });
        p.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
        assert_eq!(call(&boss, "tickets.create", p).await[0]["type"], "created");
    }
    let g = one(&boss, "members.grant", json!({ "principal": principal(&ADA), "scope": "THEME", "role": "editor" })).await;
    assert_ne!(g["type"], "error", "{g}");
    let ada = as_key(port, &ADA, "ada").await;
    let mut watch = subscribe(&boss, "tickets.watch", json!({ "epic": "THEME" })).await;
    tokio::time::sleep(Duration::from_millis(200)).await;

    // An editor may not freeze.
    let refused = call(&ada, "tickets.freeze", json!({ "id": "THEME-1" })).await;
    assert!(refused.last().unwrap().to_string().contains("forbidden"), "{refused:?}");

    // The admin freezes: the ticket back, frozen; the watch hears it.
    let done = one(&boss, "tickets.freeze", json!({ "id": "theme-1" })).await;
    assert_eq!((done["type"].as_str(), done["ticket"]["frozen"].as_bool()), (Some("frozen_changed"), Some(true)), "{done}");
    let heard = next(&mut watch).await;
    assert_eq!((heard["type"].as_str(), heard["ticket"]["id"].as_str()), (Some("frozen_changed"), Some("THEME-1")), "{heard}");
    // Not a scope: refused in band.
    let not = one(&boss, "tickets.freeze", json!({ "id": "THEME-2" })).await;
    assert_eq!(not["type"], "error", "{not}");

    // A child of THEME-1: refused, typed, naming how to unfreeze.
    let e = one(&ada, "tickets.create", json!({ "epic": "THEME", "title": "more", "parent": "THEME-1" })).await;
    assert_eq!((e["type"].as_str(), e["scope"].as_str()), (Some("frozen"), Some("THEME-1")), "{e}");
    assert!(e["message"].as_str().unwrap().contains("unfreeze it first (tickets unfreeze --id THEME-1)"), "{e}");
    // A new THEME number under THEME-4: refused.
    let e = one(&ada, "tickets.create", json!({ "epic": "THEME", "title": "under four", "parent": "THEME-4" })).await;
    assert_eq!((e["type"].as_str(), e["scope"].as_str()), (Some("frozen"), Some("THEME-1")), "{e}");
    assert!(e["message"].as_str().unwrap().contains("no new THEME numbers"), "{e}");
    // A shortcode of its own, hosted under THEME-4: born.
    let look = one(&boss, "tickets.create", json!({ "epic": "LOOK", "title": "a look", "kind": "scope", "parent": "THEME-4" })).await;
    assert_eq!((look["type"].as_str(), look["ticket"]["id"].as_str()), (Some("created"), Some("LOOK-1")), "{look}");
    // `frozen` filters.
    let listed = call(&boss, "tickets.list", json!({ "filter": "frozen", "summary": true })).await;
    let ids: Vec<&str> = listed.iter().filter_map(|e| e["summary"]["id"].as_str()).collect();
    assert_eq!(ids, ["THEME-1"], "{listed:?}");

    // An editor may not unfreeze; the admin does, and it grows again.
    let refused = call(&ada, "tickets.unfreeze", json!({ "id": "THEME-1" })).await;
    assert!(refused.last().unwrap().to_string().contains("forbidden"), "{refused:?}");
    let done = one(&boss, "tickets.unfreeze", json!({ "id": "THEME-1" })).await;
    assert_eq!((done["type"].as_str(), done["ticket"].get("frozen")), (Some("frozen_changed"), None), "{done}");
    let made = one(&ada, "tickets.create", json!({ "epic": "THEME", "title": "more", "parent": "THEME-1" })).await;
    assert_eq!((made["type"].as_str(), made["ticket"]["id"].as_str()), (Some("created"), Some("THEME-5")), "{made}");
}
/// The next data item of an open stream, within a few seconds.
async fn next(s: &mut Subscription<Value>) -> Value {
    loop {
        let item = tokio::time::timeout(Duration::from_secs(10), s.next()).await.expect("an item in time").expect("open").unwrap();
        if item["type"].as_str() == Some("data") {
            return item["content"].clone();
        }
    }
}
