//! V2-29: `docs` over the real protocol — a real TransportServer, a real
//! WebSocket client, the `menger.call` envelope, the service as `main`
//! builds it (the docs service beside `tickets`, `ui` and `media`, its file
//! beside the tickets file): the roles on the organization (a viewer reads
//! and cannot save, an editor writes and cannot delete, an admin deletes, a
//! stranger to the organization finds nothing); the body a Yjs CRDT (V2-29's
//! `pairing`): two browsers typing at once converge, an agent's whole-body
//! save merges with a live edit made meanwhile, presence appears and leaves
//! with the connection, a replay at a midpoint is the document then, a
//! squash is refused without its grant and folds the history with who wrote
//! what kept; the list newest first and searched, `watch` hearing a change,
//! `docs.may` over the seam, and `media.put` with `doc` asking it. V2-30: a
//! question asked on a range is heard live, waits in an agent's queue, is
//! answered and resolved, and its anchor follows a live edit.

use base64::Engine;
use jsonrpsee::core::client::{Subscription, SubscriptionClientT};
use jsonrpsee::core::params::ObjectParams;
use jsonrpsee::ws_client::{HeaderMap, HeaderValue, WsClient, WsClientBuilder};
use plexus_core::plexus::DynamicHub;
use plexus_transport::TransportServer;
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::Duration;
use yrs::updates::decoder::Decode;
use yrs::{GetString, Text, Transact, Update};

mod support;

fn b64(b: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(b)
}

fn unb64(t: &str) -> Vec<u8> {
    base64::engine::general_purpose::STANDARD.decode(t).unwrap()
}

/// A browser's copy of the document: its CRDT, synced by `docs.state`.
async fn copy(c: &WsClient, id: &str) -> yrs::Doc {
    let d = yrs::Doc::new();
    let st = one(c, "docs.state", json!({"id": id})).await;
    assert_eq!(st["type"], "state", "{st}");
    take(&d, st["update"].as_str().unwrap());
    d
}

fn take(d: &yrs::Doc, update: &str) {
    d.transact_mut().apply_update(Update::decode_v1(&unb64(update)).unwrap()).unwrap();
}

/// `what` typed at `at` in the copy; the update, base64.
fn typed(d: &yrs::Doc, at: u32, what: &str) -> String {
    let t = d.get_or_insert_text("body");
    let mut txn = d.transact_mut();
    t.insert(&mut txn, at, what);
    b64(&txn.encode_update_v1())
}

fn text(d: &yrs::Doc) -> String {
    let t = d.get_or_insert_text("body");
    let s = t.get_string(&d.transact());
    s
}

/// An editor on the organization.
const EDITOR: [u8; 32] = [0xe1; 32];
/// A viewer on the organization.
const VIEWER: [u8; 32] = [0x71; 32];
/// An editor on one shortcode alone — a stranger to the organization's documents.
const STRANGER: [u8; 32] = [0x5c; 32];

async fn start_server() -> (u16, tempfile::TempDir) {
    let dir = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).expect("tempdir");
    let db_path = dir.path().join("tickets.db");
    let port = support::free_port().await;
    let login = support::Login::new();
    let service = support::service(&db_path, &login).await;
    let gate = login.gate(port);
    let conv = |arc: Arc<DynamicHub>| menger::calls::rpc_module(arc, Default::default()).map_err(|e| anyhow::anyhow!("{e}"));
    tokio::spawn(async move {
        TransportServer::builder(service, conv)
            .with_websocket(port)
            .with_session_validator(gate)
            .anonymous_view()
            .credential_cookies(["access_token"])
            .build()
            .await
            .expect("build server")
            .serve()
            .await
            .expect("serve");
    });
    (port, dir)
}

async fn connect_key(seed: &[u8; 32], port: u16) -> WsClient {
    let url = format!("ws://127.0.0.1:{port}");
    for _ in 0..100 {
        let mut h = HeaderMap::new();
        h.insert("authorization", HeaderValue::from_str(&format!("Bearer {}", support::proof_of(seed, port))).unwrap());
        if let Ok(c) = WsClientBuilder::default().set_headers(h).build(&url).await {
            return c;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("server never came up on {url}");
}

async fn call(client: &WsClient, method: &str, params: Value) -> Subscription<Value> {
    let mut p = ObjectParams::new();
    p.insert("method", method).unwrap();
    p.insert("params", params).unwrap();
    client.subscribe("menger.call", p, "menger.call_unsub").await.expect("subscribe")
}

/// Every event a call answers with — each data item's content, or the
/// gate's error item whole.
async fn all(client: &WsClient, method: &str, params: Value) -> Vec<Value> {
    let mut sub = call(client, method, params).await;
    let mut out = Vec::new();
    while let Some(item) = tokio::time::timeout(Duration::from_secs(20), sub.next()).await.expect("an answer") {
        let item = item.unwrap();
        match item["type"].as_str() {
            Some("done") => break,
            Some("data") => out.push(item["content"].clone()),
            _ => {
                out.push(item);
                break;
            }
        }
    }
    out
}

async fn one(client: &WsClient, method: &str, params: Value) -> Value {
    let mut out = all(client, method, params).await;
    assert_eq!(out.len(), 1, "{method}: {out:?}");
    out.remove(0)
}

/// The next data event of an open stream.
async fn next(sub: &mut Subscription<Value>) -> Value {
    loop {
        let item = tokio::time::timeout(Duration::from_secs(10), sub.next()).await.expect("an event in time").expect("open").unwrap();
        if item["type"] == "data" {
            return item["content"].clone();
        }
        assert_ne!(item["type"], "done", "the stream ended");
    }
}

/// The next event of `kind` on an open stream, past any other (presence).
async fn next_of(sub: &mut Subscription<Value>, kind: &str) -> Value {
    loop {
        let e = next(sub).await;
        if e["type"] == kind {
            return e;
        }
    }
}

async fn grant(admin: &WsClient, seed: &[u8; 32], role: &str, scope: &str) {
    let g = one(admin, "auth.grant", json!({ "principal": support::principal_of(seed), "role": role, "scope": scope })).await;
    assert!(g.get("error_kind").is_none(), "{role} on {scope}: {g}");
}

/// The gate's refusal of a grant held nowhere.
fn gate_refused(v: &Value, grant: &str) -> bool {
    v["type"] == "error" && v["error_kind"] == "forbidden" && v["grant"] == grant
}

/// A server; the operator, an editor and a viewer on the organization, and
/// a stranger (editor on shortcode `B` alone); the org's name.
async fn people() -> (tempfile::TempDir, u16, WsClient, WsClient, WsClient, WsClient, String) {
    let (port, dir) = start_server().await;
    let admin = connect_key(&support::ADMIN, port).await;
    let made = one(&admin, "tickets.create", json!({"epic": "B", "title": "b", "kind": "scope"})).await;
    assert_eq!(made["type"], "created", "{made}");
    let orgs = all(&admin, "orgs.list", json!({})).await;
    let org = orgs[0]["org"]["name"].as_str().expect("an organization").to_string();
    grant(&admin, &EDITOR, "editor", &format!("{org}:")).await;
    grant(&admin, &VIEWER, "viewer", &format!("{org}:")).await;
    grant(&admin, &STRANGER, "editor", "B").await;
    let (e, v, s) = (connect_key(&EDITOR, port).await, connect_key(&VIEWER, port).await, connect_key(&STRANGER, port).await);
    (dir, port, admin, e, v, s, org)
}

#[tokio::test]
async fn the_roles_are_held_on_the_organization() {
    let (dir, _port, admin, editor, viewer, stranger, org) = people().await;
    // The editor makes one: in the organization, revision 1, an 8-character id.
    let made = one(&editor, "docs.create", json!({"title": "The kiln", "body": "# Fire\n"})).await;
    assert_eq!(made["type"], "created", "{made}");
    let d = &made["doc"];
    let id = d["id"].as_str().unwrap().to_string();
    assert_eq!((id.len(), d["org"].as_str(), d["revision"].as_u64(), d["body"].as_str()), (8, Some(org.as_str()), Some(1), Some("# Fire\n")));
    assert!(id.chars().all(|c| "0123456789abcdefghjkmnpqrstvwxyz".contains(c)), "{id}");
    // Its own file, beside the tickets file.
    assert!(dir.path().join("docs.db").exists(), "docs.db beside tickets.db");

    // A viewer reads it, and cannot save: docs.save is an editor's grant.
    assert_eq!(one(&viewer, "docs.get", json!({"id": id})).await["doc"]["title"], "The kiln");
    let no = one(&viewer, "docs.save", json!({"id": id, "body": "mine"})).await;
    assert!(gate_refused(&no, "docs.save"), "{no}");
    assert!(gate_refused(&one(&viewer, "docs.create", json!({"title": "x"})).await, "docs.create"));
    // A stranger to the organization finds nothing — not found, as for none.
    assert_eq!(one(&stranger, "docs.get", json!({"id": id})).await, json!({"type": "not_found", "id": id}));
    assert_eq!(one(&stranger, "docs.save", json!({"id": id, "body": "x"})).await, json!({"type": "not_found", "id": id}));
    assert_eq!(one(&stranger, "docs.create", json!({"title": "x"})).await["reason"], "forbidden", "it writes documents nowhere");
    let theirs = all(&stranger, "docs.list", json!({})).await;
    assert_eq!(theirs[0], json!({"type": "rows", "items": []}), "{theirs:?}");
    // The editor saves and renames; an editor cannot delete — admin's.
    let saved = one(&editor, "docs.save", json!({"id": id, "body": "# Fire twice\n", "base": 1})).await;
    assert_eq!((saved["type"].as_str(), saved["revision"].as_u64(), saved.get("stale")), (Some("saved"), Some(2), None), "{saved}");
    assert_eq!(one(&viewer, "docs.get", json!({"id": id})).await["doc"]["body"], "# Fire twice\n");
    assert_eq!(one(&editor, "docs.rename", json!({"id": id, "title": "The kiln, weekly"})).await["revision"], 3);
    assert!(gate_refused(&one(&editor, "docs.delete", json!({"id": id})).await, "docs.delete"));
    // An organization the editor is not in.
    assert_eq!(one(&editor, "docs.create", json!({"title": "x", "org": "nowhere"})).await, json!({"type": "not_found", "org": "nowhere"}));
    // The admin deletes; then it is none.
    let gone = one(&admin, "docs.delete", json!({"id": id})).await;
    assert_eq!(gone["type"], "deleted", "{gone}");
    assert_eq!(one(&viewer, "docs.get", json!({"id": id})).await, json!({"type": "not_found", "id": id}));
    // A bad id is refused before the method runs, naming it.
    assert!(one(&viewer, "docs.get", json!({"id": "nope"})).await.to_string().contains("doc's id"));
}

#[tokio::test]
async fn two_browsers_typing_at_once_converge() {
    let (_dir, _port, _admin, editor, viewer, _s, _org) = people().await;
    let id = one(&editor, "docs.create", json!({"title": "Paired", "body": "ab"})).await["doc"]["id"].as_str().unwrap().to_string();
    // Two pages on it, each watching with its own session.
    let (one_, two) = (copy(&editor, &id).await, copy(&editor, &id).await);
    let mut w1 = call(&editor, "docs.watch", json!({"id": id, "session": "s-one"})).await;
    let mut w2 = call(&editor, "docs.watch", json!({"id": id, "session": "s-two"})).await;
    assert_eq!(next(&mut w1).await["type"], "watching");
    assert_eq!(next(&mut w2).await["type"], "watching");
    // Both type before hearing the other.
    let (u1, u2) = (typed(&one_, 1, "ONE"), typed(&two, 1, "TWO"));
    assert_eq!(one(&editor, "docs.update", json!({"id": id, "update": u1, "session": "s-one"})).await["type"], "applied");
    let a = one(&editor, "docs.update", json!({"id": id, "update": u2, "session": "s-two"})).await;
    assert_eq!((a["type"].as_str(), a["revision"].as_u64()), (Some("applied"), Some(3)), "{a}");
    // Each hears the other's update — and not its own.
    let to_one = next_of(&mut w1, "updated").await;
    assert_eq!(to_one["session"], "s-two", "page one hears page two's");
    take(&one_, to_one["update"].as_str().unwrap());
    let to_two = next_of(&mut w2, "updated").await;
    assert_eq!(to_two["session"], "s-one");
    take(&two, to_two["update"].as_str().unwrap());
    let now = one(&viewer, "docs.get", json!({"id": id})).await["doc"]["body"].as_str().unwrap().to_string();
    assert_eq!((text(&one_), text(&two)), (now.clone(), now.clone()), "both pages and the service agree");
    assert!(now.contains("ONE") && now.contains("TWO") && now.starts_with('a') && now.ends_with('b'), "nothing lost: {now}");
    // A viewer cannot write an update: the gate's.
    assert!(gate_refused(&one(&viewer, "docs.update", json!({"id": id, "update": u1})).await, "docs.update"));
}

#[tokio::test]
async fn an_agents_whole_body_save_merges_with_a_live_edit() {
    let (_dir, _port, _admin, editor, viewer, _s, _org) = people().await;
    let id = one(&editor, "docs.create", json!({"title": "Shared", "body": "first line\nsecond line\n"})).await["doc"]["id"].as_str().unwrap().to_string();
    // The agent reads it…
    let read = one(&viewer, "docs.get", json!({"id": id})).await["doc"]["revision"].as_u64().unwrap();
    // …a person types live into the second line…
    let page = copy(&editor, &id).await;
    let live = typed(&page, "first line\nsecond".len() as u32, " (typed live)");
    one(&editor, "docs.update", json!({"id": id, "update": live})).await;
    // …and the agent saves its rewrite of the first line, from what it read.
    let saved = one(&editor, "docs.save", json!({"id": id, "body": "FIRST LINE\nsecond line\n", "base": read})).await;
    assert_eq!((saved["type"].as_str(), saved["stale"].as_bool()), (Some("saved"), Some(true)), "{saved}");
    assert_eq!(one(&viewer, "docs.get", json!({"id": id})).await["doc"]["body"], "FIRST LINE\nsecond (typed live) line\n", "merged, nothing overwritten");
    let h = all(&viewer, "docs.history", json!({"id": id})).await;
    assert_eq!(h.iter().map(|c| c["change"]["kind"].as_str().unwrap()).collect::<Vec<_>>(), ["save", "update", "create"]);
}

#[tokio::test]
async fn a_replay_at_a_midpoint_is_the_document_then() {
    let (_dir, _port, _admin, editor, viewer, _s, _org) = people().await;
    let id = one(&editor, "docs.create", json!({"title": "Built"})).await["doc"]["id"].as_str().unwrap().to_string();
    let page = copy(&editor, &id).await;
    for w in ["one ", "two ", "three"] {
        let at = text(&page).len() as u32;
        one(&editor, "docs.update", json!({"id": id, "update": typed(&page, at, w)})).await;
    }
    let mid = one(&viewer, "docs.replay", json!({"id": id, "seq": 3})).await;
    assert_eq!((mid["doc"]["body"].as_str(), mid["doc"]["revision"].as_u64()), (Some("one two "), Some(3)), "{mid}");
    assert_eq!(one(&viewer, "docs.replay", json!({"id": id, "seq": 99})).await["type"], "not_found");
    // A restore to the midpoint is a change more.
    let back = one(&editor, "docs.restore", json!({"id": id, "seq": 2})).await;
    assert_eq!((back["type"].as_str(), back["revision"].as_u64()), (Some("saved"), Some(5)));
    assert_eq!(one(&viewer, "docs.get", json!({"id": id})).await["doc"]["body"], "one ");
}

#[tokio::test]
async fn presence_appears_and_leaves_with_the_connection() {
    let (_dir, port, _admin, editor, viewer, _s, _org) = people().await;
    let id = one(&editor, "docs.create", json!({"title": "Here"})).await["doc"]["id"].as_str().unwrap().to_string();
    let mut theirs = call(&viewer, "docs.watch", json!({"id": id, "session": "s-viewer"})).await;
    assert_eq!(next(&mut theirs).await["type"], "watching");
    // Someone opens it: the viewer sees them come.
    let pat = connect_key(&EDITOR, port).await;
    let mut mine = call(&pat, "docs.watch", json!({"id": id, "session": "s-pat"})).await;
    assert_eq!(next(&mut mine).await["type"], "watching");
    let came = loop {
        let e = next_of(&mut theirs, "present").await;
        if e["present"]["session"] == "s-pat" {
            break e;
        }
    };
    // The fixture's callers sign in with agents' keys.
    assert_eq!((came["present"]["agent"].as_bool(), came["present"]["name"].is_string()), (Some(true), true), "{came}");
    assert!(came["present"]["colour"].as_str().unwrap().starts_with('#'));
    let here = all(&viewer, "docs.present", json!({"id": id})).await;
    let sessions: Vec<&str> = here.iter().map(|p| p["present"]["session"].as_str().unwrap()).collect();
    assert_eq!(sessions.len(), 2, "{here:?}");
    assert!(sessions.contains(&"s-pat") && sessions.contains(&"s-viewer"));
    // Its caret moves.
    let moved = one(&pat, "docs.here", json!({"id": id, "session": "s-pat", "client": 7, "awareness": "AQ=="})).await;
    assert_eq!(moved["present"]["awareness"], "AQ==");
    assert_eq!(next_of(&mut theirs, "present").await["present"]["client"], 7);
    // Someone else's session is not theirs to move.
    assert_eq!(one(&viewer, "docs.here", json!({"id": id, "session": "s-pat"})).await["reason"], "forbidden");
    // The connection closes: they leave.
    drop(mine);
    drop(pat);
    let left = next_of(&mut theirs, "left").await;
    assert_eq!((left["type"].as_str(), left["session"].as_str(), left["client"].as_u64()), (Some("left"), Some("s-pat"), Some(7)), "{left}");
    let here = all(&viewer, "docs.present", json!({"id": id})).await;
    assert_eq!(here.len(), 1, "only the viewer now: {here:?}");
}

#[tokio::test]
async fn a_squash_is_refused_without_its_grant_and_keeps_who_wrote_what() {
    let (_dir, _port, admin, editor, viewer, _s, _org) = people().await;
    let id = one(&editor, "docs.create", json!({"title": "Long"})).await["doc"]["id"].as_str().unwrap().to_string();
    let page = copy(&editor, &id).await;
    for w in ["a", "b", "c", "d"] {
        let at = text(&page).len() as u32;
        one(&editor, "docs.update", json!({"id": id, "update": typed(&page, at, w)})).await;
    }
    // Refused, typed, without docs.squash — an editor's and a viewer's.
    assert!(gate_refused(&one(&editor, "docs.squash", json!({"id": id, "until": 4})).await, "docs.squash"));
    assert!(gate_refused(&one(&viewer, "docs.squash", json!({"id": id, "until": 4})).await, "docs.squash"));
    let before = all(&viewer, "docs.history", json!({"id": id})).await.len();
    let mut w = call(&viewer, "docs.watch", json!({"id": id})).await;
    assert_eq!(next(&mut w).await["type"], "watching");
    // The admin holds it.
    let done = one(&admin, "docs.squash", json!({"id": id, "until": 4})).await;
    assert_eq!((done["type"].as_str(), done["removed"].as_u64()), (Some("squashed"), Some(3)), "{done}");
    let said = next_of(&mut w, "squashed").await;
    assert_eq!((said["type"].as_str(), said["until"].as_u64()), (Some("squashed"), Some(4)), "said on watch: {said}");
    let after = all(&viewer, "docs.history", json!({"id": id})).await;
    assert_eq!(after.len(), before - 3, "the history is shorter");
    let snap = after.iter().find(|c| c["change"]["kind"] == "snapshot").expect("a snapshot");
    let changes: u64 = snap["change"]["authors"].as_array().unwrap().iter().map(|a| a["changes"].as_u64().unwrap()).sum();
    assert_eq!(changes, 4, "who wrote what, kept: {snap}");
    assert_eq!(one(&viewer, "docs.get", json!({"id": id})).await["doc"]["body"], "abcd", "the document is as it was");
}

#[tokio::test]
async fn the_list_is_newest_first_searched_and_paged() {
    let (_dir, _port, _admin, editor, viewer, _s, _org) = people().await;
    let mut ids = Vec::new();
    for t in ["Kiln glaze", "Firing log", "Kiln wiring"] {
        ids.push(one(&editor, "docs.create", json!({"title": t, "body": "x"})).await["doc"]["id"].as_str().unwrap().to_string());
    }
    let page = all(&viewer, "docs.list", json!({"limit": 2})).await;
    let rows: Vec<&str> = page[0]["items"].as_array().unwrap().iter().map(|i| i["doc"]["id"].as_str().unwrap()).collect();
    assert_eq!(rows, [ids[2].as_str(), ids[1].as_str()], "newest first");
    assert!(page[0]["items"][0]["doc"].get("body").is_none(), "a row carries no body");
    assert_eq!(page[1], json!({"type": "page", "next": ids[1], "remaining": 1}));
    let rest = all(&viewer, "docs.list", json!({"limit": 2, "after": ids[1]})).await;
    assert_eq!(rest[0]["items"][0]["doc"]["id"], ids[0]);
    let kiln = all(&viewer, "docs.list", json!({"query": "KILN"})).await;
    let titles: Vec<&str> = kiln[0]["items"].as_array().unwrap().iter().map(|i| i["doc"]["title"].as_str().unwrap()).collect();
    assert_eq!(titles, ["Kiln wiring", "Kiln glaze"]);
}

#[tokio::test]
async fn watch_hears_a_save() {
    let (_dir, _port, _admin, editor, viewer, stranger, _org) = people().await;
    let id = one(&editor, "docs.create", json!({"title": "Followed", "body": "a"})).await["doc"]["id"].as_str().unwrap().to_string();
    let mut one_doc = call(&viewer, "docs.watch", json!({"id": id})).await;
    assert_eq!(next(&mut one_doc).await, json!({"type": "watching", "id": id}));
    let mut every = call(&viewer, "docs.watch", json!({})).await;
    assert_eq!(next(&mut every).await, json!({"type": "watching"}));
    // A stranger's watch of it: not found.
    assert_eq!(one(&stranger, "docs.watch", json!({"id": id})).await, json!({"type": "not_found", "id": id}));

    one(&editor, "docs.save", json!({"id": id, "body": "b", "base": 1})).await;
    let heard = next_of(&mut one_doc, "updated").await;
    assert_eq!((heard["type"].as_str(), heard["seq"].as_u64(), heard["by"].is_string()), (Some("updated"), Some(2), true), "{heard}");
    let page = yrs::Doc::new();
    take(&page, one(&viewer, "docs.state", json!({"id": id})).await["update"].as_str().unwrap());
    assert_eq!(text(&page), "b");
    let heard = next_of(&mut every, "updated").await;
    assert_eq!((heard["type"].as_str(), heard["doc"]["id"].as_str()), (Some("updated"), Some(id.as_str())));
    assert!(heard["doc"].get("body").is_none(), "every document's watch carries no body: {heard}");
    // A new document: the watch of every one hears it; the one-doc watch does not.
    let other = one(&editor, "docs.create", json!({"title": "Another"})).await["doc"]["id"].as_str().unwrap().to_string();
    let heard = next_of(&mut every, "created").await;
    assert_eq!((heard["type"].as_str(), heard["doc"]["id"].as_str()), (Some("created"), Some(other.as_str())));
    one(&editor, "docs.rename", json!({"id": id, "title": "Followed still"})).await;
    let heard = loop {
        let e = next(&mut one_doc).await;
        if !matches!(e["type"].as_str(), Some("present" | "left")) {
            break e;
        }
    };
    assert_eq!((heard["type"].as_str(), heard["title"].as_str()), (Some("renamed"), Some("Followed still")), "the create of another was not this watch's: {heard}");
}

/// `docs.may` answers as the permit would; the media service asks it over
/// the seam for an image put with `doc`, and for its readers.
#[tokio::test]
async fn docs_may_over_the_seam_and_media_put_with_a_doc() {
    const PNG_2X1: &str = "iVBORw0KGgoAAAANSUhEUgAAAAIAAAABCAYAAAD0In+KAAAAEUlEQVR42mP8z8DwnwEJMAIAKQgD/2zM2wkAAAAASUVORK5CYII=";
    let (_dir, _port, admin, editor, viewer, stranger, _org) = people().await;
    let id = one(&editor, "docs.create", json!({"title": "With a picture"})).await["doc"]["id"].as_str().unwrap().to_string();
    assert_eq!(one(&editor, "docs.may", json!({"id": id, "role": "editor"})).await, json!({"type": "may", "id": id, "role": "editor", "held": true}));
    assert_eq!(one(&viewer, "docs.may", json!({"id": id, "role": "editor"})).await, json!({"type": "may", "id": id, "role": "editor", "held": false}), "read, not written");
    assert_eq!(one(&editor, "docs.may", json!({"id": id, "role": "admin"})).await["held"], false, "an editor is no admin");
    assert_eq!(one(&admin, "docs.may", json!({"id": id, "role": "admin"})).await["held"], true);
    assert_eq!(one(&stranger, "docs.may", json!({"id": id, "role": "viewer"})).await, json!({"type": "not_found", "id": id}));

    // The editor puts an image for the doc; its viewer reads it, the stranger does not.
    let stored = one(&editor, "media.put", json!({"doc": id, "mime": "image/png", "data": PNG_2X1})).await;
    assert_eq!(stored["type"], "stored", "{stored}");
    let hash = stored["hash"].as_str().unwrap().to_string();
    assert_eq!(one(&viewer, "media.get", json!({"hash": hash})).await["data"], PNG_2X1, "the doc's viewer reads it");
    assert_eq!(one(&stranger, "media.get", json!({"hash": hash})).await, json!({"type": "not_found", "hash": hash}));
    // The stranger's put for the doc: not found (it does not read it); the viewer's: the gate's.
    assert_eq!(one(&stranger, "media.put", json!({"doc": id, "mime": "image/png", "data": PNG_2X1})).await, json!({"type": "not_found", "doc": id}));
    assert!(gate_refused(&one(&viewer, "media.put", json!({"doc": id, "mime": "image/png", "data": PNG_2X1})).await, "media.put"));
    // A ticket and a doc at once: one or the other.
    let both = one(&editor, "media.put", json!({"doc": id, "ticket": "B-1", "mime": "image/png", "data": PNG_2X1})).await;
    assert!(both["message"].as_str().unwrap_or_default().contains("not both"), "{both}");
}

/// V2-30: a question on a range, asked of an agent: heard live on the
/// document's watch, waiting in the agent's queue, answered, resolved; its
/// range follows a live edit made before it; a viewer cannot ask, a
/// stranger finds no thread.
#[tokio::test]
async fn a_question_on_a_range_is_answered_live() {
    let (_dir, _port, _admin, editor, viewer, stranger, _org) = people().await;
    let id = one(&editor, "docs.create", json!({"title": "Asked", "body": "The kiln fires twice a week.\n"})).await["doc"]["id"].as_str().unwrap().to_string();
    let mut w = call(&viewer, "docs.watch", json!({"id": id})).await;
    assert_eq!(next(&mut w).await["type"], "watching");
    let asked = one(&editor, "docs.ask", json!({"id": id, "from": 15, "to": 27, "text": "Still twice?", "to_whom": "agent"})).await;
    assert_eq!(asked["type"], "asked", "{asked}");
    let t = asked["thread"]["thread"].as_str().unwrap().to_string();
    assert_eq!((asked["thread"]["quote"].as_str(), asked["thread"]["from"].as_u64()), (Some("twice a week"), Some(15)));
    let heard = next_of(&mut w, "asked").await;
    assert_eq!(heard["thread"]["thread"], t.as_str(), "heard live");
    // The agent's queue holds it.
    let waiting = all(&viewer, "docs.waiting", json!({"to": "agent"})).await;
    assert_eq!(waiting.len(), 1, "{waiting:?}");
    assert_eq!(waiting[0]["thread"]["thread"], t.as_str());
    // A live edit before the range: the range follows.
    let page = copy(&editor, &id).await;
    one(&editor, "docs.update", json!({"id": id, "update": typed(&page, 0, "Note: ")})).await;
    let now = all(&viewer, "docs.threads", json!({"id": id})).await;
    assert_eq!((now[0]["thread"]["from"].as_u64(), now[0]["thread"]["to"].as_u64()), (Some(21), Some(33)), "{:?}", now[0]);
    // Answered, with an image and a link; then resolved.
    let r = one(&editor, "docs.reply", json!({"id": id, "thread": t, "body": "Yes — [the plan](https://example.com) ![chart](media:00)"})).await;
    assert_eq!(r["thread"]["replies"][0]["n"], 1, "{r}");
    assert_eq!(next_of(&mut w, "replied").await["thread"]["replies"].as_array().unwrap().len(), 1);
    let done = one(&editor, "docs.resolve", json!({"id": id, "thread": t})).await;
    assert!(done["thread"]["resolved"]["by"].is_string(), "{done}");
    assert!(next_of(&mut w, "resolved").await["thread"]["resolved"].is_object());
    assert!(all(&viewer, "docs.waiting", json!({})).await.is_empty(), "no longer waiting");
    // A viewer cannot ask; a stranger finds nothing; a thread that is none.
    assert!(gate_refused(&one(&viewer, "docs.ask", json!({"id": id, "from": 0, "to": 3, "text": "?"})).await, "docs.ask"));
    assert_eq!(one(&stranger, "docs.threads", json!({"id": id})).await, json!({"type": "not_found", "id": id}));
    assert_eq!(one(&editor, "docs.reply", json!({"id": id, "thread": "zzzzzzzz", "body": "x"})).await, json!({"type": "not_found", "thread": "zzzzzzzz"}));
}
