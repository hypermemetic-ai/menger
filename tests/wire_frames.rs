//! COMP-16 over the socket: `tickets.frame_reach { id }` answers what a
//! frame of a ticket's body may read for its caller — the caller's reach ∩
//! the reach of who last wrote the body ∩ what the body asks
//! (`reach-read:`) — and `get`, `list` and `search` given `frame` read under
//! exactly that bound. Its refusals: a ticket the caller cannot see is
//! `not_found`; a caller holding nothing is refused by the gate.
//!
//! The fixture operator (`--admin`) opens a body written by an editor of
//! DESIGN alone that asks `under PLEXUS-1`, and reads nothing outside
//! DESIGN; the operator's own amend of it reads PLEXUS. Its own throwaway
//! database and a free port.

#[path = "support/ports.rs"]
mod ports;
mod support;
use plexus_rpc::core::plexus::DynamicHub;
use plexus_rpc::transport::TransportServer;
use serde_json::{json, Value};
use std::sync::Arc;
use support::call;

/// The editor of DESIGN alone.
const DESIGNER: [u8; 32] = [0x41; 32];
/// A key that holds nothing.
const STRANGER: [u8; 32] = [0x42; 32];

const WIDE: &str = "---\nreach-read: under PLEXUS-1\n---\nlists every plan it can\n";

async fn start_server() -> u16 {
    let dir = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).expect("tempdir");
    let db = dir.path().join("tickets.db");
    let port = ports::free_port();
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

async fn one(c: &jsonrpsee::ws_client::WsClient, method: &str, params: Value) -> Value {
    let items = call(c, method, params.clone()).await;
    let item = items.first().unwrap_or_else(|| panic!("{method} {params}: no answer")).clone();
    assert!(item["type"] != "error", "{method} {params}: {item}");
    item
}

/// The ids a read's items carry (`summary`, `ticket`, `hit`), in order.
fn ids(items: &[Value]) -> Vec<String> {
    items
        .iter()
        .filter_map(|i| match i["type"].as_str() {
            Some("summary") => i["summary"]["id"].as_str(),
            Some("ticket") => i["ticket"]["id"].as_str(),
            Some("hit") => i["summary"]["id"].as_str(),
            _ => None,
        })
        .map(str::to_string)
        .collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn frame_reach_answers_the_bound_and_framed_reads_keep_to_it() {
    let port = start_server().await;
    let admin = support::connect(port).await;
    for (epic, kind, parent) in [("PLEXUS", "scope", None), ("DESIGN", "scope", Some("PLEXUS-1")), ("IDENTITY", "scope", Some("PLEXUS-1")), ("SECRET", "scope", None)] {
        one(&admin, "tickets.create", json!({ "epic": epic, "title": format!("{epic} plan"), "kind": kind, "parent": parent })).await;
    }
    one(&admin, "members.grant", json!({ "principal": support::principal_of(&DESIGNER), "scope": "DESIGN", "role": "editor" })).await;
    let designer = support::connect_as(&DESIGNER, port).await;
    let made = one(&designer, "tickets.create", json!({ "epic": "DESIGN", "title": "DESIGN plan board", "parent": "DESIGN-1", "body": WIDE })).await;
    assert_eq!(made["ticket"]["id"], "DESIGN-2", "{made}");

    // The bound: the designer's reach, though the operator asks and the body asks PLEXUS.
    let r = one(&admin, "tickets.frame_reach", json!({ "id": "DESIGN-2" })).await;
    assert_eq!(r["type"], "frame_reach", "{r}");
    let reach = &r["reach"];
    assert_eq!((reach["ticket"].clone(), reach["author"].clone(), reach["asked"].clone()), (json!("DESIGN-2"), json!(support::principal_of(&DESIGNER)), json!("under PLEXUS-1")), "{r}");
    assert_eq!((reach["shortcodes"].clone(), reach["tickets"].clone(), reach["alone"].clone()), (json!(["DESIGN"]), json!(2), Value::Null), "{r}");

    // The framed reads keep to it: a list, a get, a search.
    let listed = call(&admin, "tickets.list", json!({ "summary": true, "frame": "DESIGN-2" })).await;
    assert_eq!(ids(&listed), ["DESIGN-1", "DESIGN-2"], "{listed:?}");
    let outside = one(&admin, "tickets.get", json!({ "id": "IDENTITY-1", "frame": "DESIGN-2" })).await;
    assert_eq!(outside["type"], "not_found", "{outside}");
    let inside = one(&admin, "tickets.get", json!({ "id": "DESIGN-1", "frame": "DESIGN-2" })).await;
    assert_eq!(inside["ticket"]["id"], "DESIGN-1", "{inside}");
    let found = call(&admin, "tickets.search", json!({ "query": "plan", "frame": "DESIGN-2" })).await;
    let mut hits = ids(&found);
    hits.sort();
    assert_eq!(hits, ["DESIGN-1", "DESIGN-2"], "{found:?}");
    // Unframed, the operator reads everything, as before.
    assert_eq!(ids(&call(&admin, "tickets.list", json!({ "summary": true })).await).len(), 5);

    // The operator's own amend makes it the author: the bound is what it asks.
    one(&admin, "tickets.amend", json!({ "id": "DESIGN-2", "body": format!("{WIDE}\nthe operator's\n") })).await;
    let r = one(&admin, "tickets.frame_reach", json!({ "id": "DESIGN-2" })).await;
    assert_eq!((r["reach"]["shortcodes"].clone(), r["reach"]["tickets"].clone()), (json!(["DESIGN", "IDENTITY", "PLEXUS"]), json!(4)), "{r}");
    // The designer, reading its own body's frame, reads its own reach.
    let r = one(&designer, "tickets.frame_reach", json!({ "id": "DESIGN-2" })).await;
    assert_eq!(r["reach"]["shortcodes"], json!(["DESIGN"]), "{r}");

    // Refusals: a ticket out of reach, or none, is not found — and so is a framed read naming one.
    let hidden = one(&designer, "tickets.frame_reach", json!({ "id": "SECRET-1" })).await;
    assert_eq!(hidden["type"], "not_found", "{hidden}");
    let none = one(&admin, "tickets.frame_reach", json!({ "id": "DESIGN-99" })).await;
    assert_eq!(none["type"], "not_found", "{none}");
    let named = one(&designer, "tickets.list", json!({ "summary": true, "frame": "SECRET-1" })).await;
    assert_eq!(named["type"], "not_found", "{named}");
    // A caller who holds nothing is refused before the method runs.
    let stranger = support::connect_as(&STRANGER, port).await;
    let refused = call(&stranger, "tickets.frame_reach", json!({ "id": "DESIGN-2" })).await;
    assert!(refused.first().is_some_and(|i| i["type"] == "error"), "{refused:?}");
    // A key no parameter claims is refused at dispatch (MACRO-31).
    let typo = call(&admin, "tickets.frame_reach", json!({ "id": "DESIGN-2", "frames": "x" })).await;
    assert!(typo.first().is_some_and(|i| i.to_string().contains("unknown field")), "{typo:?}");
}
