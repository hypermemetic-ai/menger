//! IDENTITY-42: agents log in over the real wire (IDENTITY-40's contract).
//!
//! An agent proves its Ed25519 key at the upgrade (`plexus-key.…`, the
//! `key` provider), mints a session with `session.login`, and its later
//! connections carry only the `mgs_…` token — as `Authorization: Bearer` or
//! the `access_token` cookie. A session's scope is its ceiling: every check
//! needs the key's grants AND it. Every server here requires login, has its
//! own throwaway database and its own port; prod and test are never
//! touched. The keys are fixtures.

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
/// An agent's key, granted by the boss.
const AGENT: [u8; 32] = [0xa9; 32];

fn principal(seed: &[u8; 32]) -> String {
    key_principal(SigningKey::from_bytes(seed).verifying_key().as_bytes()).to_string()
}

fn now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as i64
}

/// A fresh nonce each call.
fn nonce() -> [u8; 32] {
    static N: AtomicU8 = AtomicU8::new(1);
    let mut n = [0u8; 32];
    n[0] = N.fetch_add(1, Ordering::Relaxed);
    n[1] = std::process::id() as u8;
    n[2..10].copy_from_slice(&std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos().to_le_bytes()[..8]);
    n
}

fn proof(seed: &[u8; 32], port: u16, agent: &str) -> String {
    sign_key_proof(seed, &format!("ws://127.0.0.1:{port}"), agent, &nonce(), now())
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
/// --admin key:<boss>` (AUTH-7: the anonymous view, the gate).
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
            // AUTH-7: as main wires it (AUTH-6).
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

async fn connect(port: u16, headers: &[(&'static str, String)]) -> Result<WsClient, String> {
    let mut map = HeaderMap::new();
    for (k, v) in headers {
        map.insert(*k, HeaderValue::from_str(v).unwrap());
    }
    WsClientBuilder::default().set_headers(map).build(format!("ws://127.0.0.1:{port}")).await.map_err(|e| e.to_string())
}

async fn bearer(port: u16, value: &str) -> WsClient {
    connect(port, &[("authorization", format!("Bearer {value}"))]).await.expect("admitted")
}

async fn call(c: &WsClient, method: &str, params: Value) -> Vec<Value> {
    let mut p = ObjectParams::new();
    p.insert("method", method).unwrap();
    p.insert("params", params).unwrap();
    let mut s: Subscription<Value> = c.subscribe("menger.call", p, "menger.call_unsub").await.unwrap();
    let mut out = Vec::new();
    while let Some(Ok(item)) = s.next().await {
        match item["type"].as_str() {
            Some("data") => out.push(item["content"].clone()),
            // AUTH-7: the gate's refusal is the stream's error item, typed
            // (`error_kind`, `grant`, `scope`, `roles`, `held_on`).
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

/// `session.login`'s token, asserting it is a login.
async fn login(c: &WsClient, params: Value) -> Value {
    let l = one(c, "session.login", params).await;
    assert_eq!(l["type"], "login", "{l}");
    l
}

/// A, B and C, each a scope with one build under it: A-1 · A-2, B-1 · B-2,
/// C-1 · C-2 (C hosted under A-1).
async fn tickets(admin: &WsClient) {
    for (epic, parent) in [("A", None), ("B", None), ("C", Some("A-1"))] {
        let made = call(admin, "tickets.create", json!({ "epic": epic, "title": format!("{epic} root"), "kind": "scope", "parent": parent })).await;
        assert_eq!(made[0]["type"], "created", "{made:?}");
        let made = call(admin, "tickets.create", json!({ "epic": epic, "title": format!("{epic} work"), "parent": format!("{epic}-1") })).await;
        assert_eq!(made[0]["type"], "created", "{made:?}");
    }
}

async fn amend(c: &WsClient, id: &str) -> Value {
    one(c, "tickets.amend", json!({ "id": id, "title": format!("{id} amended") })).await
}

#[tokio::test]
async fn a_key_proof_opens_the_socket_as_the_key_named_by_its_agent() {
    let port = start().await;
    let c = bearer(port, &proof(&AGENT, port, "worker-3")).await;
    let me = one(&c, "session.whoami", json!({})).await;
    assert_eq!(me, json!({ "type": "identity", "principal": principal(&AGENT), "name": "worker-3", "roles": [], "tenant": null, "provider": "key" }));
    assert!(principal(&AGENT).starts_with("key:") && principal(&AGENT).len() == 68);
    // The `access_token` cookie carries a proof as well (synapse's -t).
    let c = connect(port, &[("cookie", format!("access_token={}", proof(&AGENT, port, "worker-4")))]).await.unwrap();
    assert_eq!(one(&c, "session.whoami", json!({})).await["name"], "worker-4");
    // Not offered to people.
    assert!(call(&c, "session.providers", json!({})).await.is_empty());
}

#[tokio::test]
async fn a_session_token_opens_later_connections_by_itself() {
    let port = start().await;
    let c = bearer(port, &proof(&AGENT, port, "worker-3")).await;
    let l = login(&c, json!({})).await;
    let token = l["token"].as_str().unwrap().to_string();
    assert!(token.starts_with("mgs_") && token.len() == 47, "{token}");
    assert_eq!((l["principal"].as_str(), l["agent"].as_str()), (Some(principal(&AGENT).as_str()), Some("worker-3")));
    assert_eq!(l["scope"], json!([]));
    let expires = l["expires_at"].as_i64().unwrap();
    assert!((now() + 12 * 3600 - 5..=now() + 12 * 3600).contains(&expires), "12 h by default: {expires}");

    for c in [bearer(port, &token).await, connect(port, &[("cookie", format!("theme=dark; access_token={token}"))]).await.unwrap()] {
        let me = one(&c, "session.whoami", json!({})).await;
        assert_eq!((me["principal"].as_str(), me["name"].as_str(), me["provider"].as_str()), (Some(principal(&AGENT).as_str()), Some("worker-3"), Some("key")));
    }
    let short = login(&c, json!({ "ttl_secs": 60 })).await;
    assert!(short["expires_at"].as_i64().unwrap() <= now() + 60);
    let refused = one(&c, "session.login", json!({ "ttl_secs": 86_401 })).await;
    assert_eq!(refused["type"], "error", "{refused}");
    assert!(connect(port, &[("authorization", "Bearer mgs_forged".into())]).await.unwrap_err().contains("401"));
}

#[tokio::test]
async fn a_scope_is_a_ceiling_on_every_check() {
    let port = start().await;
    let boss = bearer(port, &proof(&BOSS, port, "boss")).await;
    tickets(&boss).await;

    // The boss's key is admin on *; its session asked editor on A writes
    // only in A (C is hosted under A-1), and administers nothing.
    let scoped = login(&boss, json!({ "scope": ["editor@A"] })).await;
    // ORG-4: kept as the store keeps a scope — the shortcode in its org.
    assert_eq!(scoped["scope"], json!(["editor@hypermemetic:A"]));
    let s = bearer(port, scoped["token"].as_str().unwrap()).await;
    assert_eq!(amend(&s, "A-2").await["type"], "amended");
    assert_eq!(amend(&s, "C-2").await["type"], "amended", "C is hosted under A");
    let no = amend(&s, "B-2").await;
    // AUTH-7: the gate's typed refusal, where IDENTITY-42 said "needs editor on B".
    assert_eq!(
        (no["error_kind"].as_str(), no["grant"].as_str(), no["scope"].as_str()),
        (Some("forbidden"), Some("tickets.amend"), Some("B-2")),
        "{no}"
    );
    // ORG-4: the ticket as written — the same words for one out of reach as for none.
    assert_eq!(no["message"], "needs tickets.amend on B-2", "{no}");
    assert_eq!(no["held_on"], json!(["hypermemetic:A"]), "held on A, within the session's editor@A: {no}");
    assert_eq!(one(&s, "tickets.get", json!({ "id": "B-2" })).await["type"], "not_found", "B is out of sight");
    let all = call(&s, "tickets.list", json!({})).await;
    let listed: Vec<String> = all.iter().filter_map(|e| e["ticket"]["id"].as_str().or(e["summary"]["id"].as_str()).map(str::to_string)).collect();
    assert_eq!(listed, ["A-1", "A-2", "C-1", "C-2"], "{all:?}");
    assert_eq!(one(&s, "members.grant", json!({ "principal": principal(&AGENT), "scope": "A", "role": "viewer" })).await["type"], "error", "no admin under editor@A");
    assert_eq!(one(&s, "members.requests", json!({})).await["type"], "error");
    assert_eq!(call(&s, "members.mine", json!({})).await[0]["admin_everywhere"], false, "the key's grants stand; the session is held under them");

    // One ticket as the scope: it and what is under it.
    let t = bearer(port, login(&boss, json!({ "scope": ["editor@A-2"] })).await["token"].as_str().unwrap()).await;
    assert_eq!(amend(&t, "A-2").await["type"], "amended");
    assert_eq!(amend(&t, "A-1").await["type"], "error");

    // A session minted from the scoped session stays scoped, whatever it asks.
    let wider = login(&s, json!({ "scope": ["admin@*"] })).await;
    let w = bearer(port, wider["token"].as_str().unwrap()).await;
    assert_eq!(amend(&w, "A-2").await["type"], "amended");
    assert_eq!(amend(&w, "B-2").await["type"], "error", "never wider than the session it came from");
    let inherited = login(&s, json!({})).await;
    assert_eq!(inherited["scope"], json!(["editor@hypermemetic:A"]));
    assert_eq!(amend(&bearer(port, inherited["token"].as_str().unwrap()).await, "B-2").await["type"], "error");

    // The boss's own unscoped session is admin everywhere.
    let full = bearer(port, login(&boss, json!({})).await["token"].as_str().unwrap()).await;
    assert_eq!(amend(&full, "B-2").await["type"], "amended");
}

#[tokio::test]
async fn a_key_s_grants_are_the_most_a_session_holds() {
    let port = start().await;
    let boss = bearer(port, &proof(&BOSS, port, "boss")).await;
    tickets(&boss).await;
    // members.grant takes a key: principal.
    let g = one(&boss, "members.grant", json!({ "principal": principal(&AGENT), "scope": "A", "role": "editor" })).await;
    assert_eq!((g["type"].as_str(), g["grant"]["principal"].as_str()), (Some("grant"), Some(principal(&AGENT).as_str())), "{g}");

    let agent = bearer(port, &proof(&AGENT, port, "worker-3")).await;
    let greedy = login(&agent, json!({ "scope": ["admin@*"] })).await;
    let s = bearer(port, greedy["token"].as_str().unwrap()).await;
    assert_eq!(amend(&s, "A-2").await["type"], "amended", "what the key holds");
    assert_eq!(amend(&s, "B-2").await["type"], "error", "no more");
    assert_eq!(one(&s, "members.grant", json!({ "principal": principal(&BOSS), "scope": "A", "role": "viewer" })).await["type"], "error", "editor, not admin");
}

#[tokio::test]
async fn logout_ends_the_session_everywhere() {
    let port = start().await;
    let boss = bearer(port, &proof(&BOSS, port, "boss")).await;
    let token = login(&boss, json!({})).await["token"].as_str().unwrap().to_string();
    let (a, b) = (bearer(port, &token).await, bearer(port, &token).await);
    assert_eq!(one(&a, "session.logout", json!({})).await, json!({ "type": "logged_out" }));
    let gone = one(&b, "tickets.health", json!({})).await;
    assert!(gone["message"].as_str().unwrap().contains("signed out"), "an open connection on it stops answering: {gone}");
    // AUTH-11: a sign-in that ended proves no one — session.whoami, which
    // only a signed-in caller reaches, is refused as to anyone anonymous.
    let who = one(&b, "session.whoami", json!({})).await;
    assert_eq!((who["error_kind"].as_str(), who["message"].as_str().is_some_and(|m| m.contains("signed out"))), (Some("unauthenticated"), true), "{who}");
    assert!(connect(port, &[("authorization", format!("Bearer {token}"))]).await.unwrap_err().contains("401"), "the token opens nothing");
    assert!(connect(port, &[("cookie", format!("access_token={token}"))]).await.unwrap_err().contains("401"));
    // A proof-opened connection holds no session to end.
    assert_eq!(one(&boss, "session.logout", json!({})).await["type"], "error");
}

#[tokio::test]
async fn a_proof_replayed_stale_future_or_for_another_server_is_refused_at_the_upgrade() {
    let port = start().await;
    let once = proof(&AGENT, port, "worker-3");
    bearer(port, &once).await;
    let aud = format!("ws://127.0.0.1:{port}");
    let refused = [
        once,
        sign_key_proof(&AGENT, &aud, "worker-3", &nonce(), now() - 61),
        sign_key_proof(&AGENT, &aud, "worker-3", &nonce(), now() + 120),
        sign_key_proof(&AGENT, "ws://127.0.0.1:4455", "worker-3", &nonce(), now()),
        sign_key_proof(&AGENT, &aud, "no spaces", &nonce(), now()),
    ];
    for (i, p) in refused.iter().enumerate() {
        let e = connect(port, &[("authorization", format!("Bearer {p}"))]).await.unwrap_err();
        assert!(e.contains("401"), "#{i}: {e}");
    }
    // localhost is this server too.
    bearer(port, &sign_key_proof(&AGENT, &format!("ws://localhost:{port}"), "worker-3", &nonce(), now())).await;
}

async fn heard(w: &mut Subscription<Value>) -> Vec<String> {
    let mut ids = Vec::new();
    while let Ok(Some(Ok(item))) = tokio::time::timeout(Duration::from_millis(600), w.next()).await {
        if item["type"] == "data" && item["content"]["type"] == "amended" {
            ids.push(item["content"]["ticket"]["id"].as_str().unwrap().to_string());
        }
    }
    ids
}

#[tokio::test]
async fn a_scoped_session_s_watch_hears_only_its_scope_as_grants_move() {
    let port = start().await;
    let boss = bearer(port, &proof(&BOSS, port, "boss")).await;
    tickets(&boss).await;
    one(&boss, "members.grant", json!({ "principal": principal(&AGENT), "scope": "*", "role": "editor" })).await;
    let agent = bearer(port, &proof(&AGENT, port, "worker-3")).await;
    let s = bearer(port, login(&agent, json!({ "scope": ["viewer@A"] })).await["token"].as_str().unwrap()).await;
    let mut p = ObjectParams::new();
    p.insert("method", "tickets.watch").unwrap();
    p.insert("params", json!({})).unwrap();
    let mut watch: Subscription<Value> = s.subscribe("menger.call", p, "menger.call_unsub").await.unwrap();
    tokio::time::sleep(Duration::from_millis(200)).await;
    amend(&boss, "B-2").await;
    amend(&boss, "A-2").await;
    assert_eq!(heard(&mut watch).await, ["A-2"]);
    // IDENTITY-37: the key's grants move under the open watch; the scope still holds.
    one(&boss, "members.grant", json!({ "principal": principal(&AGENT), "scope": "B", "role": "admin" })).await;
    amend(&boss, "B-2").await;
    amend(&boss, "C-2").await;
    assert_eq!(heard(&mut watch).await, ["C-2"]);
}
