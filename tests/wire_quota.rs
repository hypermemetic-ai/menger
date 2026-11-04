//! V2-31: `quota` over the real protocol — a real TransportServer, a real
//! WebSocket client, the `menger.call` envelope, the service as `main`
//! builds it (the quota service beside `tickets`, `ui` and `media`, its file
//! beside the tickets file, the built-in free tier): a person's usage with
//! the plan's caps; the tickets service's writes reserved against the
//! author's allowance through the seam — refused typed over it, with
//! nothing written; the effective cap own, else a team's, else the
//! default; the table of everyone, sorted, an admin's; the seam's four
//! methods the server's alone; parallel reserves near the cap passing what
//! fits; the recount through the stores' own methods; the box guard over a
//! fake disk.

use jsonrpsee::core::client::{Subscription, SubscriptionClientT};
use jsonrpsee::core::params::ObjectParams;
use jsonrpsee::ws_client::{HeaderMap, HeaderValue, WsClient, WsClientBuilder};
use menger_quota::{DiskUsage, FakeDisk};
use plexus_core::plexus::DynamicHub;
use plexus_transport::TransportServer;
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::Duration;

mod support;

/// An editor on `Q`.
const EDITOR: [u8; 32] = [0xe1; 32];
/// Another editor on `Q`.
const OTHER: [u8; 32] = [0xe2; 32];

/// A server on a throwaway database, as `main` builds it; the box guard
/// reading `disk` when given (else this machine's).
async fn start(disk: Option<Arc<FakeDisk>>) -> u16 {
    start_with(disk, Default::default()).await
}

/// [`start`], the call layer held to `limits`.
async fn start_with(disk: Option<Arc<FakeDisk>>, limits: menger::calls::CallLimits) -> u16 {
    let dir = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).expect("tempdir");
    let db_path = dir.path().join("tickets.db");
    let port = support::free_port().await;
    let login = support::Login::new();
    let service = match disk {
        None => support::service(&db_path, &login).await,
        Some(disk) => {
            let access = login.access.clone();
            let backend = Arc::new(menger::auth_backend::MengerAuth::new(access.clone(), Vec::new(), None, "menger"));
            let ui = menger::UiSetup::beside(&db_path).unwrap();
            let media = menger::MediaSetup::beside(&db_path).unwrap();
            let quota = menger::QuotaSetup::beside(&db_path).unwrap().guarding(disk as Arc<dyn DiskUsage>);
            menger::build_rpc_services(&db_path, None, Default::default(), Vec::new(), access, backend, None, menger::store::BODY_MAX_DEFAULT, ui, media, menger::DocsSetup::beside(&db_path).unwrap(), quota, |store, access| {
                menger::TicketsActivation::with_access(store, access)
            })
            .await
            .unwrap()
        }
    };
    let gate = login.gate(port);
    let conv = move |arc: Arc<DynamicHub>| menger::calls::rpc_module(arc, limits).map_err(|e| anyhow::anyhow!("{e}"));
    tokio::spawn(async move {
        let _keep = dir;
        TransportServer::builder(service, conv).with_websocket(port).with_session_validator(gate).anonymous_view().build().await.expect("build").serve().await.expect("serve");
    });
    port
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

/// Every event a call answers with (data items' contents; an error item whole, last).
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

async fn grant(admin: &WsClient, seed: &[u8; 32], role: &str, scope: &str) {
    let g = one(admin, "auth.grant", json!({ "principal": support::principal_of(seed), "role": role, "scope": scope })).await;
    assert!(g.get("error_kind").is_none(), "{role} on {scope}: {g}");
}

/// One source of a `usage` event.
fn source<'a>(usage: &'a Value, name: &str) -> &'a Value {
    usage["sources"].as_array().unwrap().iter().find(|s| s["source"] == name).unwrap_or_else(|| panic!("{name} in {usage}"))
}

/// A server with `Q`, an editor on it, the admin's and the editor's clients.
async fn with_editor(disk: Option<Arc<FakeDisk>>) -> (WsClient, WsClient, u16) {
    let port = start(disk).await;
    let admin = support::connect(port).await;
    assert_eq!(one(&admin, "tickets.create", json!({"epic": "Q", "title": "q", "kind": "scope"})).await["type"], "created");
    grant(&admin, &EDITOR, "editor", "Q").await;
    let editor = connect_key(&EDITOR, port).await;
    (admin, editor, port)
}

#[tokio::test]
async fn a_person_sees_their_usage_with_the_free_tiers_caps() {
    let (admin, editor, _) = with_editor(None).await;
    let mine = one(&editor, "quota.usage", json!({})).await;
    assert_eq!((mine["type"].as_str(), mine["principal"].as_str(), mine["exempt"].as_bool()), (Some("usage"), Some(support::principal_of(&EDITOR).as_str()), Some(false)), "{mine}");
    assert_eq!(source(&mine, "media.bytes")["cap"], 2_000_000_000u64);
    assert_eq!(source(&mine, "docs.bytes")["cap"], 200_000_000u64);
    assert_eq!(source(&mine, "tickets.bytes")["cap"], 50_000_000u64);
    assert_eq!(source(&mine, "media.fetch.calls")["cap"], 300);
    assert_eq!(source(&mine, "media.fetch.calls")["window"], "day");
    assert_eq!(source(&mine, "media.egress.bytes")["cap"], 2_000_000_000u64);
    assert_eq!(source(&mine, "tickets.count")["cap"], Value::Null, "unlimited");
    assert_eq!(source(&mine, "tickets.bytes")["from"], "default");
    // The operator is exempt: counted, never capped.
    let theirs = one(&admin, "quota.usage", json!({})).await;
    assert_eq!((theirs["exempt"].as_bool(), source(&theirs, "media.bytes")["cap"].clone(), source(&theirs, "media.bytes")["from"].clone()), (Some(true), Value::Null, json!("exempt")));
    // Another's usage is an admin's to read.
    let other = one(&editor, "quota.usage", json!({"principal": support::admin()})).await;
    assert_eq!(other["type"], "error", "{other}");
    assert_eq!(one(&admin, "quota.usage", json!({"principal": support::principal_of(&EDITOR)})).await["type"], "usage");
}

/// `tickets.bytes` is what the store keeps (V2-31's follow-up): each body's
/// content the first time it is kept, and each ticket's current body, its
/// last writer's. A write reserves the delta; one that leaves the body as
/// it was counts nothing — an editor saving the same body all day costs
/// nothing more; over the cap a write is refused typed, nothing written.
#[tokio::test]
async fn a_tickets_write_counts_what_is_stored_and_is_refused_typed_over_the_cap() {
    let (admin, editor, port) = with_editor(None).await;
    let me = support::principal_of(&EDITOR);
    let bytes = |u: &Value| (source(u, "tickets.bytes")["used"].as_u64().unwrap(), source(u, "tickets.bytes")["held"].as_u64().unwrap());
    let mine = || async { bytes(&one(&editor, "quota.usage", json!({})).await) };
    one(&admin, "quota.set", json!({"principal": me, "source": "tickets.bytes", "cap": "1000"})).await;

    // Born: its content kept once, and it is the current body: 2 × 60.
    let made = one(&editor, "tickets.create", json!({"epic": "Q", "title": "fits", "body": "x".repeat(60)})).await;
    assert_eq!(made["type"], "created", "{made}");
    let id = made["ticket"]["id"].as_str().unwrap().to_string();
    assert_eq!(mine().await, (120, 0));
    assert_eq!(source(&one(&editor, "quota.usage", json!({})).await, "tickets.count")["used"], 1);
    // A shorter body: its content (10) kept, the current body 60 → 10.
    assert_eq!(one(&editor, "tickets.amend", json!({"id": id, "body": "x".repeat(10)})).await["type"], "amended");
    assert_eq!(mine().await, (80, 0), "70 kept + 10 current");
    // The autosave: the same body written again and again counts nothing.
    for _ in 0..25 {
        assert_eq!(one(&editor, "tickets.amend", json!({"id": id, "body": "x".repeat(10)})).await["type"], "amended");
    }
    assert_eq!(mine().await, (80, 0), "an unchanged write keeps nothing");
    // Back to a body kept already, in the same sitting (V2-37): it replaces
    // the revision that alone held the 10, which is let go and credited.
    one(&editor, "tickets.amend", json!({"id": id, "body": "x".repeat(60)})).await;
    assert_eq!(mine().await, (120, 0), "60 kept + 60 current");
    // update's body likewise; a title alone keeps no body.
    assert_eq!(one(&editor, "tickets.update", json!({"id": id, "title": "renamed"})).await["type"], "updated");
    assert_eq!(mine().await, (120, 0));

    // Over the cap: typed, the numbers said, nothing written, nothing held.
    one(&admin, "quota.set", json!({"principal": me, "source": "tickets.bytes", "cap": "140"})).await;
    let over = one(&editor, "tickets.amend", json!({"id": id, "body": "z".repeat(41)})).await;
    assert_eq!((over["type"].as_str(), over["source"].as_str(), over["used"].as_u64(), over["cap"].as_u64(), over["asked"].as_u64()), (Some("over_allowance"), Some("tickets.bytes"), Some(120), Some(140), Some(41)), "{over}");
    assert!(over["message"].as_str().unwrap().contains("nothing was written"));
    assert_eq!(one(&editor, "tickets.get", json!({"id": id})).await["ticket"]["body"], "x".repeat(60));
    let born = one(&editor, "tickets.create", json!({"epic": "Q", "title": "too much", "body": "y".repeat(20)})).await;
    assert_eq!(born["type"], "over_allowance", "{born}");
    assert_eq!(all(&editor, "tickets.find", json!({"query": "too much"})).await[0]["items"], json!([]), "no ticket was made");
    assert_eq!(mine().await, (120, 0));
    let upd = one(&editor, "tickets.update", json!({"id": id, "body": "w".repeat(31)})).await;
    assert_eq!(upd["type"], "over_allowance", "{upd}");

    // Another writer takes the body over: theirs now, its old size credited back.
    one(&admin, "quota.set", json!({"principal": me, "source": "tickets.bytes", "cap": "unlimited"})).await;
    grant(&admin, &OTHER, "editor", "Q").await;
    let other = connect_key(&OTHER, port).await;
    assert_eq!(one(&other, "tickets.amend", json!({"id": id, "body": "o".repeat(20)})).await["type"], "amended");
    assert_eq!(mine().await, (60, 0), "the current body is no longer theirs");
    assert_eq!(bytes(&one(&other, "quota.usage", json!({})).await), (40, 0), "20 kept + 20 current");

    // The store's own count says the same: the recount finds no drift.
    let counted = all(&admin, "tickets.revisions.usage", json!({})).await;
    assert!(counted.contains(&json!({"type": "usage", "principal": me, "count": 1, "bytes": 60})), "{counted:?}");
    assert!(counted.contains(&json!({"type": "usage", "principal": support::principal_of(&OTHER), "count": 0, "bytes": 40})), "{counted:?}");
    let r = all(&admin, "quota.recount", json!({})).await;
    assert!(!r.iter().any(|e| e["type"] == "drift" && e["source"] == "tickets.bytes"), "{r:?}");
}

/// V2-37: an editor autosaving a ticket — 25 different bodies within a
/// minute — keeps one revision for the sitting, not 25 copies, and its
/// allowance counts what is kept: the store's recount agrees.
#[tokio::test]
async fn autosaves_in_one_sitting_keep_one_revision_and_count_what_is_kept() {
    let (admin, editor, _) = with_editor(None).await;
    let me = support::principal_of(&EDITOR);
    let made = one(&editor, "tickets.create", json!({"epic": "Q", "title": "long", "body": "a".repeat(1000)})).await;
    let id = made["ticket"]["id"].as_str().unwrap().to_string();
    for i in 0..25 {
        let body = format!("{}{i:02}", "a".repeat(1000));
        assert_eq!(one(&editor, "tickets.amend", json!({"id": id, "body": body})).await["type"], "amended");
    }
    let page = all(&editor, "tickets.revisions.list", json!({"id": id})).await;
    let end = page.iter().find(|e| e["type"] == "revisions_end").expect("the end");
    assert_eq!(end["count"], 2, "the birth and the sitting: {page:?}");
    let used = source(&one(&editor, "quota.usage", json!({})).await, "tickets.bytes")["used"].as_u64().unwrap();
    assert_eq!(used, 1000 + 1002 + 1002, "the birth's body, the last save's, the current one");
    let counted = all(&admin, "tickets.revisions.usage", json!({"principal": me})).await;
    assert_eq!(counted, vec![json!({"type": "usage", "principal": me, "count": 1, "bytes": used})]);
    let r = all(&admin, "quota.recount", json!({"principal": me})).await;
    assert_eq!(r.last().unwrap()["drifted"], 0, "{r:?}");
}

#[tokio::test]
async fn the_effective_cap_is_own_else_the_teams_largest_else_the_default() {
    let (admin, editor, _) = with_editor(None).await;
    let me = support::principal_of(&EDITOR);
    for t in ["crew", "big-crew"] {
        assert_eq!(one(&admin, "members.teams.create", json!({"name": t})).await["type"], "team");
        assert_eq!(one(&admin, "members.teams.add", json!({"team": t, "principal": me})).await["type"], "team_member");
    }
    one(&admin, "quota.set", json!({"team": "crew", "source": "docs.bytes", "cap": "1 GB"})).await;
    one(&admin, "quota.set", json!({"team": "big-crew", "source": "docs.bytes", "cap": "5 GB"})).await;
    let u = one(&editor, "quota.usage", json!({})).await;
    assert_eq!((source(&u, "docs.bytes")["cap"].as_u64(), source(&u, "docs.bytes")["from"].as_str()), (Some(5_000_000_000), Some("team:big-crew")));
    one(&admin, "quota.set", json!({"principal": me, "source": "docs.bytes", "cap": "10 MB"})).await;
    let u = one(&editor, "quota.usage", json!({})).await;
    assert_eq!((source(&u, "docs.bytes")["cap"].as_u64(), source(&u, "docs.bytes")["from"].as_str()), (Some(10_000_000), Some("own")));
    assert_eq!(one(&admin, "quota.set", json!({"principal": me, "source": "docs.bytes", "cap": "clear"})).await["cleared"], true);
    one(&admin, "quota.set", json!({"default": true, "source": "media.bytes", "cap": "7 GB"})).await;
    let u = one(&editor, "quota.usage", json!({})).await;
    assert_eq!(source(&u, "docs.bytes")["from"], "team:big-crew", "cleared: back to the team's");
    assert_eq!((source(&u, "media.bytes")["cap"].as_u64(), source(&u, "media.bytes")["from"].as_str()), (Some(7_000_000_000), Some("default")));
    // Exactly one holder; an unknown source is the parameter's refusal.
    assert_eq!(one(&admin, "quota.set", json!({"principal": me, "team": "crew", "source": "docs.bytes", "cap": "1"})).await["type"], "error");
    let bad = one(&admin, "quota.set", json!({"default": true, "source": "media.nothing", "cap": "1"})).await;
    assert!(bad.to_string().contains("media.nothing"), "{bad}");
    // set is an admin's.
    let refused = one(&editor, "quota.set", json!({"default": true, "source": "media.bytes", "cap": "unlimited"})).await;
    assert!(refused["type"] == "error" || refused.get("error_kind").is_some(), "{refused}");
}

#[tokio::test]
async fn the_table_is_everyone_sorted_with_the_box_first_and_the_total_last() {
    let (admin, editor, port) = with_editor(None).await;
    grant(&admin, &OTHER, "editor", "Q").await;
    let other = connect_key(&OTHER, port).await;
    one(&editor, "tickets.create", json!({"epic": "Q", "title": "small", "body": "a".repeat(10)})).await;
    one(&other, "tickets.create", json!({"epic": "Q", "title": "big", "body": "b".repeat(500)})).await;
    let t = all(&admin, "quota.table", json!({})).await;
    assert_eq!(t[0]["type"], "box", "{t:?}");
    assert!(["ok", "warn", "full", "unknown"].contains(&t[0]["state"].as_str().unwrap()));
    assert_eq!((t[0]["warn_percent"].as_f64(), t[0]["full_percent"].as_f64()), (Some(70.0), Some(85.0)));
    let rows: Vec<&Value> = t.iter().filter(|e| e["type"] == "usage").collect();
    let names: Vec<&str> = rows.iter().map(|r| r["principal"].as_str().unwrap()).collect();
    let (e, o) = (support::principal_of(&EDITOR), support::principal_of(&OTHER));
    let (pe, po) = (names.iter().position(|n| *n == e).unwrap(), names.iter().position(|n| *n == o).unwrap());
    assert!(po < pe, "the most stored first: {names:?}");
    let last = t.last().unwrap();
    assert_eq!(last["type"], "total");
    assert_eq!(last["principals"].as_u64(), Some(rows.len() as u64));
    assert!(last["bytes"].as_u64().unwrap() >= 510);
    let by_name = all(&admin, "quota.table", json!({"sort": "principal"})).await;
    let names: Vec<&str> = by_name.iter().filter(|e| e["type"] == "usage").map(|r| r["principal"].as_str().unwrap()).collect();
    let mut sorted = names.clone();
    sorted.sort();
    assert_eq!(names, sorted);
    // Only principals in an organization.
    let orgs = all(&admin, "orgs.list", json!({})).await;
    let org = orgs.iter().find_map(|o| o["org"]["name"].as_str()).expect("an organization").to_string();
    let t = all(&admin, "quota.table", json!({"org": org})).await;
    assert!(t.iter().any(|r| r["principal"] == e) && t.iter().any(|r| r["principal"] == o), "{t:?}");
    let t = all(&admin, "quota.table", json!({"org": "nowhere"})).await;
    assert_eq!(t.last().unwrap()["principals"], 0, "{t:?}");
    // An editor is refused.
    let refused = all(&editor, "quota.table", json!({})).await;
    assert!(refused[0]["type"] == "error" || refused[0].get("error_kind").is_some(), "{refused:?}");
}

#[tokio::test]
async fn the_seams_methods_are_the_servers_alone_and_parallel_reserves_pass_what_fits() {
    let (admin, editor, _) = with_editor(None).await;
    let me = support::principal_of(&EDITOR);
    let refused = one(&editor, "quota.reserve", json!({"principal": me, "source": "media.bytes", "amount": 1})).await;
    assert_eq!(refused["error_kind"], "forbidden", "{refused}");
    one(&admin, "quota.set", json!({"principal": me, "source": "tickets.count", "cap": "10"})).await;
    // 30 at once over the wire, 10 fit.
    let asks = (0..30).map(|_| one(&admin, "quota.reserve", json!({"principal": me, "source": "tickets.count", "amount": 1})));
    let answers = futures::future::join_all(asks).await;
    let held: Vec<&Value> = answers.iter().filter(|a| a["type"] == "reserved").collect();
    assert_eq!(held.len(), 10, "{answers:?}");
    assert!(answers.iter().filter(|a| a["type"] != "reserved").all(|a| a["type"] == "over_allowance" && a["cap"] == 10));
    // Commit half, release half: used counts the committed once.
    for (i, h) in held.iter().enumerate() {
        let t = h["ticket"].as_str().unwrap();
        let (m, ty) = if i % 2 == 0 { ("quota.commit", "committed") } else { ("quota.release", "released") };
        assert_eq!(one(&admin, m, json!({"ticket": t})).await["type"], ty);
        assert_eq!(one(&admin, m, json!({"ticket": t})).await["type"], "error", "once");
    }
    let u = one(&admin, "quota.usage", json!({"principal": me})).await;
    assert_eq!((source(&u, "tickets.count")["used"].as_u64(), source(&u, "tickets.count")["held"].as_u64()), (Some(5), Some(0)));
    let c = one(&admin, "quota.credit", json!({"principal": me, "source": "tickets.count", "amount": 2})).await;
    assert_eq!(c, json!({"type": "credited", "principal": me, "source": "tickets.count", "used": 3}));
    // An unknown source is refused.
    let bad = one(&admin, "quota.reserve", json!({"principal": me, "source": "lasers", "amount": 1})).await;
    assert!(bad.to_string().contains("lasers"), "{bad}");
}

#[tokio::test]
async fn recount_rebuilds_from_the_stores_and_reports_drift() {
    let (admin, editor, _) = with_editor(None).await;
    let me = support::principal_of(&EDITOR);
    let made = one(&editor, "tickets.create", json!({"epic": "Q", "title": "one", "body": "a".repeat(100)})).await;
    one(&editor, "tickets.amend", json!({"id": made["ticket"]["id"], "body": "b".repeat(40)})).await;
    // The store's own count, over the wire.
    let counted = all(&admin, "tickets.revisions.usage", json!({"principal": me})).await;
    assert_eq!(counted, vec![json!({"type": "usage", "principal": me, "count": 1, "bytes": 180})], "100 and 40 kept, 40 current");
    assert_eq!(all(&editor, "tickets.revisions.usage", json!({})).await[0]["error_kind"], "forbidden", "an admin's");
    // In step: no drift.
    let r = all(&admin, "quota.recount", json!({"principal": me})).await;
    let done = r.last().unwrap();
    assert_eq!((done["type"].as_str(), done["drifted"].as_u64()), (Some("recounted"), Some(0)), "{r:?}");
    assert!(done["sources"].as_array().unwrap().contains(&json!("tickets.bytes")));
    assert_eq!(done["not_counted"], json!([]), "every store answers: {done}");
    // Drift (a credit the stores never saw), found and set back.
    one(&admin, "quota.credit", json!({"principal": me, "source": "tickets.bytes", "amount": 40})).await;
    let r = all(&admin, "quota.recount", json!({})).await;
    assert!(r.contains(&json!({"type": "drift", "principal": me, "source": "tickets.bytes", "was": 140, "counted": 180})), "{r:?}");
    let u = one(&editor, "quota.usage", json!({})).await;
    assert_eq!(source(&u, "tickets.bytes")["used"].as_u64(), Some(180));
    assert_eq!(all(&editor, "quota.recount", json!({})).await[0]["type"], "error", "an admin's");
}

#[tokio::test]
async fn the_box_guard_refuses_the_capped_past_the_full_line_never_an_admin() {
    let disk = Arc::new(FakeDisk::at(50.0));
    let (admin, editor, _) = with_editor(Some(disk.clone())).await;
    assert_eq!(all(&admin, "quota.table", json!({})).await[0]["state"], "ok");
    disk.set(72.0);
    assert_eq!(all(&admin, "quota.table", json!({})).await[0]["state"], "warn", "admins warned from 70%");
    assert_eq!(one(&editor, "tickets.create", json!({"epic": "Q", "title": "still", "body": "ok"})).await["type"], "created", "a warning refuses nothing");
    disk.set(86.0);
    let full = one(&editor, "tickets.create", json!({"epic": "Q", "title": "no room", "body": "x"})).await;
    assert_eq!((full["type"].as_str(), full["used_percent"].as_f64(), full["full_percent"].as_f64()), (Some("box_full"), Some(86.0), Some(85.0)), "{full}");
    let t = all(&admin, "quota.table", json!({})).await;
    assert_eq!((t[0]["state"].as_str(), t[0]["used_percent"].as_f64()), (Some("full"), Some(86.0)));
    assert_eq!(one(&admin, "tickets.create", json!({"epic": "Q", "title": "the admin", "body": "x"})).await["type"], "created", "an admin of * still writes");
    disk.set(60.0);
    assert_eq!(one(&editor, "tickets.create", json!({"epic": "Q", "title": "room again", "body": "x"})).await["type"], "created");
}

/// The write rate (the call layer's bucket, per principal): past it a
/// write is answered `too_many_writes` with how long to wait, unrun; reads
/// are never held; another principal has its own bucket.
#[tokio::test]
async fn writes_past_the_rate_are_answered_too_many_writes_and_not_run() {
    let port = start_with(None, menger::calls::CallLimits { writes_per_second: 2, ..Default::default() }).await;
    let admin = support::connect(port).await;
    assert_eq!(one(&admin, "tickets.create", json!({"epic": "R", "title": "r", "kind": "scope"})).await["type"], "created");
    grant(&admin, &EDITOR, "editor", "R").await;
    grant(&admin, &OTHER, "editor", "R").await;
    let (editor, other) = (connect_key(&EDITOR, port).await, connect_key(&OTHER, port).await);
    // Eight at once: the burst (twice the rate) passes, the rest wait.
    let answers = futures::future::join_all((0..8).map(|i| one(&editor, "tickets.create", json!({"epic": "R", "title": format!("r{i}")})))).await;
    let made = answers.iter().filter(|a| a["type"] == "created").count();
    let held: Vec<&Value> = answers.iter().filter(|a| a["error_kind"] == "too_many_writes").collect();
    assert!((4..=5).contains(&made), "a burst of twice the rate passes: {answers:?}");
    assert_eq!(made + held.len(), 8, "{answers:?}");
    assert!(held.iter().all(|h| h["retry_after_ms"].as_u64().is_some_and(|ms| ms > 0 && ms <= 500) && h["recoverable"] == true), "{held:?}");
    // Nothing held was written.
    let listed = all(&editor, "tickets.list", json!({"epic": "R", "summary": true})).await;
    let rows: usize = listed.iter().filter(|e| e["type"] == "rows").map(|e| e["items"].as_array().map_or(0, Vec::len)).sum();
    assert_eq!(rows, made + 1, "{listed:?}");
    // Reads are never held.
    for _ in 0..20 {
        assert_eq!(one(&editor, "tickets.health", json!({})).await["type"], "health");
    }
    // Another principal has a bucket of its own; an admin of * none.
    assert_eq!(one(&other, "tickets.create", json!({"epic": "R", "title": "mine"})).await["type"], "created");
    let admins = futures::future::join_all((0..12).map(|i| one(&admin, "tickets.create", json!({"epic": "R", "title": format!("a{i}")})))).await;
    assert!(admins.iter().all(|a| a["type"] == "created"), "{admins:?}");
    // And it fills again with time.
    tokio::time::sleep(Duration::from_millis(1100)).await;
    assert_eq!(one(&editor, "tickets.create", json!({"epic": "R", "title": "later"})).await["type"], "created");
}

/// The media service's allowance is the quota service (EDITOR-10's call
/// site): a put counts its bytes against the uploader, a duplicate nothing,
/// a put past the cap is refused with the numbers and nothing kept; the
/// recount reads media's own count (`media.usage { everyone }`).
#[tokio::test]
async fn an_upload_is_counted_against_the_uploader_and_refused_past_the_cap() {
    use base64::Engine;
    let (admin, editor, _) = with_editor(None).await;
    let me = support::principal_of(&EDITOR);
    let b64 = |b: &[u8]| base64::engine::general_purpose::STANDARD.encode(b);
    let (a, b) = (vec![1u8; 600], vec![2u8; 600]);
    one(&admin, "quota.set", json!({"principal": me, "source": "media.bytes", "cap": "2000"})).await;
    let kept = one(&editor, "media.put", json!({"data": b64(&a), "name": "a.bin"})).await;
    assert_eq!(kept["type"], "stored", "{kept}");
    let u = one(&editor, "quota.usage", json!({})).await;
    assert_eq!((source(&u, "media.bytes")["used"].as_u64(), source(&u, "media.bytes")["held"].as_u64()), (Some(600), Some(0)));
    // The same bytes again: a duplicate, nothing more counted.
    assert_eq!(one(&editor, "media.put", json!({"data": b64(&a)})).await["duplicate"], true);
    assert_eq!(source(&one(&editor, "quota.usage", json!({})).await, "media.bytes")["used"].as_u64(), Some(600));
    // Past the cap: refused, its numbers said, nothing kept.
    one(&admin, "quota.set", json!({"principal": me, "source": "media.bytes", "cap": "1000"})).await;
    let over = one(&editor, "media.put", json!({"data": b64(&b), "name": "b.bin"})).await;
    assert_eq!((over["type"].as_str(), over["reason"].as_str(), over["max"].as_u64()), (Some("refused"), Some("over_allowance"), Some(400)), "{over}");
    let m = over["message"].as_str().unwrap();
    assert!(m.contains("600 B used of 1 KB") && m.contains("603 B more") && m.contains("nothing was kept"), "{m}");
    assert_eq!(one(&editor, "media.usage", json!({})).await["bytes"], 600);
    // Every uploader's charge is an admin's to read; the recount reads it.
    assert_eq!(one(&editor, "media.usage", json!({"everyone": true})).await["type"], "error");
    assert_eq!(all(&admin, "media.usage", json!({"everyone": true})).await, vec![json!({"type": "usage", "principal": me, "files": 1, "bytes": 600})]);
    one(&admin, "quota.credit", json!({"principal": me, "source": "media.bytes", "amount": 600})).await;
    let r = all(&admin, "quota.recount", json!({"principal": me})).await;
    assert!(r.contains(&json!({"type": "drift", "principal": me, "source": "media.bytes", "was": 0, "counted": 600})), "{r:?}");
    let done = r.last().unwrap();
    assert!(done["sources"].as_array().unwrap().contains(&json!("media.bytes")), "{done}");
    assert!(done["not_counted"].as_array().unwrap().iter().all(|n| !n.as_str().unwrap().starts_with("media.")), "{done}");
}

/// The docs service's meter is the quota service (V2-29's call site): a
/// document made counts one `docs.count` and its body's bytes, every change
/// its bytes; past the cap the write is `refused { reason: quota }` with the
/// numbers, nothing written; the recount reads `docs.usage { everyone }`.
#[tokio::test]
async fn a_document_is_counted_against_its_writer_and_refused_past_the_cap() {
    let port = start(None).await;
    let admin = support::connect(port).await;
    grant(&admin, &OTHER, "editor", "*").await;
    let writer = connect_key(&OTHER, port).await;
    let me = support::principal_of(&OTHER);
    let made = one(&writer, "docs.create", json!({"title": "kiln notes", "body": "a".repeat(300)})).await;
    assert_eq!(made["type"], "created", "{made}");
    let u = one(&writer, "quota.usage", json!({})).await;
    let first = source(&u, "docs.bytes")["used"].as_u64().unwrap();
    assert_eq!((source(&u, "docs.count")["used"].as_u64(), source(&u, "docs.bytes")["held"].as_u64()), (Some(1), Some(0)));
    assert!((300..400).contains(&first), "the body's change, as kept: {first}");
    let id = made["doc"]["id"].as_str().unwrap().to_string();
    let body = format!("{}{}", "a".repeat(300), "b".repeat(50));
    let saved = one(&writer, "docs.save", json!({"id": id, "body": body})).await;
    assert_eq!(saved["type"], "saved", "{saved}");
    let grown = source(&one(&writer, "quota.usage", json!({})).await, "docs.bytes")["used"].as_u64().unwrap();
    assert!(grown > first, "a change counts its bytes: {grown}");
    // Capped where it stands: the next change is refused, said with the numbers.
    one(&admin, "quota.set", json!({"principal": me, "source": "docs.bytes", "cap": grown.to_string()})).await;
    let over = one(&writer, "docs.save", json!({"id": id, "body": "c".repeat(400)})).await;
    assert_eq!((over["type"].as_str(), over["reason"].as_str()), (Some("refused"), Some("quota")), "{over}");
    assert!(over["message"].as_str().unwrap().contains(&format!("used of {grown} B")) || over["message"].as_str().unwrap().contains("used of"), "{over}");
    assert_eq!(one(&writer, "docs.get", json!({"id": id})).await["doc"]["body"], body, "nothing written");
    one(&admin, "quota.set", json!({"principal": me, "source": "docs.count", "cap": "1"})).await;
    let second = one(&writer, "docs.create", json!({"title": "another"})).await;
    assert_eq!(second["reason"], "quota", "{second}");
    // The store's own count, and the recount by it.
    let counted = all(&admin, "docs.usage", json!({"everyone": true})).await;
    assert_eq!(counted, vec![json!({"type": "usage", "principal": me, "count": 1, "bytes": grown})]);
    assert_eq!(all(&writer, "docs.usage", json!({"everyone": true})).await[0]["type"], "error", "an admin's");
    one(&admin, "quota.credit", json!({"principal": me, "source": "docs.bytes", "amount": 100})).await;
    let r = all(&admin, "quota.recount", json!({"principal": me})).await;
    assert!(r.contains(&json!({"type": "drift", "principal": me, "source": "docs.bytes", "was": grown - 100, "counted": grown})), "{r:?}");
    assert_eq!(r.last().unwrap()["not_counted"], json!([]), "every store answers now");
}

/// V2-39: `questions.count` counts — `docs.ask` reserves one for the asker
/// before the question is kept and commits it after; a refused ask (a range
/// not in the text) releases it; past the cap the ask is `refused {
/// over_allowance }` with the numbers and no thread is kept.
#[tokio::test]
async fn a_question_is_counted_against_its_asker_and_refused_past_the_cap() {
    let port = start(None).await;
    let admin = support::connect(port).await;
    grant(&admin, &OTHER, "editor", "*").await;
    let asker = connect_key(&OTHER, port).await;
    let me = support::principal_of(&OTHER);
    one(&admin, "quota.set", json!({"principal": me, "source": "questions.count", "cap": "1"})).await;
    let made = one(&asker, "docs.create", json!({"title": "kiln notes", "body": "Fire twice a week."})).await;
    let id = made["doc"]["id"].as_str().unwrap().to_string();
    let questions = || async { let u = one(&asker, "quota.usage", json!({})).await; (source(&u, "questions.count")["used"].as_u64(), source(&u, "questions.count")["held"].as_u64()) };
    // A refused ask holds nothing.
    let bad = one(&asker, "docs.ask", json!({"id": id, "from": 10, "to": 2, "text": "backwards?"})).await;
    assert_eq!(bad["type"], "error", "{bad}");
    assert_eq!(questions().await, (Some(0), Some(0)), "released");
    // The first: kept and counted.
    let asked = one(&asker, "docs.ask", json!({"id": id, "from": 0, "to": 4, "text": "Twice?"})).await;
    assert_eq!(asked["type"], "asked", "{asked}");
    assert_eq!(questions().await, (Some(1), Some(0)));
    // The second: over the cap, typed, nothing kept.
    let over = one(&asker, "docs.ask", json!({"id": id, "from": 5, "to": 10, "text": "Which days?"})).await;
    assert_eq!((over["type"].as_str(), over["reason"].as_str()), (Some("refused"), Some("over_allowance")), "{over}");
    assert!(over["message"].as_str().unwrap().contains("nothing was written"), "{over}");
    let threads = all(&asker, "docs.threads", json!({"id": id})).await;
    assert_eq!(threads.iter().filter(|t| t["type"] == "thread").count(), 1, "{threads:?}");
    assert_eq!(questions().await, (Some(1), Some(0)));
}

/// `docs.bytes` is the update log as stored: a squash folds changes into a
/// snapshot, shares its bytes among the authors by what each wrote, and
/// credits back what the log shrank by; the recount agrees.
#[tokio::test]
async fn a_squash_gives_back_what_the_log_shrank_by() {
    let port = start(None).await;
    let admin = support::connect(port).await;
    grant(&admin, &OTHER, "editor", "*").await;
    grant(&admin, &EDITOR, "editor", "*").await;
    let (a, b) = (connect_key(&OTHER, port).await, connect_key(&EDITOR, port).await);
    let (pa, pb) = (support::principal_of(&OTHER), support::principal_of(&EDITOR));
    let docs = |u: &Value| source(u, "docs.bytes")["used"].as_u64().unwrap();
    let made = one(&a, "docs.create", json!({"title": "kiln", "body": "a".repeat(200)})).await;
    let id = made["doc"]["id"].as_str().unwrap().to_string();
    let mut body = "a".repeat(200);
    for i in 0..12 {
        body = format!("{body} {i}{}", "b".repeat(30));
        let w = if i % 2 == 0 { &a } else { &b };
        assert_eq!(one(w, "docs.save", json!({"id": id, "body": body})).await["type"], "saved");
    }
    let before = (docs(&one(&a, "quota.usage", json!({})).await), docs(&one(&b, "quota.usage", json!({})).await));
    let history = all(&admin, "docs.history", json!({"id": id, "limit": 100})).await;
    let last = history.iter().filter_map(|h| h["change"]["seq"].as_u64()).max().unwrap();
    let squashed = one(&admin, "docs.squash", json!({"id": id, "until": last})).await;
    assert_eq!(squashed["type"], "squashed", "{squashed}");
    let after = (docs(&one(&a, "quota.usage", json!({})).await), docs(&one(&b, "quota.usage", json!({})).await));
    assert!(after.0 < before.0 && after.1 < before.1, "the log shrank, and each author got bytes back: {before:?} → {after:?}");
    let counted = all(&admin, "docs.usage", json!({"everyone": true})).await;
    let of = |p: &str| counted.iter().find(|c| c["principal"] == p).map(|c| (c["count"].as_u64().unwrap(), c["bytes"].as_u64().unwrap()));
    assert_eq!(of(&pa), Some((1, after.0)), "the creator keeps its count through the squash: {counted:?}");
    assert_eq!(of(&pb), Some((0, after.1)), "{counted:?}");
    let r = all(&admin, "quota.recount", json!({})).await;
    assert!(!r.iter().any(|e| e["type"] == "drift" && e["source"].as_str().unwrap().starts_with("docs.")), "{r:?}");
}

/// The connections a principal holds (V2-31's follow-up): a person's limit,
/// an agent key's (`key:`) its own, an admin of `*` none — made by menger's
/// call layer at a connection's first call, the one past it answered
/// `too_many_connections` and closed.
#[tokio::test]
async fn an_agents_key_has_its_own_connection_limit_and_an_admin_none() {
    let limits = menger::calls::CallLimits { per_principal: 1, per_agent: 3, ..Default::default() };
    let port = start_with(None, limits).await;
    let admin = support::connect(port).await;
    grant(&admin, &EDITOR, "editor", "*").await;
    let mut agents = Vec::new();
    for i in 0..3 {
        let c = connect_key(&EDITOR, port).await;
        assert_eq!(one(&c, "tickets.health", json!({})).await["type"], "health", "agent connection {i}");
        agents.push(c);
    }
    let fourth = connect_key(&EDITOR, port).await;
    let refused = one(&fourth, "tickets.health", json!({})).await;
    assert_eq!((refused["error_kind"].as_str(), refused["max"].as_u64()), (Some("too_many_connections"), Some(3)), "a key's own limit, not a person's 1: {refused}");
    // Its seat came back when one closed.
    drop(agents.pop());
    tokio::time::sleep(Duration::from_millis(300)).await;
    let again = connect_key(&EDITOR, port).await;
    assert_eq!(one(&again, "tickets.health", json!({})).await["type"], "health", "a closed connection gives its seat back");
    // An admin of * holds as many as it opens.
    let mut admins = Vec::new();
    for _ in 0..6 {
        let c = support::connect(port).await;
        assert_eq!(one(&c, "tickets.health", json!({})).await["type"], "health");
        admins.push(c);
    }
}
