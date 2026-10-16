//! ARCHIVE-2: `tickets.archive` and `tickets.restore` over the real
//! protocol — a real TransportServer, a real WebSocket client, the
//! `menger.call` envelope (tests/wire.rs's): a dry run, an archive of a
//! subtree that names its live work, `watch { filter }` hearing it leave,
//! every view without it unless the filter names `archived`, `get` with the
//! mark, `health`'s count, a restore with its ancestor chain, the
//! `archived` / `restored` cells, and a create under an archived parent
//! answering `created` then the chain's `restored`.

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


#[tokio::test]
async fn archive_and_restore_over_the_wire() {
    let port = start_server().await;
    let client = connect(port).await;
    for params in [
        json!({"epic": "ARC", "title": "the project", "kind": "scope"}),
        json!({"epic": "ARC", "title": "landed"}),
        json!({"epic": "ARC", "title": "the controller", "kind": "execution"}),
        json!({"epic": "ARC", "title": "the shutoff", "parent": "ARC-3"}),
    ] {
        assert_eq!(one(&client, "tickets.create", params).await["type"], "created");
    }
    one(&client, "tickets.set_status", json!({"id": "ARC-2", "status": "Complete"})).await;
    one(&client, "tickets.set_status", json!({"id": "ARC-4", "status": "Ready"})).await;
    let mut watch = call(&client, "tickets.watch", json!({})).await;
    let mut filtered = call(&client, "tickets.watch", json!({"filter": "in ARC"})).await;
    tokio::time::sleep(Duration::from_millis(200)).await;

    // The dry run: what would go, and what is live — nothing written.
    let dry = one(&client, "tickets.archive", json!({"ids": ["arc-3"], "dry_run": true})).await;
    assert_eq!((dry["type"].clone(), dry["dry_run"].clone(), dry["ids"].clone()), (json!("archived"), json!(true), json!(["ARC-3", "ARC-4"])), "{dry}");
    assert_eq!(dry["live"], json!([{"id": "ARC-3", "status": "Pending"}, {"id": "ARC-4", "status": "Ready"}]));
    assert!(one(&client, "tickets.get", json!({"id": "ARC-4"})).await["ticket"].get("archived").is_none());

    // The whole subtree, live work named, never refused.
    let a = one(&client, "tickets.archive", json!({"ids": ["ARC-1"], "message": "first season fired"})).await;
    assert_eq!(a["ids"], json!(["ARC-1", "ARC-2", "ARC-3", "ARC-4"]), "{a}");
    assert_eq!(a["live"].as_array().unwrap().len(), 3, "ARC-2 is done");
    assert_eq!((a["mark"]["message"].clone(), a["dry_run"].clone()), (json!("first season fired"), json!(false)));
    let heard = watch_until(&mut watch, "archived").await;
    assert_eq!(heard.iter().filter(|e| e["type"] == "transition").count(), 4, "{heard:?}");
    let left = watch_until(&mut filtered, "left").await;
    assert!(left.iter().any(|e| e["type"] == "archived"), "the filtered watch hears the archive, then each leaving: {left:?}");

    // Every view leaves it out, unless the filter names it.
    let listed = |evs: Vec<Value>| evs.into_iter().filter(|e| e["type"] == "summary").map(|e| e["summary"]["id"].as_str().unwrap().to_string()).collect::<Vec<_>>();
    assert!(listed(collect(call(&client, "tickets.list", json!({"epic": "ARC", "summary": true})).await).await).is_empty());
    assert!(listed(collect(call(&client, "tickets.list", json!({"filter": "status Pending", "summary": true})).await).await).is_empty());
    assert_eq!(listed(collect(call(&client, "tickets.list", json!({"filter": "archived and in ARC", "summary": true})).await).await), ["ARC-1", "ARC-2", "ARC-3", "ARC-4"]);
    assert_eq!(listed(collect(call(&client, "tickets.list", json!({"filter": "archived within 1d", "summary": true})).await).await).len(), 4);
    let health = one(&client, "tickets.health", json!({})).await;
    assert_eq!((health["ticket_count"].clone(), health["archived"].clone()), (json!(0), json!(4)), "{health}");
    let got = one(&client, "tickets.get", json!({"id": "ARC-4"})).await;
    assert_eq!((got["ticket"]["status"].clone(), got["ticket"]["archived"]["message"].clone()), (json!("Ready"), json!("first season fired")), "get still answers it");
    let history = collect(call(&client, "tickets.history", json!({"id": "ARC-1"})).await).await;
    assert!(history.iter().any(|e| e["type"] == "transition" && e["transition"]["field"] == "archive" && e["transition"]["to"] == "archived"), "{history:?}");

    // Restore one: its ancestor chain comes back, never their other children.
    let dry = one(&client, "tickets.restore", json!({"ids": ["ARC-4"], "dry_run": true})).await;
    assert_eq!((dry["ids"].clone(), dry["ancestors"].clone()), (json!(["ARC-1", "ARC-3", "ARC-4"]), json!(["ARC-1", "ARC-3"])), "{dry}");
    let r = one(&client, "tickets.restore", json!({"ids": ["ARC-4"]})).await;
    assert_eq!((r["type"].clone(), r["ids"].clone(), r["dry_run"].clone()), (json!("restored"), json!(["ARC-1", "ARC-3", "ARC-4"]), json!(false)), "{r}");
    watch_until(&mut filtered, "entered").await;
    assert_eq!(listed(collect(call(&client, "tickets.list", json!({"filter": "archived", "summary": true})).await).await), ["ARC-2"]);
    assert_eq!(listed(collect(call(&client, "tickets.list", json!({"filter": "restored today", "summary": true})).await).await), ["ARC-1", "ARC-3", "ARC-4"]);

    // A create under an archived parent: `created`, then the chain back.
    one(&client, "tickets.archive", json!({"ids": ["ARC-3"]})).await;
    let evs = collect(call(&client, "tickets.create", json!({"epic": "ARC", "title": "an alarm", "parent": "ARC-3"})).await).await;
    assert_eq!(evs.iter().map(|e| e["type"].as_str().unwrap()).collect::<Vec<_>>(), ["created", "restored"], "{evs:?}");
    assert_eq!(evs[1]["ids"], json!(["ARC-3"]), "the parent; ARC-4 stays in the archive");
    assert!(one(&client, "tickets.get", json!({"id": "ARC-4"})).await["ticket"]["archived"].is_object());
    // And a move under one.
    one(&client, "tickets.archive", json!({"ids": ["ARC-3"]})).await;
    let evs = collect(call(&client, "tickets.set_parent", json!({"id": "ARC-2", "parent": "ARC-3"})).await).await;
    assert_eq!(evs.last().unwrap()["type"], "restored", "{evs:?}");
    // Neither ids nor a filter: refused in band.
    assert_eq!(one(&client, "tickets.archive", json!({})).await["type"], "error");
}
