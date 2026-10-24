//! ORG-13: `epics` over the real wire, login on — real WebSocket clients
//! each proving its own Ed25519 key at the upgrade, the `menger.call`
//! envelope (wire_freeze.rs's harness).
//!
//! The admin makes `fresh`; an editor cannot. Ada (an editor of the org)
//! lists both and is still on `genesis`, the default (ORG-23: making an
//! epic moves nobody); she switches to `fresh` and files `MENGER-1` there
//! (`--epic hypermemetic:fresh:MENGER`), and her bare `MENGER-1` is that
//! one; a filtered list answers her epic unless it names `epic`; the nav's
//! groups are hers; a move in renumbers and the old id still answers; she
//! switches to genesis and reads it again.

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

#[tokio::test]
async fn a_new_epic_starts_fresh_and_a_reader_reads_in_hers() {
    let port = start().await;
    let boss = as_key(port, &BOSS, "boss").await;
    for (title, kind) in [("menger", "scope"), ("two", "build"), ("three", "build")] {
        let made = one(&boss, "tickets.create", json!({ "epic": "MENGER", "title": title, "kind": kind })).await;
        assert_eq!(made["type"], "created", "{made}");
    }
    let g = one(&boss, "members.grant", json!({ "principal": principal(&ADA), "scope": "hypermemetic:", "role": "editor" })).await;
    assert_ne!(g["type"], "error", "{g}");
    let ada = as_key(port, &ADA, "ada").await;

    // Only an admin makes one.
    let refused = call(&ada, "epics.create", json!({ "name": "fresh" })).await;
    assert!(refused.last().unwrap().to_string().contains("forbidden"), "{refused:?}");
    let made = one(&boss, "epics.create", json!({ "name": "fresh" })).await;
    assert_eq!((made["type"].as_str(), made["epic"]["tickets"].as_u64()), (Some("created"), Some(0)), "{made}");
    let listed = call(&ada, "epics.list", json!({})).await;
    let names: Vec<(&str, bool)> = listed.iter().map(|e| (e["epic"]["name"].as_str().unwrap(), e["epic"]["current"].as_bool().unwrap())).collect();
    assert_eq!(names, [("genesis", true), ("fresh", false)], "ORG-23: nobody moved: {listed:?}");
    let now = one(&ada, "epics.switch", json!({ "name": "fresh" })).await;
    assert_eq!(now["epic"]["name"], "fresh", "{now}");

    // MENGER-1, born again in fresh.
    let root = one(&ada, "tickets.create", json!({ "epic": "hypermemetic:fresh:MENGER", "title": "menger again", "kind": "scope" })).await;
    assert_eq!(root["ticket"]["address"], "hypermemetic:fresh:MENGER-1", "{root}");
    let got = one(&ada, "tickets.get", json!({ "id": "MENGER-1" })).await;
    assert_eq!(got["ticket"]["title"], "menger again", "her epic first: {got}");
    let got = one(&ada, "tickets.get", json!({ "id": "MENGER-2" })).await;
    assert_eq!(got["ticket"]["title"], "two", "genesis's, which fresh has not: {got}");
    let got = one(&ada, "tickets.get", json!({ "id": "hypermemetic:MENGER-1" })).await;
    assert_eq!(got["ticket"]["title"], "menger", "the full address: {got}");

    // A filtered read answers her epic; naming one lifts it.
    let titles = |evs: Vec<Value>| -> Vec<String> { evs.iter().filter_map(|e| e["summary"]["title"].as_str().map(str::to_string)).collect() };
    assert_eq!(titles(call(&ada, "tickets.list", json!({ "filter": "kind scope", "summary": true })).await), ["menger again"]);
    assert_eq!(titles(call(&ada, "tickets.list", json!({ "filter": "kind scope and epic any", "summary": true })).await).len(), 2);
    assert_eq!(titles(call(&ada, "tickets.list", json!({ "summary": true })).await), ["menger again"], "a read with no filter too");
    assert_eq!(titles(call(&boss, "tickets.list", json!({ "summary": true, "filter": "epic genesis" })).await).len(), 3);
    let groups = call(&ada, "tickets.nav.groups", json!({})).await;
    let addresses: Vec<&str> = groups.iter().filter_map(|e| e["group"]["address"].as_str()).collect();
    assert_eq!(addresses, ["hypermemetic:fresh:MENGER"], "{groups:?}");

    // A move in: renumbered there, the old id answering it.
    let moved = one(&ada, "tickets.reshortcode", json!({ "ids": ["MENGER-3"], "to": "hypermemetic:fresh:MENGER" })).await;
    assert_eq!(moved["type"], "reshortcoded", "{moved}");
    let got = one(&boss, "tickets.get", json!({ "id": "MENGER-3" })).await;
    assert_eq!((got["ticket"]["title"].as_str(), got["ticket"]["address"].as_str()), (Some("three"), Some("hypermemetic:fresh:MENGER-2")), "{got}");

    // Back to genesis.
    let now = one(&ada, "epics.switch", json!({ "name": "genesis" })).await;
    assert_eq!((now["type"].as_str(), now["epic"]["name"].as_str()), (Some("current"), Some("genesis")), "{now}");
    let got = one(&ada, "tickets.get", json!({ "id": "MENGER-1" })).await;
    assert_eq!(got["ticket"]["title"], "menger", "{got}");
    assert_eq!(one(&ada, "epics.current", json!({})).await["epic"]["name"], "genesis");
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

/// ORG-23: an admin archives `spring` — the dry run names who moves and
/// where, writes nothing; the archive moves Ada (on spring) to `fresh`, the
/// next newer, and her open `epics watch` hears `moved` at once; `list`
/// leaves spring out unless `--archived true`; `current` carries the move
/// until `dismiss`; `restore` moves nobody back.
#[tokio::test]
async fn an_archive_moves_its_people_to_the_next_epic_and_tells_them() {
    let port = start().await;
    let boss = as_key(port, &BOSS, "boss").await;
    let g = one(&boss, "members.grant", json!({ "principal": principal(&ADA), "scope": "hypermemetic:", "role": "editor" })).await;
    assert_ne!(g["type"], "error", "{g}");
    for e in ["spring", "fresh"] {
        assert_eq!(one(&boss, "epics.create", json!({ "name": e })).await["type"], "created");
    }
    let ada = as_key(port, &ADA, "ada").await;
    assert_eq!(one(&ada, "epics.switch", json!({ "name": "spring" })).await["epic"]["name"], "spring");
    let mut watch = subscribe(&ada, "epics.watch", json!({})).await;

    // Only an admin archives; the dry run writes nothing.
    let refused = call(&ada, "epics.archive", json!({ "name": "spring" })).await;
    assert!(refused.last().unwrap().to_string().contains("forbidden"), "{refused:?}");
    let dry = one(&boss, "epics.archive", json!({ "name": "spring", "dry_run": true })).await;
    assert_eq!((dry["type"].as_str(), dry["to"].as_str(), dry["dry_run"].as_bool()), (Some("archived"), Some("fresh"), Some(true)), "{dry}");
    assert_eq!(dry["moved"], json!([principal(&ADA)]), "{dry}");
    assert_eq!(one(&ada, "epics.current", json!({})).await["epic"]["name"], "spring", "nothing written");

    // The archive.
    let done = one(&boss, "epics.archive", json!({ "name": "spring" })).await;
    assert_eq!((done["type"].as_str(), done["dry_run"].as_bool()), (Some("archived"), Some(false)), "{done}");
    assert!(done["epic"]["archived"]["at_unix"].as_i64().is_some(), "{done}");
    // Ada's watch: the change, and her move.
    let mut heard = Vec::new();
    while !heard.iter().any(|e: &Value| e["type"] == "moved") {
        heard.push(next(&mut watch).await);
    }
    let moved = heard.iter().find(|e| e["type"] == "moved").unwrap();
    assert_eq!((moved["moved"]["from"].as_str(), moved["moved"]["to"].as_str()), (Some("spring"), Some("fresh")), "{heard:?}");
    assert!(heard.iter().any(|e| e["type"] == "epic_changed" && e["what"] == "archived"), "{heard:?}");
    let cur = one(&ada, "epics.current", json!({})).await;
    assert_eq!((cur["epic"]["name"].as_str(), cur["moved"]["from"].as_str()), (Some("fresh"), Some("spring")), "{cur}");

    // Listed only with --archived; the people counted where they are.
    let live: Vec<String> = call(&ada, "epics.list", json!({})).await.iter().map(|e| e["epic"]["name"].as_str().unwrap().to_string()).collect();
    assert_eq!(live, ["genesis", "fresh"]);
    let all = call(&boss, "epics.list", json!({ "archived": true })).await;
    let fresh = all.iter().find(|e| e["epic"]["name"] == "fresh").unwrap();
    assert_eq!(fresh["epic"]["members_on"], 1, "{all:?}");
    assert_eq!(all.len(), 3);
    // Switching to it is refused in band.
    let back = one(&ada, "epics.switch", json!({ "name": "spring" })).await;
    assert!(back["message"].as_str().unwrap_or("").contains("restore it first"), "{back}");

    // The notice dismissed; restore moves nobody.
    assert_eq!(one(&ada, "epics.dismiss", json!({})).await["type"], "dismissed");
    assert!(one(&ada, "epics.current", json!({})).await.get("moved").is_none());
    let restored = one(&boss, "epics.restore", json!({ "name": "spring" })).await;
    assert_eq!(restored["type"], "restored", "{restored}");
    assert_eq!(one(&ada, "epics.current", json!({})).await["epic"]["name"], "fresh");
}

