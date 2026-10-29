//! V2-4: the tickets service's surface, over the socket. A v2 server
//! registers `tickets` and the addresses, the grants its reach reads and the
//! browser's sign-in — `members` (with `teams`), `orgs`, `epics`, `session`
//! — beside the protocol's `auth`, and nothing else; `tickets`' children are
//! `commits`, `nav`, `work`, `filters`, `subtasks` and `revisions`, and
//! nothing else (inputs, components, the queue and assign went with their
//! services, and so did their methods and parameters). A body is text: what
//! v1 parsed out of it is no longer read, so nothing in it refuses a write.
//! A filter naming a cell of a service that went is refused, naming it —
//! asked directly, or through a saved filter kept from v1.
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
    let db = dir.path().join("tickets.db");
    serve(db, dir).await
}

/// A server on `db`; `dir` is kept as long as it serves.
async fn serve(db: std::path::PathBuf, dir: tempfile::TempDir) -> u16 {
    let port = free_port().await;
    let login = support::Login::new();
    let hub = support::hub(&db, &login).await;
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

/// One call's items: each data item's content, an error item whole (and
/// the end there).
async fn call(c: &WsClient, method: &str, params: Value) -> Vec<Value> {
    let mut p = ObjectParams::new();
    p.insert("method", method).unwrap();
    p.insert("params", params).unwrap();
    let mut s: Subscription<Value> = c.subscribe("menger.call", p, "menger.call_unsub").await.unwrap();
    let mut out = Vec::new();
    while let Ok(Some(Ok(item))) = tokio::time::timeout(Duration::from_secs(5), s.next()).await {
        match item["type"].as_str() {
            Some("data") => out.push(item["content"].clone()),
            Some("error") => {
                out.push(item);
                break;
            }
            _ => break,
        }
    }
    out
}

/// One call's one answer.
async fn one(c: &WsClient, method: &str, params: Value) -> Value {
    let items = call(c, method, params).await;
    let item = items.first().unwrap_or_else(|| panic!("{method}: no answer")).clone();
    assert_ne!(item["type"], "error", "{method}: {item}");
    item
}

/// The refusal a call answers — an error item, or an in-band `error`
/// event — as its message.
async fn refusal(c: &WsClient, method: &str, params: Value) -> String {
    let items = call(c, method, params.clone()).await;
    let e = items.iter().find(|i| i["type"] == "error").unwrap_or_else(|| panic!("{method} {params}: refused, but answered {items:?}"));
    e["message"].as_str().unwrap_or_default().to_string()
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

/// A schema's methods, by name.
fn methods(schema: &Value) -> BTreeSet<String> {
    schema["methods"].as_array().unwrap().iter().filter_map(|m| m["name"].as_str().map(str::to_string)).collect()
}

/// One method's parameter names.
fn params(schema: &Value, method: &str) -> BTreeSet<String> {
    let m = schema["methods"].as_array().unwrap().iter().find(|m| m["name"] == method).unwrap_or_else(|| panic!("{method} in {schema}"));
    m["params"]["properties"].as_object().map(|o| o.keys().cloned().collect()).unwrap_or_default()
}

fn set(xs: &[&str]) -> BTreeSet<String> {
    xs.iter().map(|s| s.to_string()).collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_service_registers_tickets_members_orgs_epics_and_nothing_else() {
    let port = start_server().await;
    let c = support::connect(port).await;

    let hub = children(&one(&c, "menger.schema", json!({})).await);
    let tickets_schema = one(&c, "tickets.schema", json!({})).await;
    let tickets = children(&tickets_schema);
    assert_eq!(
        (hub, tickets),
        (
            set(&["auth", "tickets", "members", "orgs", "epics", "session"]),
            set(&["commits", "nav", "work", "filters", "subtasks", "revisions"])
        ),
        "(the hub's activations, tickets' children)"
    );
    // `members` comes back with `teams`, and without `profiles` (the
    // profiles service's).
    assert_eq!(children(&one(&c, "members.schema", json!({})).await), set(&["teams"]), "members' children");

    // The queue's methods and `assign` went with their services…
    let have = methods(&tickets_schema);
    let gone: Vec<&str> = ["queue", "await_reply", "request_changes", "queued", "snooze", "unqueue", "assign"].into_iter().filter(|m| have.contains(*m)).collect();
    assert!(gone.is_empty(), "tickets still has {gone:?}");
    // …and so did the parameters that served them.
    assert!(!params(&tickets_schema, "create").contains("chat"), "create takes no chat: {:?}", params(&tickets_schema, "create"));
    let many = params(&tickets_schema, "update_many");
    assert!(!many.contains("assign") && !many.contains("unassign"), "update_many takes no assign: {many:?}");
}

/// A body is text: two inputs with one id and a `theme` block the theme
/// registry would refuse were refusals on v1, and are a body like any other
/// on v2 — created, and read back as written.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_body_is_text() {
    let port = start_server().await;
    let c = support::connect(port).await;
    let decision = "<m-decision id=\"a\"><m-question>Pick?</m-question><m-option name=\"Yes\" value=\"yes\"></m-option></m-decision>\n";
    let body = format!("{decision}{decision}\n```theme broken extends=\n:root {{ not-a-token: ; }}\n```\n");
    one(&c, "tickets.create", json!({"epic": "TXT", "title": "root", "kind": "scope"})).await;
    let made = one(&c, "tickets.create", json!({"epic": "TXT", "title": "a body", "body": body})).await;
    assert_eq!(made["type"], "created", "{made}");
    let got = one(&c, "tickets.get", json!({"id": "TXT-2"})).await;
    assert_eq!(got["ticket"]["body"], body, "{got}");
}

/// A cell of a service that went is refused, naming it: asked directly…
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_dropped_cell_is_refused_over_the_wire() {
    let port = start_server().await;
    let c = support::connect(port).await;
    let why = refusal(&c, "tickets.list", json!({"filter": "chatting"})).await;
    assert!(why.contains("chatting"), "{why}");
}

/// …or through a saved filter v1 kept, which the language no longer reads.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_saved_filter_holding_a_dropped_cell_is_refused_when_used() {
    let dir = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).expect("tempdir");
    let db = dir.path().join("tickets.db");
    // The file, with a saved filter as v1 would have kept it: saved as a
    // filter that reads, then made v1's text by hand.
    {
        let login = support::Login::new();
        let hub = support::hub(&db, &login).await;
        let admin = plexus_core::plexus::AuthContext::for_principal(&support::admin().parse().unwrap(), "v2", vec![], json!({"name": "admin"}));
        use futures::StreamExt;
        let items: Vec<_> = hub
            .route_with_ctx("tickets.filters.save", json!({"name": "Held", "filter": "blocked"}), Some(&admin), None)
            .await
            .expect("routed")
            .collect()
            .await;
        let items = serde_json::to_string(&items).unwrap();
        assert!(items.contains("Held") && !items.contains("\"error\""), "{items}");
    }
    let n = rusqlite::Connection::open(&db).unwrap().execute("UPDATE saved_filters SET filter = 'queued' WHERE slug = 'held'", []).unwrap();
    assert_eq!(n, 1);

    let port = serve(db, dir).await;
    let c = support::connect(port).await;
    let why = refusal(&c, "tickets.list", json!({"filter": "saved Held"})).await;
    assert!(why.contains("queued"), "{why}");
}
