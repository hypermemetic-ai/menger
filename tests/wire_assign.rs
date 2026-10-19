//! TICKETS-9: `tickets.assign` over the real wire, login on — real
//! WebSocket clients each proving its own Ed25519 key at the upgrade, the
//! `menger.call` envelope (wire_chats.rs's harness).
//!
//! An editor assigns an agent who reads the ticket; a principal who cannot
//! read it is refused by name and nothing is written (by `assign` and by
//! `update_many { assign }`); `history` ends with the assignment records;
//! `watch` carries `assignees_changed`; `assigned me` is the reader's own
//! and `unassigned` the rest. Every server has its own throwaway database
//! and port. The keys are fixtures.

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
/// ada, a person (editor of A); builder, an agent (reads A); sam, of B.
const ADA: [u8; 32] = [0xad; 32];
const BUILDER: [u8; 32] = [0xb1; 32];
const SAM: [u8; 32] = [0x5a; 32];

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
    static HANDED: std::sync::Mutex<Vec<u16>> = std::sync::Mutex::new(Vec::new());
    loop {
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = l.local_addr().unwrap().port();
        let mut handed = HANDED.lock().unwrap();
        if !handed.contains(&port) {
            handed.push(port);
            return port;
        }
    }
}

/// menger as `main` wires it with `--auth-provider key --require-login
/// --admin key:<boss>`.
async fn start() -> u16 {
    let port = free_port().await;
    let providers: Vec<Provider> = vec!["key".parse().unwrap()];
    let dir = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
    let sessions = Arc::new(Sessions::new(3600, &menger::login_names()));
    let access = Access::with_login(menger::session_table(&sessions), vec![principal(&BOSS)]).minting(menger::session_table(&sessions));
    let hub = build_rpc_offering(&dir.path().join("t.db"), None, Default::default(), Vec::new(), access.clone()).await.unwrap();
    let carriers = login::boundary_at(&providers, &login::socket_urls(port, None)).unwrap();
    let gate = Arc::new(Gate::new(sessions, carriers).noticing(menger::MengerAccess(access)));
    let conv = |arc: Arc<DynamicHub>| DynamicHub::arc_into_rpc_module(arc).map_err(|e| anyhow::anyhow!("{e}"));
    tokio::spawn(async move {
        let _keep = dir;
        TransportServer::builder(hub, conv)
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
            Some("data") => out.push(item["content"].clone()),
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
/// The next data item of an open stream, within a few seconds.
async fn next(s: &mut Subscription<Value>) -> Value {
    loop {
        let item = tokio::time::timeout(Duration::from_secs(10), s.next()).await.expect("an item in time").expect("open").unwrap();
        if item["type"].as_str() == Some("data") {
            return item["content"].clone();
        }
    }
}

#[tokio::test]
async fn assign_who_reads_refuse_who_cannot_and_keep_the_history() {
    let port = start().await;
    let boss = as_key(port, &BOSS, "boss").await;
    for (epic, title, extra) in [("A", "A root", json!({ "kind": "scope" })), ("A", "A work", json!({ "parent": "A-1" })), ("A", "A more", json!({ "parent": "A-1" })), ("B", "B root", json!({ "kind": "scope" }))] {
        let mut p = json!({ "epic": epic, "title": title });
        p.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
        assert_eq!(call(&boss, "tickets.create", p).await[0]["type"], "created");
    }
    for (who, scope, role) in [(&ADA, "A", "editor"), (&BUILDER, "A", "viewer"), (&SAM, "B", "viewer")] {
        let g = one(&boss, "members.grant", json!({ "principal": principal(who), "scope": scope, "role": role })).await;
        assert_ne!(g["type"], "error", "{g}");
    }
    let ada = as_key(port, &ADA, "ada").await;
    let builder = as_key(port, &BUILDER, "builder").await;
    let mut watch = subscribe(&ada, "tickets.watch", json!({ "epic": "A" })).await;

    // An agent who reads A: assigned, the whole ticket back.
    let done = one(&ada, "tickets.assign", json!({ "id": "a-2", "add": [principal(&BUILDER)], "message": "yours" })).await;
    assert_eq!(done["type"], "assignees_changed", "{done}");
    assert_eq!((done["ticket"]["assignees"].clone(), done["added"].clone()), (json!([principal(&BUILDER)]), json!([principal(&BUILDER)])));
    let heard = next(&mut watch).await;
    assert_eq!((heard["type"].as_str(), heard["ticket"]["id"].as_str()), (Some("assignees_changed"), Some("A-2")), "{heard}");

    // sam reads only B: refused, naming why; nothing written.
    let sam = one(&ada, "tickets.assign", json!({ "id": "A-3", "add": [principal(&SAM), principal(&BUILDER)] })).await;
    assert_eq!(sam["type"], "error", "{sam}");
    let msg = sam["message"].as_str().unwrap();
    assert!(msg.contains(&principal(&SAM)) && msg.contains("cannot read A-3") && msg.contains("nothing written"), "{msg}");
    assert!(!msg.contains(&principal(&BUILDER)), "only who cannot: {msg}");
    let many = one(&ada, "tickets.update_many", json!({ "ids": ["A-2", "A-3"], "assign": [principal(&SAM)] })).await;
    assert_eq!(many["type"], "error", "{many}");
    assert!(many.to_string().contains("cannot read"), "{many}");
    let a3 = one(&ada, "tickets.get", json!({ "id": "A-3" })).await;
    assert_eq!(a3["ticket"].get("assignees"), None, "nothing written: {a3}");
    // Not a principal at all: refused at dispatch, naming the parameter.
    let bad = call(&ada, "tickets.assign", json!({ "id": "A-3", "add": ["bob"] })).await;
    assert!(bad.last().unwrap().to_string().contains("add"), "{bad:?}");
    // A viewer may not assign.
    let refused = call(&builder, "tickets.assign", json!({ "id": "A-3", "add": [principal(&BUILDER)] })).await;
    assert!(refused.last().unwrap().to_string().contains("forbidden") || refused.last().unwrap()["type"] == "error", "{refused:?}");

    // update_many assigns too.
    let many = one(&ada, "tickets.update_many", json!({ "ids": ["A-3"], "assign": [principal(&ADA)] })).await;
    assert_eq!((many["type"].as_str(), many["changed"].clone()), (Some("updated_many"), json!(["assignees"])), "{many}");

    // history: the transitions, then the assignment records.
    let h = call(&ada, "tickets.history", json!({ "id": "A-2" })).await;
    let rec = h.iter().find(|e| e["type"] == "assign_record").expect("an assign_record");
    assert_eq!(
        (rec["record"]["principal"].as_str(), rec["record"]["action"].as_str(), rec["record"]["actor"].as_str(), rec["record"]["message"].as_str()),
        (Some(principal(&BUILDER).as_str()), Some("assigned"), Some(principal(&ADA).as_str()), Some("yours"))
    );
    assert_eq!(h.last().unwrap()["type"], "history_end");

    // The cells: `assigned me` is the reader's own; `unassigned` the rest.
    let ids = |evs: Vec<Value>| evs.iter().filter_map(|e| e["summary"]["id"].as_str().map(str::to_string)).collect::<Vec<_>>();
    assert_eq!(ids(call(&builder, "tickets.list", json!({ "filter": "assigned me", "summary": true })).await), ["A-2"]);
    assert_eq!(ids(call(&ada, "tickets.list", json!({ "filter": "assigned me", "summary": true })).await), ["A-3"]);
    assert_eq!(ids(call(&ada, "tickets.list", json!({ "filter": "unassigned", "summary": true })).await), ["A-1"]);
    let unassigned = one(&ada, "tickets.assign", json!({ "id": "A-2", "remove": [principal(&BUILDER)] })).await;
    assert_eq!((unassigned["removed"].clone(), unassigned["ticket"].get("assignees")), (json!([principal(&BUILDER)]), None), "none: omitted");
    assert!(ids(call(&builder, "tickets.list", json!({ "filter": "assigned me", "summary": true })).await).is_empty());
}
