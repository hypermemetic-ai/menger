//! CHAT-3: `chats` over the real wire, login on — a real TransportServer,
//! real WebSocket clients each proving its own Ed25519 key at the upgrade
//! (IDENTITY-42's `key` provider), the `menger.call` envelope.
//!
//! open → post → await_message across two connections: builder waits on its
//! own connection, ada posts on hers, and builder's wait answers with ada's
//! message, signed by ada's principal, then ends. Beside it: a ticket is
//! required (-32602 naming it), a participant who cannot read the ticket is
//! refused by name, a reader of the ticket who is not in the chat finds
//! nothing there, and a participant whose grant on the ticket is revoked is
//! told `left` on its watch. Every server has its own throwaway database
//! and port; prod and test are never touched. The keys are fixtures.

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
/// ada, a person; builder, an agent's process; rui, a reader; sam, of B.
const ADA: [u8; 32] = [0xad; 32];
const BUILDER: [u8; 32] = [0xb1; 32];
const RUI: [u8; 32] = [0x41; 32];
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
    let access = Access::with_login(menger::session_table(&sessions), true, vec![principal(&BOSS)]).minting(menger::session_table(&sessions));
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
        match item["type"].as_str() {
            Some("data") => return item["content"].clone(),
            Some("done") => return json!({ "type": "done" }),
            _ => {}
        }
    }
}

/// A (a scope, A-2 under it) and B; ada, builder and rui read A, sam reads
/// B, rui reads B too.
async fn kiln(boss: &WsClient) {
    for epic in ["A", "B"] {
        let made = call(boss, "tickets.create", json!({ "epic": epic, "title": format!("{epic} root"), "kind": "scope" })).await;
        assert_eq!(made[0]["type"], "created", "{made:?}");
        let made = call(boss, "tickets.create", json!({ "epic": epic, "title": format!("{epic} work"), "parent": format!("{epic}-1") })).await;
        assert_eq!(made[0]["type"], "created", "{made:?}");
    }
    for (who, scope) in [(&ADA, "A"), (&BUILDER, "A"), (&RUI, "A"), (&RUI, "B"), (&SAM, "B")] {
        let g = one(boss, "members.grant", json!({ "principal": principal(who), "scope": scope, "role": "viewer" })).await;
        assert_ne!(g["type"], "error", "{g}");
    }
}

#[tokio::test]
async fn open_post_and_await_message_across_two_connections() {
    let port = start().await;
    let boss = as_key(port, &BOSS, "boss").await;
    kiln(&boss).await;
    let ada = as_key(port, &ADA, "ada").await;
    let builder = as_key(port, &BUILDER, "builder").await;

    // A ticket is required: refused before the method runs, naming it.
    let none = one(&ada, "chats.open", json!({ "others": [principal(&BUILDER)] })).await;
    assert_eq!(none["type"], "error", "{none}");
    assert!(none.to_string().contains("ticket"), "{none}");

    // Someone who can't read the ticket: refused by name, nothing made.
    let sam = one(&ada, "chats.open", json!({ "ticket": "A-2", "others": [principal(&SAM)] })).await;
    assert_eq!(sam, json!({ "type": "refused", "reasons": [format!("{} can't read A-2", principal(&SAM))] }));

    // CHAT-6: no `others` at all is a chat of one; `others: []` is the same chat.
    let alone = one(&ada, "chats.open", json!({ "ticket": "A-2" })).await;
    assert_eq!(alone["type"], "opened", "{alone}");
    assert_eq!(alone["chat"]["participants"], json!([principal(&ADA)]));
    let empty = one(&ada, "chats.open", json!({ "ticket": "A-2", "others": [] })).await;
    assert_eq!((empty["chat"]["id"].as_u64(), empty["existing"].as_bool()), (alone["chat"]["id"].as_u64(), Some(true)));

    let opened = one(&ada, "chats.open", json!({ "ticket": "a-2", "others": [principal(&BUILDER)] })).await;
    assert_eq!(opened["type"], "opened", "{opened}");
    let chat = opened["chat"]["id"].as_u64().unwrap();
    assert_eq!((opened["chat"]["ticket"].as_str(), opened["chat"]["state"].as_str()), (Some("A-2"), Some("open")));
    assert_eq!(opened["chat"]["participants"], json!([principal(&ADA), principal(&BUILDER)]));
    let again = one(&builder, "chats.open", json!({ "ticket": "A-2", "others": [principal(&ADA)] })).await;
    assert_eq!((again["chat"]["id"].as_u64(), again["existing"].as_bool()), (Some(chat), Some(true)));
    // CHAT-7: `/title` — a participant renames it; the same title again is the chat as it is.
    let renamed = one(&ada, "chats.rename", json!({ "chat": chat, "title": "The hold at 1240 °C" })).await;
    assert_eq!((renamed["type"].as_str(), renamed["chat"]["title"].as_str()), (Some("renamed"), Some("The hold at 1240 °C")), "{renamed}");
    let cleared = one(&builder, "chats.rename", json!({ "chat": chat })).await;
    assert_eq!((cleared["type"].as_str(), cleared["chat"].get("title")), (Some("renamed"), None), "{cleared}");

    // builder waits on its own connection; ada watches who listens.
    let mut watch = subscribe(&ada, "chats.watch", json!({ "ticket": "A-2" })).await;
    let mut wait = subscribe(&builder, "chats.await_message", json!({ "chat": chat, "timeout_minutes": 5 })).await;
    let waiting = next(&mut wait).await;
    assert_eq!((waiting["type"].as_str(), waiting["chat"].as_u64()), (Some("waiting"), Some(chat)), "{waiting}");
    let listening = next(&mut watch).await;
    assert_eq!(listening, json!({ "type": "listening", "chat": chat, "principal": principal(&BUILDER), "waiting": true }));

    let posted = one(&ada, "chats.post", json!({ "chat": chat, "body": "Is it the limit check or the test?" })).await;
    assert_eq!((posted["type"].as_str(), posted["message"]["author"].as_str()), (Some("posted"), Some(principal(&ADA).as_str())), "{posted}");
    let heard = next(&mut wait).await;
    assert_eq!(heard["type"], "message", "{heard}");
    assert_eq!(heard["message"], posted["message"], "the message, signed by ada");
    assert_eq!(next(&mut wait).await["type"], "done", "and the wait ends");
    // ada's watch: her post, then builder no longer waiting.
    assert_eq!(next(&mut watch).await["type"], "posted");
    assert_eq!(next(&mut watch).await, json!({ "type": "listening", "chat": chat, "principal": principal(&BUILDER), "waiting": false }));

    // builder answers and drops; ada answers meanwhile. A reconnect loses
    // nothing: with `since` (seconds), the first message by another at or
    // after it; with `after` (the last it read), exactly the next.
    let reply = one(&builder, "chats.post", json!({ "chat": chat, "body": "The test." })).await;
    let since = waiting["since"].as_i64().unwrap();
    one(&ada, "chats.post", json!({ "chat": chat, "body": "1240 is allowed." })).await;
    let builder2 = as_key(port, &BUILDER, "builder").await;
    let caught = call(&builder2, "chats.await_message", json!({ "chat": chat, "since": since })).await;
    assert_eq!(caught.iter().map(|e| e["type"].as_str().unwrap()).collect::<Vec<_>>(), ["waiting", "message"]);
    assert_eq!(caught[1]["message"]["body"], "Is it the limit check or the test?", "at least once, by seconds");
    let caught = call(&builder2, "chats.await_message", json!({ "chat": chat, "after": reply["message"]["id"] })).await;
    assert_eq!(caught.iter().map(|e| e["type"].as_str().unwrap()).collect::<Vec<_>>(), ["waiting", "message"]);
    assert_eq!(caught[1]["message"]["body"], "1240 is allowed.", "exactly, by id");

    // builder's unread, then read.
    let mine = call(&builder, "chats.list", json!({ "unread": true })).await;
    assert_eq!((mine[0]["chat"]["id"].as_u64(), mine[0]["chat"]["unread"].as_u64()), (Some(chat), Some(1)), "{mine:?}");
    let last = caught[1]["message"]["id"].as_u64().unwrap();
    assert_eq!(one(&builder, "chats.read", json!({ "chat": chat, "upto": last })).await, json!({ "type": "read", "chat": chat, "upto": last, "unread": 0 }));
    let msgs = call(&ada, "chats.messages", json!({ "chat": chat })).await;
    assert_eq!(msgs.len(), 4, "three messages and the page: {msgs:?}");
    assert_eq!(msgs[3], json!({ "type": "page", "next": null }));
}

#[tokio::test]
async fn a_reader_outside_the_chat_finds_nothing_and_a_revoked_participant_is_told_left() {
    let port = start().await;
    let boss = as_key(port, &BOSS, "boss").await;
    kiln(&boss).await;
    let ada = as_key(port, &ADA, "ada").await;
    let rui = as_key(port, &RUI, "rui").await;
    let opened = one(&ada, "chats.open", json!({ "ticket": "A-2", "others": [principal(&BUILDER)] })).await;
    let private = opened["chat"]["id"].as_u64().unwrap();
    one(&ada, "chats.post", json!({ "chat": private, "body": "between us" })).await;

    // rui reads A and is not in it: not found, not listed, can't post.
    assert_eq!(one(&rui, "chats.messages", json!({ "chat": private })).await, json!({ "type": "not_found", "chat": private }));
    assert_eq!(call(&rui, "chats.list", json!({ "ticket": "A-2" })).await, [json!({ "type": "page", "next": null })]);
    assert_eq!(one(&rui, "chats.post", json!({ "chat": private, "body": "hi" })).await, json!({ "type": "not_found", "chat": private }));

    // rui in a chat of its own; its grant on A revoked: `left`, then not found.
    let shared = one(&ada, "chats.open", json!({ "ticket": "A-2", "others": [principal(&RUI)] })).await["chat"]["id"].as_u64().unwrap();
    let mut watch = subscribe(&rui, "chats.watch", json!({})).await;
    one(&ada, "chats.post", json!({ "chat": shared, "body": "rui, a look?" })).await;
    assert_eq!(next(&mut watch).await["type"], "posted");
    let revoked = one(&boss, "members.revoke", json!({ "principal": principal(&RUI), "scope": "A" })).await;
    assert_ne!(revoked["type"], "error", "{revoked}");
    // A change on any chat wakes the watch to look again, if the grants did not.
    one(&ada, "chats.post", json!({ "chat": private, "body": "still between us" })).await;
    assert_eq!(next(&mut watch).await, json!({ "type": "left", "chat": shared }));
    let rui2 = as_key(port, &RUI, "rui").await;
    assert_eq!(one(&rui2, "chats.messages", json!({ "chat": shared })).await, json!({ "type": "not_found", "chat": shared }));
}

/// CHAT-4: `candidates` — the picker's list. Every principal with a grant in
/// the ticket's organization but the caller, each saying whether it reads
/// the ticket: sam (of B only) is listed and cannot be picked for A-2; the
/// operator reads everything. A ticket the caller cannot read is none.
#[tokio::test]
async fn candidates_say_who_can_read_the_ticket() {
    let port = start().await;
    let boss = as_key(port, &BOSS, "boss").await;
    kiln(&boss).await;
    let ada = as_key(port, &ADA, "ada").await;
    let rows = call(&ada, "chats.candidates", json!({ "ticket": "a-2" })).await;
    assert!(rows.iter().all(|r| r["type"] == "candidate"), "{rows:?}");
    let got: std::collections::BTreeMap<String, bool> =
        rows.iter().map(|r| (r["principal"].as_str().unwrap().to_string(), r["reads"].as_bool().unwrap())).collect();
    let want: std::collections::BTreeMap<String, bool> =
        [(principal(&BOSS), true), (principal(&BUILDER), true), (principal(&RUI), true), (principal(&SAM), false)].into_iter().collect();
    assert_eq!(got, want, "everyone in the org but ada; sam can't read A-2");
    // sam reads B and not A: A-2 is no ticket to it.
    let sam = as_key(port, &SAM, "sam").await;
    assert_eq!(one(&sam, "chats.candidates", json!({ "ticket": "A-2" })).await, json!({ "type": "refused", "reasons": ["no ticket A-2"] }));
    // On B: rui reads it; ada does not.
    let on_b = call(&sam, "chats.candidates", json!({ "ticket": "B-2" })).await;
    let reads: Vec<(String, bool)> = on_b.iter().map(|r| (r["principal"].as_str().unwrap().to_string(), r["reads"].as_bool().unwrap())).collect();
    assert!(reads.contains(&(principal(&ADA), false)) && reads.contains(&(principal(&RUI), true)), "{reads:?}");
    // A participant is a candidate though its grant is gone (it is shown, not pickable).
    let opened = one(&ada, "chats.open", json!({ "ticket": "A-2", "others": [principal(&RUI)] })).await;
    assert_eq!(opened["type"], "opened", "{opened}");
}
