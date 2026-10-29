//! V2-4: the tickets service's surface, over the socket. A v2 server
//! registers `tickets` and the addresses and grants its reach reads —
//! `members` (with `teams`), `orgs`, `epics` — beside the protocol's `auth`,
//! and nothing else; `tickets`' children are `commits`, `nav`, `work`,
//! `filters`, `subtasks` and `revisions`, and nothing else (inputs,
//! components, the queue and assign went with their services).
//!
//! Signed in as the fixture operator (`--admin`, admin on `*`), so the
//! view is the whole hub; its own throwaway database and a free port.

mod support;
use jsonrpsee::core::client::{Subscription, SubscriptionClientT};
use jsonrpsee::core::params::ObjectParams;
use jsonrpsee::ws_client::WsClient;
use plexus_core::plexus::DynamicHub;
use plexus_transport::TransportServer;
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::Duration;

async fn free_port() -> u16 {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    listener.local_addr().unwrap().port()
}

/// A server as the wire reference starts one: the hub as `main` builds it,
/// behind the gate, on a throwaway database.
async fn start_server() -> u16 {
    let dir = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).expect("tempdir");
    let port = free_port().await;
    let login = support::Login::new();
    let hub = support::hub(&dir.path().join("tickets.db"), &login).await;
    let gate = login.gate(port);
    let conv = |arc: Arc<DynamicHub>| menger::calls::rpc_module(arc, Default::default()).map_err(|e| anyhow::anyhow!("{e}"));
    tokio::spawn(async move {
        let _keep = dir;
        TransportServer::builder(hub, conv)
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

/// One call's one answer.
async fn one(c: &WsClient, method: &str, params: Value) -> Value {
    let mut p = ObjectParams::new();
    p.insert("method", method).unwrap();
    p.insert("params", params).unwrap();
    let mut s: Subscription<Value> = c.subscribe("menger.call", p, "menger.call_unsub").await.unwrap();
    let item = tokio::time::timeout(Duration::from_secs(5), s.next()).await.expect("an answer in time").expect("an item").unwrap();
    assert_eq!(item["type"], "data", "{method}: {item}");
    item["content"].clone()
}

/// The namespaces a schema lists as its children.
fn children(schema: &Value) -> BTreeSet<String> {
    schema["children"]
        .as_array()
        .unwrap_or_else(|| panic!("a schema with children: {schema}"))
        .iter()
        .map(|c| c["namespace"].as_str().unwrap_or_else(|| panic!("a child's namespace: {c}")).to_string())
        .collect()
}

fn set(xs: &[&str]) -> BTreeSet<String> {
    xs.iter().map(|s| s.to_string()).collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_service_registers_tickets_members_orgs_epics_and_nothing_else() {
    let port = start_server().await;
    let c = support::connect(port).await;

    let hub = children(&one(&c, "menger.schema", json!({})).await);
    let tickets = children(&one(&c, "tickets.schema", json!({})).await);
    assert_eq!(
        (hub, tickets),
        (
            set(&["auth", "tickets", "members", "orgs", "epics"]),
            set(&["commits", "nav", "work", "filters", "subtasks", "revisions"])
        ),
        "(the hub's activations, tickets' children)"
    );
    // `members` comes back with `teams`, and without `profiles` (the
    // profiles service's).
    assert_eq!(children(&one(&c, "members.schema", json!({})).await), set(&["teams"]), "members' children");
}
