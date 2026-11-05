//! SYNAPSE-CC-9 (V2-44: main's `tickets-api/tests/schema_typed.rs`, on v2):
//! every position in menger's schemas is typed. Every service a v2 server
//! registers — `tickets`, `ui`, `media`, `docs`, `quota`, `orgs`, `members`,
//! `session`, `auth` — and each of their children (`tickets.commits`,
//! `ui.slots`, `members.teams`, …) answers a schema with no untyped site
//! (`plexus_core::plexus::schema::untyped_sites`): no position that is
//! neither a typed schema nor the JSON type (a bare `object` or `array`, a
//! type list like `["object","boolean"]`), and no RPC method without
//! `returns`. A `serde_json::Value` is the JSON type — a type like any other
//! (the owner, 2026-10-29: nothing is opaque). synapse refuses each untyped
//! site at IR build (SYNAPSE-CC-8).
//!
//! On v2 the services live in four crates (`menger-tickets-api`,
//! `menger-media`, `menger-docs`, `menger-quota`) and the framework's
//! (`plexus-ui-service`), so the schemas are read as synapse reads them: from
//! a server built as `main` builds it, signed in as the fixture operator
//! (admin on `*`: every method in view), `menger.schema` naming the services
//! and each `<path>.schema` its methods and children, to the leaves. Its own
//! throwaway database and a free port.

mod support;
use jsonrpsee::core::client::{Subscription, SubscriptionClientT};
use jsonrpsee::core::params::ObjectParams;
use jsonrpsee::ws_client::WsClient;
use plexus_core::plexus::schema::untyped_sites;
use plexus_core::plexus::{DynamicHub, PluginSchema};
use plexus_transport::TransportServer;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

async fn free_port() -> u16 {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    listener.local_addr().unwrap().port()
}

/// The service as `main` builds it, behind the gate, on a throwaway database.
async fn start_server() -> u16 {
    let dir = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).expect("tempdir");
    let db = dir.path().join("tickets.db");
    let port = free_port().await;
    let login = support::Login::new();
    let service = support::service(&db, &login).await;
    let gate = login.gate(port);
    let conv = |arc: Arc<DynamicHub>| menger::calls::rpc_module(arc, Default::default()).map_err(|e| anyhow::anyhow!("{e}"));
    tokio::spawn(async move {
        let _keep = dir;
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
    port
}

/// One call's first item: a data item's content.
async fn one(c: &WsClient, method: &str) -> Value {
    let mut p = ObjectParams::new();
    p.insert("method", method).unwrap();
    p.insert("params", json!({})).unwrap();
    let mut s: Subscription<Value> = c.subscribe("menger.call", p, "menger.call_unsub").await.unwrap();
    let item = tokio::time::timeout(Duration::from_secs(5), s.next()).await.unwrap_or_else(|_| panic!("{method}: no answer")).unwrap().unwrap();
    assert_eq!(item["type"], "data", "{method}: {item}");
    item["content"].clone()
}

/// Every service's schema by its path, walked from the root's children.
async fn schemas(c: &WsClient) -> BTreeMap<String, Value> {
    let child_paths = |path: Option<&str>, schema: &Value| -> Vec<String> {
        schema["children"]
            .as_array()
            .cloned()
            .unwrap_or_default()
            .iter()
            .map(|ch| {
                let ns = ch["namespace"].as_str().unwrap_or_else(|| panic!("a child's namespace: {ch}"));
                path.map_or(ns.to_string(), |p| format!("{p}.{ns}"))
            })
            .collect()
    };
    let mut todo = child_paths(None, &one(c, "menger.schema").await);
    let mut out = BTreeMap::new();
    while let Some(path) = todo.pop() {
        let schema = one(c, &format!("{path}.schema")).await;
        todo.extend(child_paths(Some(&path), &schema));
        out.insert(path, schema);
    }
    out
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn no_service_has_an_untyped_site() {
    let port = start_server().await;
    let c = support::connect(port).await;
    let all = schemas(&c).await;
    let paths: Vec<&str> = all.keys().map(String::as_str).collect();
    for p in ["tickets", "tickets.commits", "tickets.nav", "tickets.inputs", "ui", "ui.slots", "media", "docs", "quota", "orgs", "members", "members.teams", "session"] {
        assert!(paths.contains(&p), "{p} not walked: {paths:?}");
    }
    let mut found = Vec::new();
    for (path, value) in &all {
        let schema: PluginSchema = serde_json::from_value(value.clone()).unwrap_or_else(|e| panic!("{path}.schema is no PluginSchema: {e}"));
        for site in untyped_sites(&schema) {
            found.push(format!("{path}: {site}"));
        }
    }
    assert_eq!(found, Vec::<String>::new(), "give each a type (SYNAPSE-CC-6)");

    // The check reads what the wire answers: a v2 schema with one method's
    // `returns` made a bare `object` is found, by its site.
    let mut bent = all["quota"].clone();
    let m = bent["methods"].as_array_mut().and_then(|ms| ms.iter_mut().find(|m| m["name"] == "usage")).expect("quota.usage");
    m["returns"] = json!({ "type": "object" });
    let bent: PluginSchema = serde_json::from_value(bent).expect("still a PluginSchema");
    assert!(untyped_sites(&bent).iter().any(|s| s.starts_with("quota.usage returns")), "a bare object went unseen: {:?}", untyped_sites(&bent));
}
