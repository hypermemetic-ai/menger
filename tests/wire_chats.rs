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
//! told `left` on its watch. CHAT-14: a direct message and a group
//! outside tickets — the same two again is that one, a group's members
//! added and removed, only participants read it (a chat's image too), and
//! an agent's key found in `candidates` by the name it signed in with. Every server has its own throwaway database
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

    // CHAT-14: without a ticket, a chat of one is refused (it belongs on a ticket).
    let none = one(&ada, "chats.open", json!({})).await;
    assert_eq!(none["type"], "refused", "{none}");

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
    // CHAT-8: the newest page first, then back from it — oldest first within each.
    let newest = call(&ada, "chats.messages", json!({ "chat": chat, "newest": true, "limit": 2 })).await;
    let ids = |v: &[Value]| v.iter().filter_map(|e| e["message"]["id"].as_u64()).collect::<Vec<_>>();
    let all = ids(&msgs);
    assert_eq!(ids(&newest), all[1..], "{newest:?}");
    assert_eq!(newest[2], json!({ "type": "page", "next": all[1] }));
    let back = call(&ada, "chats.messages", json!({ "chat": chat, "before": all[1], "limit": 2 })).await;
    assert_eq!((ids(&back), back[1].clone()), (all[..1].to_vec(), json!({ "type": "page", "next": null })));
    let listed = call(&ada, "chats.list", json!({ "ticket": "A-2" })).await;
    assert!(listed.iter().any(|e| e["chat"]["id"].as_u64() == Some(chat) && e["chat"]["count"] == 3), "{listed:?}");
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

/// CHAT-14: chats outside tickets over the wire — a direct message (the same
/// two again: that one), a group with a title whose members add and remove,
/// a reader of every ticket who is not in it finding nothing — its image
/// `not_found` too — and builder's key named `builder` in the picker.
#[tokio::test]
async fn direct_messages_and_groups_outside_tickets() {
    let port = start().await;
    let boss = as_key(port, &BOSS, "boss").await;
    kiln(&boss).await;
    let ada = as_key(port, &ADA, "ada").await;
    let builder = as_key(port, &BUILDER, "builder").await;
    let rui = as_key(port, &RUI, "rui").await;
    // builder has signed in as `builder`: the picker finds it by that name, an agent.
    one(&builder, "chats.list", json!({})).await;
    let mut found = None;
    for _ in 0..50 {
        let rows = call(&ada, "chats.candidates", json!({})).await;
        found = rows.into_iter().find(|r| r["principal"] == principal(&BUILDER) && r["name"] == "builder");
        if found.is_some() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(40)).await;
    }
    let found = found.expect("builder named in the picker");
    assert_eq!((found["agent"].as_bool(), found["reads"].as_bool()), (Some(true), Some(true)), "{found}");

    let dm = one(&ada, "chats.open", json!({ "others": [principal(&BUILDER)] })).await;
    assert_eq!((dm["type"].as_str(), dm["chat"]["kind"].as_str(), dm["chat"].get("ticket")), (Some("opened"), Some("direct"), None), "{dm}");
    let id = dm["chat"]["id"].as_u64().unwrap();
    let again = one(&builder, "chats.open", json!({ "others": [principal(&ADA)] })).await;
    assert_eq!((again["chat"]["id"].as_u64(), again["existing"].as_bool()), (Some(id), Some(true)));
    one(&ada, "chats.post", json!({ "chat": id, "body": "just us" })).await;
    // The boss (admin of everything) and rui (who reads A) are not in it.
    for outside in [&boss, &rui] {
        assert_eq!(one(outside, "chats.messages", json!({ "chat": id })).await, json!({ "type": "not_found", "chat": id }));
    }
    // A chat's image: its participants', `not_found` to the rest.
    let png = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==";
    let stored = one(&ada, "media.put", json!({ "chat": id, "mime": "image/png", "data": png })).await;
    assert_eq!(stored["type"], "stored", "{stored}");
    let hash = stored["hash"].as_str().unwrap().to_string();
    assert_eq!(one(&builder, "media.get", json!({ "hash": hash })).await["type"], "media");
    for outside in [&boss, &rui] {
        assert_eq!(one(outside, "media.get", json!({ "hash": hash })).await["type"], "not_found");
    }
    assert_eq!(one(&rui, "media.put", json!({ "chat": id, "mime": "image/png", "data": png })).await["type"], "not_found", "rui is not in it");

    // A group: several, titled; a member adds rui, then removes builder, who is told `left`.
    let g = one(&ada, "chats.open", json!({ "others": [principal(&BUILDER), principal(&RUI)], "title": "the A crew" })).await;
    assert_eq!((g["chat"]["kind"].as_str(), g["chat"]["title"].as_str()), (Some("group"), Some("the A crew")), "{g}");
    let gid = g["chat"]["id"].as_u64().unwrap();
    let mut watch = subscribe(&builder, "chats.watch", json!({})).await;
    let removed = one(&rui, "chats.remove", json!({ "chat": gid, "principal": principal(&BUILDER) })).await;
    assert_eq!(removed["type"], "removed", "{removed}");
    assert_eq!(next(&mut watch).await, json!({ "type": "left", "chat": gid }));
    assert_eq!(one(&builder, "chats.messages", json!({ "chat": gid })).await, json!({ "type": "not_found", "chat": gid }));
    // The boss, an admin of the org, adds builder back without reading it.
    let back = one(&boss, "chats.add", json!({ "chat": gid, "principal": principal(&BUILDER) })).await;
    assert_eq!(back["type"], "added", "{back}");
    assert_eq!(one(&boss, "chats.messages", json!({ "chat": gid })).await, json!({ "type": "not_found", "chat": gid }));
    // A direct message stays between two.
    let refused = one(&ada, "chats.add", json!({ "chat": id, "principal": principal(&RUI) })).await;
    assert_eq!(refused["type"], "refused", "{refused}");
}

/// CHAT-16: an admin adds anyone to any chat. The boss (admin, in no seat)
/// adds sam — who reads B, not A — to a chat on A-2: `needs_grant` first,
/// nothing written; then `grant: chat` adds sam and grants `chatter` on A-2
/// in one write. sam posts and reads there, `tickets.get A-2` is refused,
/// another chat on A-2 is not found. Removed with `revoke`, the grant goes.
/// A non-admin outside the chat cannot add.
#[tokio::test]
async fn an_admin_adds_anyone_granting_the_ticket_or_the_chat() {
    let port = start().await;
    let boss = as_key(port, &BOSS, "boss").await;
    kiln(&boss).await;
    let ada = as_key(port, &ADA, "ada").await;
    let rui = as_key(port, &RUI, "rui").await;
    let chat = one(&ada, "chats.open", json!({ "ticket": "A-2", "others": [principal(&BUILDER)] })).await["chat"]["id"].as_u64().unwrap();
    let other = one(&ada, "chats.open", json!({ "ticket": "A-2" })).await["chat"]["id"].as_u64().unwrap();
    // rui reads A and is not in it: no add.
    assert_eq!(one(&rui, "chats.add", json!({ "chat": chat, "principal": principal(&RUI) })).await, json!({ "type": "not_found", "chat": chat }));
    let needs = one(&boss, "chats.add", json!({ "chat": chat, "principal": principal(&SAM) })).await;
    assert_eq!(needs, json!({ "type": "needs_grant", "chat": chat, "principal": principal(&SAM), "ticket": "A-2", "choices": ["ticket", "chat"] }));
    // ada, in it but no admin, can't grant.
    let no = one(&ada, "chats.add", json!({ "chat": chat, "principal": principal(&SAM), "grant": "chat" })).await;
    assert_eq!(no["type"], "refused", "{no}");
    let added = one(&boss, "chats.add", json!({ "chat": chat, "principal": principal(&SAM), "grant": "chat" })).await;
    assert_eq!((added["type"].as_str(), added["granted"].as_str()), (Some("added"), Some("chatter on A-2")), "{added}");
    assert_eq!(added["chat"]["granted"], json!([principal(&SAM)]));
    let sam = as_key(port, &SAM, "sam").await;
    assert_eq!(one(&sam, "chats.post", json!({ "chat": chat, "body": "hello from B" })).await["type"], "posted");
    let listed = call(&sam, "chats.list", json!({})).await;
    assert_eq!(listed.iter().filter_map(|e| e["chat"]["id"].as_u64()).collect::<Vec<_>>(), [chat]);
    assert_eq!(listed[0]["chat"]["ticket_title"], "A work");
    let get = one(&sam, "tickets.get", json!({ "id": "A-2" })).await;
    assert_ne!(get["type"], "ticket", "the chat only: the ticket is not read — {get}");
    assert_eq!(one(&sam, "chats.messages", json!({ "chat": other })).await, json!({ "type": "not_found", "chat": other }));
    // Removed revoking: out, and the grant gone.
    let out = one(&boss, "chats.remove", json!({ "chat": chat, "principal": principal(&SAM), "revoke": true })).await;
    assert_eq!((out["type"].as_str(), out["revoked"].as_bool()), (Some("removed"), Some(true)), "{out}");
    let grants = call(&boss, "members.list", json!({})).await;
    assert!(!grants.iter().any(|g| g["grant"]["principal"] == principal(&SAM) && g["grant"]["scope"] == "A-2"), "{grants:?}");
    // `grant: ticket`: viewer on A-2 alone; sam reads it now.
    let again = one(&boss, "chats.add", json!({ "chat": chat, "principal": principal(&SAM), "grant": "ticket" })).await;
    assert_eq!(again["granted"], "viewer on A-2", "{again}");
    let sam2 = as_key(port, &SAM, "sam").await;
    assert_eq!(one(&sam2, "tickets.get", json!({ "id": "A-2" })).await["type"], "ticket");
}

/// CHAT-18: live work over the wire — a participant's `activity` reaches
/// every participant's watch, an outsider hears nothing and finds no chat
/// there, nothing lands in `messages`, and `activity_log` answers what the
/// memory keeps.
#[tokio::test]
async fn activity_reaches_participants_alone_and_the_memory_log_answers() {
    let port = start().await;
    let boss = as_key(port, &BOSS, "boss").await;
    kiln(&boss).await;
    let ada = as_key(port, &ADA, "ada").await;
    let builder = as_key(port, &BUILDER, "builder").await;
    let rui = as_key(port, &RUI, "rui").await;
    let opened = one(&ada, "chats.open", json!({ "ticket": "A-2", "others": [principal(&BUILDER)] })).await;
    let chat = opened["chat"]["id"].as_u64().unwrap();
    let mut ada_watch = subscribe(&ada, "chats.watch", json!({})).await;
    let mut rui_watch = subscribe(&rui, "chats.watch", json!({ "ticket": "A-2" })).await;

    let started = one(&builder, "chats.activity", json!({ "chat": chat, "state": "started" })).await;
    assert_eq!((started["type"].as_str(), started["activity"]["state"].as_str()), (Some("activity"), Some("started")), "{started}");
    assert_eq!(started["activity"]["by"], principal(&BUILDER));
    let heard = next(&mut ada_watch).await;
    assert_eq!(heard, started, "ada hears it as builder said it");
    for line in ["Read frontend/src/queue.rs", "Bash: cargo test"] {
        let said = one(&builder, "chats.activity", json!({ "chat": chat, "line": line })).await;
        assert_eq!((said["activity"]["state"].as_str(), said["activity"]["line"].as_str()), (Some("working"), Some(line)));
        assert_eq!(next(&mut ada_watch).await["activity"]["line"], line);
    }
    one(&builder, "chats.activity", json!({ "chat": chat, "state": "ended" })).await;
    assert_eq!(next(&mut ada_watch).await["activity"]["state"], "ended");

    // rui reads A-2 but is not in the chat: not found, and its watch heard none of it.
    for method in ["chats.activity", "chats.activity_log"] {
        assert_eq!(one(&rui, method, json!({ "chat": chat })).await, json!({ "type": "not_found", "chat": chat }), "{method}");
    }
    let posted = one(&ada, "chats.post", json!({ "chat": chat, "body": "thanks" })).await;
    assert_eq!(posted["type"], "posted");
    assert_eq!(next(&mut ada_watch).await["type"], "posted");
    // rui's watch on the ticket: nothing of the chat it is not in (the post neither), so nothing yet.
    assert!(tokio::time::timeout(Duration::from_millis(300), rui_watch.next()).await.is_err(), "rui heard nothing");

    // Nothing in messages but ada's post.
    let msgs = call(&ada, "chats.messages", json!({ "chat": chat })).await;
    assert_eq!(msgs.iter().filter(|e| e["type"] == "message").count(), 1, "{msgs:?}");
    // The memory log, oldest first.
    let log = call(&ada, "chats.activity_log", json!({ "chat": chat })).await;
    let states: Vec<&str> = log.iter().map(|e| e["activity"]["state"].as_str().unwrap()).collect();
    assert_eq!(states, ["started", "working", "working", "ended"], "{log:?}");
    // A bad state is refused before the method runs.
    let bad = call(&builder, "chats.activity", json!({ "chat": chat, "state": "thinking" })).await;
    assert_eq!(bad[0]["type"], "error", "{bad:?}");
}

/// CHAT-44: `delete` and `restore` over the wire — the author deletes its
/// own (`--from`/`--to` too), a participant cannot delete another's (all or
/// none, named), the operator deletes anyone's, `messages` still answers a
/// deleted message marked, the other participant's watch hears
/// `messages_deleted` with its unread, and `restore` puts one back.
#[tokio::test]
async fn messages_are_deleted_kept_marked_and_restored() {
    let port = start().await;
    let boss = as_key(port, &BOSS, "boss").await;
    kiln(&boss).await;
    let ada = as_key(port, &ADA, "ada").await;
    let builder = as_key(port, &BUILDER, "builder").await;
    let opened = one(&ada, "chats.open", json!({ "ticket": "A-2", "others": [principal(&BUILDER)] })).await;
    let chat = opened["chat"]["id"].as_u64().unwrap();
    let mut ids = Vec::new();
    for body in ["the report", "the report", "the report"] {
        ids.push(one(&builder, "chats.post", json!({ "chat": chat, "body": body })).await["message"]["id"].as_u64().unwrap());
    }
    let mine = one(&ada, "chats.post", json!({ "chat": chat, "body": "one is enough" })).await["message"]["id"].as_u64().unwrap();
    let mut watch = subscribe(&ada, "chats.watch", json!({})).await;

    // ada may not delete builder's: refused, naming it; nothing written.
    let refused = one(&ada, "chats.delete", json!({ "chat": chat, "ids": [ids[1], mine] })).await;
    assert_eq!(refused["type"], "refused", "{refused}");
    assert!(refused["reasons"][0].as_str().unwrap().contains(&format!("message {}", ids[1])), "{refused}");
    // builder deletes two of its own by range; ada's watch hears it.
    let d = one(&builder, "chats.delete", json!({ "chat": chat, "from": ids[1], "to": ids[2] })).await;
    assert_eq!((d["type"].as_str(), d["ids"].clone(), d["by"].as_str()), (Some("messages_deleted"), json!([ids[1], ids[2]]), Some(principal(&BUILDER).as_str())), "{d}");
    let heard = next(&mut watch).await;
    assert_eq!((heard["type"].as_str(), heard["ids"].clone(), heard["unread"].as_u64()), (Some("messages_deleted"), json!([ids[1], ids[2]]), Some(0)), "{heard}");
    // Kept and marked.
    let msgs = call(&ada, "chats.messages", json!({ "chat": chat })).await;
    let marked: Vec<u64> = msgs.iter().filter(|e| e["message"]["deleted"].is_object()).filter_map(|e| e["message"]["id"].as_u64()).collect();
    assert_eq!((msgs.len(), marked), (5, vec![ids[1], ids[2]]), "{msgs:?}");
    assert_eq!(msgs[1]["message"]["deleted"]["by"], principal(&BUILDER));
    // The operator (admin on `*`, not in the chat) deletes anyone's.
    let by_admin = one(&boss, "chats.delete", json!({ "chat": chat, "ids": [mine] })).await;
    assert_eq!(by_admin["type"], "messages_deleted", "{by_admin}");
    // builder puts one back.
    let r = one(&builder, "chats.restore", json!({ "chat": chat, "ids": [ids[2]] })).await;
    assert_eq!((r["type"].as_str(), r["ids"].clone()), (Some("messages_restored"), json!([ids[2]])), "{r}");
    // Neither ids nor a range: refused in band.
    assert_eq!(one(&builder, "chats.delete", json!({ "chat": chat })).await["type"], "refused");
}
