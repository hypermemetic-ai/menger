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
//! PERF-9 (spec/13): the same script on sockets that asked for msgpack —
//! every frame a binary message, a map with its field names and no
//! `jsonrpc`, read with rmp-serde — round-trips the same way and equals the
//! same fixture, as JSON: JSON → msgpack → JSON for every event the script
//! reaches.
//!
//! The data is synthetic: the script makes it (DEPLOY-5: no snapshot of the
//! tracker is a fixture).
//!
//! A build that moves a method updates the fixture deliberately, its calls
//! only: ACCESS-16 moved `groups`, `extents`, `tree`, `counts`, `facets` and
//! `member` to `tickets.nav.<method>` — the six `call`s renamed, every item
//! they answer byte for byte as before. TICKETS-35 grew the rows: every
//! ticket and summary the script answers gained `uid` (119 of them) and the
//! 102 that name another ticket `uid_of` — each uid written `<uid>`, being
//! random — and nothing else moved (the fixture minus those two keys is the
//! one before it, checked key for key when it was re-blessed).


/// ACCESS-24: the test provider — every server runs `key`, every client is its `--admin`.
mod support;
use futures::StreamExt;
use jsonrpsee::core::client::{Subscription, SubscriptionClientT};
use jsonrpsee::core::params::ObjectParams;
use jsonrpsee::ws_client::{WsClient, WsClientBuilder};
use menger_tickets_api::{CommentsEvent, DeploymentsEvent, MediaEvent, MembersEvent, NotesEvent, OrgsEvent, SessionEvent, ThemesEvent, TicketsEvent};
use plexus_core::plexus::DynamicHub;
use plexus_transport::TransportServer;
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::Duration;

/// A server on an isolated, throwaway database.
async fn start_server() -> u16 {
    let dir = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).expect("tempdir");
    let db_path = dir.path().join("tickets.db");
    let port = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap().local_addr().unwrap().port();
    let login = support::Login::new();
    let hub = support::hub(&db_path, &login).await;
    let gate = login.gate(port);
    let conv = |arc: Arc<DynamicHub>| menger::calls::rpc_module(arc, Default::default()).map_err(|e| anyhow::anyhow!("{e}"));
    tokio::spawn(async move {
        let _keep = dir;
        TransportServer::builder(hub, conv).with_websocket(port).with_session_validator(gate).anonymous_view().credential_cookies(["access_token"]).build().await.expect("build server").serve().await.expect("serve");
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
        "comments" => again::<CommentsEvent>(v),
        "notes" => again::<NotesEvent>(v),
        "media" => again::<MediaEvent>(v),
        "themes" => again::<ThemesEvent>(v),
        "members" => again::<MembersEvent>(v),
        "orgs" => again::<OrgsEvent>(v),
        "session" => again::<SessionEvent>(v),
        "deployments" => again::<DeploymentsEvent>(v),
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
        // ACCESS-14: the inputs at their paths under tickets.inputs — the
        // JSON they answer unchanged (the fixture's call names moved with them).
        ("tickets.inputs.answer", json!({"ticket": "GOLD-2", "input": "landing", "value": {"type": "choice", "option": "archive"}, "rationale": "holds"})),
        ("tickets.inputs.clear", json!({"ticket": "GOLD-2", "input": "landing"})),
        ("tickets.inputs.close", json!({"ticket": "GOLD-2", "input": "landing"})),
        ("tickets.inputs.reopen", json!({"ticket": "GOLD-2", "input": "landing"})),
        ("tickets.queue", json!({"id": "GOLD-2", "urgency": "blocking", "reason": "the import waits", "input": "landing"})),
        ("tickets.snooze", json!({"id": "GOLD-2", "minutes": 10})),
        ("tickets.queued", json!({})),
        ("tickets.unqueue", json!({"id": "GOLD-2"})),
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
        ("tickets.components.list", json!({})),
        ("tickets.inputs.list", json!({})),
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
        // The other activations.
        ("comments.add", json!({"anchor": {"route": "#/ticket/GOLD-2", "path": "main > h1", "label": "Title", "excerpt": "asks", "ticket": "GOLD-2"}, "body": "rename it"})),
        ("comments.add", json!({"anchor": {"route": "#/", "path": "nav", "label": "Nav", "excerpt": ""}, "body": "slow"})),
        ("comments.resolve", json!({"id": 1})),
        ("comments.reopen", json!({"id": 1})),
        ("comments.list", json!({})),
        ("comments.list", json!({"under": "GOLD", "state": "open"})),
        ("notes.add", json!({"body": "## watch drops events\nseen twice"})),
        ("notes.list", json!({})),
        ("notes.get", json!({"id": 1})),
        ("notes.upgrade", json!({"id": 1, "epic": "GOLD"})),
        ("themes.define", json!({"name": "ocean", "light": {"--accent": "#00796b"}, "dark": {"--accent": "#4db6ac"}})),
        ("themes.list", json!({})),
        ("themes.get", json!({"name": "ocean"})),
        ("themes.set", json!({"scope": "GOLD", "theme": "ocean", "reach": "site"})),
        ("themes.theme_for", json!({"id": "GOLD-2"})),
        ("deployments.record", json!({
            "project": "gold", "environment": "prod", "state": "succeeded", "revision": "4f7114a",
            "commits": ["4f7114a"], "release": "r1", "at": "2026-10-12T22:04:19Z",
            "source": {"system": "menger-pipeline", "run": "promote"}
        })),
        ("deployments.list", json!({})),
        ("deployments.environments", json!({})),
        ("deployments.for_ticket", json!({"id": "GOLD-2"})),
        ("deployments.running", json!({})),
        // COMMENT-5: an image put, put again, read back; refused; not there.
        ("media.put", json!({"ticket": "GOLD-2", "mime": "image/png", "data": "iVBORw0KGgoAAAANSUhEUgAAAAIAAAABCAYAAAD0In+KAAAAEUlEQVR42mP8z8DwnwEJMAIAKQgD/2zM2wkAAAAASUVORK5CYII="})),
        ("media.put", json!({"ticket": "GOLD-2", "mime": "image/png", "data": "iVBORw0KGgoAAAANSUhEUgAAAAIAAAABCAYAAAD0In+KAAAAEUlEQVR42mP8z8DwnwEJMAIAKQgD/2zM2wkAAAAASUVORK5CYII="})),
        ("media.get", json!({"hash": "191a9e3df08408abb45c996790d3d22cd0287c1c9e1344671b2c97414dd5f2e4"})),
        ("media.put", json!({"ticket": "GOLD-2", "mime": "image/svg+xml", "data": "PHN2Zy8+"})),
        ("media.get", json!({"hash": "0000000000000000000000000000000000000000000000000000000000000000"})),
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
    let mut comments = call(&client, "comments.watch", json!({})).await;
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
    let reopened: fn(&Value) -> bool = |ev| ev["type"] == "reopened";
    for (name, sub, last, namespace) in [
        ("tickets.watch", &mut watch, marker, "tickets"),
        ("tickets.watch filtered", &mut filtered, marker, "tickets"),
        ("comments.watch", &mut comments, reopened, "comments"),
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

// ---------------------------------------------------------------------------
// PERF-9: the script again, in msgpack
// ---------------------------------------------------------------------------

type MpSocket = tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

/// A socket signed in as the admin that asked for `ask` alone (`None`: for nothing).
async fn raw_connect(port: u16, ask: Option<&str>) -> MpSocket {
    use tokio_tungstenite::tungstenite::client::IntoClientRequest;
    for _ in 0..50 {
        let mut req = format!("ws://127.0.0.1:{port}").into_client_request().unwrap();
        for (k, v) in support::admin_headers(port).iter() {
            req.headers_mut().insert(k.clone(), v.clone());
        }
        if let Some(a) = ask {
            req.headers_mut().insert("Sec-WebSocket-Protocol", a.parse().unwrap());
        }
        if let Ok((ws, resp)) = tokio_tungstenite::connect_async(req).await {
            assert_eq!(resp.headers().get("Sec-WebSocket-Protocol").map(|v| v.to_str().unwrap()), ask, "the 101 names the ask");
            return ws;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("no socket on {port}");
}

/// A socket that asked for msgpack alone (spec/13).
async fn mp_connect(port: u16) -> MpSocket {
    raw_connect(port, Some("plexus.msgpack")).await
}

/// `menger.call {method, params}` on a msgpack socket: the subscription's id.
async fn mp_call(ws: &mut MpSocket, method: &str, params: Value) -> Value {
    use futures::SinkExt;
    use tokio_tungstenite::tungstenite::Message;
    let call = json!({"id": 1, "method": "menger.call", "params": {"method": method, "params": params}});
    ws.send(Message::Binary(rmp_serde::to_vec_named(&call).unwrap().into())).await.unwrap();
    let answer = mp_frame(ws).await;
    assert_eq!(answer["id"], 1, "the call's answer first: {answer}");
    answer["result"].clone()
}

/// The next frame: one binary message, one msgpack map with no `jsonrpc`.
async fn mp_frame(ws: &mut MpSocket) -> Value {
    use tokio_tungstenite::tungstenite::Message;
    loop {
        match tokio::time::timeout(Duration::from_secs(10), ws.next()).await.expect("a frame in 10 s").unwrap().unwrap() {
            Message::Binary(b) => {
                let v: Value = rmp_serde::from_slice(&b).expect("one msgpack value");
                assert!(v.get("jsonrpc").is_none(), "no `jsonrpc` in a msgpack frame: {v}");
                return v;
            }
            Message::Text(t) => panic!("a text frame on a msgpack socket: {t}"),
            _ => continue,
        }
    }
}

/// The `data` items' contents of the call open on `ws`, until `done` —
/// or, with `last`, until the event it says ends it.
async fn mp_items(ws: &mut MpSocket, last: Option<fn(&Value) -> bool>) -> Vec<Value> {
    let mut out = Vec::new();
    loop {
        let f = mp_frame(ws).await;
        let item = &f["params"]["result"];
        match item["type"].as_str() {
            Some("done") if last.is_none() => return out,
            Some("data") => {
                let ev = item["content"].clone();
                let end = last.is_some_and(|l| l(&ev));
                out.push(ev);
                if end {
                    return out;
                }
            }
            Some(_) if last.is_some() => continue,
            other => panic!("unexpected stream item type {other:?}: {f}"),
        }
    }
}

#[tokio::test]
async fn in_msgpack_every_item_round_trips_and_is_the_json_it_was() {
    let port = start_server().await;
    let mut watch = mp_connect(port).await;
    mp_call(&mut watch, "tickets.watch", json!({})).await;
    let mut filtered = mp_connect(port).await;
    mp_call(&mut filtered, "tickets.watch", json!({"filter": "not kind doc"})).await;
    let mut comments = mp_connect(port).await;
    mp_call(&mut comments, "comments.watch", json!({})).await;
    tokio::time::sleep(Duration::from_millis(200)).await;

    let mut ws = mp_connect(port).await;
    let mut golden = Vec::new();
    for (method, params) in script() {
        mp_call(&mut ws, method, params.clone()).await;
        let items = mp_items(&mut ws, None).await;
        let namespace = method.split('.').next().unwrap();
        for item in &items {
            round_trip(namespace, item);
        }
        golden.push(json!({"call": method, "params": params, "items": items.iter().map(steady).collect::<Vec<_>>()}));
    }
    let marker: fn(&Value) -> bool = |ev| ev["type"] == "amended" && ev["ticket"]["id"] == "GOLD-1";
    let reopened: fn(&Value) -> bool = |ev| ev["type"] == "reopened";
    for (name, sub, last, namespace) in [
        ("tickets.watch", &mut watch, marker, "tickets"),
        ("tickets.watch filtered", &mut filtered, marker, "tickets"),
        ("comments.watch", &mut comments, reopened, "comments"),
    ] {
        let events = mp_items(sub, Some(last)).await;
        for ev in &events {
            round_trip(namespace, ev);
        }
        golden.push(json!({"call": name, "items": events.iter().map(steady).collect::<Vec<_>>()}));
    }
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/golden-wire.json");
    let want: Vec<Value> = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(golden.len(), want.len(), "the script answered a different number of calls");
    for (a, b) in golden.iter().zip(&want) {
        assert_eq!(a, b, "in msgpack, the JSON changed");
    }
}

/// PERF-9's bytes: a `data` item in msgpack, on the wire, against the same
/// item in JSON — `tickets.health` and a summary row, two of PERF-16's three
/// frames (the third, a 1 KB item, is plexus-bench's `wire_bytes_per_item`).
#[tokio::test]
async fn an_item_in_msgpack_is_smaller_than_in_json() {
    use futures::SinkExt;
    use tokio_tungstenite::tungstenite::Message;
    let port = start_server().await;
    let mut m = mp_connect(port).await;
    mp_call(&mut m, "tickets.create", json!({"epic": "SIZE", "kind": "scope", "title": "a scope", "body": "a body"})).await;
    mp_items(&mut m, None).await;

    // The first data frame of a call, as the socket carried it.
    async fn first_data(ws: &mut MpSocket, frame: Message) -> Vec<u8> {
        ws.send(frame).await.unwrap();
        loop {
            let bytes = match tokio::time::timeout(Duration::from_secs(10), ws.next()).await.expect("a frame in 10 s").unwrap().unwrap() {
                Message::Binary(b) => b.to_vec(),
                Message::Text(t) => t.as_bytes().to_vec(),
                _ => continue,
            };
            let carries = |needle: &[u8]| bytes.windows(needle.len()).any(|w| w == needle);
            if carries(b"content_type") {
                return bytes;
            }
        }
    }
    let mut j = raw_connect(port, None).await;
    for (name, method, params) in [
        ("tickets.health", "tickets.health", json!({})),
        ("a summary row", "tickets.list", json!({"epic": "SIZE", "summary": true, "limit": 1})),
    ] {
        let call = json!({"id": 1, "method": "menger.call", "params": {"method": method, "params": params}});
        let frame = first_data(&mut m, Message::Binary(rmp_serde::to_vec_named(&call).unwrap().into())).await;
        mp_items_after_first(&mut m).await;
        let at = frame.windows(7).position(|w| w == b"\xa6result").expect("the item") + 7;
        let packed = frame.len() - at;

        let mut call = call;
        call["jsonrpc"] = json!("2.0");
        let text = first_data(&mut j, Message::Text(call.to_string().into())).await;
        mp_items_after_first_json(&mut j).await;
        let at = text.windows(9).position(|w| w == b"\"result\":").expect("the item") + 9;
        let json_item = text.len() - at - 2; // the two braces that close `params` and the frame
        println!("{name}: {packed} B in msgpack, {json_item} B in JSON (PERF-16's rows: health 152 · 190 typed, a row 463 · 575)");
        assert!(packed < json_item, "{name}: {packed} B in msgpack against {json_item} B in JSON");
    }
}

/// The rest of a call whose first data frame was read: until its `done`.
async fn mp_items_after_first(ws: &mut MpSocket) {
    mp_items(ws, None).await;
}

/// The same on a JSON socket.
async fn mp_items_after_first_json(ws: &mut MpSocket) {
    use tokio_tungstenite::tungstenite::Message;
    loop {
        if let Message::Text(t) = tokio::time::timeout(Duration::from_secs(10), ws.next()).await.expect("a frame in 10 s").unwrap().unwrap() {
            if t.as_str().contains(r#""result":{"type":"done""#) {
                return;
            }
        }
    }
}
