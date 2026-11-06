//! ACCESS-8's first acceptance line: the JSON on the wire does not change
//! by a byte. A script of calls over the real protocol — every activation,
//! the writes and the reads, the refusals, both watches — and every item it
//! answers is held two ways:
//!
//! 1. **round-trip**: each `data` item read as its activation's event type
//!    (`TicketsEvent`, `CommentsEvent`, …) and written again is the JSON it
//!    arrived as — so every wire struct and event the script reaches
//!    serializes as it deserializes, whatever its fields are typed as.
//!    Compared as JSON (PERF-7, the owner's `json-bytes`, 2026-10-30): the
//!    item on the wire is encoded once from its event, its keys in the
//!    type's order, and here it is written again through a `Value` — so the
//!    two are held equal as JSON, not as text. For every event the script
//!    reaches this is also PERF-7's property: the single encode is today's
//!    encode, as JSON;
//! 2. **golden**: the items, their clocks set to 0, equal
//!    `tests/fixtures/golden-wire.json` — written from main *before* ACCESS-8
//!    (`GOLDEN_WRITE=1 cargo test --test golden_wire`, on the parent commit)
//!    and never by the build it holds.
//!
//! The data is synthetic: the script makes it (DEPLOY-5: no snapshot of the
//! tracker is a fixture).
//!
//! A build that moves a method updates the fixture deliberately, its calls
//! only: ACCESS-16 moved `groups`, `extents`, `tree`, `counts`, `facets` and
//! `member` to `tickets.nav.<method>` — the six `call`s renamed, every item
//! they answer byte for byte as before. PERF-15 put a page's rows in one
//! `rows` event: the collector reads it as the rows it carries, so the fixture
//! did not move. TICKETS-35 (V2-42) grew the rows: every ticket and summary
//! the script answers gained `uid`, and those that name another ticket
//! `uid_of` — each uid written `<uid>`, being random — and nothing else moved.


/// ACCESS-24: the test provider — every server runs `key`, every client is its `--admin`.
mod support;
use futures::StreamExt;
use jsonrpsee::core::client::{Subscription, SubscriptionClientT};
use jsonrpsee::core::params::ObjectParams;
use jsonrpsee::ws_client::{WsClient, WsClientBuilder};
use menger_tickets_api::{MembersEvent, OrgsEvent, SessionEvent, TicketsEvent};
use plexus_core::plexus::DynamicHub;
use plexus_transport::TransportServer;
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::Duration;

/// A server on an isolated, throwaway database.
async fn start_server() -> u16 {
    let dir = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).expect("tempdir");
    let db_path = dir.path().join("tickets.db");
    let port = support::free_port().await; // CI-9: a port no test beside this one holds
    let login = support::Login::new();
    let service = support::service(&db_path, &login).await;
    let gate = login.gate(port);
    let conv = |arc: Arc<DynamicHub>| menger::calls::rpc_module(arc, Default::default()).map_err(|e| anyhow::anyhow!("{e}"));
    tokio::spawn(async move {
        let _keep = dir;
        TransportServer::builder(service, conv).with_websocket(port).with_session_validator(gate).anonymous_view().credential_cookies(["access_token"]).build().await.expect("build server").serve().await.expect("serve");
    });
    port
}

async fn connect(port: u16) -> WsClient {
    let url = format!("ws://127.0.0.1:{port}");
    for _ in 0..50 {
        if let Ok(c) = WsClientBuilder::default().set_headers(support::admin_headers(port)).build(&url).await {
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

/// A finite call's `data` items, until `done`.
async fn collect(mut sub: Subscription<Value>) -> Vec<Value> {
    let mut out = Vec::new();
    while let Some(item) = sub.next().await {
        let item = item.unwrap();
        match item["type"].as_str() {
            Some("done") => return out,
            // PERF-15: a page's `rows` round-trips as itself (the `Rows`
            // variant), then reads as the per-row events it carries, in place
            // — the fixture is the same items, byte for byte.
            Some("data") if item["content"]["type"] == "rows" => {
                round_trip("tickets", &item["content"]);
                out.extend(item["content"]["items"].as_array().cloned().unwrap_or_default());
            }
            Some("data") => out.push(item["content"].clone()),
            other => panic!("unexpected stream item type {other:?}: {item}"),
        }
    }
    panic!("stream ended without a done item");
}

/// An open watch's events, up to and including the first `last` says ends it.
async fn until(sub: &mut Subscription<Value>, last: fn(&Value) -> bool) -> Vec<Value> {
    let mut out = Vec::new();
    loop {
        let item = tokio::time::timeout(Duration::from_secs(10), sub.next()).await.expect("the watch said it").unwrap().unwrap();
        if item["type"] != "data" {
            continue;
        }
        let ev = item["content"].clone();
        let done = last(&ev);
        out.push(ev);
        if done {
            return out;
        }
    }
}

/// The JSON as `T` reads and writes it — what must equal what arrived.
fn again<T: serde::de::DeserializeOwned + serde::Serialize>(v: &Value) -> Value {
    let typed: T = serde_json::from_value(v.clone()).unwrap_or_else(|e| panic!("{e}: {v}"));
    serde_json::to_value(typed).unwrap()
}

/// Round-trip `v`, an item of `namespace`'s.
fn round_trip(namespace: &str, v: &Value) {
    let back = match namespace {
        "tickets" => again::<TicketsEvent>(v),
        "members" => again::<MembersEvent>(v),
        "orgs" => again::<OrgsEvent>(v),
        "session" => again::<SessionEvent>(v),
        other => panic!("no event type for {other}"),
    };
    assert_eq!(&back, v, "{namespace}: an item does not round-trip as JSON");
}

/// `v` with its clocks set to 0 and the binary's version blanked: what the
/// golden file holds. TICKETS-35: and every uid — minted at random — written
/// `<uid>`, a ticket's own and each its `uid_of` names.
fn steady(v: &Value) -> Value {
    match v {
        Value::Object(m) => Value::Object(
            m.iter()
                .map(|(k, x)| {
                    let clock = k.ends_with("_unix") || k.ends_with("_at") || matches!(k.as_str(), "at" | "since" | "unix_now" | "began");
                    let x = match x {
                        Value::String(_) if k == "uid" => json!("<uid>"),
                        Value::Object(named) if k == "uid_of" => Value::Object(named.keys().map(|n| (n.clone(), json!("<uid>"))).collect()),
                        Value::Number(_) if clock || k == "version" => json!(0),
                        Value::String(_) if k == "version" => json!(""),
                        Value::String(t) => json!(noted(t)),
                        x => steady(x),
                    };
                    (k.clone(), x)
                })
                .collect(),
        ),
        Value::Array(a) => Value::Array(a.iter().map(steady).collect()),
        x => x.clone(),
    }
}

/// TICKETS-35: each ticket or summary in `v` — an object with a `seq`, a
/// `status` and an `id`, at any depth.
fn every_row(v: &Value, f: &mut dyn FnMut(&Value)) {
    match v {
        Value::Object(m) => {
            if m.contains_key("seq") && m.contains_key("status") && m.contains_key("id") {
                f(v);
            }
            m.values().for_each(|x| every_row(x, f));
        }
        Value::Array(a) => a.iter().for_each(|x| every_row(x, f)),
        _ => {}
    }
}

/// A note made a ticket says when it was noted — `Noted 2026-10-15 13:56
/// UTC` — a clock in prose: written `Noted <when> UTC` here.
fn noted(text: &str) -> String {
    match text.find("Noted ") {
        Some(i) if text.len() >= i + 6 + 16 && text[i + 6..i + 6 + 16].bytes().all(|b| b.is_ascii_digit() || b" -:".contains(&b)) => {
            format!("{}Noted <when>{}", &text[..i], &text[i + 6 + 16..])
        }
        _ => text.to_string(),
    }
}

const DECISION: &str = "<m-decision data-field=\"decision\" id=\"landing\">\n\n\
    <m-question data-field=\"question\">Archive or delete?</m-question>\n\n\
    <m-option data-field=\"option\" name=\"Archive\" value=\"archive\" recommended></m-option>\n\
    <m-option data-field=\"option\" name=\"Delete\"></m-option>\n\n\
    </m-decision>\n";

const COMPONENT: &str = "Defines it.\n\n```component m-gold\n<template><b>gold</b></template>\n```\n";

/// The script: `(method, params)`, in order.
fn script() -> Vec<(&'static str, Value)> {
    vec![
        ("tickets.health", json!({})),
        ("session.whoami", json!({})),
        ("session.providers", json!({})),
        ("orgs.list", json!({})),
        ("members.mine", json!({})),
        // Writes.
        ("tickets.create", json!({"epic": "GOLD", "title": "the root", "kind": "scope"})),
        ("tickets.create", json!({"epic": "GOLD", "title": "asks", "body": DECISION})),
        ("tickets.create", json!({"epic": "GOLD", "title": "a gap", "kind": "gap", "severity": "High", "blocked_by": ["GOLD-2"]})),
        ("tickets.create", json!({"epic": "GOLD", "title": "the board", "kind": "doc", "path": "_MENGER/board"})),
        ("tickets.create", json!({"epic": "GOLD", "title": "defines", "body": COMPONENT})),
        ("tickets.create", json!({"epic": "PLAT", "title": "the platform", "kind": "scope"})),
        ("tickets.create", json!({"epic": "PLAT", "title": "work"})),
        ("tickets.set_status", json!({"id": "GOLD-2", "status": "Ready"})),
        ("tickets.set_status", json!({"id": "GOLD-2", "status": "Coding", "message": "claimed"})),
        ("tickets.set_kind", json!({"id": "GOLD-5", "kind": "spike"})),
        ("tickets.set_parent", json!({"id": "GOLD-3", "parent": "GOLD-2"})),
        ("tickets.amend", json!({"id": "GOLD-3", "title": "a gap, retitled"})),
        ("tickets.commits.link", json!({"id": "GOLD-2", "commits": ["77df9a0", "plexus-macros@4f7114a"]})),
        ("tickets.commits.unlink", json!({"id": "GOLD-2", "commits": ["77df9a0"]})),
        ("tickets.label", json!({"id": "GOLD-3", "add": ["needs-you", "p1"]})),
        ("tickets.update", json!({"id": "GOLD-3", "severity": "none", "unblock": ["GOLD-2"], "block": ["PLAT-2"]})),
        ("tickets.update_many", json!({"ids": ["GOLD-3", "PLAT-2"], "label": ["swept"]})),
        ("tickets.create", json!({"epic": "GOLD", "title": "a mistake"})),
        ("tickets.delete", json!({"ids": ["GOLD-6"], "message": "filed twice"})),
        ("tickets.undelete", json!({"ids": ["GOLD-6"]})),
        ("tickets.reshortcode", json!({"ids": ["GOLD-6"], "to": "PLAT"})),
        // Reads.
        ("tickets.get", json!({"id": "GOLD-2"})),
        ("tickets.get", json!({"id": "gold/_MENGER/board"})),
        ("tickets.get", json!({"id": "GOLD-6"})),
        ("tickets.list", json!({"epic": "GOLD"})),
        ("tickets.list", json!({"summary": true, "limit": 3})),
        ("tickets.list", json!({"filter": "not kind doc", "at": 1, "summary": true})),
        ("tickets.work.ready", json!({"summary": true})),
        ("tickets.find", json!({"query": "gap"})),
        ("tickets.search", json!({"query": "platform"})),
        ("tickets.nav.member", json!({"ids": ["GOLD-2", "NOPE-1"], "filter": "kind build"})),
        ("tickets.nav.groups", json!({})),
        ("tickets.nav.extents", json!({})),
        ("tickets.nav.counts", json!({"filter": "not kind doc"})),
        ("tickets.nav.facets", json!({"filter": "kind build"})),
        ("tickets.nav.tree", json!({"epic": "GOLD", "filter": "not kind doc"})),
        ("tickets.work.waves", json!({"epics": ["GOLD", "PLAT"]})),
        ("tickets.work.waves_under", json!({"id": "GOLD-1"})),
        ("tickets.work.waves_to", json!({"id": "GOLD-3"})),
        ("tickets.work.blockers", json!({"id": "GOLD-3"})),
        ("tickets.history", json!({"id": "GOLD-2"})),
        // Refusals in band.
        ("tickets.get", json!({"id": "GOLD-99"})),
        ("tickets.set_status", json!({"id": "GOLD-4", "status": "Ready"})),
        ("tickets.create", json!({"epic": "NEW", "title": "a build first"})),
        // The other activations — V2-4: the addresses and the grants.
        ("members.list", json!({})),
        ("orgs.list", json!({})),
        // ARCHIVE-2 (added deliberately, last, so nothing before it moves):
        // a dry run, an archive with its live work named, a filter that
        // names the archive, and the restore.
        ("tickets.archive", json!({"ids": ["PLAT-1"], "dry_run": true})),
        ("tickets.archive", json!({"ids": ["PLAT-2"], "message": "parked"})),
        ("tickets.list", json!({"filter": "archived", "summary": true})),
        ("tickets.get", json!({"id": "PLAT-2"})),
        ("tickets.restore", json!({"ids": ["PLAT-2"]})),
        // The watch's marker.
        ("tickets.amend", json!({"id": "GOLD-1", "body": "the end"})),
    ]
}

#[tokio::test]
async fn every_item_round_trips_and_the_json_is_what_it_was() {
    let port = start_server().await;
    let client = connect(port).await;
    let mut watch = call(&client, "tickets.watch", json!({})).await;
    let mut filtered = call(&client, "tickets.watch", json!({"filter": "not kind doc"})).await;
    tokio::time::sleep(Duration::from_millis(200)).await;

    let mut golden = Vec::new();
    for (method, params) in script() {
        let items = collect(call(&client, method, params.clone()).await).await;
        let namespace = method.split('.').next().unwrap();
        for item in &items {
            round_trip(namespace, item);
        }
        golden.push(json!({"call": method, "params": params, "items": items.iter().map(steady).collect::<Vec<_>>()}));
    }
    let marker: fn(&Value) -> bool = |ev| ev["type"] == "amended" && ev["ticket"]["id"] == "GOLD-1";
    for (name, sub, last, namespace) in [
        ("tickets.watch", &mut watch, marker, "tickets"),
        ("tickets.watch filtered", &mut filtered, marker, "tickets"),
    ] {
        let events = until(sub, last).await;
        for ev in &events {
            round_trip(namespace, ev);
        }
        golden.push(json!({"call": name, "items": events.iter().map(steady).collect::<Vec<_>>()}));
    }

    // TICKETS-35: every ticket and summary the script answered carries a
    // uid, and names its parent's in `uid_of` — whichever call or watch
    // carried it (the one place a row could go out without them).
    let mut rows = 0;
    every_row(&Value::Array(golden.clone()), &mut |row| {
        rows += 1;
        assert!(row["uid"].is_string(), "a row without its uid: {row}");
        if let Some(parent) = row["parent"].as_str() {
            assert!(row["uid_of"][parent].is_string(), "a row whose uid_of names not its parent {parent}: {row}");
        }
    });
    assert!(rows > 100, "the script answered {rows} rows");

    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/golden-wire.json");
    let got = serde_json::to_string_pretty(&golden).unwrap() + "\n";
    if std::env::var_os("GOLDEN_WRITE").is_some() {
        std::fs::write(&path, &got).unwrap();
        return;
    }
    let want = std::fs::read_to_string(&path).expect("tests/fixtures/golden-wire.json (GOLDEN_WRITE=1 on the parent commit writes it)");
    if let Some(out) = std::env::var_os("GOLDEN_GOT") {
        std::fs::write(out, &got).unwrap();
    }
    if got != want {
        let (g, w): (Vec<Value>, Vec<Value>) = (serde_json::from_str(&got).unwrap(), serde_json::from_str(&want).unwrap());
        for (a, b) in g.iter().zip(&w) {
            assert_eq!(a, b, "the JSON changed");
        }
        assert_eq!(g.len(), w.len(), "the script answered a different number of calls");
    }
}
