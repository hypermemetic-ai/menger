//! TICKETS-26: a subtask's id, `<parent>.N`, over the real wire, login on
//! (wire_freeze.rs's harness): `tickets.rename` gives a ticket the id under
//! its own parent and refuses, in band, naming the rule, anywhere else; the id
//! is then read by `get`, a create's parent and blocker, the filter's
//! `under`, and an address; the old id stays pinned; a malformed one is
//! refused before the method runs.

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
async fn a_subtask_id_is_given_under_its_parent_and_read_everywhere() {
    let port = start().await;
    let boss = as_key(port, &BOSS, "boss").await;
    for (title, extra) in [("the theme", json!({ "kind": "scope" })), ("area", json!({ "kind": "execution" })), ("three", json!({ "parent": "THEME-2" })), ("four", json!({}))] {
        let mut p = json!({ "epic": "THEME", "title": title });
        p.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
        assert_eq!(call(&boss, "tickets.create", p).await[0]["type"], "created");
    }
    let g = one(&boss, "members.grant", json!({ "principal": principal(&ADA), "scope": "THEME", "role": "editor" })).await;
    assert_ne!(g["type"], "error", "{g}");
    let ada = as_key(port, &ADA, "ada").await;

    // Elsewhere: refused in band, naming the rule; nothing moved.
    let e = one(&ada, "tickets.rename", json!({ "id": "THEME-4", "to": "THEME-2.1" })).await;
    assert_eq!(e["type"], "error", "{e}");
    assert!(e["message"].as_str().unwrap().contains("a subtask id is"), "{e}");
    assert_eq!(one(&ada, "tickets.get", json!({ "id": "THEME-4" })).await["ticket"]["id"], "THEME-4");
    // A malformed id is refused before the method runs.
    let bad = call(&ada, "tickets.get", json!({ "id": "THEME-2.x" })).await;
    assert!(bad.last().unwrap().to_string().contains("not a ticket"), "{bad:?}");

    // Under its parent: renamed, and the answer says the move.
    let done = one(&ada, "tickets.rename", json!({ "id": "theme-3", "to": "theme-2.1" })).await;
    assert_eq!(done["type"], "reshortcoded", "{done}");
    assert_eq!((done["moves"][0]["from"].as_str(), done["moves"][0]["to"].as_str()), (Some("THEME-3"), Some("THEME-2.1")), "{done}");
    // Read by the new id, the old, and the address.
    for id in ["THEME-2.1", "theme-2.1", "THEME-3", "hypermemetic:THEME-2.1"] {
        let t = one(&ada, "tickets.get", json!({ "id": id })).await;
        assert_eq!((t["ticket"]["id"].as_str(), t["ticket"]["parent"].as_str()), (Some("THEME-2.1"), Some("THEME-2")), "{id}: {t}");
    }
    let t = one(&ada, "tickets.get", json!({ "id": "THEME-2.1" })).await;
    assert!(t["ticket"]["address"].as_str().unwrap().ends_with("THEME-2.1"), "{t}");
    // Named as a parent and a blocker; the filter's `under` reads it.
    let made = one(&ada, "tickets.create", json!({ "epic": "THEME", "title": "child", "parent": "THEME-2.1", "blocked_by": ["THEME-2.1"] })).await;
    assert_eq!((made["ticket"]["parent"].as_str(), made["ticket"]["blocked_by"][0].as_str()), (Some("THEME-2.1"), Some("THEME-2.1")), "{made}");
    for under in ["THEME-2.1", "THEME-3"] {
        let listed = call(&ada, "tickets.list", json!({ "filter": format!("under {under}"), "summary": true })).await;
        let mut ids: Vec<&str> = listed.iter().filter_map(|e| e["summary"]["id"].as_str()).collect();
        ids.sort();
        assert_eq!(ids, ["THEME-2.1", "THEME-5"], "under {under}: {listed:?}");
    }
}

/// TICKETS-28: `tickets.subtasks.create` over the wire, as an editor of the
/// shortcode: `.1 .2 .3`, hidden, absent from the nav's views, present in
/// `subtasks.list` and `list --parent`; a missing parent refused in band.
#[tokio::test]
async fn subtasks_create_names_the_next_dot_n_and_hides_it() {
    let port = start().await;
    let boss = as_key(port, &BOSS, "boss").await;
    for (title, extra) in [("the theme", json!({ "kind": "scope" })), ("area", json!({ "kind": "execution" }))] {
        let mut p = json!({ "epic": "THEME", "title": title });
        p.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
        assert_eq!(call(&boss, "tickets.create", p).await[0]["type"], "created");
    }
    let g = one(&boss, "members.grant", json!({ "principal": principal(&ADA), "scope": "THEME", "role": "editor" })).await;
    assert_ne!(g["type"], "error", "{g}");
    let ada = as_key(port, &ADA, "ada").await;

    let mut made = Vec::new();
    for title in ["one", "two", "three"] {
        let t = one(&ada, "tickets.subtasks.create", json!({ "parent": "THEME-2", "title": title, "body": format!("the body of {title}") })).await;
        assert_eq!(t["type"], "created", "{t}");
        made.push(t["ticket"].clone());
    }
    assert_eq!(made.iter().map(|t| t["id"].as_str().unwrap()).collect::<Vec<_>>(), ["THEME-2.1", "THEME-2.2", "THEME-2.3"]);
    assert!(made.iter().all(|t| t["hidden"] == true && t["parent"] == "THEME-2"), "{made:?}");

    // In the parent's subtasks, in order, and its children; in no nav view.
    let subs = call(&ada, "tickets.subtasks.list", json!({ "parent": "THEME-2" })).await;
    let ids = |evs: &[Value]| evs.iter().filter_map(|e| e["summary"]["id"].as_str().map(String::from)).collect::<Vec<_>>();
    assert_eq!(ids(&subs), ["THEME-2.1", "THEME-2.2", "THEME-2.3"], "{subs:?}");
    let kids = call(&ada, "tickets.list", json!({ "parent": "THEME-2", "summary": true })).await;
    assert_eq!(ids(&kids), ["THEME-2.1", "THEME-2.2", "THEME-2.3"], "{kids:?}");
    let top = call(&ada, "tickets.nav.tree", json!({ "epic": "THEME", "filter": "not category done", "matches_only": true })).await;
    assert!(!top.is_empty() && !top.iter().any(|e| e.to_string().contains("THEME-2.")), "{top:?}");
    let all = call(&ada, "tickets.list", json!({ "filter": "in THEME", "summary": true })).await;
    assert_eq!(ids(&all), ["THEME-1", "THEME-2"], "{all:?}");

    // A parent that is no ticket: refused in band, nothing written.
    let e = one(&ada, "tickets.subtasks.create", json!({ "parent": "THEME-99", "title": "x" })).await;
    assert_eq!(e["type"], "error", "{e}");
    assert_eq!(one(&ada, "tickets.subtasks.create", json!({ "parent": "THEME-2", "title": "four" })).await["ticket"]["id"], "THEME-2.4");
}
