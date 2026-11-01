//! V2-20: the queue and its filter words over the real wire, login on (the
//! harness of wire_ask.rs): `queue`, `queued`, `snooze`, `unqueue`,
//! `request_changes`, `await_reply`, `tickets.watch`'s events, the words
//! `queued`, `asking` and `asked`, and an input answered through
//! `tickets.inputs` in each of its kinds. The keys are fixtures.

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
/// rui, an editor of OTHER alone.
const RUI: [u8; 32] = [0xc1; 32];

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


/// The fixture every test here starts from: KILN-1 a scope, KILN-2 and
/// KILN-3 works, KILN-2's body asking a decision, a multi and a text; ada an
/// editor of KILN, builder a viewer of it, rui an editor of OTHER only.
async fn fixture(port: u16) -> (WsClient, WsClient, WsClient, WsClient) {
    let boss = as_key(port, &BOSS, "boss").await;
    let body = "<m-decision id=\"cone\"><m-question>Which cone?</m-question><m-option name=\"Six\" value=\"6\" recommended></m-option><m-option name=\"Five\" value=\"5\"></m-option></m-decision>\n\n\
                <m-input id=\"checks\" kind=\"multi\"><m-question>Which ran?</m-question><m-choice value=\"unit\">Unit</m-choice><m-choice value=\"wire\">Wire</m-choice></m-input>\n\n\
                <m-input id=\"notes\" kind=\"text\"><m-question>Anything else?</m-question></m-input>\n";
    for p in [
        json!({ "epic": "KILN", "title": "the kiln", "kind": "scope" }),
        json!({ "epic": "KILN", "title": "a firing", "parent": "KILN-1", "body": body }),
        json!({ "epic": "KILN", "title": "a glaze", "parent": "KILN-1" }),
        json!({ "epic": "OTHER", "title": "the other", "kind": "scope" }),
    ] {
        assert_eq!(call(&boss, "tickets.create", p).await[0]["type"], "created");
    }
    for (who, scope, role) in [(&ADA, "KILN", "editor"), (&BUILDER, "KILN", "viewer"), (&RUI, "OTHER", "editor")] {
        let g = one(&boss, "members.grant", json!({ "principal": principal(who), "scope": scope, "role": role })).await;
        assert_ne!(g["type"], "error", "{g}");
    }
    let (ada, builder, rui) = (as_key(port, &ADA, "ada").await, as_key(port, &BUILDER, "builder").await, as_key(port, &RUI, "rui").await);
    (boss, ada, builder, rui)
}

fn ids(rows: &[Value]) -> Vec<String> {
    rows.iter().filter_map(|e| e["ticket"]["id"].as_str().or(e["summary"]["id"].as_str())).map(str::to_string).collect()
}

/// Every kind of input answered in place: a decision, a multi, a text; each
/// `input_changed` holds the answer; `asking` holds until the last is
/// settled; `inputs list` is a page.
#[tokio::test]
async fn each_kind_of_input_is_answered_and_asking_moves() {
    let port = start().await;
    let (_boss, ada, builder, _) = fixture(port).await;
    let asking = |evs: Vec<Value>| ids(&evs);
    assert_eq!(asking(call(&ada, "tickets.list", json!({ "filter": "asking", "summary": true })).await), ["KILN-2"]);

    let page = call(&ada, "tickets.inputs.list", json!({ "state": "open", "limit": 2 })).await;
    let inputs: Vec<&str> = page.iter().filter_map(|e| e["input"]["id"].as_str()).collect();
    assert_eq!(inputs, ["cone", "checks"], "{page:?}");
    let end = page.iter().find(|e| e["type"] == "page").expect("a page event");
    assert_eq!(end["remaining"], 1, "{end}");
    let rest = call(&ada, "tickets.inputs.list", json!({ "state": "open", "limit": 2, "after": end["next"] })).await;
    assert_eq!(rest.iter().filter_map(|e| e["input"]["id"].as_str()).collect::<Vec<_>>(), ["notes"], "{rest:?}");

    let a = one(&ada, "tickets.inputs.answer", json!({ "ticket": "KILN-2", "input": "cone", "value": { "type": "choice", "option": "6" }, "rationale": "hotter" })).await;
    assert_eq!((a["type"].as_str(), a["input"]["state"].as_str(), a["input"]["answer"]["rationale"].as_str()), (Some("input_changed"), Some("answered"), Some("hotter")), "{a}");
    let a = one(&ada, "tickets.inputs.answer", json!({ "ticket": "KILN-2", "input": "checks", "value": { "type": "multi", "options": ["wire", "unit"] } })).await;
    assert_eq!(a["input"]["answer"]["value"]["options"], json!(["unit", "wire"]), "the body's order: {a}");
    let bad = one(&ada, "tickets.inputs.answer", json!({ "ticket": "KILN-2", "input": "cone", "value": { "type": "choice", "option": "9" } })).await;
    assert_eq!(bad["type"], "error", "{bad}");
    assert!(bad.to_string().contains("not one of its options"), "{bad}");
    assert_eq!(asking(call(&ada, "tickets.list", json!({ "filter": "asking", "summary": true })).await), ["KILN-2"], "notes is still open");
    let a = one(&ada, "tickets.inputs.answer", json!({ "ticket": "KILN-2", "input": "notes", "value": { "type": "text", "text": "ship it" } })).await;
    assert_eq!(a["input"]["state"], "answered", "{a}");
    assert!(asking(call(&ada, "tickets.list", json!({ "filter": "asking", "summary": true })).await).is_empty());
    // A clear reopens it; a viewer reads and may not answer.
    let c = one(&ada, "tickets.inputs.clear", json!({ "ticket": "KILN-2", "input": "notes" })).await;
    assert_eq!(c["input"]["state"], "open", "{c}");
    assert_eq!(call(&builder, "tickets.inputs.list", json!({ "ticket": "KILN-2", "state": "open" })).await.iter().filter(|e| e["type"] == "input").count(), 1);
    let refused = call(&builder, "tickets.inputs.answer", json!({ "ticket": "KILN-2", "input": "notes", "value": { "type": "text", "text": "mine" } })).await;
    assert!(refused.last().unwrap().to_string().contains("forbidden") || refused.last().unwrap()["type"] == "error", "{refused:?}");
}

/// `queue`, `queued` in its order and as a page, `snooze`, `unqueue`, the
/// watch hearing each, and reach: rui's `queued` holds nothing of KILN.
#[tokio::test]
async fn a_ticket_is_queued_ordered_snoozed_and_taken_out() {
    let port = start().await;
    let (boss, ada, builder, rui) = fixture(port).await;
    let mut watch = subscribe(&ada, "tickets.watch", json!({ "epic": "KILN" })).await;
    let q = one(&ada, "tickets.queue", json!({ "id": "KILN-3", "urgency": "today", "reason": "the glaze waits" })).await;
    assert_eq!((q["type"].as_str(), q["action"].as_str(), q["entry"]["urgency"].as_str()), (Some("queued"), Some("queued"), Some("today")), "{q}");
    let q = one(&ada, "tickets.queue", json!({ "id": "KILN-2", "urgency": "blocking", "reason": "idle on the cone", "input": ["cone", "notes"], "blocking": "cone" })).await;
    assert_eq!((q["entry"]["blocking"].as_str(), q["entry"]["inputs"].as_array().map(Vec::len)), (Some("cone"), Some(2)), "{q}");
    assert_eq!(next(&mut watch).await["type"], "queued");
    assert_eq!(next(&mut watch).await["type"], "queued");
    // Most urgent first; a viewer reads it; out of reach, nothing.
    let rows = call(&builder, "tickets.queued", json!({})).await;
    assert_eq!(ids(&rows), ["KILN-2", "KILN-3"], "{rows:?}");
    let end = rows.iter().find(|e| e["type"] == "queue_end").expect("queue_end");
    assert_eq!((end["count"].as_u64(), end["snoozed"].as_u64()), (Some(2), Some(0)), "{end}");
    assert!(ids(&call(&rui, "tickets.queued", json!({})).await).is_empty());
    let first = call(&ada, "tickets.queued", json!({ "limit": 1 })).await;
    assert_eq!(ids(&first), ["KILN-2"]);
    let page = first.iter().find(|e| e["type"] == "page").expect("a page");
    assert_eq!(page["remaining"], 1, "{page}");
    assert_eq!(ids(&call(&ada, "tickets.queued", json!({ "limit": 1, "after": page["next"] })).await), ["KILN-3"]);
    // Raised, never lowered; the order moves.
    let up = one(&ada, "tickets.queue", json!({ "id": "KILN-3", "urgency": "now", "reason": "sooner" })).await;
    assert_eq!((up["action"].as_str(), up["entry"]["urgency"].as_str()), (Some("raised"), Some("now")), "{up}");
    let keep = one(&ada, "tickets.queue", json!({ "id": "KILN-3", "urgency": "whenever", "reason": "later" })).await;
    assert_eq!((keep["action"].as_str(), keep["entry"]["urgency"].as_str()), (Some("requeued"), Some("now")), "{keep}");
    // A blocking one names its input.
    let refused = one(&ada, "tickets.queue", json!({ "id": "KILN-3", "urgency": "blocking", "reason": "why" })).await;
    assert!(refused.to_string().contains("names the input"), "{refused}");
    // Snoozed: out of the alerting view, still queued.
    let s = one(&ada, "tickets.snooze", json!({ "id": "KILN-3", "minutes": 30 })).await;
    assert_eq!(s["type"], "snoozed", "{s}");
    assert_eq!(ids(&call(&ada, "tickets.queued", json!({ "snoozed": false })).await), ["KILN-2"]);
    assert_eq!(ids(&call(&ada, "tickets.list", json!({ "filter": "queued", "summary": true })).await), ["KILN-2", "KILN-3"]);
    assert_eq!(ids(&call(&ada, "tickets.list", json!({ "filter": "queued now", "summary": true })).await), ["KILN-3"]);
    // Viewer may not; unqueue twice says so.
    let no = call(&builder, "tickets.unqueue", json!({ "id": "KILN-3" })).await;
    assert!(no.last().unwrap().to_string().contains("forbidden") || no.last().unwrap()["type"] == "error", "{no:?}");
    assert_eq!(one(&ada, "tickets.unqueue", json!({ "id": "KILN-3" })).await["type"], "unqueued");
    assert_eq!(one(&ada, "tickets.unqueue", json!({ "id": "KILN-3" })).await["type"], "not_queued");
    // Its history says each.
    let lines = call(&boss, "tickets.history", json!({ "id": "KILN-3" })).await;
    let actions: Vec<&str> = lines.iter().filter(|e| e["type"] == "queue_record").filter_map(|e| e["record"]["action"].as_str()).collect();
    assert_eq!(actions, ["queued", "raised", "requeued", "snoozed", "unqueued"], "{lines:?}");
}

/// Closing the last input an entry names takes it out; `asked` is a question
/// another queued, waiting on the reader, and never one's own.
#[tokio::test]
async fn closing_the_last_named_input_takes_the_entry_out_and_asked_is_the_readers() {
    let port = start().await;
    let (boss, ada, builder, _) = fixture(port).await;
    one(&ada, "tickets.queue", json!({ "id": "KILN-2", "urgency": "now", "reason": "two questions", "input": ["cone", "notes"] })).await;
    let asked = json!({ "filter": "asked", "summary": true });
    assert_eq!(ids(&call(&boss, "tickets.list", asked.clone()).await), ["KILN-2"], "ada's question waits on the boss");
    assert_eq!(ids(&call(&builder, "tickets.list", asked.clone()).await), ["KILN-2"]);
    assert!(ids(&call(&ada, "tickets.list", asked).await).is_empty(), "her own question waits on others");
    let saved = call(&boss, "tickets.list", json!({ "filter": "saved \"Needs you\"", "summary": true })).await;
    assert_eq!(ids(&saved), ["KILN-2"], "{saved:?}");

    assert_eq!(one(&ada, "tickets.inputs.close", json!({ "ticket": "KILN-2", "input": "cone" })).await["type"], "input_changed");
    assert_eq!(ids(&call(&ada, "tickets.queued", json!({})).await), ["KILN-2"], "one closed of two is progress");
    let mut watch = subscribe(&ada, "tickets.watch", json!({ "epic": "KILN" })).await;
    assert_eq!(one(&ada, "tickets.inputs.close", json!({ "ticket": "KILN-2", "input": "notes" })).await["type"], "input_changed");
    let mut heard = next(&mut watch).await;
    while heard["type"] != "unqueued" {
        heard = next(&mut watch).await;
    }
    assert_eq!(heard["action"], "closed", "{heard}");
    assert!(ids(&call(&ada, "tickets.queued", json!({})).await).is_empty());
}

/// `watch { filter: "queued" }` hears a ticket enter and leave; `asking`
/// the same as an input opens and settles.
#[tokio::test]
async fn a_watch_on_the_words_hears_membership_move() {
    let port = start().await;
    let (_boss, ada, _, _) = fixture(port).await;
    let mut queued = subscribe(&ada, "tickets.watch", json!({ "filter": "queued", "membership": true })).await;
    let mut asking = subscribe(&ada, "tickets.watch", json!({ "filter": "asking", "membership": true })).await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    one(&ada, "tickets.queue", json!({ "id": "KILN-3", "urgency": "now", "reason": "look" })).await;
    let mut e = next(&mut queued).await;
    while e["type"] != "entered" {
        e = next(&mut queued).await;
    }
    assert!(e.to_string().contains("KILN-3"), "{e}");
    one(&ada, "tickets.unqueue", json!({ "id": "KILN-3" })).await;
    while e["type"] != "left" {
        e = next(&mut queued).await;
    }
    one(&ada, "tickets.inputs.ask", json!({ "ticket": "KILN-3", "question": "Glazed?" })).await;
    let mut a = next(&mut asking).await;
    while a["type"] != "entered" {
        a = next(&mut asking).await;
    }
    assert!(a.to_string().contains("KILN-3"), "{a}");
    one(&ada, "tickets.inputs.answer", json!({ "ticket": "KILN-3", "input": "ask-1", "value": { "type": "text", "text": "yes" } })).await;
    while a["type"] != "left" {
        a = next(&mut asking).await;
    }
}

/// `await_reply`: `waiting`, then the first response — an answer (the named
/// input only), changes requested, another's unqueue, a cancel.
#[tokio::test]
async fn an_agent_waits_for_the_first_response() {
    let port = start().await;
    let (boss, ada, _, _) = fixture(port).await;
    one(&ada, "tickets.queue", json!({ "id": "KILN-2", "urgency": "blocking", "reason": "idle", "input": "cone" })).await;
    let mut wait = subscribe(&ada, "tickets.await_reply", json!({ "id": "KILN-2", "input": "cone" })).await;
    let w = next(&mut wait).await;
    assert_eq!((w["type"].as_str(), w["queued"]["urgency"].as_str()), (Some("waiting"), Some("blocking")), "{w}");
    one(&boss, "tickets.inputs.answer", json!({ "ticket": "KILN-2", "input": "notes", "value": { "type": "text", "text": "not that one" } })).await;
    one(&boss, "tickets.inputs.answer", json!({ "ticket": "KILN-2", "input": "cone", "value": { "type": "choice", "option": "5" } })).await;
    let r = next(&mut wait).await;
    assert_eq!((r["type"].as_str(), r["reply"]["kind"].as_str(), r["reply"]["input"].as_str()), (Some("replied"), Some("answered"), Some("cone")), "{r}");

    // Changes requested: the body kept on the queue's log, the entry out.
    one(&ada, "tickets.queue", json!({ "id": "KILN-3", "urgency": "now", "reason": "look" })).await;
    let mut wait = subscribe(&ada, "tickets.await_reply", json!({ "id": "KILN-3" })).await;
    assert_eq!(next(&mut wait).await["type"], "waiting");
    let c = one(&boss, "tickets.request_changes", json!({ "id": "KILN-3", "body": "thinner, please" })).await;
    assert_eq!((c["type"].as_str(), c["body"].as_str()), (Some("changes_requested"), Some("thinner, please")), "{c}");
    let r = next(&mut wait).await;
    assert_eq!((r["reply"]["kind"].as_str(), r["reply"]["body"].as_str()), (Some("changes_requested"), Some("thinner, please")), "{r}");
    assert_eq!(ids(&call(&ada, "tickets.queued", json!({})).await), ["KILN-2"], "KILN-3 is out");
    let not = one(&boss, "tickets.request_changes", json!({ "id": "KILN-3", "body": "again" })).await;
    assert_eq!(not["type"], "error", "not queued: {not}");

    // Another's unqueue; a cancel.
    one(&ada, "tickets.queue", json!({ "id": "KILN-3", "urgency": "now", "reason": "again" })).await;
    let mut wait = subscribe(&ada, "tickets.await_reply", json!({ "id": "KILN-3" })).await;
    assert_eq!(next(&mut wait).await["type"], "waiting");
    one(&boss, "tickets.unqueue", json!({ "id": "KILN-3" })).await;
    assert_eq!(next(&mut wait).await["reply"]["kind"], "unqueued");
    let mut wait = subscribe(&ada, "tickets.await_reply", json!({ "id": "KILN-3" })).await;
    assert_eq!(next(&mut wait).await["type"], "waiting");
    one(&boss, "tickets.set_status", json!({ "id": "KILN-3", "status": "Canceled" })).await;
    let r = next(&mut wait).await;
    assert_eq!((r["reply"]["kind"].as_str(), r["reply"]["how"].as_str()), (Some("ended"), Some("canceled")), "{r}");
}

/// `asked` on a watch says what `list` says: a question answered or cleared,
/// closed, requeued by the reader, or dropped from the body moves it, with
/// no cell moving and no urgency either. V2-20's review, B5.
#[tokio::test]
async fn a_watch_of_asked_agrees_with_list() {
    let port = start().await;
    let (boss, ada, _, _) = fixture(port).await;
    // Two questions: the entry names cone; `notes` keeps the ticket asking.
    one(&ada, "tickets.queue", json!({ "id": "KILN-2", "urgency": "now", "reason": "look", "input": "cone" })).await;
    let mut watch = subscribe(&boss, "tickets.watch", json!({ "filter": "asked", "membership": true })).await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    let asked = || call(&boss, "tickets.list", json!({ "filter": "asked", "summary": true }));
    assert_eq!(ids(&asked().await), ["KILN-2"]);
    async fn until(watch: &mut Subscription<Value>, want: &str) {
        let mut e = next(watch).await;
        while e["type"] != want {
            assert!(!matches!(e["type"].as_str(), Some("entered" | "left")), "{want}: {e}");
            e = next(watch).await;
        }
    }
    one(&boss, "tickets.inputs.answer", json!({ "ticket": "KILN-2", "input": "cone", "value": { "type": "choice", "option": "6" } })).await;
    until(&mut watch, "left").await;
    assert!(ids(&asked().await).is_empty());
    one(&boss, "tickets.inputs.clear", json!({ "ticket": "KILN-2", "input": "cone" })).await;
    until(&mut watch, "entered").await;
    // Requeued at the same urgency by the reader: its own question now.
    one(&boss, "tickets.queue", json!({ "id": "KILN-2", "urgency": "now", "reason": "mine now" })).await;
    until(&mut watch, "left").await;
    assert!(ids(&asked().await).is_empty());
    // Requeued by ada: hers again.
    one(&ada, "tickets.queue", json!({ "id": "KILN-2", "urgency": "whenever", "reason": "hers" })).await;
    until(&mut watch, "entered").await;
    // Closed, the entry goes with its last input; reopened, it is queued anew.
    one(&boss, "tickets.inputs.close", json!({ "ticket": "KILN-2", "input": "cone" })).await;
    until(&mut watch, "left").await;
    one(&boss, "tickets.inputs.reopen", json!({ "ticket": "KILN-2", "input": "cone" })).await;
    one(&ada, "tickets.queue", json!({ "id": "KILN-2", "urgency": "now", "reason": "again", "input": ["cone", "checks"] })).await;
    until(&mut watch, "entered").await;
    // Closed, one of two, with no cell moving: `checks` still asks.
    one(&boss, "tickets.inputs.close", json!({ "ticket": "KILN-2", "input": "cone" })).await;
    assert_eq!(ids(&asked().await), ["KILN-2"], "still asked");
    one(&boss, "tickets.inputs.close", json!({ "ticket": "KILN-2", "input": "checks" })).await;
    until(&mut watch, "left").await;
    one(&boss, "tickets.inputs.reopen", json!({ "ticket": "KILN-2", "input": "cone" })).await;
    one(&ada, "tickets.queue", json!({ "id": "KILN-2", "urgency": "now", "reason": "once more", "input": "cone" })).await;
    until(&mut watch, "entered").await;
    // Dropped from the body, `notes` still asking.
    let body = "<m-input id=\"notes\" kind=\"text\"><m-question>Anything else?</m-question></m-input>\n";
    one(&boss, "tickets.amend", json!({ "id": "KILN-2", "body": body })).await;
    until(&mut watch, "left").await;
    assert!(ids(&asked().await).is_empty());
}

/// A wait set aside what stood in its own second; the next answer, the same
/// value in that second too, is its reply. V2-20's review.
#[tokio::test]
async fn a_wait_hears_an_answer_in_its_own_second() {
    let port = start().await;
    let (boss, ada, _, _) = fixture(port).await;
    let answer = json!({ "ticket": "KILN-2", "input": "notes", "value": { "type": "text", "text": "same" } });
    one(&boss, "tickets.inputs.answer", answer.clone()).await;
    let mut wait = subscribe(&ada, "tickets.await_reply", json!({ "id": "KILN-2" })).await;
    assert_eq!(next(&mut wait).await["type"], "waiting");
    one(&boss, "tickets.inputs.answer", answer).await;
    assert_eq!(next(&mut wait).await["type"], "replied");
}
