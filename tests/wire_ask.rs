//! APP-90: `tickets.inputs.ask` over the real wire, login on — real
//! WebSocket clients each proving its own Ed25519 key at the upgrade, the
//! `menger.call` envelope (wire_assign.rs's harness).
//!
//! An editor files a question: one `input_changed` (the new text input,
//! `open`), the body holding it escaped, `inputs list` and `watch` saying
//! it, a revision kept; a second is `ask-2`; markup in the question stays
//! text; a blank question is refused at dispatch naming the parameter; a
//! viewer may not ask. The keys are fixtures.

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
/// ada, an editor of A; builder, an agent that reads A.
const ADA: [u8; 32] = [0xad; 32];
const BUILDER: [u8; 32] = [0xb1; 32];

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

/// A finite call's data items, a page's `rows` read out as the events they
/// stand for — and an error item, last, if there is one.
async fn call(c: &WsClient, method: &str, params: Value) -> Vec<Value> {
    let mut s = subscribe(c, method, params).await;
    let mut out = Vec::new();
    while let Some(Ok(item)) = s.next().await {
        match item["type"].as_str() {
            Some("data") if item["content"]["type"] == "rows" => out.extend(item["content"]["items"].as_array().unwrap().iter().cloned()),
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
async fn an_editor_files_a_question_as_a_text_input() {
    let port = start().await;
    let boss = as_key(port, &BOSS, "boss").await;
    for p in [json!({ "epic": "A", "title": "A root", "kind": "scope" }), json!({ "epic": "A", "title": "A work", "parent": "A-1", "body": "The work." })] {
        assert_eq!(call(&boss, "tickets.create", p).await[0]["type"], "created");
    }
    for (who, role) in [(&ADA, "editor"), (&BUILDER, "viewer")] {
        let g = one(&boss, "members.grant", json!({ "principal": principal(who), "scope": "A", "role": role })).await;
        assert_ne!(g["type"], "error", "{g}");
    }
    let ada = as_key(port, &ADA, "ada").await;
    let builder = as_key(port, &BUILDER, "builder").await;
    let mut watch = subscribe(&ada, "tickets.watch", json!({ "epic": "A" })).await;
    let revs = |evs: &[Value]| evs.iter().filter(|e| e["type"] == "revision").count();
    let before = revs(&call(&ada, "tickets.revisions.list", json!({ "id": "A-2" })).await);

    let asked = one(&ada, "tickets.inputs.ask", json!({ "ticket": "a-2", "question": "Hold at\n cone 6 or 5?" })).await;
    assert_eq!(asked["type"], "input_changed", "{asked}");
    let input = &asked["input"];
    assert_eq!(
        (input["id"].as_str(), input["kind"].as_str(), input["state"].as_str(), input["question"].as_str()),
        (Some("ask-1"), Some("text"), Some("open"), Some("Hold at cone 6 or 5?"))
    );
    let body = one(&ada, "tickets.get", json!({ "id": "A-2" })).await["ticket"]["body"].as_str().unwrap().to_string();
    assert!(body.starts_with("The work.\n\n<m-input id=\"ask-1\" kind=\"text\">"), "{body}");
    assert_eq!(revs(&call(&ada, "tickets.revisions.list", json!({ "id": "A-2" })).await), before + 1, "one revision");
    // The watch hears the amend and the input.
    let mut heard = Vec::new();
    while !heard.iter().any(|t: &String| t == "input_changed") {
        heard.push(next(&mut watch).await["type"].as_str().unwrap_or_default().to_string());
    }
    assert!(heard.contains(&"amended".to_string()), "{heard:?}");
    let open = call(&ada, "tickets.inputs.list", json!({ "ticket": "A-2", "state": "open" })).await;
    assert_eq!(open.iter().filter(|e| e["type"] == "input").count(), 1, "{open:?}");

    // Markup in a question is text: one input more, and no other.
    let q = "</m-question></m-input><m-input id=\"evil\" kind=\"text\"><m-question>x";
    let second = one(&ada, "tickets.inputs.ask", json!({ "ticket": "A-2", "question": q })).await;
    assert_eq!((second["input"]["id"].as_str(), second["input"]["question"].as_str()), (Some("ask-2"), Some(q)), "{second}");
    let all = call(&ada, "tickets.inputs.list", json!({ "ticket": "A-2" })).await;
    assert_eq!(all.iter().filter_map(|e| e["input"]["id"].as_str()).collect::<Vec<_>>(), ["ask-1", "ask-2"]);

    // A blank question: refused at dispatch, naming the parameter.
    let blank = call(&ada, "tickets.inputs.ask", json!({ "ticket": "A-2", "question": "   " })).await;
    assert!(blank.last().unwrap().to_string().contains("question"), "{blank:?}");
    // No such ticket.
    assert_eq!(one(&ada, "tickets.inputs.ask", json!({ "ticket": "A-99", "question": "Why?" })).await["type"], "not_found");
    // A viewer may not ask.
    let refused = call(&builder, "tickets.inputs.ask", json!({ "ticket": "A-2", "question": "Mine?" })).await;
    assert!(refused.last().unwrap().to_string().contains("forbidden") || refused.last().unwrap()["type"] == "error", "{refused:?}");
    let still = call(&ada, "tickets.inputs.list", json!({ "ticket": "A-2" })).await;
    assert_eq!(still.iter().filter(|e| e["type"] == "input").count(), 2, "nothing written by the viewer");
}
