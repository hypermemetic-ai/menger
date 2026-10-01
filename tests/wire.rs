//! Wire-level tests: a real TransportServer, a real WebSocket client,
//! the same envelope web/index.html speaks — `menger.call` subscriptions
//! carrying `{method, params}`, events arriving as PlexusStreamItems.

use jsonrpsee::core::client::{Subscription, SubscriptionClientT};
use jsonrpsee::core::params::ObjectParams;
use jsonrpsee::ws_client::{WsClient, WsClientBuilder};
use menger::build_rpc;
use plexus_core::plexus::DynamicHub;
use plexus_transport::TransportServer;
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::Duration;

/// MNG-10's oracle: AGS-20's nav `plan()`, transcribed — the same file the
/// store's tests hold the roll-ups to.
#[path = "../tickets-api/src/nav_plan.rs"]
mod nav_plan;

async fn free_port() -> u16 {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    listener.local_addr().unwrap().port()
}

/// Starts a server whose database lives at `db_path`. The caller owns the
/// location's lifetime; `keep` is dropped (cleaning up) when the test's
/// runtime tears the server task down.
async fn start_server_at(db_path: std::path::PathBuf, keep: Option<tempfile::TempDir>) -> u16 {
    let port = free_port().await;
    let hub = build_rpc(&db_path).await.expect("open database");
    let conv = |arc: Arc<DynamicHub>| {
        DynamicHub::arc_into_rpc_module(arc).map_err(|e| anyhow::anyhow!("{e}"))
    };
    tokio::spawn(async move {
        let _keep = keep;
        TransportServer::builder(hub, conv)
            .with_websocket(port)
            .build()
            .await
            .expect("build server")
            .serve()
            .await
            .expect("serve");
    });
    port
}

/// Starts a server on an isolated, throwaway database location — servers
/// in this binary run concurrently and must never share storage.
async fn start_server() -> u16 {
    let dir = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).expect("tempdir");
    let db_path = dir.path().join("tickets.db");
    start_server_at(db_path, Some(dir)).await
}

async fn connect(port: u16) -> WsClient {
    let url = format!("ws://127.0.0.1:{port}");
    for _ in 0..50 {
        if let Ok(c) = WsClientBuilder::default().build(&url).await {
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
    client
        .subscribe("menger.call", p, "menger.call_unsub")
        .await
        .expect("subscribe")
}

/// Unwraps a PlexusStreamItem, returning the activation event inside `data`.
fn content(item: Value) -> Value {
    assert_eq!(item["type"], "data", "expected data item, got {item}");
    item["content"].clone()
}

/// MNG-9: a finite `list` / `ready_set` call, split the way the contract
/// reads it — the row events, then the one trailing `page` item.
async fn paged(sub: Subscription<Value>) -> (Vec<Value>, Value) {
    let mut evs = collect(sub).await;
    let page = evs.pop().expect("a list always ends with the page item");
    assert_eq!(page["type"], "page", "the last item is the page: {page}");
    assert!(
        !evs.iter().any(|e| e["type"] == "page"),
        "exactly one page item: {evs:?}"
    );
    (evs, page)
}

/// The rows of a finite list call, page item asserted and dropped.
async fn rows(sub: Subscription<Value>) -> Vec<Value> {
    paged(sub).await.0
}

/// Drains a finite call: collects every `data` item's content until `done`.
async fn collect(mut sub: Subscription<Value>) -> Vec<Value> {
    let mut out = Vec::new();
    while let Some(item) = sub.next().await {
        let item = item.unwrap();
        match item["type"].as_str() {
            Some("done") => return out,
            Some("data") => out.push(item["content"].clone()),
            other => panic!("unexpected stream item type {other:?}: {item}"),
        }
    }
    panic!("stream ended without a done item");
}

#[tokio::test]
async fn health_create_list_watch_over_the_wire() {
    let port = start_server().await;
    let client = connect(port).await;

    // health
    let mut sub = call(&client, "tickets.health", json!({})).await;
    let ev = content(sub.next().await.unwrap().unwrap());
    assert_eq!(ev["type"], "health");
    assert_eq!(ev["ticket_count"], 0);

    // watch, subscribed before any mutation
    let mut watch = call(&client, "tickets.watch", json!({})).await;

    // create
    let mut sub = call(
        &client,
        "tickets.create",
        json!({"epic": "WIRE", "title": "first over the wire", "kind": "scope"}),
    )
    .await;
    let ev = content(sub.next().await.unwrap().unwrap());
    assert_eq!(ev["type"], "created");
    assert_eq!(ev["ticket"]["id"], "WIRE-1");
    assert_eq!(ev["ticket"]["status"], "Pending");

    // the watch subscriber saw it pushed, unprompted
    let ev = content(watch.next().await.unwrap().unwrap());
    assert_eq!(ev["type"], "created");
    assert_eq!(ev["ticket"]["id"], "WIRE-1");
    // …and, after it, the new shortcode's row (MNG-10)
    let ev = content(watch.next().await.unwrap().unwrap());
    assert_eq!(ev["type"], "group_changed");
    assert_eq!(ev["group"], json!({"epic": "WIRE", "total": 1, "done": 0, "root": "WIRE-1", "host": null}));

    // set_status → watch sees the transition too (Pending → Ready moves no
    // count, so no group follows it)
    let mut sub = call(
        &client,
        "tickets.set_status",
        json!({"id": "WIRE-1", "status": "Ready"}),
    )
    .await;
    let ev = content(sub.next().await.unwrap().unwrap());
    assert_eq!(ev["type"], "status_changed");
    assert_eq!(ev["previous"], "Pending");

    let ev = content(watch.next().await.unwrap().unwrap());
    assert_eq!(ev["type"], "status_changed");
    assert_eq!(ev["ticket"]["status"], "Ready");

    // list replays current state
    let mut sub = call(&client, "tickets.list", json!({})).await;
    let ev = content(sub.next().await.unwrap().unwrap());
    assert_eq!(ev["type"], "ticket");
    assert_eq!(ev["ticket"]["id"], "WIRE-1");
}

#[tokio::test]
async fn in_band_errors_cross_the_wire() {
    let port = start_server().await;
    let client = connect(port).await;

    let mut sub = call(
        &client,
        "tickets.set_status",
        json!({"id": "X-1", "status": "wibble"}),
    )
    .await;
    let ev = content(sub.next().await.unwrap().unwrap());
    assert_eq!(ev["type"], "error");
    assert!(ev["message"].as_str().unwrap().contains("wibble"));
}

/// A watch-only client's local mirror: every mutation event carries the
/// full affected ticket, so applying it is one upsert. A `group_changed`
/// (MNG-10) carries a roll-up, not a row — this mirror keeps rows.
fn apply(mirror: &mut std::collections::HashMap<String, Value>, ev: &Value) {
    match ev["type"].as_str() {
        Some("created")
        | Some("status_changed")
        | Some("unlocks_changed")
        | Some("children_changed")
        | Some("parent_changed")
        | Some("child_removed") => {
            let ticket = ev["ticket"].clone();
            let id = ticket["id"].as_str().expect("ticket id").to_string();
            mirror.insert(id, ticket);
        }
        Some("group_changed") => {}
        other => panic!("unexpected watch event {other:?}: {ev}"),
    }
}

/// The client-side ready-set computation: Ready, and every blocked_by id
/// resolves in the mirror to a Complete or Superseded ticket.
fn local_ready(mirror: &std::collections::HashMap<String, Value>) -> Vec<String> {
    let mut ids: Vec<String> = mirror
        .values()
        .filter(|t| t["status"] == "Ready")
        .filter(|t| {
            t["blocked_by"].as_array().expect("blocked_by").iter().all(|b| {
                mirror
                    .get(b.as_str().expect("blocker id"))
                    .is_some_and(|bt| bt["status"] == "Complete" || bt["status"] == "Superseded")
            })
        })
        .map(|t| t["id"].as_str().expect("id").to_string())
        .collect();
    ids.sort();
    ids
}

fn ready_ids(events: Vec<Value>) -> Vec<String> {
    let mut ids: Vec<String> = events
        .into_iter()
        .map(|ev| {
            assert_eq!(ev["type"], "ticket", "got {ev}");
            ev["ticket"]["id"].as_str().expect("id").to_string()
        })
        .collect();
    ids.sort();
    ids
}

/// Acceptance 1 + 2: the ready set gates on blockers, and a client that
/// listed once and then only watched computes the same ready sets as the
/// server, without re-calling list.
#[tokio::test]
async fn ready_set_over_the_wire_is_watch_derivable() {
    let port = start_server().await;
    let client = connect(port).await;
    let mut mirror = std::collections::HashMap::new();

    // A exists before the client arrives.
    let evs = collect(call(&client, "tickets.create", json!({"epic": "RS", "title": "A", "kind": "scope"})).await).await;
    assert_eq!(evs[0]["type"], "created");

    // The client fetches the full set once...
    for ev in rows(call(&client, "tickets.list", json!({})).await).await {
        assert_eq!(ev["type"], "ticket");
        let ticket = ev["ticket"].clone();
        mirror.insert(ticket["id"].as_str().unwrap().to_string(), ticket);
    }
    assert_eq!(mirror.len(), 1);

    // ...then consumes only watch events.
    let mut watch = call(&client, "tickets.watch", json!({})).await;

    // create B blocked_by [A]; promote both to Ready.
    collect(call(&client, "tickets.create",
        json!({"epic": "RS", "title": "B", "blocked_by": ["RS-1"]})).await).await;
    collect(call(&client, "tickets.set_status", json!({"id": "RS-1", "status": "Ready"})).await).await;
    collect(call(&client, "tickets.set_status", json!({"id": "RS-2", "status": "Ready"})).await).await;

    // Watch delivered: created(RS-2), children_changed(RS-1) — RS-2 named
    // no parent and hangs under the root (MNG-14) — unlocks_changed(RS-1),
    // group_changed(RS, now 2 tickets; MNG-10), 2x status_changed.
    for _ in 0..6 {
        let ev = content(watch.next().await.unwrap().unwrap());
        apply(&mut mirror, &ev);
    }
    assert_eq!(mirror["RS-1"]["unlocks"], json!(["RS-2"]));

    // Checkpoint 1: A is ready, B is not — server and mirror agree.
    let server = ready_ids(rows(call(&client, "tickets.ready_set", json!({})).await).await);
    assert_eq!(server, vec!["RS-1"]);
    assert_eq!(server, local_ready(&mirror));

    // A completes; B unblocks.
    collect(call(&client, "tickets.set_status", json!({"id": "RS-1", "status": "Complete"})).await).await;
    let ev = content(watch.next().await.unwrap().unwrap());
    apply(&mut mirror, &ev);

    let server = ready_ids(rows(call(&client, "tickets.ready_set", json!({})).await).await);
    assert_eq!(server, vec!["RS-2"]);
    assert_eq!(server, local_ready(&mirror));
}

/// Acceptance 3: a dangling blocker keeps a Ready ticket out of the ready
/// set, and the blockers query marks the id as dangling — a distinct
/// event category, not prose.
#[tokio::test]
async fn dangling_blocker_over_the_wire() {
    let port = start_server().await;
    let client = connect(port).await;

    collect(call(&client, "tickets.create",
        json!({"epic": "DG", "title": "C", "kind": "scope", "blocked_by": ["DG-99"]})).await).await;
    collect(call(&client, "tickets.set_status", json!({"id": "DG-1", "status": "Ready"})).await).await;

    let ready = rows(call(&client, "tickets.ready_set", json!({})).await).await;
    assert!(ready.is_empty(), "DG-1 must not be ready: {ready:?}");

    let evs = collect(call(&client, "tickets.blockers", json!({"id": "DG-1"})).await).await;
    assert_eq!(evs.len(), 1);
    assert_eq!(evs[0]["type"], "dangling_blocker");
    assert_eq!(evs[0]["id"], "DG-99");
}

/// Acceptance 4: a dangling forward reference resolves at the referenced
/// ticket's birth — the newborn carries its referrer in `unlocks`.
#[tokio::test]
async fn forward_reference_resolves_at_birth_over_the_wire() {
    let port = start_server().await;
    let client = connect(port).await;

    collect(call(&client, "tickets.create",
        json!({"epic": "FW", "title": "D-1", "kind": "scope", "blocked_by": ["FW-2"]})).await).await;

    let evs = collect(call(&client, "tickets.create",
        json!({"epic": "FW", "title": "becomes D-2"})).await).await;
    assert_eq!(evs[0]["type"], "created");
    assert_eq!(evs[0]["ticket"]["id"], "FW-2");
    assert_eq!(evs[0]["ticket"]["unlocks"], json!(["FW-1"]));

    let evs = collect(call(&client, "tickets.get", json!({"id": "FW-2"})).await).await;
    assert_eq!(evs[0]["type"], "ticket");
    assert_eq!(evs[0]["ticket"]["unlocks"], json!(["FW-1"]));
}

/// Acceptance 5: the create whose allocated id would close a blocked_by
/// cycle errors in-band and stores nothing.
#[tokio::test]
async fn cycle_create_errors_in_band_and_stores_nothing() {
    let port = start_server().await;
    let client = connect(port).await;

    // CY-1 blocked_by the not-yet-existing CY-2.
    collect(call(&client, "tickets.create",
        json!({"epic": "CY", "title": "one", "kind": "scope", "blocked_by": ["CY-2"]})).await).await;

    // The create that would become CY-2, blocked_by CY-1, closes the loop.
    let evs = collect(call(&client, "tickets.create",
        json!({"epic": "CY", "title": "loop", "blocked_by": ["CY-1"]})).await).await;
    assert_eq!(evs.len(), 1);
    assert_eq!(evs[0]["type"], "error");
    assert!(evs[0]["message"].as_str().unwrap().contains("cycle"), "{}", evs[0]);

    // Nothing was stored under the would-be id.
    let evs = collect(call(&client, "tickets.get", json!({"id": "CY-2"})).await).await;
    assert_eq!(evs[0]["type"], "not_found");
}

/// AGS-19 acceptance 1, 2 and 4 over the real wire: a child names its
/// parent at create, `get` shows both halves, the watch-only mirror sees
/// the parent's update as `children_changed` carrying the full ticket, a
/// nonexistent parent is an in-band error naming the reason, a 3-deep
/// chain (crossing epics) reads back, and `list {parent}` returns exactly
/// that ticket's children.
#[tokio::test]
async fn parent_children_and_the_parent_filter_over_the_wire() {
    let port = start_server().await;
    let client = connect(port).await;
    let mut mirror = std::collections::HashMap::new();

    let evs = collect(call(&client, "tickets.create", json!({"epic": "PA", "title": "P", "kind": "scope"})).await).await;
    assert_eq!(evs[0]["type"], "created");
    assert_eq!(evs[0]["ticket"]["children"], json!([]), "born childless");
    assert!(evs[0]["ticket"]["parent"].is_null(), "and parentless");
    mirror.insert("PA-1".to_string(), evs[0]["ticket"].clone());

    // A watch-only client from here on.
    let mut watch = call(&client, "tickets.watch", json!({})).await;

    // Acceptance 2, first half: a parent that names no ticket.
    let evs = collect(call(&client, "tickets.create",
        json!({"epic": "PA", "title": "orphan", "parent": "PA-99"})).await).await;
    assert_eq!(evs.len(), 1);
    assert_eq!(evs[0]["type"], "error");
    let message = evs[0]["message"].as_str().unwrap();
    assert!(message.contains("PA-99"), "names the parent: {message}");
    assert!(message.contains("must exist"), "names the reason: {message}");
    let evs = collect(call(&client, "tickets.get", json!({"id": "PA-2"})).await).await;
    assert_eq!(evs[0]["type"], "not_found", "nothing was stored");

    // Acceptance 1: C is born as P's child.
    let evs = collect(call(&client, "tickets.create",
        json!({"epic": "PA", "title": "C", "kind": "execution", "parent": "PA-1"})).await).await;
    assert_eq!(evs[0]["type"], "created");
    assert_eq!(evs[0]["ticket"]["id"], "PA-2");
    assert_eq!(evs[0]["ticket"]["parent"], "PA-1");

    // The watcher saw the birth and the parent's update, full ticket.
    for _ in 0..2 {
        let ev = content(watch.next().await.unwrap().unwrap());
        if ev["type"] == "children_changed" {
            assert_eq!(ev["ticket"]["id"], "PA-1");
            assert_eq!(ev["added"], "PA-2");
            assert_eq!(ev["ticket"]["children"], json!(["PA-2"]));
            assert_eq!(ev["ticket"]["title"], "P", "the full parent, not a delta");
        }
        apply(&mut mirror, &ev);
    }
    assert_eq!(mirror["PA-1"]["children"], json!(["PA-2"]),
        "a watch-only client's mirror stays coherent");

    // Both ends agree when asked directly.
    let evs = collect(call(&client, "tickets.get", json!({"id": "PA-1"})).await).await;
    assert_eq!(evs[0]["ticket"]["children"], json!(["PA-2"]));
    let evs = collect(call(&client, "tickets.get", json!({"id": "PA-2"})).await).await;
    assert_eq!(evs[0]["ticket"]["parent"], "PA-1");

    // Acceptance 2, second half: three deep, the middle link in another
    // epic — PB's scope root hangs under PA-2 (a sub-shortcode); PB's own
    // tickets stay inside PB (MNG-14). (Self-ancestry at create needs no
    // test: the id is allocated by the very create that would name it.)
    collect(call(&client, "tickets.create",
        json!({"epic": "PB", "title": "grandchild", "kind": "scope", "parent": "PA-2"})).await).await;
    let evs = collect(call(&client, "tickets.create",
        json!({"epic": "PA", "title": "escapes", "parent": "PB-1"})).await).await;
    assert_eq!(evs[0]["type"], "error", "a non-root's parent must be in its shortcode");
    assert!(evs[0]["message"].as_str().unwrap().contains("outside"), "{}", evs[0]);
    collect(call(&client, "tickets.create",
        json!({"epic": "PB", "title": "great-grandchild", "parent": "PB-1"})).await).await;
    let chain = ["PA-1", "PA-2", "PB-1", "PB-2"];
    for (child, parent) in chain.iter().skip(1).zip(chain.iter()) {
        let evs = collect(call(&client, "tickets.get", json!({"id": child})).await).await;
        assert_eq!(evs[0]["ticket"]["parent"], *parent, "{child}");
        let evs = collect(call(&client, "tickets.get", json!({"id": parent})).await).await;
        assert_eq!(evs[0]["ticket"]["children"], json!([child]), "{parent}");
    }
    let evs = collect(call(&client, "tickets.get", json!({"id": "PB-2"})).await).await;
    assert_eq!(evs[0]["ticket"]["children"], json!([]), "the leaf has none");

    // Acceptance 4: exactly the children, and nothing else. PA-3 names no
    // parent and so hangs under PA's root (MNG-14).
    let evs = collect(call(&client, "tickets.create",
        json!({"epic": "PA", "title": "second child of P"})).await).await; // PA-3
    assert_eq!(evs[0]["ticket"]["parent"], "PA-1", "defaulted to the shortcode root");
    collect(call(&client, "tickets.create",
        json!({"epic": "PC", "title": "cross-epic child of P", "kind": "scope", "parent": "PA-1"})).await).await;
    let ids = |evs: Vec<Value>| -> Vec<String> {
        evs.into_iter()
            .map(|ev| {
                assert_eq!(ev["type"], "ticket", "got {ev}");
                ev["ticket"]["id"].as_str().unwrap().to_string()
            })
            .collect()
    };
    assert_eq!(
        ids(rows(call(&client, "tickets.list", json!({"parent": "PA-1"})).await).await),
        vec!["PA-2", "PA-3", "PC-1"]
    );
    assert_eq!(
        ids(rows(call(&client, "tickets.list", json!({"parent": "PA-3"})).await).await),
        Vec::<String>::new()
    );
    assert_eq!(
        ids(rows(call(&client, "tickets.list", json!({})).await).await).len(),
        6,
        "the unfiltered list is unchanged"
    );
}

/// MNG-9 acceptance 1, 3, 4 and 6 over the real wire: a `summary` +
/// `limit` call yields summary rows (no body) and closes with
/// `page { next, remaining }`; walking `after = next` visits every id
/// exactly once in (epic, seq) order; `list {}` is what it always was
/// plus `page { next: null, remaining: 0 }`; an `after` naming no ticket
/// is one error and done; `ready_set` takes the same three params.
#[tokio::test]
async fn list_pages_over_the_wire() {
    let port = start_server().await;
    let client = connect(port).await;

    // Two epics, ten tickets: PGA-1..7 then PGZ-1..3 in the store's order.
    for (epic, n) in [("PGA", 7), ("PGZ", 3)] {
        for i in 1..=n {
            // MNG-12: a prefix's first ticket is its scope root.
            let kind = if i == 1 { "scope" } else { "build" };
            collect(call(&client, "tickets.create",
                json!({"epic": epic, "title": format!("{epic} {i}"), "kind": kind, "body": "## Problem\n\nbodies are the weight"})).await).await;
        }
    }

    // Acceptance 1 + 6: the page's shape.
    let (evs, page) = paged(call(&client, "tickets.list", json!({"summary": true, "limit": 4})).await).await;
    assert_eq!(evs.len(), 4, "limit is honoured");
    assert_eq!(evs[0]["type"], "summary");
    let row = &evs[0]["summary"];
    assert_eq!(row["id"], "PGA-1");
    assert_eq!(row["epic"], "PGA");
    assert_eq!(row["seq"], 1);
    assert_eq!(row["status"], "Pending");
    assert_eq!(row["kind"], "scope");
    assert!(row["severity"].is_null());
    assert!(row["parent"].is_null(), "the root is top-level");
    // MNG-14: the six tickets that named no parent hang under the root.
    assert_eq!(row["children"], json!(["PGA-2", "PGA-3", "PGA-4", "PGA-5", "PGA-6", "PGA-7"]));
    assert!(row.get("body").is_none(), "a summary carries no body: {row}");
    assert_eq!(page, json!({"type": "page", "next": "PGA-4", "remaining": 6}));

    // Acceptance 2's shape: walk to the end, every id once, in order.
    let mut walked: Vec<String> = Vec::new();
    let mut after = json!(null);
    loop {
        let params = json!({"summary": true, "limit": 4, "after": after});
        let (evs, page) = paged(call(&client, "tickets.list", params).await).await;
        walked.extend(evs.iter().map(|e| e["summary"]["id"].as_str().unwrap().to_string()));
        assert_eq!(page["remaining"], (10 - walked.len()) as u64);
        if page["next"].is_null() {
            break;
        }
        assert_eq!(page["next"], *walked.last().unwrap());
        after = page["next"].clone();
    }
    assert_eq!(
        walked,
        ["PGA-1", "PGA-2", "PGA-3", "PGA-4", "PGA-5", "PGA-6", "PGA-7", "PGZ-1", "PGZ-2", "PGZ-3"]
    );

    // Acceptance 3: `list {}` unchanged apart from the trailing page.
    let (evs, page) = paged(call(&client, "tickets.list", json!({})).await).await;
    assert_eq!(evs.len(), 10);
    assert_eq!(evs[0]["type"], "ticket");
    assert!(evs[0]["ticket"]["body"].as_str().unwrap().contains("bodies are the weight"));
    assert_eq!(page, json!({"type": "page", "next": null, "remaining": 0}));

    // Acceptance 4: a cursor naming no ticket.
    let evs = collect(call(&client, "tickets.list", json!({"after": "ZZZ-9"})).await).await;
    assert_eq!(evs.len(), 1, "one error and done, nothing else: {evs:?}");
    assert_eq!(evs[0], json!({"type": "error", "message": "after: no ticket ZZZ-9"}));

    // Acceptance 6: ready_set takes the same params (and pages its own set).
    for id in ["PGA-1", "PGA-2", "PGZ-1"] {
        collect(call(&client, "tickets.set_status", json!({"id": id, "status": "Ready"})).await).await;
    }
    let (evs, page) =
        paged(call(&client, "tickets.ready_set", json!({"summary": true, "limit": 2})).await).await;
    assert_eq!(
        evs.iter().map(|e| e["summary"]["id"].as_str().unwrap()).collect::<Vec<_>>(),
        ["PGA-1", "PGA-2"]
    );
    assert_eq!(page, json!({"type": "page", "next": "PGA-2", "remaining": 1}));
    let (evs, page) =
        paged(call(&client, "tickets.ready_set", json!({"after": "PGA-2"})).await).await;
    assert_eq!(evs.len(), 1);
    assert_eq!(evs[0]["ticket"]["id"], "PGZ-1", "no summary asked for, so full tickets");
    assert_eq!(page, json!({"type": "page", "next": null, "remaining": 0}));
}

/// MNG-9 acceptance 5: the cursor is a position in the store's live
/// order, not a snapshot — a ticket born above a stalled walk's cursor is
/// visited by that walk; one born below it never is, which is exactly
/// what the `watch` opened first is for.
#[tokio::test]
async fn a_walk_sees_births_above_its_cursor_and_watch_carries_the_rest() {
    let port = start_server().await;
    let client = connect(port).await;

    for i in 1..=4 {
        let kind = if i == 1 { "scope" } else { "build" }; // MNG-12
        collect(call(&client, "tickets.create", json!({"epic": "MID", "title": format!("m{i}"), "kind": kind})).await).await;
    }

    // The app's order: watch first, then walk.
    let mut watch = call(&client, "tickets.watch", json!({})).await;
    let (evs, page) = paged(call(&client, "tickets.list", json!({"summary": true, "limit": 2})).await).await;
    assert_eq!(evs.len(), 2);
    assert_eq!(page["next"], "MID-2");

    // Mid-walk: one above the cursor (MID-5), one below it (AAA-1).
    collect(call(&client, "tickets.create", json!({"epic": "MID", "title": "born above"})).await).await;
    collect(call(&client, "tickets.create", json!({"epic": "AAA", "title": "born below", "kind": "scope"})).await).await;

    let (evs, page) = paged(call(&client, "tickets.list",
        json!({"summary": true, "after": page["next"]})).await).await;
    let ids: Vec<&str> = evs.iter().map(|e| e["summary"]["id"].as_str().unwrap()).collect();
    assert_eq!(ids, ["MID-3", "MID-4", "MID-5"], "MID-5 joined the walk");
    assert_eq!(page, json!({"type": "page", "next": null, "remaining": 0}));

    // AAA-1 sorts below the cursor, so only watch carries it. (MID-5's
    // birth also updates its root's children — MNG-14 — so the watch
    // interleaves a children_changed; the births are what this pins.)
    let mut pushed = Vec::new();
    while pushed.len() < 2 {
        let ev = content(watch.next().await.unwrap().unwrap());
        if ev["type"] == "created" {
            pushed.push(ev["ticket"]["id"].as_str().unwrap().to_string());
        }
    }
    assert_eq!(pushed, ["MID-5", "AAA-1"]);
}

/// MNG-10 acceptance 1 and 2 over the real wire. `groups {}` answers one
/// `group` item per shortcode row and then the stream's own `done` — no
/// page, nothing else — with exactly the numbers AGS-20's nav `plan()`
/// computes from every row the app's walk would fetch: here a shortcode
/// hosting a sub-shortcode that hosts another, and one standing alone.
/// Then on `watch`: a completion in a top-level shortcode is its
/// `status_changed` followed by exactly one `group_changed`, `done` + 1; a
/// completion three rows deep moves all three, innermost first; and
/// `watch { epic }` hears the group of its own shortcode only.
#[tokio::test]
async fn groups_are_the_navs_rollups_and_a_completion_pushes_one_group_changed() {
    let port = start_server().await;
    let client = connect(port).await;

    for params in [
        json!({"epic": "HUB", "title": "the host", "kind": "scope"}), // HUB-1
        json!({"epic": "HUB", "title": "core"}),                      // HUB-2 → HUB-1
        json!({"epic": "HUB", "title": "under core", "parent": "HUB-2"}), // HUB-3
        json!({"epic": "SUB", "title": "hosted", "kind": "scope", "parent": "HUB-2"}), // SUB-1
        json!({"epic": "SUB", "title": "a"}),                         // SUB-2 → SUB-1
        json!({"epic": "SUB", "title": "b", "parent": "SUB-2"}),      // SUB-3
        json!({"epic": "DEEP", "title": "deeper", "kind": "scope", "parent": "SUB-3"}), // DEEP-1
        json!({"epic": "DEEP", "title": "c"}),                        // DEEP-2 → DEEP-1
        json!({"epic": "LONE", "title": "alone", "kind": "scope"}),   // LONE-1
        json!({"epic": "LONE", "title": "d"}),                        // LONE-2 → LONE-1
    ] {
        let evs = collect(call(&client, "tickets.create", params).await).await;
        assert_eq!(evs[0]["type"], "created", "{evs:?}");
    }
    for id in ["HUB-3", "SUB-2", "DEEP-2"] {
        collect(call(&client, "tickets.set_status", json!({"id": id, "status": "Complete"})).await).await;
    }

    // `collect` reads to the stream's `done` and refuses any other item
    // type, so this is the whole answer: group items, then done.
    let items = collect(call(&client, "tickets.groups", json!({})).await).await;
    assert!(items.iter().all(|ev| ev["type"] == "group"), "only groups: {items:?}");
    let groups: Vec<Value> = items.into_iter().map(|ev| ev["group"].clone()).collect();
    assert_eq!(
        groups,
        vec![
            json!({"epic": "DEEP", "total": 2, "done": 1, "root": "DEEP-1", "host": "SUB-3"}),
            json!({"epic": "HUB", "total": 8, "done": 3, "root": "HUB-1", "host": null}),
            json!({"epic": "LONE", "total": 2, "done": 0, "root": "LONE-1", "host": null}),
            json!({"epic": "SUB", "total": 5, "done": 2, "root": "SUB-1", "host": "HUB-2"}),
        ]
    );

    // Acceptance 1's comparison: the nav's plan() over every row, as the
    // app's summary walk would have fetched them.
    let all = rows(call(&client, "tickets.list", json!({"summary": true})).await).await;
    let facts: Vec<nav_plan::Facts> = all
        .iter()
        .map(|ev| {
            let s = &ev["summary"];
            nav_plan::Facts {
                id: s["id"].as_str().unwrap().into(),
                epic: s["epic"].as_str().unwrap().into(),
                seq: s["seq"].as_u64().unwrap() as u32,
                kind: s["kind"].as_str().unwrap().into(),
                parent: s["parent"].as_str().map(Into::into),
            }
        })
        .collect();
    let complete: std::collections::BTreeSet<String> = all
        .iter()
        .filter(|ev| ev["summary"]["status"] == "Complete")
        .map(|ev| ev["summary"]["id"].as_str().unwrap().to_string())
        .collect();
    let nav: Vec<Value> = nav_plan::rows(&facts, &complete)
        .into_iter()
        .map(|r| json!({"epic": r.epic, "total": r.total, "done": r.done, "root": r.root, "host": r.host}))
        .collect();
    assert_eq!(groups, nav, "the server's roll-ups are the nav's");

    // Acceptance 2: the status change, then exactly one group_changed.
    let mut watch = call(&client, "tickets.watch", json!({})).await;
    let mut hub_only = call(&client, "tickets.watch", json!({"epic": "HUB"})).await;
    collect(call(&client, "tickets.set_status", json!({"id": "LONE-2", "status": "Complete"})).await).await;
    let ev = content(watch.next().await.unwrap().unwrap());
    assert_eq!(ev["type"], "status_changed");
    let ev = content(watch.next().await.unwrap().unwrap());
    assert_eq!(ev["type"], "group_changed");
    assert_eq!(ev["group"], json!({"epic": "LONE", "total": 2, "done": 1, "root": "LONE-1", "host": null}));
    // Nothing else came of it: the next item is the next write's.
    collect(call(&client, "tickets.set_status", json!({"id": "HUB-1", "status": "Ready"})).await).await;
    let ev = content(watch.next().await.unwrap().unwrap());
    assert_eq!((ev["type"].as_str(), ev["ticket"]["id"].as_str()), (Some("status_changed"), Some("HUB-1")));

    // Three rows deep: DEEP, then SUB, then HUB.
    collect(call(&client, "tickets.set_status", json!({"id": "DEEP-1", "status": "Complete"})).await).await;
    let ev = content(watch.next().await.unwrap().unwrap());
    assert_eq!(ev["type"], "status_changed");
    let mut moved = Vec::new();
    for _ in 0..3 {
        let ev = content(watch.next().await.unwrap().unwrap());
        assert_eq!(ev["type"], "group_changed");
        moved.push((ev["group"]["epic"].clone(), ev["group"]["done"].clone()));
    }
    assert_eq!(moved, [(json!("DEEP"), json!(2)), (json!("SUB"), json!(3)), (json!("HUB"), json!(4))]);

    // `watch { epic: HUB }`: neither LONE's group nor DEEP's ticket — the
    // HUB-1 flip, then HUB's own row.
    let ev = content(hub_only.next().await.unwrap().unwrap());
    assert_eq!((ev["type"].as_str(), ev["ticket"]["id"].as_str()), (Some("status_changed"), Some("HUB-1")));
    let ev = content(hub_only.next().await.unwrap().unwrap());
    assert_eq!(ev["type"], "group_changed");
    assert_eq!(ev["group"], json!({"epic": "HUB", "total": 8, "done": 4, "root": "HUB-1", "host": null}));
}

/// SCALE-16 over the real wire. `extents {}` answers one `extent` item per
/// shortcode that holds a ticket — flat `{ epic, last_seq }` — in
/// shortcode order, and then the stream's own `done`: no page, nothing
/// else. The `last_seq`s add up to `health`'s count; a refused create
/// moves nothing; and they are not the roll-ups — HUB's group counts the
/// sub-shortcode nested under it, its extent only its own tickets.
/// Acceptance 2: for every shortcode E and row k, `list { epic: E, after:
/// "E-k", limit: 1 }` yields `E-(k+1)` — a position becomes a cursor.
#[tokio::test]
async fn extents_are_each_shortcodes_last_seq_and_a_row_is_a_cursor() {
    let port = start_server().await;
    let client = connect(port).await;

    for params in [
        json!({"epic": "HUB", "title": "the host", "kind": "scope"}),
        json!({"epic": "HUB", "title": "a"}),
        json!({"epic": "SUB", "title": "hosted", "kind": "scope", "parent": "HUB-2"}),
        json!({"epic": "SUB", "title": "b"}),
        json!({"epic": "SUB", "title": "c"}),
        json!({"epic": "HUB", "title": "d"}),
        json!({"epic": "AAA", "title": "first by name", "kind": "scope"}),
    ] {
        let evs = collect(call(&client, "tickets.create", params).await).await;
        assert_eq!(evs[0]["type"], "created", "{evs:?}");
    }
    // Refused (a build cannot open a shortcode): nothing allocated.
    let evs = collect(call(&client, "tickets.create", json!({"epic": "NEW", "title": "x"})).await).await;
    assert_eq!(evs[0]["type"], "error", "{evs:?}");

    let items = collect(call(&client, "tickets.extents", json!({})).await).await;
    assert_eq!(
        items,
        vec![
            json!({"type": "extent", "epic": "AAA", "last_seq": 1}),
            json!({"type": "extent", "epic": "HUB", "last_seq": 3}),
            json!({"type": "extent", "epic": "SUB", "last_seq": 3}),
        ],
        "one flat extent per shortcode, in order, and nothing after them"
    );
    let health = collect(call(&client, "tickets.health", json!({})).await).await;
    let sum: u64 = items.iter().map(|ev| ev["last_seq"].as_u64().unwrap()).sum();
    assert_eq!(json!(sum), health[0]["ticket_count"]);
    let groups = collect(call(&client, "tickets.groups", json!({})).await).await;
    let hub = groups.iter().find(|ev| ev["group"]["epic"] == "HUB").unwrap();
    assert_eq!(hub["group"]["total"], 6, "the roll-up nests SUB; the extent does not");

    for ev in &items {
        let (epic, last) = (ev["epic"].as_str().unwrap(), ev["last_seq"].as_u64().unwrap());
        for k in 1..=last {
            let (rows, page) = paged(
                call(&client, "tickets.list", json!({"epic": epic, "after": format!("{epic}-{k}"), "limit": 1, "summary": true}))
                    .await,
            )
            .await;
            let got: Vec<&str> = rows.iter().map(|r| r["summary"]["id"].as_str().unwrap()).collect();
            if k < last {
                assert_eq!(got, [format!("{epic}-{}", k + 1)], "{epic} after row {k}");
            } else {
                assert!(got.is_empty() && page["next"].is_null(), "{epic} ends at row {last}: {got:?} {page}");
            }
        }
    }

    // A birth is its shortcode's new last_seq — `created` already says so.
    let evs = collect(call(&client, "tickets.create", json!({"epic": "SUB", "title": "e"})).await).await;
    assert_eq!((evs[0]["ticket"]["epic"].as_str(), evs[0]["ticket"]["seq"].as_u64()), (Some("SUB"), Some(4)));
    let items = collect(call(&client, "tickets.extents", json!({})).await).await;
    assert_eq!(items[2], json!({"type": "extent", "epic": "SUB", "last_seq": 4}));
}

#[tokio::test]
async fn shell_serves_the_page() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let shell_port = free_port().await;
    tokio::spawn(async move {
        // Same call, MFE-2 signature: config struct instead of two ports.
        // No REST upstream and an empty app dist — this test only cares
        // that the vanilla page at `/` still serves, unchanged.
        let _ = menger::shell::serve(menger::shell::ShellConfig {
            port: shell_port,
            ws_port: 4455,
            rest_port: None,
            app_dist: std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR")),
        })
        .await;
    });

    let mut resp = Vec::new();
    for _ in 0..50 {
        match tokio::net::TcpStream::connect(("127.0.0.1", shell_port)).await {
            Ok(mut s) => {
                s.write_all(b"GET / HTTP/1.0\r\nHost: localhost\r\n\r\n")
                    .await
                    .unwrap();
                s.read_to_end(&mut resp).await.unwrap();
                break;
            }
            Err(_) => tokio::time::sleep(Duration::from_millis(50)).await,
        }
    }
    let text = String::from_utf8_lossy(&resp);
    assert!(text.starts_with("HTTP/1.0 200") || text.starts_with("HTTP/1.1 200"));
    assert!(text.contains("<main id=\"main\">"));
    assert!(text.contains("menger.call"), "shell speaks the envelope");
}

/// MNG-2 criterion 5: reads are cache-first, observed black-box — after
/// the database location vanishes mid-run, `get` and `list` still answer
/// with full data, and a write answers in-band (never a crash or hang).
#[tokio::test]
async fn reads_survive_db_location_unavailable() {
    let dir = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).expect("tempdir");
    let db_dir = dir.path().join("db");
    std::fs::create_dir_all(&db_dir).unwrap();
    let port = start_server_at(db_dir.join("tickets.db"), None).await;
    let client = connect(port).await;

    // Seed one ticket while the database is healthy.
    let mut sub = call(
        &client,
        "tickets.create",
        json!({
            "epic": "CACHE",
            "title": "survives the outage",
            "kind": "scope",
            "severity": "High",
            "body": "reads never touch storage"
        }),
    )
    .await;
    let ev = content(sub.next().await.unwrap().unwrap());
    assert_eq!(ev["type"], "created");
    let created = ev["ticket"].clone();

    // Make the database location unavailable.
    std::fs::rename(&db_dir, dir.path().join("gone")).unwrap();

    // Reads still answer, with the full ticket, from the cache.
    let mut sub = call(&client, "tickets.get", json!({"id": "CACHE-1"})).await;
    let ev = content(sub.next().await.unwrap().unwrap());
    assert_eq!(ev["type"], "ticket");
    assert_eq!(ev["ticket"], created, "get returns full data from the cache");

    let mut sub = call(&client, "tickets.list", json!({})).await;
    let ev = content(sub.next().await.unwrap().unwrap());
    assert_eq!(ev["type"], "ticket");
    assert_eq!(ev["ticket"], created, "list returns full data from the cache");

    // A write during the outage answers in-band: acknowledged only if the
    // engine still made it durable, otherwise an error event — no crash.
    let mut sub = call(
        &client,
        "tickets.create",
        json!({"epic": "CACHE", "title": "written during the outage"}),
    )
    .await;
    let ev = content(sub.next().await.unwrap().unwrap());
    assert!(
        ev["type"] == "created" || ev["type"] == "error",
        "write answered in-band, got {ev}"
    );

    // And reads keep working after the write attempt.
    let mut sub = call(&client, "tickets.get", json!({"id": "CACHE-1"})).await;
    let ev = content(sub.next().await.unwrap().unwrap());
    assert_eq!(ev["type"], "ticket");
    assert_eq!(ev["ticket"]["title"], "survives the outage");
}

/// MNG-2 criterion 7: under a mutation stream with an open `watch`, many
/// concurrent list/get/health calls all complete, and the watcher sees
/// every acknowledged change in order.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_reads_complete_under_mutation_stream() {
    const MUTATIONS: usize = 15;

    let port = start_server().await;
    let writer = connect(port).await;
    let watcher = connect(port).await;
    let mut watch = call(&watcher, "tickets.watch", json!({})).await;

    // Guarantee at least one ticket before the readers start.
    let mut sub = call(
        &writer,
        "tickets.create",
        json!({"epic": "LOAD", "title": "t1", "kind": "scope"}),
    )
    .await;
    assert_eq!(content(sub.next().await.unwrap().unwrap())["type"], "created");

    // Mutation stream: create + status flip, back to back.
    let mutations = tokio::spawn(async move {
        for i in 2..=MUTATIONS {
            let mut sub = call(
                &writer,
                "tickets.create",
                json!({"epic": "LOAD", "title": format!("t{i}")}),
            )
            .await;
            let ev = content(sub.next().await.unwrap().unwrap());
            assert_eq!(ev["type"], "created");
            let id = ev["ticket"]["id"].as_str().unwrap().to_string();
            let mut sub = call(&writer, "tickets.set_status", json!({"id": id, "status": "Ready"}))
                .await;
            let ev = content(sub.next().await.unwrap().unwrap());
            assert_eq!(ev["type"], "status_changed");
        }
    });

    // Concurrent readers on their own connections.
    let mut readers = Vec::new();
    for _ in 0..8 {
        readers.push(tokio::spawn(async move {
            let client = connect(port).await;
            for _ in 0..10 {
                let mut sub = call(&client, "tickets.list", json!({"epic": "LOAD"})).await;
                let ev = content(sub.next().await.unwrap().unwrap());
                assert_eq!(ev["type"], "ticket");

                let mut sub = call(&client, "tickets.get", json!({"id": "LOAD-1"})).await;
                let ev = content(sub.next().await.unwrap().unwrap());
                assert_eq!(ev["type"], "ticket");
                assert_eq!(ev["ticket"]["id"], "LOAD-1");

                let mut sub = call(&client, "tickets.health", json!({})).await;
                let ev = content(sub.next().await.unwrap().unwrap());
                assert_eq!(ev["type"], "health");
            }
        }));
    }

    tokio::time::timeout(Duration::from_secs(60), async move {
        mutations.await.expect("mutation stream completed");
        for r in readers {
            r.await.expect("reader completed");
        }
        // The watcher saw every acknowledged change, creates in seq order.
        // Each create after the root also updates the root's `children`
        // (MNG-14), and every create moves LOAD's roll-up (MNG-10; a flip
        // to Ready moves none), so: 2 + (MUTATIONS - 1) × (created,
        // children_changed, group_changed, status_changed).
        let mut next_seq = 1u64;
        for _ in 0..(4 * MUTATIONS - 2) {
            let ev = content(watch.next().await.unwrap().unwrap());
            match ev["type"].as_str().unwrap() {
                "created" => {
                    assert_eq!(ev["ticket"]["seq"], next_seq);
                    next_seq += 1;
                }
                "children_changed" => assert_eq!(ev["ticket"]["id"], "LOAD-1"),
                "group_changed" => {
                    assert_eq!(ev["group"]["epic"], "LOAD");
                    assert_eq!(ev["group"]["total"], next_seq - 1, "follows its create");
                }
                "status_changed" => {}
                other => panic!("unexpected watch event {other}"),
            }
        }
        assert_eq!(next_seq, MUTATIONS as u64 + 1);
    })
    .await
    .expect("concurrency smoke finished in time");
}

/// MNG-12/14 over the real wire: the first ticket of a prefix must be a
/// scope (in-band error, nothing allocated); a ticket naming no parent
/// hangs under the shortcode root; `set_parent` moves a ticket and the
/// watch-only mirror sees the child (`parent_changed`) and both parents
/// (`children_changed`, `child_removed`) as full tickets; `list {parent}`
/// agrees; the guards — ancestry, outside the shortcode, missing — cross
/// as in-band errors, and a shortcode's root may be hosted elsewhere.
#[tokio::test]
async fn scope_first_default_parent_and_set_parent_over_the_wire() {
    let port = start_server().await;
    let client = connect(port).await;

    let evs = collect(call(&client, "tickets.create", json!({"epic": "SC", "title": "a build first"})).await).await;
    assert_eq!(evs.len(), 1);
    assert_eq!(evs[0]["type"], "error");
    let message = evs[0]["message"].as_str().unwrap();
    assert!(message.contains("SC") && message.contains("scope"), "names the rule: {message}");
    let evs = collect(call(&client, "tickets.get", json!({"id": "SC-1"})).await).await;
    assert_eq!(evs[0]["type"], "not_found", "nothing was allocated");

    let evs = collect(call(&client, "tickets.create",
        json!({"epic": "SC", "title": "root", "kind": "scope"})).await).await;
    assert_eq!(evs[0]["ticket"]["id"], "SC-1");
    assert!(evs[0]["ticket"]["parent"].is_null(), "the root is top-level");
    let evs = collect(call(&client, "tickets.create", json!({"epic": "SC", "title": "a"})).await).await;
    assert_eq!(evs[0]["ticket"]["id"], "SC-2");
    assert_eq!(evs[0]["ticket"]["parent"], "SC-1", "no parent named: the shortcode root");
    let evs = collect(call(&client, "tickets.create", json!({"epic": "SC", "title": "b"})).await).await;
    assert_eq!(evs[0]["ticket"]["id"], "SC-3");
    assert_eq!(evs[0]["ticket"]["parent"], "SC-1");
    // Another shortcode, hosted under SC-1 through its root.
    let evs = collect(call(&client, "tickets.create",
        json!({"epic": "SD", "title": "sub", "kind": "scope", "parent": "SC-1"})).await).await;
    assert_eq!(evs[0]["ticket"]["parent"], "SC-1", "a root may hang under another shortcode");
    let evs = collect(call(&client, "tickets.create",
        json!({"epic": "SD", "title": "escapes", "parent": "SC-2"})).await).await;
    assert_eq!(evs[0]["type"], "error");
    assert!(evs[0]["message"].as_str().unwrap().contains("outside"), "{}", evs[0]);

    let mut mirror = std::collections::HashMap::new();
    for ev in rows(call(&client, "tickets.list", json!({})).await).await {
        mirror.insert(ev["ticket"]["id"].as_str().unwrap().to_string(), ev["ticket"].clone());
    }
    assert_eq!(mirror["SC-1"]["children"], json!(["SC-2", "SC-3", "SD-1"]));
    let mut watch = call(&client, "tickets.watch", json!({})).await;

    let evs = collect(call(&client, "tickets.set_parent",
        json!({"id": "SC-3", "parent": "SC-2"})).await).await;
    assert_eq!(evs.len(), 1);
    assert_eq!(evs[0]["type"], "parent_changed");
    assert_eq!(evs[0]["ticket"]["id"], "SC-3");
    assert_eq!(evs[0]["ticket"]["parent"], "SC-2");
    assert_eq!(evs[0]["previous"], "SC-1");

    for _ in 0..3 {
        let ev = content(watch.next().await.unwrap().unwrap());
        match ev["type"].as_str().unwrap() {
            "children_changed" => {
                assert_eq!(ev["ticket"]["id"], "SC-2");
                assert_eq!(ev["added"], "SC-3");
                assert_eq!(ev["ticket"]["title"], "a", "the full parent, not a delta");
            }
            "child_removed" => {
                assert_eq!(ev["ticket"]["id"], "SC-1");
                assert_eq!(ev["removed"], "SC-3");
            }
            _ => {}
        }
        apply(&mut mirror, &ev);
    }
    assert_eq!(mirror["SC-3"]["parent"], "SC-2");
    assert_eq!(mirror["SC-2"]["children"], json!(["SC-3"]));
    assert_eq!(mirror["SC-1"]["children"], json!(["SC-2", "SD-1"]));

    let evs = rows(call(&client, "tickets.list", json!({"parent": "SC-2"})).await).await;
    assert_eq!(evs.len(), 1);
    assert_eq!(evs[0]["ticket"]["id"], "SC-3");

    let evs = collect(call(&client, "tickets.set_parent",
        json!({"id": "SC-1", "parent": "SC-3"})).await).await;
    assert_eq!(evs[0]["type"], "error");
    assert!(evs[0]["message"].as_str().unwrap().contains("own ancestor"), "{}", evs[0]);
    let evs = collect(call(&client, "tickets.set_parent",
        json!({"id": "SC-2", "parent": "SD-1"})).await).await;
    assert_eq!(evs[0]["type"], "error");
    assert!(evs[0]["message"].as_str().unwrap().contains("outside"), "{}", evs[0]);
    let evs = collect(call(&client, "tickets.set_parent",
        json!({"id": "SC-7", "parent": "SC-1"})).await).await;
    assert_eq!(evs[0]["type"], "not_found");
}
