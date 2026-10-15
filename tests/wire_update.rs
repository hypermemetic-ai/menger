//! MENGER-30: `tickets.update` over the real protocol — a real
//! TransportServer, a real WebSocket client, the `menger.call` envelope
//! (tests/wire.rs's) — its answer, `watch` carrying each aspect's own events
//! in order, blockers moving `blockers` and `waves`, the no-op, and a
//! refusal that names every reason and writes nothing. ACT-2: and
//! `tickets.update_many` — a hundred tickets in one call.

use jsonrpsee::core::client::{Subscription, SubscriptionClientT};
use jsonrpsee::core::params::ObjectParams;
use jsonrpsee::ws_client::{WsClient, WsClientBuilder};
use menger::build_rpc;
use plexus_core::plexus::DynamicHub;
use plexus_transport::TransportServer;
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::Duration;

/// A port no other test in this binary has been handed (MNG-19).
async fn free_port() -> u16 {
    static HANDED: std::sync::Mutex<Vec<u16>> = std::sync::Mutex::new(Vec::new());
    loop {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let mut handed = HANDED.lock().unwrap();
        if !handed.contains(&port) {
            handed.push(port);
            return port;
        }
    }
}

/// A server on an isolated, throwaway database.
async fn start_server() -> u16 {
    let dir = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).expect("tempdir");
    let db_path = dir.path().join("tickets.db");
    let port = free_port().await;
    let hub = build_rpc(&db_path).await.expect("open database");
    let conv = |arc: Arc<DynamicHub>| DynamicHub::arc_into_rpc_module(arc).map_err(|e| anyhow::anyhow!("{e}"));
    tokio::spawn(async move {
        let _keep = dir;
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
    client.subscribe("menger.call", p, "menger.call_unsub").await.expect("subscribe")
}

/// Drains a finite call: every `data` item's content until `done`.
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

/// The one event a write answers with.
async fn one(client: &WsClient, method: &str, params: Value) -> Value {
    let evs = collect(call(client, method, params.clone()).await).await;
    assert_eq!(evs.len(), 1, "{method} {params}: {evs:?}");
    evs[0].clone()
}

/// A `watch`'s events up to and including the first of type `kind`.
async fn watch_until(watch: &mut Subscription<Value>, kind: &str) -> Vec<Value> {
    let mut seen = Vec::new();
    loop {
        let item = tokio::time::timeout(Duration::from_secs(10), watch.next())
            .await
            .unwrap_or_else(|_| panic!("no {kind} after {seen:?}"))
            .expect("the watch is open")
            .unwrap();
        assert_eq!(item["type"], "data", "{item}");
        let ev = item["content"].clone();
        let last = ev["type"] == kind;
        seen.push(ev);
        if last {
            return seen;
        }
    }
}

fn types(evs: &[Value]) -> Vec<&str> {
    evs.iter().map(|e| e["type"].as_str().unwrap_or("?")).collect()
}

#[tokio::test]
async fn update_over_the_wire_one_write_its_events_in_order() {
    let port = start_server().await;
    let client = connect(port).await;
    for params in [
        json!({"epic": "UPD", "title": "root", "kind": "scope"}),
        json!({"epic": "UPD", "title": "first"}),
        json!({"epic": "UPD", "title": "second"}),
    ] {
        assert_eq!(one(&client, "tickets.create", params).await["type"], "created");
    }
    let mut watch = call(&client, "tickets.watch", json!({})).await;

    // Many aspects at once: one answer naming them, and on `watch` each
    // aspect's own event, in order, then the placements it moved.
    let ev = one(
        &client,
        "tickets.update",
        json!({"id": "upd-3", "title": "second, updated", "kind": "gap", "status": "Ready", "severity": "High", "block": ["UPD-2"]}),
    )
    .await;
    assert_eq!(ev["type"], "updated", "{ev}");
    assert_eq!(ev["changed"], json!(["title", "kind", "status", "severity", "blocked_by"]));
    let t = &ev["ticket"];
    assert_eq!(
        (t["title"].clone(), t["kind"].clone(), t["status"].clone(), t["severity"].clone(), t["blocked_by"].clone()),
        (json!("second, updated"), json!("gap"), json!("Ready"), json!("High"), json!(["UPD-2"]))
    );
    assert_eq!(t["placement"], json!({"type": "wave", "n": 2}));
    let seen = watch_until(&mut watch, "placements_changed").await;
    assert_eq!(
        types(&seen),
        ["amended", "kind_changed", "transition", "transition", "blockers_changed", "unlocks_changed", "severity_changed", "placements_changed"]
    );
    assert_eq!(seen[4]["added"], json!(["UPD-2"]));
    assert_eq!((seen[5]["ticket"]["id"].clone(), seen[5]["ticket"]["unlocks"].clone()), (json!("UPD-2"), json!(["UPD-3"])));
    assert_eq!(seen[7]["changes"], json!([{"id": "UPD-3", "placement": {"type": "wave", "n": 2}}]));

    // `blockers` and `waves` read what it wrote.
    let blockers = collect(call(&client, "tickets.blockers", json!({"id": "UPD-3"})).await).await;
    assert_eq!(blockers[0]["ticket"]["id"], "UPD-2", "{blockers:?}");
    let waves = collect(call(&client, "tickets.waves", json!({"epics": ["UPD"]})).await).await;
    let placed: Vec<(Value, Value)> =
        waves.iter().filter(|e| e["type"] == "placed").map(|e| (e["summary"]["id"].clone(), e["placement"].clone())).collect();
    assert!(placed.contains(&(json!("UPD-3"), json!({"type": "wave", "n": 2}))), "{waves:?}");

    // Unblock: its side and the blocker's, and the wave back.
    let ev = one(&client, "tickets.update", json!({"id": "UPD-3", "unblock": ["UPD-2"], "severity": "none"})).await;
    assert_eq!(ev["changed"], json!(["severity", "blocked_by"]));
    assert!(ev["ticket"].get("severity").is_none_or(Value::is_null), "cleared: {ev}");
    let seen = watch_until(&mut watch, "placements_changed").await;
    assert_eq!(types(&seen), ["blockers_changed", "unlocks_removed", "severity_changed", "placements_changed"]);
    assert_eq!((seen[1]["removed"].clone(), seen[1]["ticket"]["unlocks"].clone()), (json!("UPD-3"), json!([])));
    assert_eq!(seen[2]["previous"], "High");

    // Nothing to change: `changed: []`, and nothing on `watch` — the next
    // event is the next write's.
    let ev = one(&client, "tickets.update", json!({"id": "UPD-3", "title": "second, updated", "unblock": ["UPD-2"]})).await;
    assert_eq!((ev["type"].clone(), ev["changed"].clone()), (json!("updated"), json!([])));
    one(&client, "tickets.label", json!({"id": "UPD-2", "add": ["mark"]})).await;
    let seen = watch_until(&mut watch, "labels_changed").await;
    assert_eq!(types(&seen), ["labels_changed"], "the no-op sent nothing");

    // ACCESS-8: an unknown severity is no `SeverityChange` — refused at
    // dispatch, naming the parameter, before anything is asked.
    let mut sub = call(&client, "tickets.update", json!({"id": "UPD-3", "title": "fine", "status": "Doc", "severity": "Huge"})).await;
    let item = sub.next().await.expect("an item").unwrap();
    assert_eq!((item["code"].as_str(), item["error_kind"].as_str()), (Some("-32602"), Some("invalid_params")), "{item}");
    assert!(item["message"].as_str().unwrap().contains("severity: unknown severity 'huge'"), "{item}");
    // A refusal names every reason the store holds and writes nothing — a
    // valid title beside a status refused and a block on itself.
    let ev = one(&client, "tickets.update", json!({"id": "UPD-3", "title": "fine", "status": "Doc", "block": ["UPD-3"]})).await;
    assert_eq!(ev["type"], "error", "{ev}");
    let message = ev["message"].as_str().unwrap();
    assert!(message.contains("Doc is a doc's own status") && message.contains("UPD-3"), "{message}");
    let got = one(&client, "tickets.get", json!({"id": "UPD-3"})).await;
    assert_eq!(got["ticket"]["title"], "second, updated", "nothing written");
    let ev = one(&client, "tickets.update", json!({"id": "UPD-2", "block": ["UPD-3"], "unblock": []})).await;
    assert_eq!(ev["type"], "updated", "no cycle once UPD-3 no longer waits on it: {ev}");
    let ev = one(&client, "tickets.update", json!({"id": "UPD-3", "block": ["UPD-2"]})).await;
    assert_eq!(ev["type"], "error");
    assert!(ev["message"].as_str().unwrap().contains("UPD-3 -> UPD-2 -> UPD-3"), "{ev}");
    let ev = one(&client, "tickets.update", json!({"id": "UPD-404", "title": "x"})).await;
    assert_eq!(ev["type"], "not_found");
}

/// ACT-2: `tickets.update_many` over the real protocol — 100 tickets moved,
/// labelled and re-statused in one call: one answer, on `watch` each
/// ticket's own events then one `updated_many`; a filter's selection; a
/// refusal naming each refused id, writing nothing; `label` over `ids`.
#[tokio::test]
async fn update_many_over_the_wire_a_hundred_tickets_in_one_call() {
    let port = start_server().await;
    let client = connect(port).await;
    assert_eq!(one(&client, "tickets.create", json!({"epic": "MANY", "title": "root", "kind": "scope"})).await["type"], "created");
    // ORG-5: the home inside MANY is work, not a scope.
    assert_eq!(one(&client, "tickets.create", json!({"epic": "MANY", "title": "home", "kind": "execution"})).await["type"], "created");
    for i in 0..100 {
        assert_eq!(one(&client, "tickets.create", json!({"epic": "MANY", "title": format!("t{i}")})).await["type"], "created");
    }
    let ids: Vec<String> = (3..103).map(|i| format!("MANY-{i}")).collect();
    let mut watch = call(&client, "tickets.watch", json!({})).await;

    let ev = one(
        &client,
        "tickets.update_many",
        json!({"ids": ids, "parent": "MANY-2", "label": ["moved"], "status": "Ready", "message": "the hundred"}),
    )
    .await;
    assert_eq!(ev["type"], "updated_many", "{ev}");
    assert_eq!(ev["changed"], json!(["status", "parent", "labels"]));
    assert_eq!(ev["ids"].as_array().unwrap().len(), 100);
    assert_eq!(ev["unchanged"], json!([]));
    let tickets = ev["tickets"].as_array().unwrap();
    assert_eq!(tickets.len(), 100);
    assert!(tickets.iter().all(|t| t["parent"] == "MANY-2" && t["status"] == "Ready" && t["labels"] == json!(["moved"])), "{ev}");

    // Each ticket's own events (status, transition, parent, both parents'
    // children, labels: 600 and the groups), then one `updated_many`. The
    // write sends them at once, past the broadcast's 256 (SCALE-5), so a
    // watch may lag: then one `lagged` says how many it lost, and the tail
    // — `updated_many` last — still arrives.
    let seen = watch_until(&mut watch, "updated_many").await;
    let kinds = types(&seen);
    assert_eq!(kinds.iter().filter(|k| **k == "updated_many").count(), 1);
    let missed: u64 = seen.iter().filter(|e| e["type"] == "lagged").map(|e| e["missed"].as_u64().unwrap()).sum();
    if missed == 0 {
        for kind in ["status_changed", "transition", "parent_changed", "children_changed", "child_removed", "labels_changed"] {
            assert_eq!(kinds.iter().filter(|k| **k == kind).count(), 100, "{kind}");
        }
    } else {
        assert!(missed as usize + seen.len() > 600, "{missed} lost, {} seen", seen.len());
    }
    let last = seen.last().unwrap();
    assert_eq!((last["ids"].as_array().unwrap().len(), last["changed"].clone()), (100, json!(["status", "parent", "labels"])));
    assert_eq!(last["tickets"], json!([]), "watch carries the ids, not the tickets");
    // The message on every transition, one shared cause: the first's.
    let history = collect(call(&client, "tickets.history", json!({"id": "MANY-3"})).await).await;
    let all: Vec<&Value> = history.iter().filter(|e| e["type"] == "transition").map(|e| &e["transition"]).collect();
    let first = all.iter().find(|t| t["ticket"] == "MANY-3" && t["to"] == "Ready").expect("MANY-3's transition");
    assert_eq!((first["message"].clone(), first["cause"].clone()), (json!("the hundred"), Value::Null), "{history:?}");
    let caused: Vec<&&Value> = all.iter().filter(|t| t["ticket"] != "MANY-3").collect();
    assert_eq!(caused.len(), 99, "the rest, caused by the first: {history:?}");
    assert!(caused.iter().all(|t| t["message"] == "the hundred" && t["cause"] == first["id"]));
    let home = one(&client, "tickets.get", json!({"id": "MANY-2"})).await;
    assert_eq!(home["ticket"]["child_count"], 100);

    // By a filter, capped: over the cap refused, naming the count.
    let ev = one(&client, "tickets.update_many", json!({"filter": "label moved", "max": 50, "status": "Coding"})).await;
    assert_eq!(ev["type"], "error");
    assert!(ev["message"].as_str().unwrap().contains("admits 100 tickets"), "{ev}");
    let ev = one(&client, "tickets.update_many", json!({"filter": "label moved", "severity": "High"})).await;
    assert_eq!((ev["type"].clone(), ev["ids"].as_array().map(Vec::len)), (json!("updated_many"), Some(100)), "{ev}");

    // All or none: a doc and an unknown id refuse the write, each named.
    one(&client, "tickets.create", json!({"epic": "MANY", "title": "a doc", "kind": "doc"})).await;
    let ev = one(&client, "tickets.update_many", json!({"ids": ["MANY-3", "MANY-103", "MANY-404"], "status": "Complete"})).await;
    assert_eq!(ev["type"], "error", "{ev}");
    let message = ev["message"].as_str().unwrap();
    assert!(message.starts_with("2 tickets refused, nothing written") && message.contains("MANY-103:") && message.contains("MANY-404:"), "{message}");
    assert_eq!(one(&client, "tickets.get", json!({"id": "MANY-3"})).await["ticket"]["status"], "Ready", "nothing written");
    let ev = one(&client, "tickets.update_many", json!({"status": "Complete"})).await;
    assert_eq!(ev["type"], "error", "ids or a filter: {ev}");

    // `label` and `link_commits` over ids: one event per ticket, one write.
    let evs = collect(call(&client, "tickets.label", json!({"ids": ["MANY-3", "MANY-4"], "add": ["needs-you"]})).await).await;
    assert_eq!(types(&evs), ["labels_changed", "labels_changed"]);
    assert_eq!(evs[1]["ticket"]["labels"], json!(["moved", "needs-you"]));
    let evs = collect(call(&client, "tickets.link_commits", json!({"ids": ["MANY-3", "MANY-4"], "commits": ["77df9a0"]})).await).await;
    assert_eq!(types(&evs), ["commits_linked", "commits_linked"]);
    let ev = one(&client, "tickets.label", json!({"id": "MANY-3", "ids": ["MANY-4"], "add": ["x"]})).await;
    assert_eq!(ev["type"], "error", "id or ids, not both: {ev}");
}
