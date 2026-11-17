//! V2-29, NOTES-3: `notes` over the real protocol — a real TransportServer, a real
//! WebSocket client, the `menger.call` envelope, the service as `main`
//! builds it (the notes service beside `tickets`, `ui` and `media`, its file
//! beside the tickets file): the roles on the organization (a viewer reads
//! and cannot save, an editor writes and cannot delete, an admin deletes, a
//! stranger to the organization finds nothing); the body a Yjs CRDT (V2-29's
//! `pairing`): two browsers typing at once converge, an agent's whole-body
//! save merges with a live edit made meanwhile, presence appears and leaves
//! with the connection, a replay at a midpoint is the note then, a
//! squash is refused without its grant and folds the history with who wrote
//! what kept; the list newest first and searched, `watch` hearing a change,
//! `notes.may` over the seam, and `media.put` with `note` asking it. V2-30: a
//! question asked on a range is heard live, waits in an agent's queue, is
//! answered and resolved, and its anchor follows a live edit.

use base64::Engine;
use jsonrpsee::core::client::{Subscription, SubscriptionClientT};
use jsonrpsee::core::params::ObjectParams;
use jsonrpsee::ws_client::{HeaderMap, HeaderValue, WsClient, WsClientBuilder};
use plexus_rpc::core::plexus::DynamicHub;
use plexus_rpc::transport::TransportServer;
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

/// A browser's copy of the note: its CRDT, synced by `notes.state`.
async fn copy(c: &WsClient, id: &str) -> yrs::Doc {
    let d = yrs::Doc::new();
    let st = one(c, "notes.state", json!({"id": id})).await;
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
/// An editor on one shortcode alone — a stranger to the organization's notes.
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
    let made = one(&editor, "notes.create", json!({"title": "The kiln", "body": "# Fire\n"})).await;
    assert_eq!(made["type"], "created", "{made}");
    let d = &made["note"];
    let id = d["id"].as_str().unwrap().to_string();
    assert_eq!((id.len(), d["org"].as_str(), d["revision"].as_u64(), d["body"].as_str()), (8, Some(org.as_str()), Some(1), Some("# Fire\n")));
    assert!(id.chars().all(|c| "0123456789abcdefghjkmnpqrstvwxyz".contains(c)), "{id}");
    // Its own file, beside the tickets file.
    assert!(dir.path().join("notes.db").exists(), "notes.db beside tickets.db");

    // A viewer reads it, and cannot save: notes.save is an editor's grant.
    assert_eq!(one(&viewer, "notes.get", json!({"id": id})).await["note"]["title"], "The kiln");
    let no = one(&viewer, "notes.save", json!({"id": id, "body": "mine"})).await;
    assert!(gate_refused(&no, "notes.save"), "{no}");
    assert!(gate_refused(&one(&viewer, "notes.create", json!({"title": "x"})).await, "notes.create"));
    // A stranger to the organization finds nothing — not found, as for none.
    assert_eq!(one(&stranger, "notes.get", json!({"id": id})).await, json!({"type": "not_found", "id": id}));
    assert_eq!(one(&stranger, "notes.save", json!({"id": id, "body": "x"})).await, json!({"type": "not_found", "id": id}));
    assert_eq!(one(&stranger, "notes.create", json!({"title": "x"})).await["reason"], "forbidden", "it writes notes nowhere");
    let theirs = all(&stranger, "notes.list", json!({})).await;
    assert_eq!(theirs[0], json!({"type": "rows", "items": []}), "{theirs:?}");
    // The editor saves and renames; an editor cannot delete — admin's.
    let saved = one(&editor, "notes.save", json!({"id": id, "body": "# Fire twice\n", "base": 1})).await;
    assert_eq!((saved["type"].as_str(), saved["revision"].as_u64(), saved.get("stale")), (Some("saved"), Some(2), None), "{saved}");
    assert_eq!(one(&viewer, "notes.get", json!({"id": id})).await["note"]["body"], "# Fire twice\n");
    assert_eq!(one(&editor, "notes.rename", json!({"id": id, "title": "The kiln, weekly"})).await["revision"], 3);
    assert!(gate_refused(&one(&editor, "notes.delete", json!({"id": id})).await, "notes.delete"));
    // An organization the editor is not in.
    assert_eq!(one(&editor, "notes.create", json!({"title": "x", "org": "nowhere"})).await, json!({"type": "not_found", "org": "nowhere"}));
    // The admin deletes; then it is none.
    let gone = one(&admin, "notes.delete", json!({"id": id})).await;
    assert_eq!(gone["type"], "deleted", "{gone}");
    assert_eq!(one(&viewer, "notes.get", json!({"id": id})).await, json!({"type": "not_found", "id": id}));
    // A bad id is refused before the method runs, naming it.
    assert!(one(&viewer, "notes.get", json!({"id": "nope"})).await.to_string().contains("note's id"));
}

#[tokio::test]
async fn two_browsers_typing_at_once_converge() {
    let (_dir, _port, _admin, editor, viewer, _s, _org) = people().await;
    let id = one(&editor, "notes.create", json!({"title": "Paired", "body": "ab"})).await["note"]["id"].as_str().unwrap().to_string();
    // Two pages on it, each watching with its own session.
    let (one_, two) = (copy(&editor, &id).await, copy(&editor, &id).await);
    let mut w1 = call(&editor, "notes.watch", json!({"id": id, "session": "s-one"})).await;
    let mut w2 = call(&editor, "notes.watch", json!({"id": id, "session": "s-two"})).await;
    assert_eq!(next(&mut w1).await["type"], "watching");
    assert_eq!(next(&mut w2).await["type"], "watching");
    // Both type before hearing the other.
    let (u1, u2) = (typed(&one_, 1, "ONE"), typed(&two, 1, "TWO"));
    assert_eq!(one(&editor, "notes.update", json!({"id": id, "update": u1, "session": "s-one"})).await["type"], "applied");
    let a = one(&editor, "notes.update", json!({"id": id, "update": u2, "session": "s-two"})).await;
    assert_eq!((a["type"].as_str(), a["revision"].as_u64()), (Some("applied"), Some(3)), "{a}");
    // Each hears the other's update — and not its own.
    let to_one = next_of(&mut w1, "updated").await;
    assert_eq!(to_one["session"], "s-two", "page one hears page two's");
    take(&one_, to_one["update"].as_str().unwrap());
    let to_two = next_of(&mut w2, "updated").await;
    assert_eq!(to_two["session"], "s-one");
    take(&two, to_two["update"].as_str().unwrap());
    let now = one(&viewer, "notes.get", json!({"id": id})).await["note"]["body"].as_str().unwrap().to_string();
    assert_eq!((text(&one_), text(&two)), (now.clone(), now.clone()), "both pages and the service agree");
    assert!(now.contains("ONE") && now.contains("TWO") && now.starts_with('a') && now.ends_with('b'), "nothing lost: {now}");
    // A viewer cannot write an update: the gate's.
    assert!(gate_refused(&one(&viewer, "notes.update", json!({"id": id, "update": u1})).await, "notes.update"));
}

#[tokio::test]
async fn an_agents_whole_body_save_merges_with_a_live_edit() {
    let (_dir, _port, _admin, editor, viewer, _s, _org) = people().await;
    let id = one(&editor, "notes.create", json!({"title": "Shared", "body": "first line\nsecond line\n"})).await["note"]["id"].as_str().unwrap().to_string();
    // The agent reads it…
    let read = one(&viewer, "notes.get", json!({"id": id})).await["note"]["revision"].as_u64().unwrap();
    // …a person types live into the second line…
    let page = copy(&editor, &id).await;
    let live = typed(&page, "first line\nsecond".len() as u32, " (typed live)");
    one(&editor, "notes.update", json!({"id": id, "update": live})).await;
    // …and the agent saves its rewrite of the first line, from what it read.
    let saved = one(&editor, "notes.save", json!({"id": id, "body": "FIRST LINE\nsecond line\n", "base": read})).await;
    assert_eq!((saved["type"].as_str(), saved["stale"].as_bool()), (Some("saved"), Some(true)), "{saved}");
    assert_eq!(one(&viewer, "notes.get", json!({"id": id})).await["note"]["body"], "FIRST LINE\nsecond (typed live) line\n", "merged, nothing overwritten");
    let h = all(&viewer, "notes.history", json!({"id": id})).await;
    assert_eq!(h.iter().map(|c| c["change"]["kind"].as_str().unwrap()).collect::<Vec<_>>(), ["save", "update", "create"]);
}

#[tokio::test]
async fn a_replay_at_a_midpoint_is_the_document_then() {
    let (_dir, _port, _admin, editor, viewer, _s, _org) = people().await;
    let id = one(&editor, "notes.create", json!({"title": "Built"})).await["note"]["id"].as_str().unwrap().to_string();
    let page = copy(&editor, &id).await;
    for w in ["one ", "two ", "three"] {
        let at = text(&page).len() as u32;
        one(&editor, "notes.update", json!({"id": id, "update": typed(&page, at, w)})).await;
    }
    let mid = one(&viewer, "notes.replay", json!({"id": id, "seq": 3})).await;
    assert_eq!((mid["note"]["body"].as_str(), mid["note"]["revision"].as_u64()), (Some("one two "), Some(3)), "{mid}");
    assert_eq!(one(&viewer, "notes.replay", json!({"id": id, "seq": 99})).await["type"], "not_found");
    // A restore to the midpoint is a change more.
    let back = one(&editor, "notes.restore", json!({"id": id, "seq": 2})).await;
    assert_eq!((back["type"].as_str(), back["revision"].as_u64()), (Some("saved"), Some(5)));
    assert_eq!(one(&viewer, "notes.get", json!({"id": id})).await["note"]["body"], "one ");
}

#[tokio::test]
async fn presence_appears_and_leaves_with_the_connection() {
    let (_dir, port, _admin, editor, viewer, _s, _org) = people().await;
    let id = one(&editor, "notes.create", json!({"title": "Here"})).await["note"]["id"].as_str().unwrap().to_string();
    let mut theirs = call(&viewer, "notes.watch", json!({"id": id, "session": "s-viewer"})).await;
    assert_eq!(next(&mut theirs).await["type"], "watching");
    // Someone opens it: the viewer sees them come.
    let pat = connect_key(&EDITOR, port).await;
    let mut mine = call(&pat, "notes.watch", json!({"id": id, "session": "s-pat"})).await;
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
    let here = all(&viewer, "notes.present", json!({"id": id})).await;
    let sessions: Vec<&str> = here.iter().map(|p| p["present"]["session"].as_str().unwrap()).collect();
    assert_eq!(sessions.len(), 2, "{here:?}");
    assert!(sessions.contains(&"s-pat") && sessions.contains(&"s-viewer"));
    // Its caret moves.
    let moved = one(&pat, "notes.here", json!({"id": id, "session": "s-pat", "client": 7, "awareness": "AQ=="})).await;
    assert_eq!(moved["present"]["awareness"], "AQ==");
    assert_eq!(next_of(&mut theirs, "present").await["present"]["client"], 7);
    // Someone else's session is not theirs to move.
    assert_eq!(one(&viewer, "notes.here", json!({"id": id, "session": "s-pat"})).await["reason"], "forbidden");
    // The connection closes: they leave.
    drop(mine);
    drop(pat);
    let left = next_of(&mut theirs, "left").await;
    assert_eq!((left["type"].as_str(), left["session"].as_str(), left["client"].as_u64()), (Some("left"), Some("s-pat"), Some(7)), "{left}");
    let here = all(&viewer, "notes.present", json!({"id": id})).await;
    assert_eq!(here.len(), 1, "only the viewer now: {here:?}");
}

#[tokio::test]
async fn a_squash_is_refused_without_its_grant_and_keeps_who_wrote_what() {
    let (_dir, _port, admin, editor, viewer, _s, _org) = people().await;
    let id = one(&editor, "notes.create", json!({"title": "Long"})).await["note"]["id"].as_str().unwrap().to_string();
    let page = copy(&editor, &id).await;
    for w in ["a", "b", "c", "d"] {
        let at = text(&page).len() as u32;
        one(&editor, "notes.update", json!({"id": id, "update": typed(&page, at, w)})).await;
    }
    // Refused, typed, without notes.squash — an editor's and a viewer's.
    assert!(gate_refused(&one(&editor, "notes.squash", json!({"id": id, "until": 4})).await, "notes.squash"));
    assert!(gate_refused(&one(&viewer, "notes.squash", json!({"id": id, "until": 4})).await, "notes.squash"));
    let before = all(&viewer, "notes.history", json!({"id": id})).await.len();
    let mut w = call(&viewer, "notes.watch", json!({"id": id})).await;
    assert_eq!(next(&mut w).await["type"], "watching");
    // The admin holds it.
    let done = one(&admin, "notes.squash", json!({"id": id, "until": 4})).await;
    assert_eq!((done["type"].as_str(), done["removed"].as_u64()), (Some("squashed"), Some(3)), "{done}");
    let said = next_of(&mut w, "squashed").await;
    assert_eq!((said["type"].as_str(), said["until"].as_u64()), (Some("squashed"), Some(4)), "said on watch: {said}");
    let after = all(&viewer, "notes.history", json!({"id": id})).await;
    assert_eq!(after.len(), before - 3, "the history is shorter");
    let snap = after.iter().find(|c| c["change"]["kind"] == "snapshot").expect("a snapshot");
    let changes: u64 = snap["change"]["authors"].as_array().unwrap().iter().map(|a| a["changes"].as_u64().unwrap()).sum();
    assert_eq!(changes, 4, "who wrote what, kept: {snap}");
    assert_eq!(one(&viewer, "notes.get", json!({"id": id})).await["note"]["body"], "abcd", "the note is as it was");
}

#[tokio::test]
async fn the_list_is_newest_first_searched_and_paged() {
    let (_dir, _port, _admin, editor, viewer, _s, _org) = people().await;
    let mut ids = Vec::new();
    for t in ["Kiln glaze", "Firing log", "Kiln wiring"] {
        ids.push(one(&editor, "notes.create", json!({"title": t, "body": "x"})).await["note"]["id"].as_str().unwrap().to_string());
    }
    let page = all(&viewer, "notes.list", json!({"limit": 2})).await;
    let rows: Vec<&str> = page[0]["items"].as_array().unwrap().iter().map(|i| i["note"]["id"].as_str().unwrap()).collect();
    assert_eq!(rows, [ids[2].as_str(), ids[1].as_str()], "newest first");
    assert!(page[0]["items"][0]["note"].get("body").is_none(), "a row carries no body");
    assert_eq!(page[1], json!({"type": "page", "next": ids[1], "remaining": 1}));
    let rest = all(&viewer, "notes.list", json!({"limit": 2, "after": ids[1]})).await;
    assert_eq!(rest[0]["items"][0]["note"]["id"], ids[0]);
    let kiln = all(&viewer, "notes.list", json!({"query": "KILN"})).await;
    let titles: Vec<&str> = kiln[0]["items"].as_array().unwrap().iter().map(|i| i["note"]["title"].as_str().unwrap()).collect();
    assert_eq!(titles, ["Kiln wiring", "Kiln glaze"]);
}

#[tokio::test]
async fn watch_hears_a_save() {
    let (_dir, _port, _admin, editor, viewer, stranger, _org) = people().await;
    let id = one(&editor, "notes.create", json!({"title": "Followed", "body": "a"})).await["note"]["id"].as_str().unwrap().to_string();
    let mut one_doc = call(&viewer, "notes.watch", json!({"id": id})).await;
    assert_eq!(next(&mut one_doc).await, json!({"type": "watching", "id": id}));
    let mut every = call(&viewer, "notes.watch", json!({})).await;
    assert_eq!(next(&mut every).await, json!({"type": "watching"}));
    // A stranger's watch of it: not found.
    assert_eq!(one(&stranger, "notes.watch", json!({"id": id})).await, json!({"type": "not_found", "id": id}));

    one(&editor, "notes.save", json!({"id": id, "body": "b", "base": 1})).await;
    let heard = next_of(&mut one_doc, "updated").await;
    assert_eq!((heard["type"].as_str(), heard["seq"].as_u64(), heard["by"].is_string()), (Some("updated"), Some(2), true), "{heard}");
    let page = yrs::Doc::new();
    take(&page, one(&viewer, "notes.state", json!({"id": id})).await["update"].as_str().unwrap());
    assert_eq!(text(&page), "b");
    let heard = next_of(&mut every, "updated").await;
    assert_eq!((heard["type"].as_str(), heard["note"]["id"].as_str()), (Some("updated"), Some(id.as_str())));
    assert!(heard["note"].get("body").is_none(), "every note's watch carries no body: {heard}");
    // A new note: the watch of every one hears it; the one-note watch does not.
    let other = one(&editor, "notes.create", json!({"title": "Another"})).await["note"]["id"].as_str().unwrap().to_string();
    let heard = next_of(&mut every, "created").await;
    assert_eq!((heard["type"].as_str(), heard["note"]["id"].as_str()), (Some("created"), Some(other.as_str())));
    one(&editor, "notes.rename", json!({"id": id, "title": "Followed still"})).await;
    let heard = loop {
        let e = next(&mut one_doc).await;
        if !matches!(e["type"].as_str(), Some("present" | "left")) {
            break e;
        }
    };
    assert_eq!((heard["type"].as_str(), heard["title"].as_str()), (Some("renamed"), Some("Followed still")), "the create of another was not this watch's: {heard}");
}

/// `notes.may` answers as the permit would; the media service asks it over
/// the seam for an image put with `note`, and for its readers.
#[tokio::test]
async fn docs_may_over_the_seam_and_media_put_with_a_doc() {
    const PNG_2X1: &str = "iVBORw0KGgoAAAANSUhEUgAAAAIAAAABCAYAAAD0In+KAAAAEUlEQVR42mP8z8DwnwEJMAIAKQgD/2zM2wkAAAAASUVORK5CYII=";
    let (_dir, _port, admin, editor, viewer, stranger, _org) = people().await;
    let id = one(&editor, "notes.create", json!({"title": "With a picture"})).await["note"]["id"].as_str().unwrap().to_string();
    assert_eq!(one(&editor, "notes.may", json!({"id": id, "role": "editor"})).await, json!({"type": "may", "id": id, "role": "editor", "held": true}));
    assert_eq!(one(&viewer, "notes.may", json!({"id": id, "role": "editor"})).await, json!({"type": "may", "id": id, "role": "editor", "held": false}), "read, not written");
    assert_eq!(one(&editor, "notes.may", json!({"id": id, "role": "admin"})).await["held"], false, "an editor is no admin");
    assert_eq!(one(&admin, "notes.may", json!({"id": id, "role": "admin"})).await["held"], true);
    assert_eq!(one(&stranger, "notes.may", json!({"id": id, "role": "viewer"})).await, json!({"type": "not_found", "id": id}));

    // The editor puts an image for the note; its viewer reads it, the stranger does not.
    let stored = one(&editor, "media.put", json!({"note": id, "mime": "image/png", "data": PNG_2X1})).await;
    assert_eq!(stored["type"], "stored", "{stored}");
    let hash = stored["hash"].as_str().unwrap().to_string();
    assert_eq!(one(&viewer, "media.get", json!({"hash": hash})).await["data"], PNG_2X1, "the note's viewer reads it");
    assert_eq!(one(&stranger, "media.get", json!({"hash": hash})).await, json!({"type": "not_found", "hash": hash}));
    // The stranger's put for the note: not found (it does not read it); the viewer's: the gate's.
    assert_eq!(one(&stranger, "media.put", json!({"note": id, "mime": "image/png", "data": PNG_2X1})).await, json!({"type": "not_found", "note": id}));
    assert!(gate_refused(&one(&viewer, "media.put", json!({"note": id, "mime": "image/png", "data": PNG_2X1})).await, "media.put"));
    // A ticket and a note at once: one or the other.
    let both = one(&editor, "media.put", json!({"note": id, "ticket": "B-1", "mime": "image/png", "data": PNG_2X1})).await;
    assert!(both["message"].as_str().unwrap_or_default().contains("not both"), "{both}");

    // EDITOR-10: a file in chunks for the note, the note's like any put; a stranger begins none for it,
    // and fetches none for it either (asked before the server reads anything).
    let begun = one(&editor, "media.begin", json!({"note": id, "name": "deck.pdf", "size": 10})).await;
    assert_eq!(begun["type"], "upload", "{begun}");
    let upload = begun["upload"].as_str().unwrap();
    use base64::Engine;
    let data = base64::engine::general_purpose::STANDARD.encode(b"%PDF-1.4 x");
    assert_eq!(one(&editor, "media.chunk", json!({"upload": upload, "offset": 0, "data": data})).await["type"], "chunked");
    let pdf = one(&editor, "media.finish", json!({"upload": upload})).await;
    assert_eq!((pdf["type"].as_str(), pdf["kind"].as_str()), (Some("stored"), Some("pdf")), "{pdf}");
    assert_eq!(one(&viewer, "media.info", json!({"hash": pdf["hash"]})).await["kind"], "pdf", "the note's viewer sees it");
    assert_eq!(one(&stranger, "media.info", json!({"hash": pdf["hash"]})).await["type"], "not_found");
    assert_eq!(one(&stranger, "media.begin", json!({"note": id, "size": 10})).await, json!({"type": "not_found", "note": id}));
    assert_eq!(one(&stranger, "media.fetch", json!({"note": id, "url": "http://169.254.169.254/"})).await, json!({"type": "not_found", "note": id}));
}

/// V2-30: a question on a range, asked of an agent: heard live on the
/// note's watch, waiting in the agent's queue, answered, resolved; its
/// range follows a live edit made before it; a viewer cannot ask, a
/// stranger finds no thread.
#[tokio::test]
async fn a_question_on_a_range_is_answered_live() {
    let (_dir, _port, _admin, editor, viewer, stranger, _org) = people().await;
    let id = one(&editor, "notes.create", json!({"title": "Asked", "body": "The kiln fires twice a week.\n"})).await["note"]["id"].as_str().unwrap().to_string();
    let mut w = call(&viewer, "notes.watch", json!({"id": id})).await;
    assert_eq!(next(&mut w).await["type"], "watching");
    let asked = one(&editor, "notes.ask", json!({"id": id, "from": 15, "to": 27, "text": "Still twice?", "to_whom": "agent"})).await;
    assert_eq!(asked["type"], "asked", "{asked}");
    let t = asked["thread"]["thread"].as_str().unwrap().to_string();
    assert_eq!((asked["thread"]["quote"].as_str(), asked["thread"]["from"].as_u64()), (Some("twice a week"), Some(15)));
    let heard = next_of(&mut w, "asked").await;
    assert_eq!(heard["thread"]["thread"], t.as_str(), "heard live");
    // The agent's queue holds it.
    let waiting = all(&viewer, "notes.waiting", json!({"to": "agent"})).await;
    assert_eq!(waiting.len(), 1, "{waiting:?}");
    assert_eq!(waiting[0]["thread"]["thread"], t.as_str());
    // A live edit before the range: the range follows.
    let page = copy(&editor, &id).await;
    one(&editor, "notes.update", json!({"id": id, "update": typed(&page, 0, "Note: ")})).await;
    let now = all(&viewer, "notes.threads", json!({"id": id})).await;
    assert_eq!((now[0]["thread"]["from"].as_u64(), now[0]["thread"]["to"].as_u64()), (Some(21), Some(33)), "{:?}", now[0]);
    // Answered, with an image and a link; then resolved.
    let r = one(&editor, "notes.reply", json!({"id": id, "thread": t, "body": "Yes — [the plan](https://example.com) ![chart](media:00)"})).await;
    assert_eq!(r["thread"]["replies"][0]["n"], 1, "{r}");
    assert_eq!(next_of(&mut w, "replied").await["thread"]["replies"].as_array().unwrap().len(), 1);
    let done = one(&editor, "notes.resolve", json!({"id": id, "thread": t})).await;
    assert!(done["thread"]["resolved"]["by"].is_string(), "{done}");
    assert!(next_of(&mut w, "resolved").await["thread"]["resolved"].is_object());
    assert!(all(&viewer, "notes.waiting", json!({})).await.is_empty(), "no longer waiting");
    // A viewer cannot ask; a stranger finds nothing; a thread that is none.
    assert!(gate_refused(&one(&viewer, "notes.ask", json!({"id": id, "from": 0, "to": 3, "text": "?"})).await, "notes.ask"));
    assert_eq!(one(&stranger, "notes.threads", json!({"id": id})).await, json!({"type": "not_found", "id": id}));
    assert_eq!(one(&editor, "notes.reply", json!({"id": id, "thread": "zzzzzzzz", "body": "x"})).await, json!({"type": "not_found", "thread": "zzzzzzzz"}));
}

// ── NOTES-4: a note's slot ────────────────────────────────────────────────

/// A version of the bundle `notes` holding `components` (each a svelte/1
/// module, as the tool builds one), kept in the server's own ui file
/// (`ui.db` beside `tickets.db`) as a `--no-live` push keeps one: its hash.
fn keep_notes_version(dir: &std::path::Path, n: u32, components: &[&str]) -> String {
    use plexus_ui_service::{Keep, NewVersion, UiStore};
    let js = |c: &str| format!("export function mount(target) {{ target.textContent = \"{c} v{n}\"; return {{}}; }}\n");
    let entries: serde_json::Map<String, Value> = components.iter().map(|c| (c.to_string(), json!({ "profile": "svelte/1", "module": support::sha_hex(js(c).as_bytes()), "uses": [] }))).collect();
    let manifest = json!({ "abi": "engine/1", "name": "notes", "contract": "unknown", "routes": {}, "slots": {}, "components": entries }).to_string();
    let mut b = tar::Builder::new(Vec::new());
    let mut files = vec![("manifest.json".to_string(), manifest.clone())];
    files.extend(components.iter().map(|c| (format!("components/{c}.js"), js(c))));
    for (path, body) in files {
        let mut h = tar::Header::new_gnu();
        h.set_size(body.len() as u64);
        h.set_mode(0o644);
        h.set_cksum();
        b.append_data(&mut h, path, body.as_bytes()).unwrap();
    }
    let bundle = b.into_inner().unwrap();
    let hash = support::sha_hex(&bundle);
    let store = UiStore::open(&dir.join("ui.db")).unwrap();
    let v = NewVersion { bundle, manifest, commit: None, pushed_by: "test".into(), pushed_at: n as i64 };
    store.push_kept(v, &hash, &Keep { live: false, aside_moves_name: true, ..Keep::default() }).unwrap().version
}

/// A note filled while it is open is said on every watch of it, read by a
/// viewer as it stands, and refused to a viewer's fill (`forbidden`, the
/// gate's); a fill naming a component the version does not hold, or a
/// version nobody keeps, is refused naming both, and the slot is as it was.
#[tokio::test]
async fn a_slot_is_filled_said_read_and_held_to_the_editor() {
    let (dir, _port, _admin, editor, viewer, _s, _org) = people().await;
    let id = one(&editor, "notes.create", json!({"title": "The service map", "body": "Draw every service."})).await["note"]["id"].as_str().unwrap().to_string();
    let empty = one(&viewer, "notes.slot", json!({"id": id})).await;
    assert_eq!((empty["type"].as_str(), empty["slot"]["fills"].as_array().map(Vec::len), empty["slot"].get("fill")), (Some("slot"), Some(0), None), "{empty}");

    let v1 = keep_notes_version(dir.path(), 1, &["graph-nodes"]);
    let mut w = call(&viewer, "notes.watch", json!({"id": id})).await;
    assert_eq!(next(&mut w).await["type"], "watching");
    let filled = one(&editor, "notes.fill", json!({"id": id, "component": "graph-nodes", "version": &v1[..12], "say": "nodes on a ring", "props": {"nodes": 6}})).await;
    assert_eq!(filled["type"], "filled", "{filled}");
    assert_eq!(filled["slot"]["fill"]["version"], v1, "a prefix is kept as the whole hash: {filled}");
    let heard = next_of(&mut w, "slot_changed").await;
    assert_eq!((heard["id"].as_str(), &heard["slot"]), (Some(id.as_str()), &filled["slot"]), "{heard}");
    let read = one(&viewer, "notes.slot", json!({"id": id})).await;
    assert_eq!(read["slot"], filled["slot"]);
    assert_eq!((read["slot"]["fill"]["n"].as_u64(), read["slot"]["fill"]["by"].is_string(), &read["slot"]["props"]), (Some(1), true, &json!({"nodes": 6})));

    // A viewer may not fill: the gate's refusal; the slot answers them all the same.
    assert!(gate_refused(&one(&viewer, "notes.fill", json!({"id": id, "component": "graph-nodes", "version": v1})).await, "notes.fill"));
    assert_eq!(one(&viewer, "notes.slot", json!({"id": id})).await["slot"], filled["slot"]);

    // A component the version does not hold; a version nobody keeps — each named, nothing written.
    let no = one(&editor, "notes.fill", json!({"id": id, "component": "node-legend", "version": v1})).await;
    assert_eq!(no["reason"], "unknown_component", "{no}");
    assert!(no["message"].as_str().is_some_and(|m| m.contains("node-legend") && m.contains(&v1)), "{no}");
    let none = "0".repeat(64);
    let no = one(&editor, "notes.fill", json!({"id": id, "component": "graph-nodes", "version": none})).await;
    assert_eq!(no["reason"], "unknown_version", "{no}");
    assert!(no["message"].as_str().is_some_and(|m| m.contains("graph-nodes") && m.contains(&none)), "{no}");
    assert_eq!(one(&viewer, "notes.slot", json!({"id": id})).await["slot"], filled["slot"], "as it was");

    // An empty: nothing drawn, the fills kept.
    let gone = one(&editor, "notes.empty", json!({"id": id})).await;
    assert_eq!((gone["type"].as_str(), gone["slot"].get("fill"), gone["slot"]["fills"].as_array().map(Vec::len)), (Some("emptied"), None, Some(1)), "{gone}");
    assert_eq!(next_of(&mut w, "slot_changed").await["slot"], gone["slot"]);
}

/// A build said on a filled slot reaches every watch with its step; the
/// fill stays; a failure carries the refusal's line; a fill ends it.
#[tokio::test]
async fn a_build_is_said_step_by_step_and_a_failure_keeps_the_fill() {
    let (dir, _port, _admin, editor, viewer, _s, _org) = people().await;
    let id = one(&editor, "notes.create", json!({"title": "Map"})).await["note"]["id"].as_str().unwrap().to_string();
    let (v1, v2) = (keep_notes_version(dir.path(), 1, &["graph-nodes"]), keep_notes_version(dir.path(), 2, &["graph-nodes"]));
    one(&editor, "notes.fill", json!({"id": id, "component": "graph-nodes", "version": v1})).await;
    let mut w = call(&viewer, "notes.watch", json!({"id": id})).await;
    assert_eq!(next(&mut w).await["type"], "watching");

    let b = one(&editor, "notes.building", json!({"id": id, "component": "graph-nodes", "step": "written", "say": "drag a node"})).await;
    assert_eq!(b["type"], "building", "{b}");
    let heard = next_of(&mut w, "building_changed").await;
    assert_eq!((heard["building"]["step"].as_str(), heard["building"]["say"].as_str(), heard["slot"]["fill"]["version"].as_str()), (Some("written"), Some("drag a node"), Some(v1.as_str())), "the step, and the component that was there: {heard}");

    let why = "graph-nodes.svelte:9:54 `drag` is not defined";
    one(&editor, "notes.building", json!({"id": id, "component": "graph-nodes", "step": "written", "failed": true, "say": why})).await;
    let heard = next_of(&mut w, "building_changed").await;
    assert_eq!((heard["building"]["failed"].as_bool(), heard["building"]["say"].as_str(), heard["slot"]["fill"]["version"].as_str()), (Some(true), Some(why), Some(v1.as_str())), "{heard}");
    assert!(gate_refused(&one(&viewer, "notes.building", json!({"id": id, "component": "graph-nodes"})).await, "notes.building"));
    assert!(one(&editor, "notes.building", json!({"id": id, "component": "graph-nodes", "step": "drawn"})).await.to_string().contains("step"), "a step the vocabulary does not hold is refused naming it");

    let filled = one(&editor, "notes.fill", json!({"id": id, "component": "graph-nodes", "version": v2})).await;
    assert!(filled["slot"].get("building").is_none(), "a fill ends it: {filled}");
    assert!(next_of(&mut w, "slot_changed").await["slot"].get("building").is_none());
}

/// A component's props, patched on one page, reach a second page's watch;
/// a `null` takes a key away; the slot holds them to 64 KiB.
#[tokio::test]
async fn props_patched_on_one_page_reach_another() {
    let (dir, port, _admin, editor, _viewer, _s, _org) = people().await;
    let id = one(&editor, "notes.create", json!({"title": "Map"})).await["note"]["id"].as_str().unwrap().to_string();
    let v1 = keep_notes_version(dir.path(), 1, &["graph-nodes"]);
    one(&editor, "notes.fill", json!({"id": id, "component": "graph-nodes", "version": v1, "props": {"lines": [], "keep": true}})).await;
    let second = connect_key(&EDITOR, port).await;
    let mut w = call(&second, "notes.watch", json!({"id": id})).await;
    assert_eq!(next(&mut w).await["type"], "watching");

    let p = one(&editor, "notes.props", json!({"id": id, "patch": {"lines": [["tickets", "ui"]], "keep": null}})).await;
    assert_eq!((p["type"].as_str(), &p["slot"]["props"]), (Some("props"), &json!({"lines": [["tickets", "ui"]]})), "{p}");
    let heard = next_of(&mut w, "props_changed").await;
    assert_eq!((&heard["props"], heard["by"].is_string(), &heard["slot"]["props"]), (&json!({"lines": [["tickets", "ui"]]}), true, &json!({"lines": [["tickets", "ui"]]})), "{heard}");
    // A reload starts from them.
    assert_eq!(one(&second, "notes.slot", json!({"id": id})).await["slot"]["props"], json!({"lines": [["tickets", "ui"]]}));

    let half = "y".repeat(40 * 1024);
    assert_eq!(one(&editor, "notes.props", json!({"id": id, "patch": {"a": half}})).await["type"], "props");
    let over = one(&editor, "notes.props", json!({"id": id, "patch": {"b": half}})).await;
    assert_eq!(over["reason"], "too_large", "{over}");
    assert!(one(&editor, "notes.props", json!({"id": id, "patch": [1]})).await.to_string().contains("JSON object"), "a patch is an object");
}

/// NOTES-4: a menger started fresh with the Svelte compiler keeps the
/// bundle `notes`, so an editor on `*` authors into it at once; the
/// component authored fills a slot, and a push of the app's bundle (a
/// deploy) leaves the slot drawing the same component at the same version,
/// and the bundle's newest version where it was.
#[tokio::test]
async fn a_fresh_menger_keeps_the_bundle_notes_and_a_deploy_leaves_a_fill() {
    use support::{connect, free_port, push_params, spawn_menger, ui_args, UI_SIGNER};
    let dir = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
    let ws = free_port().await;
    let mut extra = vec!["--no-shell".to_string()];
    extra.extend(ui_args(&dir.path().join("ui.db")));
    extra.extend(["--ui-author-key".to_string(), dir.path().join("author.key").display().to_string()]);
    extra.extend(["--ui-svelte-compiler".to_string(), support::svelte_compiler_wasm().display().to_string()]);
    let _m = spawn_menger(ws, &dir.path().join("t.db"), &extra).await;
    let c = connect(ws).await;

    let seed = support::call(&c, "ui.history", json!({"bundle": "notes"})).await;
    assert!(seed.iter().any(|e| e["type"] == "move"), "the bundle notes is kept from the first start: {seed:?}");
    let src = "<script module>\n  export const uses = [\"notes.props\"];\n</script>\n\n<script>\n  let { note = \"\", props = \"{}\" } = $props();\n  const p = $derived(JSON.parse(props || \"{}\"));\n</script>\n\n<p class=\"g\">{note}: {p.nodes ?? 0} nodes</p>\n\n<style>\n  .g { color: var(--muted); }\n</style>\n";
    let made = support::call(&c, "ui.author", json!({"bundle": "notes", "sources": [{"name": "graph-nodes", "profile": "svelte/1", "source": src}]})).await;
    assert_eq!(made[0]["type"], "authored", "{made:?}");
    let version = made[0]["version"].as_str().unwrap().to_string();

    assert_eq!(support::call(&c, "tickets.create", json!({"epic": "B", "title": "b", "kind": "scope"})).await[0]["type"], "created");
    let id = support::call(&c, "notes.create", json!({"title": "The service map"})).await[0]["note"]["id"].as_str().unwrap().to_string();
    let filled = support::call(&c, "notes.fill", json!({"id": id, "component": "graph-nodes", "version": version})).await;
    assert_eq!(filled[0]["type"], "filled", "{filled:?}");

    let pushed = support::call(&c, "ui.push", push_params(1, &UI_SIGNER)).await;
    assert_eq!(pushed[0]["type"], "pushed", "the app's bundle, as a deploy pushes it: {pushed:?}");
    let slot = support::call(&c, "notes.slot", json!({"id": id})).await;
    assert_eq!((slot[0]["slot"]["fill"]["component"].as_str(), slot[0]["slot"]["fill"]["version"].as_str()), (Some("graph-nodes"), Some(version.as_str())), "{slot:?}");
    let newest = support::call(&c, "ui.history", json!({"bundle": "notes", "limit": 1})).await;
    assert_eq!(newest[0]["version"].as_str(), Some(version.as_str()), "the bundle's newest is still the authored one: {newest:?}");
    assert!(support::call(&c, "ui.versions", json!({})).await.iter().any(|v| v["hash"].as_str() == Some(version.as_str()) && v["live"] == false), "kept, never live");
}
