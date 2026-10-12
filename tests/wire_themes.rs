//! THEME-8: the `themes` activation over the real protocol — a real
//! TransportServer, a real WebSocket client, the `menger.call` envelope
//! (tests/wire.rs's): define and its refusal, set (the site, a shortcode),
//! list, get, `theme_for` naming each region's level, and `watch` carrying
//! each write in order.

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

/// The one event a call answers with.
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
async fn themes_over_the_wire_define_set_theme_for_and_watch() {
    let port = start_server().await;
    let client = connect(port).await;
    let mut watch = call(&client, "themes.watch", json!({})).await;

    let ev = one(&client, "themes.define", json!({"name": "ocean", "light": {"--accent": "#00796b", "bg": "#f3f8f8"}, "dark": {"accent": "#4db6ac"}})).await;
    assert_eq!(ev["type"], "theme", "{ev}");
    assert_eq!(ev["theme"]["light"], json!({"--accent": "#00796b", "--bg": "#f3f8f8"}));
    assert_eq!(ev["theme"]["dark"], json!({"--accent": "#4db6ac"}));
    assert_eq!(one(&client, "themes.define", json!({"name": "pantry", "light": {"--accent": "#d4572a"}})).await["type"], "theme");
    let refused = one(&client, "themes.define", json!({"name": "mint", "light": {"--nope": "#000"}})).await;
    assert_eq!(refused["type"], "error", "{refused}");
    assert!(refused["message"].as_str().unwrap().contains("`--nope` is not a token"), "{refused}");

    for params in [
        json!({"epic": "PANTRY", "title": "root", "kind": "scope"}),
        json!({"epic": "PANTRY", "title": "the page", "body": "---\ntheme: pantry\nreach: site\nframe-takeover: off\n---\n<p>hi</p>"}),
    ] {
        assert_eq!(one(&client, "tickets.create", params).await["type"], "created");
    }
    let site = one(&client, "themes.set", json!({"scope": "site", "theme": "ocean"})).await;
    assert_eq!(site, json!({"type": "setting", "setting": {"scope": "site", "theme": "ocean", "reach": "ticket", "takeover": "on"}}));
    let code = one(&client, "themes.set", json!({"scope": "pantry", "takeover": "off"})).await;
    assert_eq!(code["setting"]["scope"], "PANTRY");
    let bad = one(&client, "themes.set", json!({"scope": "site", "reach": "site"})).await;
    assert_eq!(bad["type"], "error", "{bad}");

    let listed = collect(call(&client, "themes.list", json!({})).await).await;
    assert_eq!(types(&listed), ["theme", "theme", "theme", "setting", "setting"]);
    assert_eq!(listed[0]["theme"]["name"], "default");
    assert_eq!(listed[3]["setting"]["scope"], "site");
    assert_eq!(one(&client, "themes.get", json!({"name": "pantry"})).await["theme"]["light"]["--accent"], "#d4572a");

    let r = one(&client, "themes.theme_for", json!({"id": "pantry-2"})).await;
    assert_eq!(r["type"], "regions", "{r}");
    assert_eq!(r["id"], "PANTRY-2");
    assert_eq!(r["regions"]["site"], json!({"theme": "pantry", "level": "ticket"}));
    assert_eq!(r["regions"]["ticket"], json!({"theme": "pantry", "level": "ticket"}));
    assert_eq!(r["regions"]["frame"], json!({"theme": "default", "level": "frame"}));
    assert_eq!(r["levels"]["site"], "ocean");
    assert_eq!(r["levels"]["shortcode"]["takeover"], "off");
    assert_eq!(one(&client, "themes.theme_for", json!({"id": "PANTRY-9"})).await["type"], "error");

    // The watch heard every write, in order.
    let heard = watch_until(&mut watch, "setting").await;
    assert_eq!(types(&heard), ["theme", "theme", "setting"]);
    let heard = watch_until(&mut watch, "setting").await;
    assert_eq!(heard[0]["setting"]["scope"], "PANTRY");
}
