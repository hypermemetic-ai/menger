//! DEPLOY-11: `deployments` over the real protocol — a real
//! TransportServer, a real WebSocket client, the `menger.call` envelope
//! (tests/wire.rs's): a deploy to test, a promote to prod, then a rollback of
//! prod, reading `for_ticket` after each; `deployments.watch` hearing each,
//! `tickets.watch { filter: "deployed prod" }` hearing the tickets enter and
//! leave, and `list` / `environments` saying what happened.

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

fn types(evs: &[Value]) -> Vec<&str> {
    evs.iter().map(|e| e["type"].as_str().unwrap_or("?")).collect()
}

/// Where `id` runs, by `for_ticket`: each environment `running` (with its
/// since) or `not`.
async fn where_runs(client: &WsClient, id: &str) -> Vec<String> {
    collect(call(client, "deployments.for_ticket", json!({"id": id})).await)
        .await
        .iter()
        .map(|e| match e["type"].as_str().unwrap() {
            "running" => format!("{} since {}", e["environment"].as_str().unwrap(), e["since"]),
            "not_running" => format!("{} not", e["environment"].as_str().unwrap()),
            other => panic!("{other}: {e}"),
        })
        .collect()
}

#[tokio::test]
async fn record_test_then_prod_then_a_rollback() {
    let port = start_server().await;
    let client = connect(port).await;
    for params in [
        json!({"epic": "MENGER", "title": "menger", "kind": "scope"}),
        json!({"epic": "MENGER", "title": "the old work", "kind": "build"}),
        json!({"epic": "MENGER", "title": "the new work", "kind": "build"}),
    ] {
        one(&client, "tickets.create", params).await;
    }
    one(&client, "tickets.commits.link", json!({"id": "MENGER-2", "commits": ["29a50be"]})).await;
    one(&client, "tickets.commits.link", json!({"id": "MENGER-3", "commits": ["824e943"]})).await;
    let mut deployed = call(&client, "deployments.watch", json!({})).await;
    let mut entering = call(&client, "tickets.watch", json!({"filter": "deployed prod"})).await;
    assert!(where_runs(&client, "MENGER-3").await.is_empty(), "no environment yet");

    // The old work, everywhere; then the new work to test — DEPLOY-10's
    // record, flat, as `deploy/record --send` passes it.
    for (env, at) in [("test", "2026-10-12T21:00:00Z"), ("prod", "2026-10-12T21:30:00Z")] {
        let ev = one(&client, "deployments.record", json!({
            "project": "menger", "environment": env, "state": "succeeded", "revision": "29a50be",
            "commits": ["29a50be"], "at": at, "source": {"system": "menger-pipeline", "run": format!("old-{env}")}
        }))
        .await;
        assert_eq!(ev["entered"], json!(["MENGER-2"]), "{ev}");
    }
    let ev = one(&client, "deployments.record", json!({
        "project": "menger", "environment": "test", "state": "succeeded", "revision": "235ff62",
        "previous": "29a50be", "commits": ["824e943aa1b2c3d4e5f60718293a4b5c6d7e8f90", "235ff62"],
        "release": "20261012T220402Z-235ff62", "at": "2026-10-12T22:04:02Z",
        "source": {"system": "menger-pipeline", "run": "deploy", "url": null}
    }))
    .await;
    assert_eq!(
        (ev["type"].clone(), ev["carried"].clone(), ev["unlinked"].clone()),
        (json!("deployed"), json!(["MENGER-3"]), json!(["235ff62"]))
    );
    let (test_at, prod_at) = (1_791_842_642i64, 1_791_844_939i64);
    assert_eq!(where_runs(&client, "MENGER-3").await, [format!("test since {test_at}"), "prod not".to_string()]);

    // The promote: prod runs it too.
    let promote = json!({
        "project": "menger", "environment": "prod", "state": "succeeded", "revision": "235ff62",
        "previous": "29a50be", "commits": ["824e943", "235ff62"], "release": "20261012T220402Z-235ff62",
        "at": "2026-10-12T22:42:19Z", "source": {"system": "menger-pipeline", "run": "promote"}
    });
    let ev = one(&client, "deployments.record", promote.clone()).await;
    assert_eq!((ev["entered"].clone(), ev["duplicate"].clone()), (json!(["MENGER-3"]), json!(false)));
    assert_eq!(where_runs(&client, "MENGER-3").await, [format!("test since {test_at}"), format!("prod since {prod_at}")]);
    // A CI's retry is the same deployment.
    let again = one(&client, "deployments.record", promote).await;
    assert_eq!((again["duplicate"].clone(), again["deployment"]["id"].clone()), (json!(true), ev["deployment"]["id"].clone()));

    // The rollback of prod: back to 29a50be, 235ff62 taken out.
    let ev = one(&client, "deployments.record", json!({
        "project": "MENGER", "environment": "prod", "state": "rolled_back", "revision": "29a50be",
        "previous": "235ff62", "at": "2026-10-12T23:00:00Z", "source": {"system": "menger-pipeline", "run": "rollback"}
    }))
    .await;
    assert_eq!((ev["entered"].clone(), ev["left"].clone()), (json!([]), json!(["MENGER-3"])));
    assert_eq!(where_runs(&client, "MENGER-3").await, [format!("test since {test_at}"), "prod not".to_string()]);
    assert_eq!(where_runs(&client, "MENGER-2").await.len(), 2, "the old work still runs in both");
    let got = one(&client, "tickets.get", json!({"id": "MENGER-3"})).await;
    assert_eq!(got["ticket"]["status"], "Pending", "a deployment never moves a status");

    // deployments.watch heard each recorded deployment (not the retry), each
    // followed by its environment; the filtered tickets.watch heard MENGER-2
    // and MENGER-3 enter prod, and MENGER-3 leave it.
    let mut kinds = Vec::new();
    for _ in 0..5 {
        kinds.extend(watch_until(&mut deployed, "environment_changed").await.into_iter().map(|e| e["type"].as_str().unwrap().to_string()));
    }
    assert_eq!(kinds, ["deployed", "environment_changed"].repeat(5));
    let mut moved = Vec::new();
    while moved.len() < 3 {
        let item = tokio::time::timeout(Duration::from_secs(10), entering.next()).await.expect("a membership event").unwrap().unwrap();
        let e = &item["content"];
        if e["type"] == "entered" || e["type"] == "left" {
            moved.push(format!("{} {}", e["type"].as_str().unwrap(), e["id"].as_str().unwrap()));
        }
    }
    assert_eq!(moved, ["entered MENGER-2", "entered MENGER-3", "left MENGER-3"]);

    // What the reads say now.
    let envs = collect(call(&client, "deployments.environments", json!({"project": "menger"})).await).await;
    let says: Vec<(String, String)> = envs
        .iter()
        .map(|e| (e["environment"]["name"].as_str().unwrap().into(), e["environment"]["revision"].as_str().unwrap().into()))
        .collect();
    assert_eq!(says, [("test".to_string(), "235ff62".to_string()), ("prod".to_string(), "29a50be".to_string())]);
    let list = collect(call(&client, "deployments.list", json!({"environment": "prod", "limit": 2})).await).await;
    assert_eq!(types(&list), ["deployment", "deployment", "page"]);
    assert_eq!((list[0]["deployment"]["state"].clone(), list[1]["live"].clone()), (json!("rolled_back"), json!(false)));
    let filtered = collect(call(&client, "tickets.list", json!({"filter": "not deployed prod and kind build", "summary": true})).await).await;
    let text = format!("{filtered:?}");
    assert!(text.contains("MENGER-3") && !text.contains("MENGER-2"), "{text}");
    // DEPLOY-13: `running` — every environment's cells in one read, the
    // rollback's taken out; an environment with none answers empty.
    let cells = collect(call(&client, "deployments.running", json!({"project": "menger"})).await).await;
    let says: Vec<(String, Vec<String>)> = cells
        .iter()
        .map(|c| {
            assert_eq!(c["type"], "cells");
            let ids = c["running"].as_array().unwrap().iter().map(|r| r["ticket"].as_str().unwrap().to_string()).collect();
            (c["environment"].as_str().unwrap().to_string(), ids)
        })
        .collect();
    assert_eq!(says, [("test".to_string(), vec!["MENGER-2".to_string(), "MENGER-3".to_string()]), ("prod".to_string(), vec!["MENGER-2".to_string()])]);
    let only = collect(call(&client, "deployments.running", json!({"environment": "prod", "ids": ["MENGER-3"]})).await).await;
    assert_eq!(only[0]["running"], json!([]));
    let refused = one(&client, "deployments.record", json!({"project": "nope", "environment": "prod", "state": "succeeded", "revision": "29a50be"})).await;
    assert_eq!(refused["type"], "error");
}
