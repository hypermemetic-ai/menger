//! FILTER-36: `saved "Name"` over the real protocol — a real
//! TransportServer, a real WebSocket client, the `menger.call` envelope
//! (tests/wire.rs's): a store seeded with the collection and two saved
//! filters the way the app's Save as… writes them; `list` answering the
//! union of two names as the two listed apart, `counts` agreeing, a missing
//! name and a cycle each one in-band `error`, and a membership watch on the
//! union said what a saved doc's amend did to it.

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
    let conv = |arc: Arc<DynamicHub>| menger::calls::rpc_module(arc, Default::default()).map_err(|e| anyhow::anyhow!("{e}"));
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


fn content(item: Value) -> Value {
    assert_eq!(item["type"], "data", "expected data item, got {item}");
    item["content"].clone()
}

/// The ids of a filtered `list { summary }`, in the store's order — or the
/// in-band error's message.
async fn listed(client: &WsClient, filter: &str) -> Result<Vec<String>, String> {
    let evs = collect(call(client, "tickets.list", json!({"filter": filter, "summary": true, "limit": 500})).await).await;
    if let Some(e) = evs.iter().find(|e| e["type"] == "error") {
        return Err(e["message"].as_str().unwrap_or_default().to_string());
    }
    Ok(evs.iter().filter(|e| e["type"] == "summary").map(|e| e["summary"]["id"].as_str().unwrap().to_string()).collect())
}

/// `counts`' matches, summed — or the in-band error's message.
async fn counted(client: &WsClient, filter: &str) -> Result<u64, String> {
    let evs = collect(call(client, "tickets.counts", json!({"filter": filter})).await).await;
    if let Some(e) = evs.iter().find(|e| e["type"] == "error") {
        return Err(e["message"].as_str().unwrap_or_default().to_string());
    }
    Ok(evs.iter().map(|e| e["held"].as_u64().unwrap()).sum())
}

/// A saved filter's doc, as the app's Save as… writes one (FILTER-6): in
/// VIEW, under the collection, at `_MENGER/view/<slug>`, titled with its
/// name, its body the one `filter:` line.
async fn save(client: &WsClient, name: &str, slug: &str, text: &str) -> String {
    let ev = one(
        client,
        "tickets.create",
        json!({"epic": "VIEW", "title": name, "kind": "doc", "parent": "VIEW-1", "path": format!("_MENGER/view/{slug}"), "body": format!("filter: {text}")}),
    )
    .await;
    assert_eq!(ev["type"], "created", "{ev}");
    ev["ticket"]["id"].as_str().unwrap().to_string()
}

#[tokio::test]
async fn saved_names_over_the_wire_list_count_refuse_and_move_a_watch() {
    let port = start_server().await;
    let client = connect(port).await;
    // The collection, as the app's first save makes it, then two saved filters.
    one(&client, "tickets.create", json!({"epic": "FILTER", "title": "filters", "kind": "scope"})).await;
    let view = one(&client, "tickets.create", json!({"epic": "VIEW", "title": "Saved filters", "kind": "doc", "parent": "FILTER-1", "body": ""})).await;
    assert_eq!(view["ticket"]["id"], "VIEW-1", "{view}");
    save(&client, "Needs you", "needs-you", "(asking or label needs-you)").await;
    let plexus = save(&client, "Plexus-Ready", "plexus-ready", "in PLEXUS and status Ready").await;
    let list = "- <m-demand id=\"needs-you\">Needs you</m-demand>\n- <m-demand id=\"plexus-ready\">Plexus-Ready</m-demand>\n";
    one(&client, "tickets.amend", json!({"id": "VIEW-1", "body": list})).await;
    // The work they choose from: PLEXUS-2, 3 Ready; 3, 4 needing you.
    one(&client, "tickets.create", json!({"epic": "PLEXUS", "title": "the platform", "kind": "scope"})).await;
    for i in 0..3 {
        one(&client, "tickets.create", json!({"epic": "PLEXUS", "title": format!("work {i}")})).await;
    }
    one(&client, "tickets.set_status", json!({"id": "PLEXUS-2", "status": "Ready"})).await;
    one(&client, "tickets.set_status", json!({"id": "PLEXUS-3", "status": "Ready"})).await;
    collect(call(&client, "tickets.label", json!({"ids": ["PLEXUS-3", "PLEXUS-4"], "add": ["needs-you"]})).await).await;

    // Each name is its filter; `or` is the union, `and` the intersection.
    let needs = listed(&client, "saved \"Needs you\"").await.unwrap();
    let ready = listed(&client, "saved Plexus-Ready").await.unwrap();
    assert_eq!(needs, listed(&client, "(asking or label needs-you)").await.unwrap());
    assert_eq!(ready, listed(&client, "in PLEXUS and status Ready").await.unwrap());
    assert_eq!((needs.clone(), ready.clone()), (vec!["PLEXUS-3".to_string(), "PLEXUS-4".into()], vec!["PLEXUS-2".to_string(), "PLEXUS-3".into()]));
    let union = "saved \"Needs you\" or saved Plexus-Ready";
    let mut both: Vec<String> = needs.iter().chain(&ready).cloned().collect();
    both.sort();
    both.dedup();
    assert_eq!(listed(&client, union).await.unwrap(), both);
    assert_eq!(listed(&client, "saved \"Needs you\" and saved Plexus-Ready").await.unwrap(), ["PLEXUS-3"]);
    // counts agree.
    assert_eq!(counted(&client, union).await.unwrap(), both.len() as u64);
    assert_eq!(counted(&client, "saved needs-you").await.unwrap(), 2, "a name by its slug");

    // A name nothing holds; a cycle: one in-band error each, naming it.
    let missing = listed(&client, "saved Nope or ready").await.unwrap_err();
    assert!(missing.contains("no saved filter named \"Nope\""), "{missing}");
    assert!(counted(&client, "saved Nope").await.unwrap_err().contains("no saved filter named \"Nope\""));
    save(&client, "Loop A", "loop-a", "saved \"Loop B\" and ready").await;
    save(&client, "Loop B", "loop-b", "(blocked or saved \"Loop A\")").await;
    let cycle = listed(&client, "saved \"Loop A\"").await.unwrap_err();
    assert!(cycle.contains("a cycle: \"Loop A\" names \"Loop B\" names \"Loop A\""), "{cycle}");
    let mut bad = call(&client, "tickets.watch", json!({"filter": "saved Nope"})).await;
    let ev = content(bad.next().await.unwrap().unwrap());
    assert_eq!(ev["type"], "error", "{ev}");

    // A watch on the union: Plexus-Ready's text amended moves it — PLEXUS-2
    // leaves (Ready, needing no one), PLEXUS-1 (the root, Pending) enters.
    let mut members = call(&client, "tickets.watch", json!({"filter": union, "membership": true})).await;
    let mut all = call(&client, "tickets.watch", json!({"filter": union})).await;
    one(&client, "tickets.amend", json!({"id": plexus, "body": "filter: in PLEXUS and status Pending"})).await;
    let heard = watch_until(&mut all, "left").await;
    let kinds: Vec<&str> = heard.iter().map(|e| e["type"].as_str().unwrap()).collect();
    assert_eq!(&kinds[kinds.len() - 3..], ["amended", "entered", "left"], "after the change that moved it: {kinds:?}");
    let evs = watch_until(&mut members, "left").await;
    assert_eq!(
        evs,
        [
            json!({"type": "entered", "id": "PLEXUS-1", "epic": "PLEXUS", "shortcode": "PLEXUS", "seq": 1}),
            json!({"type": "left", "id": "PLEXUS-2", "epic": "PLEXUS", "shortcode": "PLEXUS", "seq": 2}),
        ]
    );
    assert_eq!(listed(&client, union).await.unwrap(), ["PLEXUS-1", "PLEXUS-3", "PLEXUS-4"]);
    // …and the watch judges the next cell move by the filter as it now is:
    // PLEXUS-2 back to Pending enters.
    one(&client, "tickets.set_status", json!({"id": "PLEXUS-2", "status": "Pending"})).await;
    let evs = watch_until(&mut members, "entered").await;
    assert_eq!(evs, [json!({"type": "entered", "id": "PLEXUS-2", "epic": "PLEXUS", "shortcode": "PLEXUS", "seq": 2})]);
}
