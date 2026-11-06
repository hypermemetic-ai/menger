//! V2-4: the tickets service's surface, over the socket. A v2 server
//! registers `tickets` and the addresses, the grants its reach reads and the
//! browser's sign-in — `members` (with `teams`), `orgs`, `session` (V2-25:
//! `epics` went; the board is the organization's whole set)
//! — beside the protocol's `auth`, (UI-3) the `ui` service, (V2-24) the
//! `media` service and (V2-29) the `docs` service and (V2-31) the `quota` service, and nothing else; `tickets`' children are
//! `commits`, `nav`, `work`, `filters`, `subtasks` and `revisions`, and
//! nothing else (components and assign went with their services, and so did
//! their methods and parameters; V2-20: inputs and the queue came back). A body is text: what
//! v1 parsed out of it is no longer read, so nothing in it refuses a write.
//! A filter naming a cell of a service that went is refused, naming it —
//! asked directly, or through a saved filter kept from v1.
//!
//! Signed in as the fixture operator (`--admin`, admin on `*`), so the
//! view is the whole service; its own throwaway database and a free port.

#[path = "support/ports.rs"]
mod ports;
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
    ports::free_port()
}

/// A server as the wire reference starts one: the service as `main` builds it,
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
            // PERF-15: a page's `rows` read as the per-row events it carries.
            Some("data") => match item["content"].clone() {
                c if c["type"] == "rows" => out.extend(c["items"].as_array().cloned().unwrap_or_default()),
                c => out.push(c),
            },
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
async fn the_service_registers_tickets_members_orgs_session_and_nothing_else() {
    let port = start_server().await;
    let c = support::connect(port).await;

    let service = children(&one(&c, "menger.schema", json!({})).await);
    let tickets_schema = one(&c, "tickets.schema", json!({})).await;
    let tickets = children(&tickets_schema);
    assert_eq!(
        (service, tickets),
        (
            set(&["auth", "tickets", "members", "orgs", "session", "ui", "media", "docs", "quota"]),
            set(&["commits", "inputs", "nav", "work", "filters", "subtasks", "revisions"])
        ),
        "(the service's activations, tickets' children)"
    );
    // `members` comes back with `teams`, and without `profiles` (the
    // profiles service's).
    assert_eq!(children(&one(&c, "members.schema", json!({})).await), set(&["teams"]), "members' children");

    // V2-20: the queue's methods are back…
    let have = methods(&tickets_schema);
    let back: Vec<&str> = ["queue", "await_reply", "request_changes", "queued", "snooze", "unqueue"].into_iter().filter(|m| !have.contains(*m)).collect();
    assert!(back.is_empty(), "tickets lacks {back:?}");
    // …and `assign` went with its service…
    assert!(!have.contains("assign"), "tickets still has assign");
    // …and so did the parameters that served them.
    assert!(!params(&tickets_schema, "create").contains("chat"), "create takes no chat: {:?}", params(&tickets_schema, "create"));
    let many = params(&tickets_schema, "update_many");
    assert!(!many.contains("assign") && !many.contains("unassign"), "update_many takes no assign: {many:?}");
}

/// A body is text but for its inputs: a `theme` block the theme registry
/// would refuse was a refusal on v1, and is a body like any other on v2 —
/// created, and read back as written; V2-20: two inputs with one id are
/// refused again, naming the input.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_body_is_text() {
    let port = start_server().await;
    let c = support::connect(port).await;
    let decision = "<m-decision id=\"a\"><m-question>Pick?</m-question><m-option name=\"Yes\" value=\"yes\"></m-option></m-decision>\n";
    let body = format!("{decision}\n```theme broken extends=\n:root {{ not-a-token: ; }}\n```\n");
    one(&c, "tickets.create", json!({"epic": "TXT", "title": "root", "kind": "scope"})).await;
    let made = one(&c, "tickets.create", json!({"epic": "TXT", "title": "a body", "body": body})).await;
    assert_eq!(made["type"], "created", "{made}");
    let got = one(&c, "tickets.get", json!({"id": "TXT-2"})).await;
    assert_eq!(got["ticket"]["body"], body, "{got}");
    let why = refusal(&c, "tickets.create", json!({"epic": "TXT", "title": "twice", "body": format!("{decision}\n{decision}")})).await;
    assert!(why.contains("'a' twice"), "{why}");
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
        let service = support::service(&db, &login).await;
        let admin = plexus_core::plexus::AuthContext::for_principal(&support::admin().parse().unwrap(), "v2", vec![], json!({"name": "admin"}));
        use futures::StreamExt;
        let items: Vec<_> = service
            .route_with_ctx("tickets.filters.save", json!({"name": "Held", "filter": "blocked"}), Some(&admin), None)
            .await
            .expect("routed")
            .collect()
            .await;
        let items = serde_json::to_string(&items).unwrap();
        assert!(items.contains("Held") && !items.contains("\"error\""), "{items}");
    }
    let n = rusqlite::Connection::open(&db).unwrap().execute("UPDATE saved_filters SET filter = 'chatting' WHERE slug = 'held'", []).unwrap();
    assert_eq!(n, 1);

    let port = serve(db, dir).await;
    let c = support::connect(port).await;
    let why = refusal(&c, "tickets.list", json!({"filter": "saved Held"})).await;
    assert!(why.contains("chatting"), "{why}");
}

/// One method's `returns`: its event's variants, by their `type`.
fn variants<'a>(schema: &'a Value, method: &str) -> Vec<&'a Value> {
    let m = schema["methods"].as_array().unwrap().iter().find(|m| m["name"] == method).unwrap_or_else(|| panic!("{method} in the schema"));
    m["returns"]["oneOf"].as_array().unwrap_or_else(|| panic!("{method} returns a oneOf: {}", m["returns"])).iter().collect()
}

/// A variant's `type`.
fn tag(v: &Value) -> &str {
    v["properties"]["type"]["const"].as_str().or_else(|| v["properties"]["type"]["enum"][0].as_str()).unwrap_or_default()
}

/// PERF-15: the schema says a page is one event — every paged read
/// (`list`, `work.ready`, `nav.tree`, `find`, `search`) and the board's
/// listings (`nav.groups`, `nav.extents`, `nav.counts`) return a `rows`
/// variant whose `items` are the per-row events as they were (`summary`,
/// `ticket`, `node`, `hit`, `group`, `extent`, `count`), each by its `type`.
/// The schema's hash moves with it, and the bundle's contract with that.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_page_is_one_rows_event_in_the_schema() {
    let port = start_server().await;
    let c = support::connect(port).await;
    let tickets = one(&c, "tickets.schema", json!({})).await;
    let nav = one(&c, "tickets.nav.schema", json!({})).await;
    let work = one(&c, "tickets.work.schema", json!({})).await;
    for (schema, method) in [(&tickets, "list"), (&tickets, "find"), (&tickets, "search"), (&work, "ready"), (&nav, "tree"), (&nav, "groups"), (&nav, "extents"), (&nav, "counts")] {
        let vs = variants(schema, method);
        let rows = vs.iter().find(|v| tag(v) == "rows").unwrap_or_else(|| panic!("{method}: no `rows` variant among {:?}", vs.iter().map(|v| tag(v)).collect::<Vec<_>>()));
        let required: BTreeSet<String> = rows["required"].as_array().unwrap().iter().map(|r| r.as_str().unwrap().to_string()).collect();
        assert_eq!(required, set(&["type", "items"]), "{method}: rows {{ items }}: {rows}");
        assert_eq!(rows["properties"]["items"]["type"], "array", "{method}: items is a list: {rows}");
        // The item: a `$ref` to the one row type, or the variants inline.
        let item = &rows["properties"]["items"]["items"];
        let defs = &schema["methods"].as_array().unwrap().iter().find(|m| m["name"] == method).unwrap()["returns"]["$defs"];
        let item = match item["$ref"].as_str() {
            Some(r) => &defs[r.trim_start_matches("#/$defs/")],
            None => item,
        };
        let kinds: BTreeSet<String> = item["oneOf"].as_array().unwrap_or_else(|| panic!("{method}: an item is one of the row events: {item}")).iter().map(|v| tag(v).to_string()).collect();
        assert_eq!(kinds, set(&["summary", "ticket", "node", "hit", "group", "extent", "count", "queue_row", "input"]), "{method}: the items are the per-row events");
        // The trailer each page ends with is still its own event.
        let trailer = match method { "search" => Some("searched"), "list" | "find" | "ready" | "tree" => Some("page"), _ => None };
        if let Some(t) = trailer {
            assert!(vs.iter().any(|v| tag(v) == t), "{method}: {t} still among its variants");
        }
    }
    // A watch's changes are each their own event.
    let watch = variants(&tickets, "watch");
    for t in ["created", "status_changed", "deleted"] {
        assert!(watch.iter().any(|v| tag(v) == t), "watch: {t} among its variants");
    }
    // `fields`: a list of the summary row's field names.
    let list = tickets["methods"].as_array().unwrap().iter().find(|m| m["name"] == "list").unwrap();
    let fields = &list["params"]["properties"]["fields"];
    assert!(fields.to_string().contains("SummaryField") || fields.to_string().contains("\"title\""), "list takes fields of the summary row: {fields}");
}

/// PERF-15: the summary row is its fields — the schema's `TicketSummary`
/// properties, written out, so thinning the row is a deliberate edit here
/// (a page thins by `fields`, never by trimming the row); and `fields`
/// names exactly these.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_summary_row_is_its_fields() {
    let port = start_server().await;
    let c = support::connect(port).await;
    let tickets = one(&c, "tickets.schema", json!({})).await;
    let list = tickets["methods"].as_array().unwrap().iter().find(|m| m["name"] == "list").unwrap();
    let defs = &list["returns"]["$defs"];
    let row: BTreeSet<String> = defs["TicketSummary"]["properties"].as_object().unwrap_or_else(|| panic!("TicketSummary in {}", list["returns"])).keys().cloned().collect();
    let written = set(&[
        // TICKETS-35 (V2-42): `uid`, and `uid_of` — the uids of what it names.
        "uid", "uid_of", "id", "epic", "shortcode", "address", "seq", "title", "status", "kind", "severity", "parent", "parent_hidden", "child_count", "blocked_by", "unlocks",
        "hidden_blocked_by", "hidden_unlocks", "commits", "path", "placement", "labels", "archived", "frozen", "hidden", "created_unix", "updated_unix",
    ]);
    assert_eq!(row, written, "the summary row's fields");
    let named: BTreeSet<String> = menger_tickets_api::vocab::SummaryField::ALL.iter().map(|f| f.as_str().to_string()).collect();
    assert_eq!(named, written, "fields names exactly the summary row's keys");
}
