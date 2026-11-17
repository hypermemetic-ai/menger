//! LOGS-2: `logs` over the real protocol — a real TransportServer, a real
//! WebSocket client, the `menger.call` envelope, the service as `main`
//! builds it (`support::service`: the logs service beside `tickets`, its
//! file beside the tickets file, its seam asking the real tickets service
//! as the caller): define → append → describe; a viewer refused append and
//! reading; an outsider shown no log; a deprecated log refusing; a bad
//! schema and a bad entry among many making nothing; the entries counted on
//! the writer's `logs.bytes`; an admin of the ticket alone defining; and a
//! log following its ticket to another shortcode.

use jsonrpsee::ws_client::WsClient;
use plexus_rpc::core::plexus::DynamicHub;
use plexus_rpc::transport::TransportServer;
use serde_json::{json, Value};
use std::sync::Arc;

mod support;

/// An editor on `A`.
const EDITOR: [u8; 32] = [0xd1; 32];
/// A viewer on `A`.
const VIEWER: [u8; 32] = [0xd2; 32];
/// An editor on `B` alone — outside `A`.
const OUTSIDER: [u8; 32] = [0xd3; 32];
/// The admin of `A` alone.
const A_ADMIN: [u8; 32] = [0xd4; 32];

const PERF: &str = r#"{"type": "object", "required": ["app", "scenario", "value"], "properties": {
  "app": {"type": "string"}, "scenario": {"type": "string"}, "value": {"type": "number"},
  "machine": {"type": "string"}, "config": {"type": "object"}}}"#;

async fn start() -> u16 {
    let dir = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).expect("tempdir");
    let db_path = dir.path().join("tickets.db");
    let port = support::free_port().await;
    let login = support::Login::new();
    let service = support::service(&db_path, &login).await;
    let gate = login.gate(port);
    let conv = |arc: Arc<DynamicHub>| menger::calls::rpc_module(arc, Default::default()).map_err(|e| anyhow::anyhow!("{e}"));
    tokio::spawn(async move {
        let _keep = dir;
        TransportServer::builder(service, conv).with_websocket(port).with_session_validator(gate).anonymous_view().build().await.expect("build").serve().await.expect("serve");
    });
    port
}

async fn one(c: &WsClient, method: &str, params: Value) -> Value {
    let mut all = support::call(c, method, params).await;
    assert_eq!(all.len(), 1, "{method}: {all:?}");
    all.remove(0)
}

fn result(i: u64) -> Value {
    json!({"app": "plexus", "scenario": "stream.10k.64B", "value": 800_000 + i, "machine": "box", "config": {"window": 64}})
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_log_is_defined_appended_and_described_over_the_wire() {
    let port = start().await;
    let admin = support::connect(port).await;
    for (epic, title, kind) in [("A", "a", "scope"), ("A", "the runtime", "build"), ("B", "b", "scope"), ("P", "perf", "scope")] {
        let made = one(&admin, "tickets.create", json!({"epic": epic, "title": title, "kind": kind})).await;
        assert_eq!(made["type"], "created", "{made}");
    }
    for (seed, role, scope) in [(&EDITOR, "editor", "A"), (&VIEWER, "viewer", "A"), (&OUTSIDER, "editor", "B"), (&A_ADMIN, "admin", "A")] {
        let g = one(&admin, "auth.grant", json!({ "principal": support::principal_of(seed), "role": role, "scope": scope })).await;
        assert!(g.get("error_kind").is_none(), "{role}: {g}");
    }
    let (editor, viewer, outsider, a_admin) = (support::connect_as(&EDITOR, port).await, support::connect_as(&VIEWER, port).await, support::connect_as(&OUTSIDER, port).await, support::connect_as(&A_ADMIN, port).await);

    // A bad schema makes nothing, naming its place.
    let bad = one(&a_admin, "logs.define", json!({"name": "perf", "on": "A-2", "schema": r##"{"type": "object", "properties": {"a": {"$ref": "#/x"}}}"##})).await;
    assert_eq!((bad["type"].as_str(), bad["reason"].as_str(), bad["place"].as_str()), (Some("refused"), Some("bad_schema"), Some("/properties/a/$ref")), "{bad}");
    assert_eq!(one(&admin, "logs.describe", json!({"log": "perf"})).await, json!({"type": "not_found", "log": "perf"}));
    // Defining is the ticket's admin's: an editor there is refused by the
    // ticket; the admin of A alone (held nowhere else) defines.
    let no = one(&editor, "logs.define", json!({"name": "perf", "on": "A-2", "schema": PERF})).await;
    assert_eq!(no["error_kind"], "forbidden", "the gate: define is the admin's: {no}");
    let made = one(&a_admin, "logs.define", json!({"name": "perf", "on": "a-2", "schema": PERF})).await;
    assert_eq!((made["type"].as_str(), made["log"].as_str(), made["ticket"].as_str()), (Some("defined"), Some("perf"), Some("A-2")), "{made}");
    let uid = made["uid"].as_str().unwrap().to_string();
    assert_eq!(made["columns"].as_array().unwrap().len(), 5 + 5);
    let taken = one(&admin, "logs.define", json!({"name": "perf", "on": "B-1", "schema": PERF})).await;
    assert_eq!(taken["reason"], "name_taken", "{taken}");

    // Append: one, then many as JSON lines — the envelope the server's.
    let a = one(&editor, "logs.append", json!({"log": "perf", "entry": result(1).to_string(), "tags": ["plexus", "websocket"], "ticket": "A-1"})).await;
    assert_eq!((a["type"].as_str(), a["seq"].as_u64(), a["last"].as_u64(), a["count"].as_u64()), (Some("appended"), Some(1), Some(1), Some(1)), "{a}");
    let lines = (2..=4).map(|i| result(i).to_string()).collect::<Vec<_>>().join("\n");
    let many = one(&editor, "logs.append", json!({"log": "perf", "entries": lines, "tags": ["plexus"]})).await;
    assert_eq!((many["seq"].as_u64(), many["last"].as_u64()), (Some(2), Some(4)), "{many}");
    let usage = one(&editor, "quota.usage", json!({})).await;
    let logs = usage["sources"].as_array().unwrap().iter().find(|s| s["source"] == "logs.bytes").unwrap_or_else(|| panic!("{usage}"));
    assert_eq!(logs["used"].as_u64(), Some(a["bytes"].as_u64().unwrap() + many["bytes"].as_u64().unwrap()), "the writer's: {logs}");

    // A bad entry among many: none kept, its index and field named.
    let mut twenty: Vec<Value> = (0..20).map(result).collect();
    twenty[8]["value"] = json!("fast");
    let lines = twenty.iter().map(Value::to_string).collect::<Vec<_>>().join("\n");
    let r = one(&editor, "logs.append", json!({"log": "perf", "entries": lines})).await;
    assert_eq!((r["reason"].as_str(), r["index"].as_u64(), r["field"].as_str()), (Some("bad_entry"), Some(8), Some("value")), "{r}");

    // A viewer reads and may not append; an outsider is shown nothing.
    let no = one(&viewer, "logs.append", json!({"log": "perf", "entry": result(9).to_string()})).await;
    assert_eq!((no["error_kind"].as_str(), no["grant"].as_str()), (Some("forbidden"), Some("logs.append")), "the gate: {no}");
    let no = one(&outsider, "logs.append", json!({"log": "perf", "entry": result(9).to_string()})).await;
    assert_eq!(no, json!({"type": "not_found", "log": "perf"}), "an editor elsewhere: the log is not theirs to see");
    let d = one(&viewer, "logs.describe", json!({"log": "perf"})).await;
    assert_eq!((d["type"].as_str(), d["ticket"].as_str(), d["entries"].as_u64(), d["uid"].as_str()), (Some("described"), Some("A-2"), Some(4), Some(uid.as_str())), "{d}");
    assert_eq!(d["schema"], serde_json::from_str::<Value>(PERF).unwrap());
    let seen = support::call(&viewer, "logs.list", json!({})).await;
    assert_eq!(seen.iter().map(|l| l["log"].as_str().unwrap()).collect::<Vec<_>>(), ["perf"], "{seen:?}");
    assert!(support::call(&outsider, "logs.list", json!({})).await.is_empty(), "an outsider is not shown the log");
    assert_eq!(one(&outsider, "logs.describe", json!({"log": "perf"})).await, json!({"type": "not_found", "log": "perf"}));

    // Deprecated: no new entry, what was written still counted.
    let dep = one(&a_admin, "logs.deprecate", json!({"log": "perf", "message": "replaced by perf2"})).await;
    assert_eq!(dep, json!({"type": "deprecated", "log": "perf", "message": "replaced by perf2"}));
    let r = one(&editor, "logs.append", json!({"log": "perf", "entry": result(5).to_string()})).await;
    assert_eq!(r["reason"], "deprecated", "{r}");
    assert!(r["message"].as_str().unwrap().contains("replaced by perf2"), "{r}");
    let d = one(&viewer, "logs.describe", json!({"log": "perf"})).await;
    assert_eq!((d["entries"].as_u64(), d["deprecated"].as_str()), (Some(4), Some("replaced by perf2")), "{d}");

    // The ticket moves to another shortcode: the log follows, by its uid.
    let moved = one(&admin, "tickets.reshortcode", json!({"ids": ["A-2"], "to": "P"})).await;
    assert!(moved.get("error_kind").is_none() && moved["type"] != "error", "{moved}");
    let d = one(&admin, "logs.describe", json!({"log": "perf"})).await;
    assert_eq!((d["ticket"].as_str(), d["uid"].as_str()), (Some("P-2"), Some(uid.as_str())), "{d}");
}
