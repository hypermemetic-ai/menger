//! Wire-level tests: a real TransportServer, a real WebSocket client,
//! the same envelope web/index.html speaks — `menger.call` subscriptions
//! carrying `{method, params}`, events arriving as PlexusStreamItems.


/// ACCESS-24: the test provider — every server runs `key`, every client is its `--admin`.
#[path = "support/ports.rs"]
mod ports;
mod support;
use jsonrpsee::core::client::{Subscription, SubscriptionClientT};
use jsonrpsee::core::params::ObjectParams;
use jsonrpsee::ws_client::{WsClient, WsClientBuilder};
use plexus_rpc::core::plexus::DynamicHub;
use plexus_rpc::transport::TransportServer;
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::Duration;

/// MNG-10's oracle: AGS-20's nav `plan()`, transcribed — the same file the
/// store's tests hold the roll-ups to.
#[path = "../tickets-api/src/nav_plan.rs"]
mod nav_plan;

/// A port no other test in this binary has been handed (MNG-19). Binding
/// `:0` and dropping the listener only *suggests* a port: the kernel can
/// suggest the same one to a second test before the first test's server
/// has bound it — one server then fails to start and both tests talk to
/// the other, each seeing a store that is not its own.
async fn free_port() -> u16 {
    ports::free_port()
}

/// Starts a server whose database lives at `db_path`. The caller owns the
/// location's lifetime; `keep` is dropped (cleaning up) when the test's
/// runtime tears the server task down.
async fn start_server_at(db_path: std::path::PathBuf, keep: Option<tempfile::TempDir>) -> u16 {
    let port = free_port().await;
    let login = support::Login::new();
    let service = support::service(&db_path, &login).await;
    let gate = login.gate(port);
    let conv = |arc: Arc<DynamicHub>| {
        menger::calls::rpc_module(arc, Default::default()).map_err(|e| anyhow::anyhow!("{e}"))
    };
    tokio::spawn(async move {
        let _keep = keep;
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

/// Starts a server on an isolated, throwaway database location — servers
/// in this binary run concurrently and must never share storage.
async fn start_server() -> u16 {
    let dir = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).expect("tempdir");
    let db_path = dir.path().join("tickets.db");
    start_server_at(db_path, Some(dir)).await
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
    client
        .subscribe("menger.call", p, "menger.call_unsub")
        .await
        .expect("subscribe")
}

/// PERF-15: a page's first row — the first item of its `rows` (or the
/// event itself, when it is none).
fn first_row(item: Value) -> Value {
    let ev = content(item);
    if ev["type"] == "rows" { ev["items"][0].clone() } else { ev }
}

/// Unwraps a PlexusStreamItem, returning the activation event inside `data`.
fn content(item: Value) -> Value {
    assert_eq!(item["type"], "data", "expected data item, got {item}");
    item["content"].clone()
}

/// MNG-9: a finite `list` / `ready_set` call, split the way the contract
/// reads it — the row events, then the one trailing `page` item.
async fn paged(sub: Subscription<Value>) -> (Vec<Value>, Value) {
    let mut evs = collect(sub).await;
    let page = evs.pop().expect("a list always ends with the page item");
    assert_eq!(page["type"], "page", "the last item is the page: {page}");
    assert!(
        !evs.iter().any(|e| e["type"] == "page"),
        "exactly one page item: {evs:?}"
    );
    (evs, page)
}

/// The rows of a finite list call, page item asserted and dropped.
async fn rows(sub: Subscription<Value>) -> Vec<Value> {
    paged(sub).await.0
}

/// ACCESS-8: a parameter that is not its type is refused at dispatch,
/// before the method runs — one `error` item, `-32602 invalid_params`, its
/// message naming the parameter (MACRO-15). Its message.
async fn invalid(client: &WsClient, method: &str, params: Value) -> String {
    let mut sub = call(client, method, params.clone()).await;
    let item = sub.next().await.expect("an item").unwrap();
    assert_eq!(
        (item["type"].as_str(), item["code"].as_str(), item["error_kind"].as_str()),
        (Some("error"), Some("-32602"), Some("invalid_params")),
        "{method} {params}: {item}"
    );
    item["message"].as_str().unwrap().to_string()
}

/// Drains a finite call: collects every `data` item's content until `done`.
async fn collect(mut sub: Subscription<Value>) -> Vec<Value> {
    let mut out = Vec::new();
    while let Some(item) = sub.next().await {
        let item = item.unwrap();
        match item["type"].as_str() {
            Some("done") => return out,
            // PERF-15: a page's `rows` read as the per-row events it carries.
            Some("data") => match item["content"].clone() {
                c if c["type"] == "rows" => out.extend(c["items"].as_array().cloned().unwrap_or_default()),
                c => out.push(c),
            },
            other => panic!("unexpected stream item type {other:?}: {item}"),
        }
    }
    panic!("stream ended without a done item");
}

#[tokio::test]
async fn health_create_list_watch_over_the_wire() {
    let port = start_server().await;
    let client = connect(port).await;

    // health
    let mut sub = call(&client, "tickets.health", json!({})).await;
    let ev = content(sub.next().await.unwrap().unwrap());
    assert_eq!(ev["type"], "health");
    assert_eq!(ev["ticket_count"], 0);

    // watch, subscribed before any mutation
    let mut watch = call(&client, "tickets.watch", json!({})).await;

    // create
    let mut sub = call(
        &client,
        "tickets.create",
        json!({"epic": "WIRE", "title": "first over the wire", "kind": "scope"}),
    )
    .await;
    let ev = content(sub.next().await.unwrap().unwrap());
    assert_eq!(ev["type"], "created");
    assert_eq!(ev["ticket"]["id"], "WIRE-1");
    assert_eq!(ev["ticket"]["status"], "Draft");

    // the watch subscriber saw it pushed, unprompted
    let ev = content(watch.next().await.unwrap().unwrap());
    assert_eq!(ev["type"], "created");
    assert_eq!(ev["ticket"]["id"], "WIRE-1");
    // MENGER-13: its first status, a transition from nothing, right after.
    let ev = content(watch.next().await.unwrap().unwrap());
    assert_eq!(ev["type"], "transition");
    assert_eq!(
        (ev["transition"]["ticket"].as_str(), ev["transition"]["field"].as_str(), ev["transition"]["to"].as_str()),
        (Some("WIRE-1"), Some("status"), Some("Draft"))
    );
    assert_eq!((ev["transition"]["from"].clone(), ev["transition"]["actor"].as_str(), ev["transition"]["source"].as_str()), (Value::Null, Some(support::admin().as_str()), Some("api")));
    // …and, after it, the new shortcode's row (MNG-10)
    let ev = content(watch.next().await.unwrap().unwrap());
    assert_eq!(ev["type"], "group_changed");
    assert_eq!(
        ev["group"],
        json!({
            "epic": "WIRE", "shortcode": "WIRE", "address": "hypermemetic:WIRE", "total": 1, "done": 0, "docs": 0, "root": "WIRE-1",
            // AGS-23: the root's own row, which the nav draws at the top
            "root_row": {"title": "first over the wire", "status": "Draft", "kind": "scope"},
            "host": null
        })
    );

    // set_status → watch sees the transition too (Draft → Ready moves no
    // count — AGS-23: but WIRE-1 is its shortcode's root, whose status is on
    // the group's row, so the group follows it)
    let mut sub = call(
        &client,
        "tickets.set_status",
        json!({"id": "WIRE-1", "status": "Ready"}),
    )
    .await;
    let ev = content(sub.next().await.unwrap().unwrap());
    assert_eq!(ev["type"], "status_changed");
    assert_eq!(ev["previous"], "Draft");

    let ev = content(watch.next().await.unwrap().unwrap());
    assert_eq!(ev["type"], "status_changed");
    assert_eq!(ev["ticket"]["status"], "Ready");
    // MENGER-13: the transition, right after it.
    let ev = content(watch.next().await.unwrap().unwrap());
    assert_eq!((ev["type"].as_str(), ev["transition"]["from"].as_str(), ev["transition"]["to"].as_str()), (Some("transition"), Some("Draft"), Some("Ready")));
    let ev = content(watch.next().await.unwrap().unwrap());
    assert_eq!(ev["type"], "group_changed");
    assert_eq!(ev["group"]["root_row"]["status"], "Ready");
    assert_eq!(ev["group"]["total"], 1, "no count moved");

    // list replays current state
    let mut sub = call(&client, "tickets.list", json!({})).await;
    let ev = first_row(sub.next().await.unwrap().unwrap());
    assert_eq!(ev["type"], "ticket");
    assert_eq!(ev["ticket"]["id"], "WIRE-1");
}

/// ACCESS-13: the frame, over the wire — every description in the schema
/// comes from a `///` doc (the method's and each parameter's), a bad
/// parameter is `invalid_params` naming the values it takes, and a call that
/// reaches the dispatch with no identity at all is refused there, before
/// the body runs.
#[tokio::test]
async fn the_frame_over_the_wire() {
    let port = start_server().await;
    let client = connect(port).await;

    let schema = content(call(&client, "tickets.schema", json!({})).await.next().await.unwrap().unwrap());
    let methods = schema["methods"].as_array().expect("methods");
    let set_status = methods.iter().find(|m| m["name"] == "set_status").expect("set_status");
    assert!(set_status["description"].as_str().unwrap().starts_with("Set a ticket's status"), "{set_status}");
    assert_eq!(set_status["params"]["properties"]["id"]["description"], "Ticket id, e.g. MNG-2.", "{set_status}");
    for m in methods {
        let d = m["description"].as_str().unwrap_or_default();
        assert!(!d.trim().is_empty(), "{} has no description", m["name"]);
        for (name, p) in m["params"]["properties"].as_object().into_iter().flatten() {
            if p.get("x-plexus-source").is_none() {
                assert!(p.get("description").is_some_and(|d| !d.as_str().unwrap_or_default().is_empty()), "{}.{name}: {p}", m["name"]);
            }
        }
    }

    let message = invalid(&client, "tickets.set_status", json!({"id": "X-1", "status": "wibble"})).await;
    assert!(message.contains("status: unknown status 'wibble' (expected Pending, Ready, Coding"), "{message}");
    let message = invalid(&client, "tickets.create", json!({"epic": "X", "title": "t", "kind": "bug"})).await;
    assert!(message.contains("kind: unknown kind 'bug' (expected scope"), "{message}");

    // The dispatch itself, with no identity: refused before the body runs.
    let dir = tempfile::tempdir().unwrap();
    let service = support::service(&dir.path().join("t.db"), &support::Login::new()).await;
    let refused = match service.route("tickets.health", json!({}), None).await {
        Err(e) => e.to_string(),
        Ok(mut s) => {
            use futures::StreamExt;
            format!("{:?}", s.next().await.expect("an item"))
        }
    };
    assert!(refused.contains("Authentication required"), "{refused}");
}

/// ACCESS-15: `tickets commits` — a child of `tickets` on the frame. Its
/// schema is its own, every method and parameter described from its `///`;
/// `link` and `unlink` answer at `tickets.commits.*`; a malformed commit is
/// `invalid_params` naming it; the old names (`tickets.link_commits`,
/// `tickets.unlink_commits`) are gone.
#[tokio::test]
async fn tickets_commits_over_the_wire() {
    let port = start_server().await;
    let client = connect(port).await;

    let schema = content(call(&client, "tickets.commits.schema", json!({})).await.next().await.unwrap().unwrap());
    assert_eq!(schema["namespace"], "commits", "{schema}");
    let names: Vec<&str> = schema["methods"].as_array().unwrap().iter().filter_map(|m| m["name"].as_str()).filter(|m| *m != "schema").collect();
    assert_eq!(names, ["link", "unlink"], "{schema}");
    for m in schema["methods"].as_array().unwrap() {
        assert!(!m["description"].as_str().unwrap_or_default().trim().is_empty(), "{} has no description", m["name"]);
        for (name, p) in m["params"]["properties"].as_object().into_iter().flatten() {
            if p.get("x-plexus-source").is_none() {
                assert!(p.get("description").is_some_and(|d| !d.as_str().unwrap_or_default().is_empty()), "{}.{name}: {p}", m["name"]);
            }
        }
    }

    collect(call(&client, "tickets.create", json!({"epic": "LNK", "title": "root", "kind": "scope"})).await).await;
    let linked = collect(call(&client, "tickets.commits.link", json!({"id": "lnk-1", "commits": ["77df9a0", "plexus-macros@4F7114A"]})).await).await;
    assert_eq!((linked[0]["type"].as_str(), linked[0]["added"].clone()), (Some("commits_linked"), json!(["77df9a0", "plexus-macros@4f7114a"])), "{linked:?}");
    let unlinked = collect(call(&client, "tickets.commits.unlink", json!({"id": "LNK-1", "commits": ["77df9a0"]})).await).await;
    assert_eq!((unlinked[0]["type"].as_str(), unlinked[0]["ticket"]["commits"].clone()), (Some("commits_unlinked"), json!(["plexus-macros@4f7114a"])), "{unlinked:?}");
    let message = invalid(&client, "tickets.commits.link", json!({"id": "LNK-1", "commits": ["77df9a0;rm"]})).await;
    assert!(message.contains("commits: `77df9a0;rm` is not a commit reference"), "{message}");

    for old in ["tickets.link_commits", "tickets.unlink_commits"] {
        let item = call(&client, old, json!({"id": "LNK-1", "commits": ["77df9a0"]})).await.next().await.expect("an item").unwrap();
        assert_eq!(item["type"], "error", "{old} is gone: {item}");
    }
}

#[tokio::test]
async fn in_band_errors_cross_the_wire() {
    let port = start_server().await;
    let client = connect(port).await;

    // ACCESS-8: a status that is none is no `Status` — refused at dispatch,
    // naming the parameter; a domain refusal stays in band.
    let message = invalid(&client, "tickets.set_status", json!({"id": "X-1", "status": "wibble"})).await;
    assert!(message.contains("status: unknown status 'wibble'"), "{message}");
    let message = invalid(&client, "tickets.set_status", json!({"id": "not an id", "status": "Ready"})).await;
    assert!(message.starts_with("Invalid params: id: `not an id` is not a ticket"), "{message}");
    // An alias still reads: `active` is Coding, `mng-1` is MNG-1.
    let ev = content(call(&client, "tickets.set_status", json!({"id": "x-1", "status": "active"})).await.next().await.unwrap().unwrap());
    assert_eq!(ev, json!({"type": "not_found", "id": "X-1"}));
    let ev = content(call(&client, "tickets.set_parent", json!({"id": "X-1", "parent": "X-2"})).await.next().await.unwrap().unwrap());
    assert_eq!(ev["type"], "not_found", "{ev}");
}

/// A watch-only client's local mirror: every mutation event carries the
/// full affected ticket, so applying it is one upsert. A `group_changed`
/// (MNG-10) carries a roll-up, not a row — this mirror keeps rows.
/// SCALE-4: a parent's side of a birth or a move, `children_changed` /
/// `child_removed`, is its id and new `child_count` — applying it sets the
/// count on the row the mirror holds.
fn apply(mirror: &mut std::collections::HashMap<String, Value>, ev: &Value) {
    match ev["type"].as_str() {
        Some("created")
        | Some("status_changed")
        | Some("unlocks_changed")
        | Some("parent_changed") => {
            let ticket = ev["ticket"].clone();
            let id = ticket["id"].as_str().expect("ticket id").to_string();
            mirror.insert(id, ticket);
        }
        Some("children_changed") | Some("child_removed") => {
            let id = ev["id"].as_str().expect("parent id");
            if let Some(row) = mirror.get_mut(id) {
                row["child_count"] = ev["child_count"].clone();
            }
        }
        // MENGER-13: a transition says what its ticket's event said.
        Some("group_changed") | Some("transition") => {}
        other => panic!("unexpected watch event {other:?}: {ev}"),
    }
}

/// The client-side ready-set computation: Ready, and every blocked_by id
/// resolves in the mirror to a Complete or Superseded ticket.
fn local_ready(mirror: &std::collections::HashMap<String, Value>) -> Vec<String> {
    let mut ids: Vec<String> = mirror
        .values()
        .filter(|t| t["status"] == "Ready")
        .filter(|t| {
            t["blocked_by"].as_array().expect("blocked_by").iter().all(|b| {
                mirror
                    .get(b.as_str().expect("blocker id"))
                    .is_some_and(|bt| bt["status"] == "Complete" || bt["status"] == "Superseded" || bt["status"] == "Canceled")
            })
        })
        .map(|t| t["id"].as_str().expect("id").to_string())
        .collect();
    ids.sort();
    ids
}

fn ready_ids(events: Vec<Value>) -> Vec<String> {
    let mut ids: Vec<String> = events
        .into_iter()
        .map(|ev| {
            assert_eq!(ev["type"], "ticket", "got {ev}");
            ev["ticket"]["id"].as_str().expect("id").to_string()
        })
        .collect();
    ids.sort();
    ids
}

/// Acceptance 1 + 2: the ready set gates on blockers, and a client that
/// listed once and then only watched computes the same ready sets as the
/// server, without re-calling list.
#[tokio::test]
async fn ready_set_over_the_wire_is_watch_derivable() {
    let port = start_server().await;
    let client = connect(port).await;
    let mut mirror = std::collections::HashMap::new();

    // TICKETS-30: RS-1 roots the shortcode, so A (RS-2) and B (RS-3) have no
    // children to wait for when A completes.
    collect(call(&client, "tickets.create", json!({"epic": "RS", "title": "root", "kind": "scope"})).await).await;
    // A exists before the client arrives.
    let evs = collect(call(&client, "tickets.create", json!({"epic": "RS", "title": "A"})).await).await;
    assert_eq!(evs[0]["type"], "created");

    // The client fetches the full set once...
    for ev in rows(call(&client, "tickets.list", json!({})).await).await {
        assert_eq!(ev["type"], "ticket");
        let ticket = ev["ticket"].clone();
        mirror.insert(ticket["id"].as_str().unwrap().to_string(), ticket);
    }
    assert_eq!(mirror.len(), 2);

    // ...then consumes only watch events.
    let mut watch = call(&client, "tickets.watch", json!({})).await;

    // create B blocked_by [A]; promote both to Ready.
    collect(call(&client, "tickets.create",
        json!({"epic": "RS", "title": "B", "blocked_by": ["RS-2"]})).await).await;
    collect(call(&client, "tickets.set_status", json!({"id": "RS-2", "status": "Ready"})).await).await;
    collect(call(&client, "tickets.set_status", json!({"id": "RS-3", "status": "Ready"})).await).await;

    // Watch delivered: created(RS-3), children_changed(RS-2) — RS-3 named
    // no parent and hangs under the root (MNG-14) — unlocks_changed(RS-2),
    // group_changed(RS, now 2 tickets; MNG-10), 2x status_changed, and
    // (AGS-23) after RS-2's, group_changed(RS): RS-2 roots RS, and its
    // status is on the group's row. MENGER-13: and a transition after the
    // created and after each status_changed.
    for _ in 0..9 {
        let ev = content(watch.next().await.unwrap().unwrap());
        apply(&mut mirror, &ev);
    }
    assert_eq!(mirror["RS-2"]["unlocks"], json!(["RS-3"]));

    // Checkpoint 1: A is ready, B is not — server and mirror agree.
    let server = ready_ids(rows(call(&client, "tickets.work.ready", json!({})).await).await);
    assert_eq!(server, vec!["RS-2"]);
    assert_eq!(server, local_ready(&mirror));

    // A completes; B unblocks.
    collect(call(&client, "tickets.set_status", json!({"id": "RS-2", "status": "Complete"})).await).await;
    let ev = content(watch.next().await.unwrap().unwrap());
    apply(&mut mirror, &ev);

    let server = ready_ids(rows(call(&client, "tickets.work.ready", json!({})).await).await);
    assert_eq!(server, vec!["RS-3"]);
    assert_eq!(server, local_ready(&mirror));
}

/// Acceptance 3: a dangling blocker keeps a Ready ticket out of the ready
/// set, and the blockers query marks the id as dangling — a distinct
/// event category, not prose.
#[tokio::test]
async fn dangling_blocker_over_the_wire() {
    let port = start_server().await;
    let client = connect(port).await;

    collect(call(&client, "tickets.create",
        json!({"epic": "DG", "title": "C", "kind": "scope", "blocked_by": ["DG-99"]})).await).await;
    collect(call(&client, "tickets.set_status", json!({"id": "DG-1", "status": "Ready"})).await).await;

    let ready = rows(call(&client, "tickets.work.ready", json!({})).await).await;
    assert!(ready.is_empty(), "DG-1 must not be ready: {ready:?}");

    let evs = collect(call(&client, "tickets.work.blockers", json!({"id": "DG-1"})).await).await;
    assert_eq!(evs.len(), 1);
    assert_eq!(evs[0]["type"], "dangling_blocker");
    assert_eq!(evs[0]["id"], "DG-99");
}

/// Acceptance 4: a dangling forward reference resolves at the referenced
/// ticket's birth — the newborn carries its referrer in `unlocks`.
#[tokio::test]
async fn forward_reference_resolves_at_birth_over_the_wire() {
    let port = start_server().await;
    let client = connect(port).await;

    collect(call(&client, "tickets.create",
        json!({"epic": "FW", "title": "D-1", "kind": "scope", "blocked_by": ["FW-2"]})).await).await;

    let evs = collect(call(&client, "tickets.create",
        json!({"epic": "FW", "title": "becomes D-2"})).await).await;
    assert_eq!(evs[0]["type"], "created");
    assert_eq!(evs[0]["ticket"]["id"], "FW-2");
    assert_eq!(evs[0]["ticket"]["unlocks"], json!(["FW-1"]));

    let evs = collect(call(&client, "tickets.get", json!({"id": "FW-2"})).await).await;
    assert_eq!(evs[0]["type"], "ticket");
    assert_eq!(evs[0]["ticket"]["unlocks"], json!(["FW-1"]));
}

/// Acceptance 5: the create whose allocated id would close a blocked_by
/// cycle errors in-band and stores nothing.
#[tokio::test]
async fn cycle_create_errors_in_band_and_stores_nothing() {
    let port = start_server().await;
    let client = connect(port).await;

    // CY-1 blocked_by the not-yet-existing CY-2.
    collect(call(&client, "tickets.create",
        json!({"epic": "CY", "title": "one", "kind": "scope", "blocked_by": ["CY-2"]})).await).await;

    // The create that would become CY-2, blocked_by CY-1, closes the loop.
    let evs = collect(call(&client, "tickets.create",
        json!({"epic": "CY", "title": "loop", "blocked_by": ["CY-1"]})).await).await;
    assert_eq!(evs.len(), 1);
    assert_eq!(evs[0]["type"], "error");
    assert!(evs[0]["message"].as_str().unwrap().contains("cycle"), "{}", evs[0]);

    // Nothing was stored under the would-be id.
    let evs = collect(call(&client, "tickets.get", json!({"id": "CY-2"})).await).await;
    assert_eq!(evs[0]["type"], "not_found");
}

/// AGS-19 acceptance 1, 2 and 4 over the real wire: a child names its
/// parent at create, `get` shows both halves (SCALE-4: the parent's half
/// is its `child_count`), the watch-only mirror sees the parent's update
/// as `children_changed` carrying its id and count, a
/// nonexistent parent is an in-band error naming the reason, a 3-deep
/// chain (crossing epics) reads back, and `list {parent}` returns exactly
/// that ticket's children.
#[tokio::test]
async fn parent_children_and_the_parent_filter_over_the_wire() {
    let port = start_server().await;
    let client = connect(port).await;
    let mut mirror = std::collections::HashMap::new();

    let evs = collect(call(&client, "tickets.create", json!({"epic": "PA", "title": "P", "kind": "scope"})).await).await;
    assert_eq!(evs[0]["type"], "created");
    assert_eq!(evs[0]["ticket"]["child_count"], 0, "born childless");
    assert!(evs[0]["ticket"].get("children").is_none(), "SCALE-4: no list on the row");
    assert!(evs[0]["ticket"]["parent"].is_null(), "and parentless");
    mirror.insert("PA-1".to_string(), evs[0]["ticket"].clone());

    // A watch-only client from here on.
    let mut watch = call(&client, "tickets.watch", json!({})).await;

    // Acceptance 2, first half: a parent that names no ticket.
    let evs = collect(call(&client, "tickets.create",
        json!({"epic": "PA", "title": "orphan", "parent": "PA-99"})).await).await;
    assert_eq!(evs.len(), 1);
    assert_eq!(evs[0]["type"], "error");
    let message = evs[0]["message"].as_str().unwrap();
    assert!(message.contains("PA-99"), "names the parent: {message}");
    assert!(message.contains("must exist"), "names the reason: {message}");
    let evs = collect(call(&client, "tickets.get", json!({"id": "PA-2"})).await).await;
    assert_eq!(evs[0]["type"], "not_found", "nothing was stored");

    // Acceptance 1: C is born as P's child.
    let evs = collect(call(&client, "tickets.create",
        json!({"epic": "PA", "title": "C", "kind": "execution", "parent": "PA-1"})).await).await;
    assert_eq!(evs[0]["type"], "created");
    assert_eq!(evs[0]["ticket"]["id"], "PA-2");
    assert_eq!(evs[0]["ticket"]["parent"], "PA-1");

    // The watcher saw the birth and the parent's update: SCALE-4, the
    // parent's id and new count, and nothing else (MENGER-13: but the
    // birth's transition, right after it).
    for _ in 0..3 {
        let ev = content(watch.next().await.unwrap().unwrap());
        if ev["type"] == "children_changed" {
            assert_eq!(ev, json!({"type": "children_changed", "id": "PA-1", "added": "PA-2", "child_count": 1}));
        }
        apply(&mut mirror, &ev);
    }
    assert_eq!(mirror["PA-1"]["child_count"], 1,
        "a watch-only client's mirror stays coherent");

    // Both ends agree when asked directly.
    let evs = collect(call(&client, "tickets.get", json!({"id": "PA-1"})).await).await;
    assert_eq!(evs[0]["ticket"]["child_count"], 1);
    let evs = collect(call(&client, "tickets.get", json!({"id": "PA-2"})).await).await;
    assert_eq!(evs[0]["ticket"]["parent"], "PA-1");

    // Acceptance 2, second half: three deep, the middle link in another
    // epic — PB's scope root hangs under PA-2 (a sub-shortcode); PB's own
    // tickets stay inside PB (MNG-14). (Self-ancestry at create needs no
    // test: the id is allocated by the very create that would name it.)
    collect(call(&client, "tickets.create",
        json!({"epic": "PB", "title": "grandchild", "kind": "scope", "parent": "PA-2"})).await).await;
    let evs = collect(call(&client, "tickets.create",
        json!({"epic": "PA", "title": "escapes", "parent": "PB-1"})).await).await;
    assert_eq!(evs[0]["type"], "error", "a non-root's parent must be in its shortcode");
    assert!(evs[0]["message"].as_str().unwrap().contains("outside"), "{}", evs[0]);
    collect(call(&client, "tickets.create",
        json!({"epic": "PB", "title": "great-grandchild", "parent": "PB-1"})).await).await;
    let chain = ["PA-1", "PA-2", "PB-1", "PB-2"];
    for (child, parent) in chain.iter().skip(1).zip(chain.iter()) {
        let evs = collect(call(&client, "tickets.get", json!({"id": child})).await).await;
        assert_eq!(evs[0]["ticket"]["parent"], *parent, "{child}");
        let evs = collect(call(&client, "tickets.get", json!({"id": parent})).await).await;
        assert_eq!(evs[0]["ticket"]["child_count"], 1, "{parent}");
        let evs = rows(call(&client, "tickets.list", json!({"parent": parent})).await).await;
        assert_eq!(evs.len(), 1, "{parent}");
        assert_eq!(evs[0]["ticket"]["id"], *child, "{parent}");
    }
    let evs = collect(call(&client, "tickets.get", json!({"id": "PB-2"})).await).await;
    assert_eq!(evs[0]["ticket"]["child_count"], 0, "the leaf has none");

    // Acceptance 4: exactly the children, and nothing else. PA-3 names no
    // parent and so hangs under PA's root (MNG-14).
    let evs = collect(call(&client, "tickets.create",
        json!({"epic": "PA", "title": "second child of P"})).await).await; // PA-3
    assert_eq!(evs[0]["ticket"]["parent"], "PA-1", "defaulted to the shortcode root");
    collect(call(&client, "tickets.create",
        json!({"epic": "PC", "title": "cross-epic child of P", "kind": "scope", "parent": "PA-1"})).await).await;
    let ids = |evs: Vec<Value>| -> Vec<String> {
        evs.into_iter()
            .map(|ev| {
                assert_eq!(ev["type"], "ticket", "got {ev}");
                ev["ticket"]["id"].as_str().unwrap().to_string()
            })
            .collect()
    };
    assert_eq!(
        ids(rows(call(&client, "tickets.list", json!({"parent": "PA-1"})).await).await),
        vec!["PA-2", "PA-3", "PC-1"]
    );
    assert_eq!(
        ids(rows(call(&client, "tickets.list", json!({"parent": "PA-3"})).await).await),
        Vec::<String>::new()
    );
    assert_eq!(
        ids(rows(call(&client, "tickets.list", json!({})).await).await).len(),
        6,
        "the unfiltered list is unchanged"
    );
}

/// MNG-9 acceptance 1, 3, 4 and 6 over the real wire: a `summary` +
/// `limit` call yields summary rows (no body) and closes with
/// `page { next, remaining }`; walking `after = next` visits every id
/// exactly once in (epic, seq) order; `list {}` is what it always was
/// plus `page { next: null, remaining: 0 }`; an `after` naming no ticket
/// is one error and done; `ready_set` takes the same three params.
#[tokio::test]
async fn list_pages_over_the_wire() {
    let port = start_server().await;
    let client = connect(port).await;

    // Two epics, ten tickets: PGA-1..7 then PGZ-1..3 in the store's order.
    for (epic, n) in [("PGA", 7), ("PGZ", 3)] {
        for i in 1..=n {
            // MNG-12: a prefix's first ticket is its scope root.
            let kind = if i == 1 { "scope" } else { "build" };
            collect(call(&client, "tickets.create",
                json!({"epic": epic, "title": format!("{epic} {i}"), "kind": kind, "body": "## Problem\n\nbodies are the weight"})).await).await;
        }
    }

    // Acceptance 1 + 6: the page's shape.
    let (evs, page) = paged(call(&client, "tickets.list", json!({"summary": true, "limit": 4})).await).await;
    assert_eq!(evs.len(), 4, "limit is honoured");
    assert_eq!(evs[0]["type"], "summary");
    let row = &evs[0]["summary"];
    assert_eq!(row["id"], "PGA-1");
    assert_eq!(row["epic"], "PGA");
    assert_eq!(row["seq"], 1);
    assert_eq!(row["status"], "Draft");
    assert_eq!(row["kind"], "scope");
    assert!(row["severity"].is_null());
    assert!(row["parent"].is_null(), "the root is top-level");
    // MNG-14: the six tickets that named no parent hang under the root —
    // SCALE-4: counted on the row, listed by `list { parent }`.
    assert_eq!(row["child_count"], 6);
    assert!(row.get("body").is_none(), "a summary carries no body: {row}");
    assert_eq!(page, json!({"type": "page", "next": "PGA-4", "remaining": 6}));

    // Acceptance 2's shape: walk to the end, every id once, in order.
    let mut walked: Vec<String> = Vec::new();
    let mut after = json!(null);
    loop {
        let params = json!({"summary": true, "limit": 4, "after": after});
        let (evs, page) = paged(call(&client, "tickets.list", params).await).await;
        walked.extend(evs.iter().map(|e| e["summary"]["id"].as_str().unwrap().to_string()));
        assert_eq!(page["remaining"], (10 - walked.len()) as u64);
        if page["next"].is_null() {
            break;
        }
        assert_eq!(page["next"], *walked.last().unwrap());
        after = page["next"].clone();
    }
    assert_eq!(
        walked,
        ["PGA-1", "PGA-2", "PGA-3", "PGA-4", "PGA-5", "PGA-6", "PGA-7", "PGZ-1", "PGZ-2", "PGZ-3"]
    );

    // Acceptance 3: `list {}` unchanged apart from the trailing page.
    let (evs, page) = paged(call(&client, "tickets.list", json!({})).await).await;
    assert_eq!(evs.len(), 10);
    assert_eq!(evs[0]["type"], "ticket");
    assert!(evs[0]["ticket"]["body"].as_str().unwrap().contains("bodies are the weight"));
    assert_eq!(page, json!({"type": "page", "next": null, "remaining": 0}));

    // Acceptance 4: a cursor naming no ticket.
    let evs = collect(call(&client, "tickets.list", json!({"after": "ZZZ-9"})).await).await;
    assert_eq!(evs.len(), 1, "one error and done, nothing else: {evs:?}");
    assert_eq!(evs[0], json!({"type": "error", "message": "after: no ticket ZZZ-9"}));

    // Acceptance 6: ready_set takes the same params (and pages its own set).
    for id in ["PGA-1", "PGA-2", "PGZ-1"] {
        collect(call(&client, "tickets.set_status", json!({"id": id, "status": "Ready"})).await).await;
    }
    let (evs, page) =
        paged(call(&client, "tickets.work.ready", json!({"summary": true, "limit": 2})).await).await;
    assert_eq!(
        evs.iter().map(|e| e["summary"]["id"].as_str().unwrap()).collect::<Vec<_>>(),
        ["PGA-1", "PGA-2"]
    );
    assert_eq!(page, json!({"type": "page", "next": "PGA-2", "remaining": 1}));
    let (evs, page) =
        paged(call(&client, "tickets.work.ready", json!({"after": "PGA-2"})).await).await;
    assert_eq!(evs.len(), 1);
    assert_eq!(evs[0]["ticket"]["id"], "PGZ-1", "no summary asked for, so full tickets");
    assert_eq!(page, json!({"type": "page", "next": null, "remaining": 0}));
}

/// MNG-9 acceptance 5: the cursor is a position in the store's live
/// order, not a snapshot — a ticket born above a stalled walk's cursor is
/// visited by that walk; one born below it never is, which is exactly
/// what the `watch` opened first is for.
#[tokio::test]
async fn a_walk_sees_births_above_its_cursor_and_watch_carries_the_rest() {
    let port = start_server().await;
    let client = connect(port).await;

    for i in 1..=4 {
        let kind = if i == 1 { "scope" } else { "build" }; // MNG-12
        collect(call(&client, "tickets.create", json!({"epic": "MID", "title": format!("m{i}"), "kind": kind})).await).await;
    }

    // The app's order: watch first, then walk.
    let mut watch = call(&client, "tickets.watch", json!({})).await;
    let (evs, page) = paged(call(&client, "tickets.list", json!({"summary": true, "limit": 2})).await).await;
    assert_eq!(evs.len(), 2);
    assert_eq!(page["next"], "MID-2");

    // Mid-walk: one above the cursor (MID-5), one below it (AAA-1).
    collect(call(&client, "tickets.create", json!({"epic": "MID", "title": "born above"})).await).await;
    collect(call(&client, "tickets.create", json!({"epic": "AAA", "title": "born below", "kind": "scope"})).await).await;

    let (evs, page) = paged(call(&client, "tickets.list",
        json!({"summary": true, "after": page["next"]})).await).await;
    let ids: Vec<&str> = evs.iter().map(|e| e["summary"]["id"].as_str().unwrap()).collect();
    assert_eq!(ids, ["MID-3", "MID-4", "MID-5"], "MID-5 joined the walk");
    assert_eq!(page, json!({"type": "page", "next": null, "remaining": 0}));

    // AAA-1 sorts below the cursor, so only watch carries it. (MID-5's
    // birth also moves its root's child count — MNG-14 — so the watch
    // interleaves a children_changed; the births are what this pins.)
    let mut pushed = Vec::new();
    while pushed.len() < 2 {
        let ev = content(watch.next().await.unwrap().unwrap());
        if ev["type"] == "created" {
            pushed.push(ev["ticket"]["id"].as_str().unwrap().to_string());
        }
    }
    assert_eq!(pushed, ["MID-5", "AAA-1"]);
}

/// SCALE-6 acceptance 1 and 3 over the real wire, against a store holding
/// exactly `AGS-1`, `AGS-18` and `MFE-7 · "The Ticket Page"` — written into
/// the database before the server opens it, since `create` would number
/// AGS-2..17 on the way to AGS-18. `find {query}` yields `summary` items
/// and closes with one `page`; `"ags-1"` is AGS-1 then AGS-18, `"ticket
/// page"` is MFE-7, `"zzz"` is no rows and `page { next: null, remaining:
/// 0 }`; `limit: 2` then `after = next` visits every match once; an
/// `after` naming no ticket is one error and done.
#[tokio::test]
async fn find_over_the_wire() {
    let dir = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).expect("tempdir");
    let db = dir.path().join("tickets.db");
    // The schema as the server writes it; then the three rows by hand.
    drop(menger::store::Store::open(&db).expect("create the schema"));
    let conn = rusqlite::Connection::open(&db).unwrap();
    for (id, epic, seq, title) in [
        ("AGS-1", "AGS", 1, "Keyboard navigation"),
        ("AGS-18", "AGS", 18, "The jump palette"),
        ("MFE-7", "MFE", 7, "The Ticket Page"),
    ] {
        conn.execute(
            "INSERT INTO tickets (id, epic, seq, title, status, kind, blocked_by, unlocks, body, created_unix, updated_unix)
             VALUES (?1, ?2, ?3, ?4, 'Pending', 'scope', '[]', '[]', 'bodies are not searched', 0, 0)",
            rusqlite::params![id, epic, seq, title],
        )
        .unwrap();
    }
    drop(conn);
    let port = start_server_at(db, Some(dir)).await;
    let client = connect(port).await;

    let found = |evs: &[Value]| -> Vec<String> {
        evs.iter()
            .map(|e| {
                assert_eq!(e["type"], "summary", "find yields summary rows: {e}");
                assert!(e["summary"].get("body").is_none(), "no body: {e}");
                e["summary"]["id"].as_str().unwrap().to_string()
            })
            .collect()
    };

    let (evs, page) = paged(call(&client, "tickets.find", json!({"query": "ags-1"})).await).await;
    assert_eq!(found(&evs), ["AGS-1", "AGS-18"]);
    assert_eq!(page, json!({"type": "page", "next": null, "remaining": 0}));

    let (evs, _) = paged(call(&client, "tickets.find", json!({"query": "ticket page"})).await).await;
    assert_eq!(found(&evs), ["MFE-7"]);

    let (evs, page) = paged(call(&client, "tickets.find", json!({"query": "zzz"})).await).await;
    assert!(evs.is_empty(), "{evs:?}");
    assert_eq!(page, json!({"type": "page", "next": null, "remaining": 0}));

    // Acceptance 3: two at a time, every match once — here, the blank
    // query's every ticket, and a title query's two.
    for (query, all) in [(" ", vec!["AGS-1", "AGS-18", "MFE-7"]), ("THE", vec!["AGS-18", "MFE-7"])] {
        let mut walked: Vec<String> = Vec::new();
        let mut after = json!(null);
        loop {
            let (evs, page) = paged(call(&client, "tickets.find",
                json!({"query": query, "limit": 2, "after": after})).await).await;
            walked.extend(found(&evs));
            assert_eq!(page["remaining"], (all.len() - walked.len()) as u64, "{query:?}");
            if page["next"].is_null() {
                break;
            }
            after = page["next"].clone();
        }
        assert_eq!(walked, all, "{query:?}");
    }

    let evs = collect(call(&client, "tickets.find", json!({"query": "ags", "after": "ZZZ-9"})).await).await;
    assert_eq!(evs, [json!({"type": "error", "message": "after: no ticket ZZZ-9"})]);
}

/// MNG-10 acceptance 1 and 2 over the real wire. `groups {}` answers one
/// `group` item per shortcode row and then the stream's own `done` — no
/// page, nothing else — with exactly the numbers AGS-20's nav `plan()`
/// computes from every row the app's walk would fetch: here a shortcode
/// hosting a sub-shortcode that hosts another, and one standing alone.
/// Then on `watch`: a completion in a top-level shortcode is its
/// `status_changed` followed by exactly one `group_changed`, `done` + 1; a
/// completion three rows deep moves all three, innermost first; and
/// `watch { epic }` hears the group of its own shortcode only.
#[tokio::test]
async fn groups_are_the_navs_rollups_and_a_completion_pushes_one_group_changed() {
    let port = start_server().await;
    let client = connect(port).await;

    for params in [
        json!({"epic": "HOST", "title": "the host", "kind": "scope"}), // HOST-1
        json!({"epic": "HOST", "title": "core"}),                      // HOST-2 → HOST-1
        json!({"epic": "HOST", "title": "under core", "parent": "HOST-2"}), // HOST-3
        json!({"epic": "SUB", "title": "hosted", "kind": "scope", "parent": "HOST-2"}), // SUB-1
        json!({"epic": "SUB", "title": "a"}),                         // SUB-2 → SUB-1
        json!({"epic": "SUB", "title": "b"}),                      // SUB-3 → SUB-1 (TICKETS-30: SUB-2 stays a leaf, so it can complete)
        json!({"epic": "DEEP", "title": "deeper", "kind": "scope", "parent": "SUB-3"}), // DEEP-1
        json!({"epic": "DEEP", "title": "c"}),                        // DEEP-2 → DEEP-1
        json!({"epic": "LONE", "title": "alone", "kind": "scope"}),   // LONE-1
        json!({"epic": "LONE", "title": "d"}),                        // LONE-2 → LONE-1
    ] {
        let evs = collect(call(&client, "tickets.create", params).await).await;
        assert_eq!(evs[0]["type"], "created", "{evs:?}");
    }
    for id in ["HOST-3", "SUB-2", "DEEP-2"] {
        collect(call(&client, "tickets.set_status", json!({"id": id, "status": "Complete"})).await).await;
    }

    // `collect` reads to the stream's `done` and refuses any other item
    // type, so this is the whole answer: group items, then done.
    // AGS-23: each with its root's row — every root here a Draft scope.
    let root_row = |title: &str| json!({"title": title, "status": "Draft", "kind": "scope"});
    let items = collect(call(&client, "tickets.nav.groups", json!({})).await).await;
    assert!(items.iter().all(|ev| ev["type"] == "group"), "only groups: {items:?}");
    let groups: Vec<Value> = items.into_iter().map(|ev| ev["group"].clone()).collect();
    assert_eq!(
        groups,
        vec![
            json!({"epic": "DEEP", "shortcode": "DEEP", "address": "hypermemetic:HOST/SUB/DEEP", "total": 2, "done": 1, "docs": 0, "root": "DEEP-1", "root_row": root_row("deeper"), "host": "SUB-3"}),
            json!({"epic": "HOST", "shortcode": "HOST", "address": "hypermemetic:HOST", "total": 8, "done": 3, "docs": 0, "root": "HOST-1", "root_row": root_row("the host"), "host": null}),
            json!({"epic": "LONE", "shortcode": "LONE", "address": "hypermemetic:LONE", "total": 2, "done": 0, "docs": 0, "root": "LONE-1", "root_row": root_row("alone"), "host": null}),
            json!({"epic": "SUB", "shortcode": "SUB", "address": "hypermemetic:HOST/SUB", "total": 5, "done": 2, "docs": 0, "root": "SUB-1", "root_row": root_row("hosted"), "host": "HOST-2"}),
        ]
    );

    // Acceptance 1's comparison: the nav's plan() over every row, as the
    // app's summary walk would have fetched them.
    let all = rows(call(&client, "tickets.list", json!({"summary": true})).await).await;
    let facts: Vec<nav_plan::Facts> = all
        .iter()
        .map(|ev| {
            let s = &ev["summary"];
            nav_plan::Facts {
                id: s["id"].as_str().unwrap().into(),
                epic: s["epic"].as_str().unwrap().into(),
                seq: s["seq"].as_u64().unwrap() as u32,
                kind: s["kind"].as_str().unwrap().into(),
                parent: s["parent"].as_str().map(Into::into),
            }
        })
        .collect();
    let complete: std::collections::BTreeSet<String> = all
        .iter()
        .filter(|ev| ev["summary"]["status"] == "Complete")
        .map(|ev| ev["summary"]["id"].as_str().unwrap().to_string())
        .collect();
    let nav_rows = nav_plan::rows(&facts, &complete);
    let nav: Vec<Value> = nav_rows
        .iter()
        .map(|r| {
            // AGS-23: the root's row, as the summary walk has it.
            let row = r.root.as_ref().map(|id| {
                let s = &all.iter().find(|ev| ev["summary"]["id"] == json!(id)).expect("the root is a row")["summary"];
                json!({"title": s["title"], "status": s["status"], "kind": s["kind"]})
            });
            // ORG-3: its full address — the codes of the shortcodes above it.
            let mut chain = vec![r.epic.clone()];
            let mut host = r.host.clone();
            while let Some(h) = host {
                let code = h.rsplit_once('-').expect("an id").0.to_string();
                host = nav_rows.iter().find(|o| o.epic == code).and_then(|o| o.host.clone());
                chain.insert(0, code);
            }
            let address = format!("hypermemetic:{}", chain.join("/"));
            json!({"epic": r.epic, "shortcode": r.epic, "address": address, "total": r.total, "done": r.done, "docs": r.docs, "root": r.root, "root_row": row, "host": r.host})
        })
        .collect();
    assert_eq!(groups, nav, "the server's roll-ups are the nav's");

    // Acceptance 2: the status change, then exactly one group_changed.
    let mut watch = call(&client, "tickets.watch", json!({})).await;
    let mut host_only = call(&client, "tickets.watch", json!({"epic": "HOST"})).await;
    collect(call(&client, "tickets.set_status", json!({"id": "LONE-2", "status": "Complete"})).await).await;
    let ev = content(watch.next().await.unwrap().unwrap());
    assert_eq!(ev["type"], "status_changed");
    let ev = content(watch.next().await.unwrap().unwrap());
    assert_eq!(ev["type"], "transition", "MENGER-13: right after its status_changed");
    let ev = content(watch.next().await.unwrap().unwrap());
    assert_eq!(ev["type"], "group_changed");
    assert_eq!(
        ev["group"],
        json!({"epic": "LONE", "shortcode": "LONE", "address": "hypermemetic:LONE", "total": 2, "done": 1, "docs": 0, "root": "LONE-1", "root_row": root_row("alone"), "host": null})
    );
    // WAVE-6: and, last, the placement it moved.
    let ev = content(watch.next().await.unwrap().unwrap());
    assert_eq!(ev, json!({"type": "placements_changed", "changes": [{"id": "LONE-2", "placement": {"type": "done"}}]}));
    // Nothing else came of it: the next item is the next write's.
    collect(call(&client, "tickets.set_status", json!({"id": "HOST-1", "status": "Ready"})).await).await;
    let ev = content(watch.next().await.unwrap().unwrap());
    assert_eq!((ev["type"].as_str(), ev["ticket"]["id"].as_str()), (Some("status_changed"), Some("HOST-1")));
    let ev = content(watch.next().await.unwrap().unwrap());
    assert_eq!(ev["type"], "transition", "MENGER-13: right after its status_changed");
    // AGS-23: HOST-1 roots HOST, and its status is on HOST's row: no count
    // moved, but the group did — once, its own.
    let ev = content(watch.next().await.unwrap().unwrap());
    assert_eq!(ev["type"], "group_changed");
    assert_eq!((ev["group"]["epic"].as_str(), ev["group"]["done"].as_u64()), (Some("HOST"), Some(3)));
    assert_eq!(ev["group"]["root_row"]["status"], "Ready");

    // Three rows deep: DEEP, then SUB, then HOST.
    collect(call(&client, "tickets.set_status", json!({"id": "DEEP-1", "status": "Complete"})).await).await;
    let ev = content(watch.next().await.unwrap().unwrap());
    assert_eq!(ev["type"], "status_changed");
    let ev = content(watch.next().await.unwrap().unwrap());
    assert_eq!(ev["type"], "transition", "MENGER-13: right after its status_changed");
    let mut moved = Vec::new();
    for _ in 0..3 {
        let ev = content(watch.next().await.unwrap().unwrap());
        assert_eq!(ev["type"], "group_changed");
        moved.push((ev["group"]["epic"].clone(), ev["group"]["done"].clone()));
    }
    assert_eq!(moved, [(json!("DEEP"), json!(2)), (json!("SUB"), json!(3)), (json!("HOST"), json!(4))]);

    // `watch { epic: HOST }`: neither LONE's group nor DEEP's ticket — the
    // HOST-1 flip and (AGS-23) the root row it moved, then HOST's own row.
    let ev = content(host_only.next().await.unwrap().unwrap());
    assert_eq!((ev["type"].as_str(), ev["ticket"]["id"].as_str()), (Some("status_changed"), Some("HOST-1")));
    // MENGER-13: HOST-1's transition is HOST's; DEEP-1's is not.
    let ev = content(host_only.next().await.unwrap().unwrap());
    assert_eq!((ev["type"].as_str(), ev["transition"]["ticket"].as_str()), (Some("transition"), Some("HOST-1")));
    let ev = content(host_only.next().await.unwrap().unwrap());
    assert_eq!((ev["type"].as_str(), ev["group"]["done"].as_u64()), (Some("group_changed"), Some(3)));
    let ev = content(host_only.next().await.unwrap().unwrap());
    assert_eq!(ev["type"], "group_changed");
    assert_eq!(
        ev["group"],
        json!({
            "epic": "HOST", "shortcode": "HOST", "address": "hypermemetic:HOST", "total": 8, "done": 4, "docs": 0, "root": "HOST-1",
            "root_row": {"title": "the host", "status": "Ready", "kind": "scope"},
            "host": null
        })
    );
}

/// SCALE-16 over the real wire. `extents {}` answers one `extent` item per
/// shortcode that holds a ticket — flat `{ epic, last_seq }` — in
/// shortcode order, and then the stream's own `done`: no page, nothing
/// else. The `last_seq`s add up to `health`'s count; a refused create
/// moves nothing; and they are not the roll-ups — HOST's group counts the
/// sub-shortcode nested under it, its extent only its own tickets.
/// Acceptance 2: for every shortcode E and row k, `list { epic: E, after:
/// "E-k", limit: 1 }` yields `E-(k+1)` — a position becomes a cursor.
#[tokio::test]
async fn extents_are_each_shortcodes_last_seq_and_a_row_is_a_cursor() {
    let port = start_server().await;
    let client = connect(port).await;

    for params in [
        json!({"epic": "HOST", "title": "the host", "kind": "scope"}),
        json!({"epic": "HOST", "title": "a"}),
        json!({"epic": "SUB", "title": "hosted", "kind": "scope", "parent": "HOST-2"}),
        json!({"epic": "SUB", "title": "b"}),
        json!({"epic": "SUB", "title": "c"}),
        json!({"epic": "HOST", "title": "d"}),
        json!({"epic": "AAA", "title": "first by name", "kind": "scope"}),
    ] {
        let evs = collect(call(&client, "tickets.create", params).await).await;
        assert_eq!(evs[0]["type"], "created", "{evs:?}");
    }
    // Refused (a build cannot open a shortcode): nothing allocated.
    let evs = collect(call(&client, "tickets.create", json!({"epic": "NEW", "title": "x"})).await).await;
    assert_eq!(evs[0]["type"], "error", "{evs:?}");

    let items = collect(call(&client, "tickets.nav.extents", json!({})).await).await;
    assert_eq!(
        items,
        vec![
            json!({"type": "extent", "epic": "AAA", "shortcode": "AAA", "last_seq": 1}),
            json!({"type": "extent", "epic": "HOST", "shortcode": "HOST", "last_seq": 3}),
            json!({"type": "extent", "epic": "SUB", "shortcode": "SUB", "last_seq": 3}),
        ],
        "one flat extent per shortcode, in order, and nothing after them"
    );
    let health = collect(call(&client, "tickets.health", json!({})).await).await;
    let sum: u64 = items.iter().map(|ev| ev["last_seq"].as_u64().unwrap()).sum();
    assert_eq!(json!(sum), health[0]["ticket_count"]);
    let groups = collect(call(&client, "tickets.nav.groups", json!({})).await).await;
    let host = groups.iter().find(|ev| ev["group"]["epic"] == "HOST").unwrap();
    assert_eq!(host["group"]["total"], 6, "the roll-up nests SUB; the extent does not");

    for ev in &items {
        let (epic, last) = (ev["epic"].as_str().unwrap(), ev["last_seq"].as_u64().unwrap());
        for k in 1..=last {
            let (rows, page) = paged(
                call(&client, "tickets.list", json!({"epic": epic, "after": format!("{epic}-{k}"), "limit": 1, "summary": true}))
                    .await,
            )
            .await;
            let got: Vec<&str> = rows.iter().map(|r| r["summary"]["id"].as_str().unwrap()).collect();
            if k < last {
                assert_eq!(got, [format!("{epic}-{}", k + 1)], "{epic} after row {k}");
            } else {
                assert!(got.is_empty() && page["next"].is_null(), "{epic} ends at row {last}: {got:?} {page}");
            }
        }
    }

    // A birth is its shortcode's new last_seq — `created` already says so.
    let evs = collect(call(&client, "tickets.create", json!({"epic": "SUB", "title": "e"})).await).await;
    assert_eq!((evs[0]["ticket"]["epic"].as_str(), evs[0]["ticket"]["seq"].as_u64()), (Some("SUB"), Some(4)));
    let items = collect(call(&client, "tickets.nav.extents", json!({})).await).await;
    assert_eq!(items[2], json!({"type": "extent", "epic": "SUB", "shortcode": "SUB", "last_seq": 4}));
}

#[tokio::test]
async fn shell_serves_the_page() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let shell_port = free_port().await;
    let empty = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
    let app_dist = empty.path().to_path_buf();
    tokio::spawn(async move {
        // Same call, MFE-2 signature: config struct instead of two ports.
        // An empty app dist — `/` serves v2's placeholder.
        let _ = menger::shell::serve(menger::shell::ShellConfig {
            port: shell_port,
            ws_port: 4455,
            app: menger::shell::AppSource::Dir(app_dist),
            env: None,
            login: None,
            allowed_hosts: Vec::new(),
            https: false,
            metadata: None,
            framing: Default::default(),
            ui: None,
            studio: None, media: None,
            mcp: None,
        })
        .await;
    });

    let mut resp = Vec::new();
    for _ in 0..50 {
        match tokio::net::TcpStream::connect(("127.0.0.1", shell_port)).await {
            Ok(mut s) => {
                s.write_all(format!("GET / HTTP/1.0\r\nHost: localhost:{shell_port}\r\n\r\n").as_bytes())
                    .await
                    .unwrap();
                s.read_to_end(&mut resp).await.unwrap();
                break;
            }
            Err(_) => tokio::time::sleep(Duration::from_millis(50)).await,
        }
    }
    let text = String::from_utf8_lossy(&resp);
    assert!(text.starts_with("HTTP/1.0 200") || text.starts_with("HTTP/1.1 200"), "{text}");
    // V2-3/V2-4: v2 has no frontend — `/` is the placeholder naming the
    // service on the socket's port.
    assert!(text.contains("<title>menger</title>") && text.contains("-P 4455"), "{text}");
}

/// MNG-2 criterion 5: reads are cache-first, observed black-box — after
/// the database location vanishes mid-run, `get` and `list` still answer
/// with full data, and a write answers in-band (never a crash or hang).
#[tokio::test]
async fn reads_survive_db_location_unavailable() {
    let dir = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).expect("tempdir");
    let db_dir = dir.path().join("db");
    std::fs::create_dir_all(&db_dir).unwrap();
    let port = start_server_at(db_dir.join("tickets.db"), None).await;
    let client = connect(port).await;

    // Seed one ticket while the database is healthy.
    let mut sub = call(
        &client,
        "tickets.create",
        json!({
            "epic": "CACHE",
            "title": "survives the outage",
            "kind": "scope",
            "severity": "High",
            "body": "reads never touch storage"
        }),
    )
    .await;
    let ev = content(sub.next().await.unwrap().unwrap());
    assert_eq!(ev["type"], "created");
    let created = ev["ticket"].clone();

    // Make the database location unavailable.
    std::fs::rename(&db_dir, dir.path().join("gone")).unwrap();

    // Reads still answer, with the full ticket, from the cache.
    let mut sub = call(&client, "tickets.get", json!({"id": "CACHE-1"})).await;
    let ev = content(sub.next().await.unwrap().unwrap());
    assert_eq!(ev["type"], "ticket");
    assert_eq!(ev["ticket"], created, "get returns full data from the cache");

    let mut sub = call(&client, "tickets.list", json!({})).await;
    let ev = first_row(sub.next().await.unwrap().unwrap());
    assert_eq!(ev["type"], "ticket");
    assert_eq!(ev["ticket"], created, "list returns full data from the cache");

    // A write during the outage answers in-band: acknowledged only if the
    // engine still made it durable, otherwise an error event — no crash.
    let mut sub = call(
        &client,
        "tickets.create",
        json!({"epic": "CACHE", "title": "written during the outage"}),
    )
    .await;
    let ev = content(sub.next().await.unwrap().unwrap());
    assert!(
        ev["type"] == "created" || ev["type"] == "error",
        "write answered in-band, got {ev}"
    );

    // And reads keep working after the write attempt.
    let mut sub = call(&client, "tickets.get", json!({"id": "CACHE-1"})).await;
    let ev = content(sub.next().await.unwrap().unwrap());
    assert_eq!(ev["type"], "ticket");
    assert_eq!(ev["ticket"]["title"], "survives the outage");
}

/// MNG-2 criterion 7: under a mutation stream with an open `watch`, many
/// concurrent list/get/health calls all complete, and the watcher sees
/// every acknowledged change in order.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_reads_complete_under_mutation_stream() {
    const MUTATIONS: usize = 15;

    let port = start_server().await;
    let writer = connect(port).await;
    let watcher = connect(port).await;
    let mut watch = call(&watcher, "tickets.watch", json!({})).await;

    // Guarantee at least one ticket before the readers start.
    let mut sub = call(
        &writer,
        "tickets.create",
        json!({"epic": "LOAD", "title": "t1", "kind": "scope"}),
    )
    .await;
    assert_eq!(content(sub.next().await.unwrap().unwrap())["type"], "created");

    // Mutation stream: create + status flip, back to back.
    let mutations = tokio::spawn(async move {
        for i in 2..=MUTATIONS {
            let mut sub = call(
                &writer,
                "tickets.create",
                json!({"epic": "LOAD", "title": format!("t{i}")}),
            )
            .await;
            let ev = content(sub.next().await.unwrap().unwrap());
            assert_eq!(ev["type"], "created");
            let id = ev["ticket"]["id"].as_str().unwrap().to_string();
            let mut sub = call(&writer, "tickets.set_status", json!({"id": id, "status": "Ready"}))
                .await;
            let ev = content(sub.next().await.unwrap().unwrap());
            assert_eq!(ev["type"], "status_changed");
        }
    });

    // Concurrent readers on their own connections.
    let mut readers = Vec::new();
    for _ in 0..8 {
        readers.push(tokio::spawn(async move {
            let client = connect(port).await;
            for _ in 0..10 {
                let mut sub = call(&client, "tickets.list", json!({"epic": "LOAD"})).await;
                let ev = first_row(sub.next().await.unwrap().unwrap());
                assert_eq!(ev["type"], "ticket");

                let mut sub = call(&client, "tickets.get", json!({"id": "LOAD-1"})).await;
                let ev = content(sub.next().await.unwrap().unwrap());
                assert_eq!(ev["type"], "ticket");
                assert_eq!(ev["ticket"]["id"], "LOAD-1");

                let mut sub = call(&client, "tickets.health", json!({})).await;
                let ev = content(sub.next().await.unwrap().unwrap());
                assert_eq!(ev["type"], "health");
            }
        }));
    }

    tokio::time::timeout(Duration::from_secs(60), async move {
        mutations.await.expect("mutation stream completed");
        for r in readers {
            r.await.expect("reader completed");
        }
        // The watcher saw every acknowledged change, creates in seq order.
        // Each create after the root also moves the root's `child_count`
        // (MNG-14, SCALE-4), and every create moves LOAD's roll-up (MNG-10; a flip
        // to Ready moves none), and (MENGER-13) each birth and each flip is a
        // transition, so: 3 + (MUTATIONS - 1) × (created, transition,
        // children_changed, group_changed, status_changed, transition).
        let mut next_seq = 1u64;
        for _ in 0..(6 * MUTATIONS - 3) {
            let ev = content(watch.next().await.unwrap().unwrap());
            match ev["type"].as_str().unwrap() {
                "created" => {
                    assert_eq!(ev["ticket"]["seq"], next_seq);
                    next_seq += 1;
                }
                "children_changed" => {
                    assert_eq!(ev["id"], "LOAD-1");
                    assert_eq!(ev["child_count"], next_seq - 2, "follows its create");
                }
                "group_changed" => {
                    assert_eq!(ev["group"]["epic"], "LOAD");
                    assert_eq!(ev["group"]["total"], next_seq - 1, "follows its create");
                }
                "status_changed" | "transition" => {}
                other => panic!("unexpected watch event {other}"),
            }
        }
        assert_eq!(next_seq, MUTATIONS as u64 + 1);
    })
    .await
    .expect("concurrency smoke finished in time");
}

/// MNG-12/14 over the real wire: the first ticket of a prefix must be a
/// scope (in-band error, nothing allocated); a ticket naming no parent
/// hangs under the shortcode root; `set_parent` moves a ticket and the
/// watch-only mirror sees the child (`parent_changed`) as a full ticket and
/// both parents (`children_changed`, `child_removed`) as their ids and new
/// counts (SCALE-4); `list {parent}`
/// agrees; the guards — ancestry, outside the shortcode, missing — cross
/// as in-band errors, and a shortcode's root may be hosted elsewhere.
#[tokio::test]
async fn scope_first_default_parent_and_set_parent_over_the_wire() {
    let port = start_server().await;
    let client = connect(port).await;

    let evs = collect(call(&client, "tickets.create", json!({"epic": "SC", "title": "a build first"})).await).await;
    assert_eq!(evs.len(), 1);
    assert_eq!(evs[0]["type"], "error");
    let message = evs[0]["message"].as_str().unwrap();
    assert!(message.contains("SC") && message.contains("scope"), "names the rule: {message}");
    let evs = collect(call(&client, "tickets.get", json!({"id": "SC-1"})).await).await;
    assert_eq!(evs[0]["type"], "not_found", "nothing was allocated");

    let evs = collect(call(&client, "tickets.create",
        json!({"epic": "SC", "title": "root", "kind": "scope"})).await).await;
    assert_eq!(evs[0]["ticket"]["id"], "SC-1");
    assert!(evs[0]["ticket"]["parent"].is_null(), "the root is top-level");
    let evs = collect(call(&client, "tickets.create", json!({"epic": "SC", "title": "a"})).await).await;
    assert_eq!(evs[0]["ticket"]["id"], "SC-2");
    assert_eq!(evs[0]["ticket"]["parent"], "SC-1", "no parent named: the shortcode root");
    let evs = collect(call(&client, "tickets.create", json!({"epic": "SC", "title": "b"})).await).await;
    assert_eq!(evs[0]["ticket"]["id"], "SC-3");
    assert_eq!(evs[0]["ticket"]["parent"], "SC-1");
    // Another shortcode, hosted under SC-1 through its root.
    let evs = collect(call(&client, "tickets.create",
        json!({"epic": "SD", "title": "sub", "kind": "scope", "parent": "SC-1"})).await).await;
    assert_eq!(evs[0]["ticket"]["parent"], "SC-1", "a root may hang under another shortcode");
    let evs = collect(call(&client, "tickets.create",
        json!({"epic": "SD", "title": "escapes", "parent": "SC-2"})).await).await;
    assert_eq!(evs[0]["type"], "error");
    assert!(evs[0]["message"].as_str().unwrap().contains("outside"), "{}", evs[0]);

    let mut mirror = std::collections::HashMap::new();
    for ev in rows(call(&client, "tickets.list", json!({})).await).await {
        mirror.insert(ev["ticket"]["id"].as_str().unwrap().to_string(), ev["ticket"].clone());
    }
    assert_eq!(mirror["SC-1"]["child_count"], 3);
    let mut watch = call(&client, "tickets.watch", json!({})).await;

    let evs = collect(call(&client, "tickets.set_parent",
        json!({"id": "SC-3", "parent": "SC-2"})).await).await;
    assert_eq!(evs.len(), 1);
    assert_eq!(evs[0]["type"], "parent_changed");
    assert_eq!(evs[0]["ticket"]["id"], "SC-3");
    assert_eq!(evs[0]["ticket"]["parent"], "SC-2");
    assert_eq!(evs[0]["previous"], "SC-1");

    for _ in 0..3 {
        let ev = content(watch.next().await.unwrap().unwrap());
        match ev["type"].as_str().unwrap() {
            "children_changed" => {
                assert_eq!(ev, json!({"type": "children_changed", "id": "SC-2", "added": "SC-3", "child_count": 1}));
            }
            "child_removed" => {
                assert_eq!(ev, json!({"type": "child_removed", "id": "SC-1", "removed": "SC-3", "child_count": 2}));
            }
            _ => {}
        }
        apply(&mut mirror, &ev);
    }
    assert_eq!(mirror["SC-3"]["parent"], "SC-2");
    assert_eq!(mirror["SC-2"]["child_count"], 1);
    assert_eq!(mirror["SC-1"]["child_count"], 2);

    let evs = rows(call(&client, "tickets.list", json!({"parent": "SC-2"})).await).await;
    assert_eq!(evs.len(), 1);
    assert_eq!(evs[0]["ticket"]["id"], "SC-3");

    let evs = collect(call(&client, "tickets.set_parent",
        json!({"id": "SC-1", "parent": "SC-3"})).await).await;
    assert_eq!(evs[0]["type"], "error");
    assert!(evs[0]["message"].as_str().unwrap().contains("own ancestor"), "{}", evs[0]);
    let evs = collect(call(&client, "tickets.set_parent",
        json!({"id": "SC-2", "parent": "SD-1"})).await).await;
    assert_eq!(evs[0]["type"], "error");
    assert!(evs[0]["message"].as_str().unwrap().contains("outside"), "{}", evs[0]);
    let evs = collect(call(&client, "tickets.set_parent",
        json!({"id": "SC-7", "parent": "SC-1"})).await).await;
    assert_eq!(evs[0]["type"], "not_found");
}

/// SDLC-11 over the wire, the ticket's "done when": a doc root, a doc child
/// and a work child under the doc. The doc is born `Doc` (and `watch` says
/// so); `set_status` on a doc — any status — is one in-band `error` that
/// changes nothing, and `Doc` on work is one too; `ready_set` holds the work
/// once it is `Ready` and never a doc, whole or summarized; `list { status:
/// Doc }` finds the docs; the group counts the work in `done/total` and the
/// docs apart; and nothing may be blocked by a doc, nor a doc by anything.
#[tokio::test]
async fn docs_root_a_shortcode_refuse_set_status_in_band_and_are_never_ready() {
    let port = start_server().await;
    let client = connect(port).await;
    let mut watch = call(&client, "tickets.watch", json!({})).await;

    let mut created = Vec::new();
    for params in [
        json!({"epic": "MANIFEST", "title": "the findings", "kind": "doc"}), // MANIFEST-1, the root
        json!({"epic": "MANIFEST", "title": "finding one", "kind": "doc"}),  // MANIFEST-2 → MANIFEST-1
        json!({"epic": "MANIFEST", "title": "act on it", "kind": "build", "parent": "MANIFEST-2"}), // MANIFEST-3
    ] {
        let evs = collect(call(&client, "tickets.create", params).await).await;
        assert_eq!((evs.len(), evs[0]["type"].as_str()), (1, Some("created")), "{evs:?}");
        created.push(evs[0]["ticket"].clone());
    }
    let shape: Vec<(Value, Value, Value, Value)> = created
        .iter()
        .map(|t| (t["id"].clone(), t["kind"].clone(), t["status"].clone(), t["parent"].clone()))
        .collect();
    assert_eq!(
        shape,
        [
            (json!("MANIFEST-1"), json!("doc"), json!("Doc"), Value::Null),
            (json!("MANIFEST-2"), json!("doc"), json!("Doc"), json!("MANIFEST-1")),
            (json!("MANIFEST-3"), json!("build"), json!("Draft"), json!("MANIFEST-2")),
        ]
    );
    let ev = content(watch.next().await.unwrap().unwrap());
    assert_eq!((ev["type"].as_str(), ev["ticket"]["status"].as_str()), (Some("created"), Some("Doc")));

    // set_status on a doc: one in-band error, whatever the status.
    for (id, status) in [("MANIFEST-1", "Ready"), ("MANIFEST-2", "Complete"), ("MANIFEST-2", "active"), ("MANIFEST-2", "Doc")] {
        let evs = collect(call(&client, "tickets.set_status", json!({"id": id, "status": status})).await).await;
        assert_eq!(evs.len(), 1, "{id} {status}: one event, {evs:?}");
        assert_eq!(evs[0]["type"], "error", "{id} {status}");
        let message = evs[0]["message"].as_str().unwrap();
        assert!(message.contains(id) && message.contains("a doc has no lifecycle"), "{message}");
    }
    // …and Doc on work.
    let evs = collect(call(&client, "tickets.set_status", json!({"id": "MANIFEST-3", "status": "Doc"})).await).await;
    assert_eq!(evs[0]["type"], "error");
    assert!(evs[0]["message"].as_str().unwrap().contains("Doc is a doc's own status"), "{evs:?}");

    // The work is promoted; the ready set holds it, and never a doc.
    let evs = collect(call(&client, "tickets.set_status", json!({"id": "MANIFEST-3", "status": "Ready"})).await).await;
    assert_eq!(evs[0]["type"], "status_changed");
    for params in [json!({}), json!({"summary": true}), json!({"epic": "MANIFEST", "summary": true})] {
        let (evs, page) = paged(call(&client, "tickets.work.ready", params.clone()).await).await;
        let ids: Vec<&str> = evs
            .iter()
            .map(|ev| ev["ticket"]["id"].as_str().or(ev["summary"]["id"].as_str()).unwrap())
            .collect();
        assert_eq!(ids, ["MANIFEST-3"], "{params}");
        assert_eq!(page["remaining"], 0);
    }

    // The docs stand where they were born.
    let docs = rows(call(&client, "tickets.list", json!({"status": "Doc", "summary": true})).await).await;
    let docs: Vec<(&str, &str)> = docs
        .iter()
        .map(|ev| (ev["summary"]["id"].as_str().unwrap(), ev["summary"]["status"].as_str().unwrap()))
        .collect();
    assert_eq!(docs, [("MANIFEST-1", "Doc"), ("MANIFEST-2", "Doc")]);

    // One piece of work, two docs, rooted at the doc.
    let items = collect(call(&client, "tickets.nav.groups", json!({})).await).await;
    let groups: Vec<Value> = items.into_iter().map(|ev| ev["group"].clone()).collect();
    assert_eq!(
        groups,
        [json!({
            "epic": "MANIFEST", "shortcode": "MANIFEST", "address": "hypermemetic:MANIFEST", "total": 1, "done": 0, "docs": 2, "root": "MANIFEST-1",
            // AGS-23: the root's row — a doc's, in `Doc`
            "root_row": {"title": "the findings", "status": "Doc", "kind": "doc"},
            "host": null
        })]
    );

    // Nothing waits on a doc; a doc waits on nothing.
    for (params, says) in [
        (json!({"epic": "MANIFEST", "title": "x", "blocked_by": ["MANIFEST-2"]}), "'MANIFEST-2' is a doc"),
        (json!({"epic": "MANIFEST", "title": "x", "kind": "doc", "blocked_by": ["MANIFEST-3"]}), "a doc is never blocked"),
    ] {
        let evs = collect(call(&client, "tickets.create", params).await).await;
        assert_eq!(evs[0]["type"], "error", "{evs:?}");
        assert!(evs[0]["message"].as_str().unwrap().contains(says), "{evs:?}");
    }
    let mut sub = call(&client, "tickets.health", json!({})).await;
    assert_eq!(content(sub.next().await.unwrap().unwrap())["ticket_count"], 3, "nothing else was created");
}

/// WAVE-4 acceptance 3: `tickets.work.waves` over the real protocol — `epics` a
/// JSON list, case-insensitive; one `placed { summary, placement,
/// waiting_on }` per ticket (a summary row: no body), the placement tagged
/// `{"type": "wave", "n": …}` / `{"type": "missing_blocker"}` /
/// `{"type": "done"}`; a blocker followed into a shortcode not asked for;
/// then exactly one `wave_counts`, then `done`. A completion moves a
/// dependent left on the next ask. The errors are in-band data items.
#[tokio::test]
async fn waves_over_the_wire() {
    let port = start_server().await;
    let client = connect(port).await;
    let create = |epic: &str, kind: &str, blocked_by: Value| {
        json!({"epic": epic, "title": format!("a {kind}"), "kind": kind, "blocked_by": blocked_by})
    };
    for params in [
        create("UP", "scope", json!([])),           // UP-1
        create("UP", "build", json!([])),           // UP-2: wave 1
        create("WV", "scope", json!([])),           // WV-1
        create("WV", "build", json!(["UP-2"])),     // WV-2: wave 2, its blocker off-board
        create("WV", "spike", json!(["WV-2"])),     // WV-3: wave 3
        create("WV", "gap", json!(["WV-404"])),     // WV-4: missing
        create("WV", "build", json!([])),           // WV-5: done
        create("WV", "doc", json!([])),             // WV-6: a doc, never placed
    ] {
        let evs = collect(call(&client, "tickets.create", params.clone()).await).await;
        assert_eq!(evs[0]["type"], "created", "{params}: {evs:?}");
    }
    collect(call(&client, "tickets.set_status", json!({"id": "WV-5", "status": "Complete"})).await).await;

    let mut evs = collect(call(&client, "tickets.work.waves", json!({"epics": ["wv"]})).await).await;
    let counts = evs.pop().expect("a waves answer ends with the tally");
    assert_eq!(counts, json!({"type": "wave_counts", "waves": [0, 1, 1], "missing": 1, "done": 1}));
    assert!(evs.iter().all(|e| e["type"] == "placed"), "{evs:?}");
    let got: Vec<(String, Value, Value)> = evs
        .iter()
        .map(|e| (e["summary"]["id"].as_str().unwrap().to_string(), e["placement"].clone(), e["waiting_on"].clone()))
        .collect();
    assert_eq!(
        got,
        vec![
            ("WV-2".into(), json!({"type": "wave", "n": 2}), json!(["UP-2"])),
            ("WV-3".into(), json!({"type": "wave", "n": 3}), json!(["WV-2"])),
            ("WV-4".into(), json!({"type": "missing_blocker"}), json!(["WV-404"])),
            ("WV-5".into(), json!({"type": "done"}), json!([])),
        ]
    );
    assert!(evs[0]["summary"].get("body").is_none(), "a summary row: {}", evs[0]);
    assert_eq!(evs[0]["summary"]["kind"], "build");

    // UP-2 completes: everything behind it moves one wave left.
    collect(call(&client, "tickets.set_status", json!({"id": "UP-2", "status": "Complete"})).await).await;
    let evs = collect(call(&client, "tickets.work.waves", json!({"epics": ["WV", "UP"], "kinds": ["build", "spike"]})).await).await;
    let placed: Vec<(&str, &Value)> = evs
        .iter()
        .filter(|e| e["type"] == "placed")
        .map(|e| (e["summary"]["id"].as_str().unwrap(), &e["placement"]))
        .collect();
    assert_eq!(
        placed,
        vec![
            ("WV-2", &json!({"type": "wave", "n": 1})),
            ("WV-3", &json!({"type": "wave", "n": 2})),
            ("UP-2", &json!({"type": "done"})),
            ("WV-5", &json!({"type": "done"})),
        ],
        "kinds: the gap is not asked for; the done in store order"
    );
    assert_eq!(evs.last().unwrap(), &json!({"type": "wave_counts", "waves": [1, 1], "missing": 0, "done": 2}));

    // In-band errors: one data item, then done.
    let evs = collect(call(&client, "tickets.work.waves", json!({"epics": []})).await).await;
    assert_eq!(evs.len(), 1, "{evs:?}");
    assert_eq!(evs[0]["type"], "error");
    assert!(evs[0]["message"].as_str().unwrap().contains("epics"), "{}", evs[0]);
    // ACCESS-8: a kind outside the six is refused at dispatch.
    let message = invalid(&client, "tickets.work.waves", json!({"epics": ["WV"], "kinds": ["bug"]})).await;
    assert!(message.contains("kinds: unknown kind 'bug'"), "{message}");
}

/// WAVE-10 acceptance 2: a path address over the real protocol — a
/// `PLEXUS/_MENGER/board`-style doc created with `path`, the path on the
/// `created` ticket (and absent from a ticket without one), `get` by the
/// address (the shortcode in any case) answering the same ticket as `get`
/// by its id, `not_found` naming an address that names nothing, and a
/// second holder of the path refused in-band.
#[tokio::test]
async fn a_path_address_over_the_wire() {
    let port = start_server().await;
    let client = connect(port).await;
    let root = collect(call(&client, "tickets.create", json!({"epic": "PLX", "title": "the platform", "kind": "scope"})).await).await;
    assert!(root[0]["ticket"].get("path").is_none(), "no path, no key: {}", root[0]);
    let evs = collect(
        call(
            &client,
            "tickets.create",
            json!({"epic": "PLX", "title": "board: PLX", "kind": "doc", "path": "_MENGER/board", "body": "notes"}),
        )
        .await,
    )
    .await;
    assert_eq!(evs[0]["type"], "created", "{evs:?}");
    let born = &evs[0]["ticket"];
    assert_eq!((born["id"].as_str(), born["parent"].as_str()), (Some("PLX-2"), Some("PLX-1")));
    assert_eq!(born["path"], "_MENGER/board");

    let by_id = collect(call(&client, "tickets.get", json!({"id": "PLX-2"})).await).await;
    for address in ["PLX/_MENGER/board", "plx/_MENGER/board"] {
        let evs = collect(call(&client, "tickets.get", json!({"id": address})).await).await;
        assert_eq!(evs, by_id, "{address}");
        assert_eq!(evs[0]["ticket"]["body"], "notes");
    }
    let evs = collect(call(&client, "tickets.get", json!({"id": "PLX/_MENGER/nothing"})).await).await;
    assert_eq!(evs, vec![json!({"type": "not_found", "id": "PLX/_MENGER/nothing"})]);

    let evs = collect(
        call(&client, "tickets.create", json!({"epic": "PLX", "title": "again", "kind": "doc", "path": "_MENGER/board"})).await,
    )
    .await;
    assert_eq!(evs.len(), 1);
    assert_eq!(evs[0]["type"], "error");
    assert!(evs[0]["message"].as_str().unwrap().contains("PLX-2"), "names the holder: {}", evs[0]);
    let evs = collect(call(&client, "tickets.create", json!({"epic": "PLX", "title": "x", "path": "_MENGER/x"})).await).await;
    assert_eq!(evs[0]["type"], "error", "a build carries no path: {evs:?}");

    // The summary row carries it too.
    let rows = rows(call(&client, "tickets.list", json!({"epic": "PLX", "summary": true})).await).await;
    assert_eq!(rows[1]["summary"]["path"], "_MENGER/board");
    assert!(rows[0]["summary"].get("path").is_none(), "{}", rows[0]);
}

/// ORG-9 over the real protocol: a shortcode may hold hyphens. Before it,
/// `tickets.get --id SYNAPSE-CC-1` was refused at dispatch (`is not a
/// ticket`) and no such shortcode could be made; now the scope is born
/// SYNAPSE-CC-1 under SYNAPSE-1, every read names it, and a malformed code
/// is still refused at dispatch.
#[tokio::test]
async fn a_hyphenated_shortcode_over_the_wire() {
    let port = start_server().await;
    let client = connect(port).await;
    collect(call(&client, "tickets.create", json!({"epic": "SYNAPSE", "title": "synapse", "kind": "scope"})).await).await;
    let evs = collect(
        call(&client, "tickets.create", json!({"epic": "synapse-cc", "title": "synapse-cc", "kind": "scope", "parent": "SYNAPSE-1"})).await,
    )
    .await;
    assert_eq!(evs[0]["type"], "created", "{evs:?}");
    assert_eq!(
        (evs[0]["ticket"]["id"].as_str(), evs[0]["ticket"]["epic"].as_str(), evs[0]["ticket"]["address"].as_str()),
        (Some("SYNAPSE-CC-1"), Some("SYNAPSE-CC"), Some("hypermemetic:SYNAPSE/SYNAPSE-CC-1"))
    );
    collect(call(&client, "tickets.create", json!({"epic": "SYNAPSE-CC", "title": "a gap", "kind": "gap", "parent": "SYNAPSE-CC-1"})).await).await;

    for id in ["SYNAPSE-CC-1", "synapse-cc-1", "SYNAPSE/SYNAPSE-CC-1"] {
        let evs = collect(call(&client, "tickets.get", json!({"id": id})).await).await;
        assert_eq!(evs[0]["ticket"]["id"], "SYNAPSE-CC-1", "{id}: {evs:?}");
    }
    let ids = |rows: Vec<Value>| rows.iter().map(|r| r["summary"]["id"].as_str().unwrap().to_string()).collect::<Vec<_>>();
    let listed = rows(call(&client, "tickets.list", json!({"epic": "SYNAPSE-CC", "summary": true})).await).await;
    assert_eq!(ids(listed), ["SYNAPSE-CC-1", "SYNAPSE-CC-2"]);
    let filtered = rows(call(&client, "tickets.list", json!({"filter": "in SYNAPSE-CC and kind gap", "summary": true})).await).await;
    assert_eq!(ids(filtered), ["SYNAPSE-CC-2"]);
    let under = rows(call(&client, "tickets.list", json!({"filter": "under SYNAPSE-CC-1", "summary": true})).await).await;
    assert_eq!(ids(under), ["SYNAPSE-CC-1", "SYNAPSE-CC-2"]);
    let synapse = rows(call(&client, "tickets.list", json!({"filter": "in SYNAPSE", "summary": true})).await).await;
    assert_eq!(ids(synapse), ["SYNAPSE-1"], "SYNAPSE is its own shortcode");

    // Only the last `-N` is a number; a malformed code is no name at all.
    for bad in ["A--B-1", "-A-1", "A-1-2"] {
        let why = invalid(&client, "tickets.get", json!({"id": bad})).await;
        assert!(why.contains("id"), "{bad}: {why}");
    }
    let why = invalid(&client, "tickets.create", json!({"epic": "A-1", "title": "x", "kind": "scope"})).await;
    assert!(why.contains("epic"), "A-1 is a ticket, never a shortcode: {why}");
}

/// WAVE-6 over the real protocol: every row carries its kept placement —
/// tagged as `waves` tags it, omitted on a doc — and a completion two
/// blockers back from a ticket moves it in one `placements_changed { changes
/// }`, after the write's other events; `watch { epic }` carries only the
/// changes in its shortcode.
#[tokio::test]
async fn kept_placements_over_the_wire() {
    let port = start_server().await;
    let client = connect(port).await;
    let mut created = Vec::new();
    for params in [
        json!({"epic": "UP", "title": "up", "kind": "scope"}),
        json!({"epic": "UP", "title": "y"}),                                   // UP-2: wave 1
        json!({"epic": "DN", "title": "down", "kind": "scope"}),
        json!({"epic": "DN", "title": "x", "blocked_by": ["UP-2"]}),          // DN-2: wave 2
        json!({"epic": "DN", "title": "c", "blocked_by": ["DN-2"]}),          // DN-3: wave 3
        json!({"epic": "DN", "title": "notes", "kind": "doc"}),               // DN-4: a doc
    ] {
        let evs = collect(call(&client, "tickets.create", params.clone()).await).await;
        assert_eq!(evs[0]["type"], "created", "{params}: {evs:?}");
        created.push(evs[0]["ticket"].clone());
    }
    assert_eq!(created[1]["placement"], json!({"type": "wave", "n": 1}));
    assert_eq!(created[4]["placement"], json!({"type": "wave", "n": 3}));
    assert!(created[5].get("placement").is_none(), "a doc carries none: {}", created[5]);
    let rows = rows(call(&client, "tickets.list", json!({"epic": "DN", "summary": true})).await).await;
    assert_eq!(rows[2]["summary"]["placement"], json!({"type": "wave", "n": 3}), "on the summary row");

    let mut all = call(&client, "tickets.watch", json!({})).await;
    let mut down = call(&client, "tickets.watch", json!({"epic": "dn"})).await;
    collect(call(&client, "tickets.set_status", json!({"id": "UP-2", "status": "Complete"})).await).await;
    let ev = content(all.next().await.unwrap().unwrap());
    assert_eq!((ev["type"].as_str(), ev["ticket"]["placement"].clone()), (Some("status_changed"), json!({"type": "done"})));
    let ev = content(all.next().await.unwrap().unwrap());
    assert_eq!(ev["type"], "transition", "MENGER-13: right after its status_changed");
    let ev = content(all.next().await.unwrap().unwrap());
    assert_eq!(ev["type"], "group_changed");
    let ev = content(all.next().await.unwrap().unwrap());
    assert_eq!(
        ev,
        json!({"type": "placements_changed", "changes": [
            {"id": "UP-2", "placement": {"type": "done"}},
            {"id": "DN-2", "placement": {"type": "wave", "n": 1}},
            {"id": "DN-3", "placement": {"type": "wave", "n": 2}},
        ]})
    );
    // DN's watch: nothing of UP's — its ticket, its group, its change.
    let ev = content(down.next().await.unwrap().unwrap());
    assert_eq!(
        ev,
        json!({"type": "placements_changed", "changes": [
            {"id": "DN-2", "placement": {"type": "wave", "n": 1}},
            {"id": "DN-3", "placement": {"type": "wave", "n": 2}},
        ]})
    );
    let ev = collect(call(&client, "tickets.get", json!({"id": "DN-3"})).await).await;
    assert_eq!(ev[0]["ticket"]["placement"], json!({"type": "wave", "n": 2}), "get reads the kept one");
}

/// COMP-3: a `watch`'s events up to and including the first of type `kind`.
async fn watch_until(watch: &mut Subscription<Value>, kind: &str) -> Vec<Value> {
    let mut seen = Vec::new();
    loop {
        let item = tokio::time::timeout(Duration::from_secs(10), watch.next())
            .await
            .unwrap_or_else(|_| panic!("no {kind} after {seen:?}"))
            .expect("the watch is open")
            .unwrap();
        let ev = content(item);
        let last = ev["type"] == kind;
        seen.push(ev);
        if last {
            return seen;
        }
    }
}

// ---------------------------------------------------------------------------
// FILTER-4: the filter on the wire.

/// One call's single answer.
async fn answer(client: &WsClient, method: &str, params: Value) -> Value {
    let mut sub = call(client, method, params).await;
    content(sub.next().await.unwrap().unwrap())
}

/// The next `n` events of a watch, each within a deadline.
async fn next_events(watch: &mut Subscription<Value>, n: usize) -> Vec<Value> {
    let mut out = Vec::new();
    for _ in 0..n {
        let item = tokio::time::timeout(Duration::from_secs(20), watch.next())
            .await
            .expect("an event in time")
            .expect("the watch goes on")
            .unwrap();
        out.push(content(item));
    }
    out
}

/// A watch's events up to and including the first `amended` — the marker
/// write each step ends with, which moves no cell.
async fn until_marker(watch: &mut Subscription<Value>) -> Vec<Value> {
    let mut out = Vec::new();
    loop {
        let ev = next_events(watch, 1).await.remove(0);
        let done = ev["type"] == "amended";
        out.push(ev);
        if done {
            return out;
        }
    }
}

fn kinds(evs: &[Value]) -> Vec<String> {
    evs.iter().map(|e| e["type"].as_str().unwrap().to_string()).collect()
}

/// Acceptance 4: `watch { filter }` over the real protocol — a created match
/// is `created` then `entered`; a status change out of the filter is
/// `status_changed` then `left`; each is its write's last event; `watch {}`
/// sends none of the three; `membership` sends only them; `epic` filters
/// them.
#[tokio::test]
async fn watch_filter_says_entered_and_left_last() {
    let port = start_server().await;
    let client = connect(port).await;
    answer(&client, "tickets.create", json!({"epic": "WF", "title": "root", "kind": "scope"})).await;
    answer(&client, "tickets.create", json!({"epic": "OTH", "title": "other root", "kind": "scope"})).await;
    let filter = "status Draft and kind build";
    let mut plain = call(&client, "tickets.watch", json!({})).await;
    let mut filtered = call(&client, "tickets.watch", json!({"filter": filter})).await;
    let mut members = call(&client, "tickets.watch", json!({"filter": filter, "membership": true})).await;
    let mut in_wf = call(&client, "tickets.watch", json!({"filter": filter, "epic": "wf"})).await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    // A body, not a title: a root's new title moves its group too (AGS-23).
    let marker = |n: u32| json!({"id": "WF-1", "body": format!("marker {n}")});

    // A created match: `created`, …, `entered` last.
    answer(&client, "tickets.create", json!({"epic": "WF", "title": "a build"})).await;
    answer(&client, "tickets.create", json!({"epic": "OTH", "title": "another"})).await;
    answer(&client, "tickets.amend", marker(1)).await;
    let evs = until_marker(&mut filtered).await;
    // MENGER-13: each birth's transition right after its `created`.
    assert_eq!(
        kinds(&evs),
        [
            "created", "transition", "children_changed", "group_changed", "entered", "created", "transition",
            "children_changed", "group_changed", "entered", "amended"
        ],
        "{evs:#?}"
    );
    assert_eq!(evs[4], json!({"type": "entered", "id": "WF-2", "epic": "WF", "shortcode": "WF", "seq": 2}));
    assert_eq!(evs[9], json!({"type": "entered", "id": "OTH-2", "epic": "OTH", "shortcode": "OTH", "seq": 2}));
    let plain_evs = until_marker(&mut plain).await;
    assert_eq!(
        kinds(&plain_evs),
        ["created", "transition", "children_changed", "group_changed", "created", "transition", "children_changed", "group_changed", "amended"]
    );

    // A status change out of it: `status_changed`, then `left`.
    answer(&client, "tickets.set_status", json!({"id": "WF-2", "status": "Ready"})).await;
    answer(&client, "tickets.amend", marker(2)).await;
    let evs = until_marker(&mut filtered).await;
    assert_eq!(kinds(&evs), ["status_changed", "transition", "left", "amended"], "{evs:#?}");
    assert_eq!(evs[2], json!({"type": "left", "id": "WF-2", "epic": "WF", "shortcode": "WF", "seq": 2}));
    assert_eq!(kinds(&until_marker(&mut plain).await), ["status_changed", "transition", "amended"], "watch {{}} sends none of the three");

    // `membership`: only the three; `epic`: only its shortcode's.
    let evs = next_events(&mut members, 3).await;
    assert_eq!(kinds(&evs), ["entered", "entered", "left"]);
    let evs = next_events(&mut in_wf, 1).await;
    assert_eq!(evs[0]["type"], "created");
    let mut evs = until_marker(&mut in_wf).await;
    evs.extend(until_marker(&mut in_wf).await);
    assert_eq!(
        kinds(&evs),
        ["transition", "children_changed", "group_changed", "entered", "amended", "status_changed", "transition", "left", "amended"],
        "OTH's create and its entered are not WF's: {evs:#?}"
    );

    // A filter not taken is one error, and the stream ends.
    let mut bad = call(&client, "tickets.watch", json!({"filter": "text \"x\""})).await;
    let ev = content(bad.next().await.unwrap().unwrap());
    assert_eq!(ev["type"], "error");
    assert!(ev["message"].as_str().unwrap().contains("palette"), "{ev}");
}

/// SDLC-13 acceptance 3: `set_kind` over the real protocol. A doc collection's
/// root made a scope: its answer is `kind_changed { ticket, previous }`; on
/// `watch {}` the `kind_changed` comes first, then the `group_changed` (the
/// root's row, its new kind) and the `placements_changed` (a scope is placed;
/// a doc was not); `watch { filter: "kind scope" }` hears the same, then
/// `entered` for it, last. A build made a complete gap frees what waited on
/// it, in the same order. The refusals are in-band errors naming what they
/// refuse, and the ticket stands as the last change left it.
#[tokio::test]
async fn set_kind_over_the_wire() {
    let port = start_server().await;
    let client = connect(port).await;
    answer(&client, "tickets.create", json!({"epic": "IDW", "title": "identity", "kind": "doc"})).await; // IDW-1
    answer(&client, "tickets.create", json!({"epic": "IDW", "title": "a build"})).await; // IDW-2
    answer(&client, "tickets.create", json!({"epic": "IDW", "title": "waits", "blocked_by": ["IDW-2"]})).await; // IDW-3
    let mut plain = call(&client, "tickets.watch", json!({})).await;
    let mut scopes = call(&client, "tickets.watch", json!({"filter": "kind scope"})).await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    let marker = |n: u32| json!({"id": "IDW-2", "body": format!("marker {n}")});

    // The doc root made a scope: Epic, and placed.
    let ev = answer(&client, "tickets.set_kind", json!({"id": "IDW-1", "kind": "scope"})).await;
    assert_eq!(
        (ev["type"].as_str(), ev["previous"].as_str(), ev["ticket"]["kind"].as_str(), ev["ticket"]["status"].as_str()),
        (Some("kind_changed"), Some("doc"), Some("scope"), Some("Epic")),
        "{ev}"
    );
    assert_eq!(ev["ticket"]["placement"], json!({"type": "wave", "n": 1}));
    answer(&client, "tickets.amend", marker(1)).await;
    let evs = until_marker(&mut plain).await;
    // MENGER-13: the kind's transition, then the status's, which it caused.
    assert_eq!(
        kinds(&evs),
        ["kind_changed", "transition", "transition", "group_changed", "placements_changed", "amended"],
        "{evs:#?}"
    );
    assert_eq!(evs[0], ev, "the watch carries the answer");
    assert_eq!((evs[1]["transition"]["field"].as_str(), evs[1]["transition"]["from"].as_str(), evs[1]["transition"]["to"].as_str()), (Some("kind"), Some("doc"), Some("scope")));
    assert_eq!((evs[2]["transition"]["field"].as_str(), evs[2]["transition"]["to"].as_str()), (Some("status"), Some("Epic")));
    assert_eq!(evs[2]["transition"]["cause"], evs[1]["transition"]["id"], "the status moved because the kind did");
    assert_eq!(
        evs[3]["group"],
        json!({
            "epic": "IDW", "shortcode": "IDW", "address": "hypermemetic:IDW", "total": 3, "done": 0, "docs": 0, "root": "IDW-1",
            "root_row": {"title": "identity", "status": "Epic", "kind": "scope"}, "host": null
        })
    );
    assert_eq!(evs[4]["changes"], json!([{"id": "IDW-1", "placement": {"type": "wave", "n": 1}}]));
    let evs = until_marker(&mut scopes).await;
    assert_eq!(
        kinds(&evs),
        ["kind_changed", "transition", "transition", "group_changed", "placements_changed", "entered", "amended"],
        "{evs:#?}"
    );
    assert_eq!(evs[5], json!({"type": "entered", "id": "IDW-1", "epic": "IDW", "shortcode": "IDW", "seq": 1}));

    // A build made a complete gap: `done` moves, and what waited on it is
    // placed again.
    let ev = answer(&client, "tickets.set_kind", json!({"id": "IDW-2", "kind": "gap", "status": "Complete"})).await;
    assert_eq!((ev["ticket"]["kind"].as_str(), ev["ticket"]["status"].as_str()), (Some("gap"), Some("Complete")), "{ev}");
    answer(&client, "tickets.amend", marker(2)).await;
    let evs = until_marker(&mut plain).await;
    assert_eq!(
        kinds(&evs),
        ["kind_changed", "transition", "transition", "group_changed", "placements_changed", "amended"],
        "{evs:#?}"
    );
    assert_eq!((evs[3]["group"]["total"].as_u64(), evs[3]["group"]["done"].as_u64()), (Some(3), Some(1)));
    assert_eq!(
        evs[4]["changes"],
        json!([{"id": "IDW-2", "placement": {"type": "done"}}, {"id": "IDW-3", "placement": {"type": "wave", "n": 1}}])
    );
    assert_eq!(
        kinds(&until_marker(&mut scopes).await),
        ["kind_changed", "transition", "transition", "group_changed", "placements_changed", "amended"]
    );

    // The refusals, each one in-band error naming what it refuses.
    for (params, says) in [
        (json!({"id": "IDW-2", "kind": "doc"}), "'IDW-3' waits on it"),
        (json!({"id": "IDW-1", "kind": "build"}), "'IDW-1' is the root of IDW"),
        (json!({"id": "IDW-3", "kind": "spike", "status": "Doc"}), "Doc is a doc's own status"),
    ] {
        let evs = collect(call(&client, "tickets.set_kind", params.clone()).await).await;
        assert_eq!(evs.len(), 1, "{params}: {evs:?}");
        assert_eq!(evs[0]["type"], "error", "{params}");
        assert!(evs[0]["message"].as_str().unwrap().contains(says), "{params}: {evs:?}");
    }
    // ACCESS-8: an unknown kind or status, refused at dispatch.
    for (params, says) in [
        (json!({"id": "IDW-3", "kind": "bug"}), "kind: unknown kind 'bug'"),
        (json!({"id": "IDW-3", "kind": "gap", "status": "Shipped"}), "status: unknown status 'shipped'"),
    ] {
        let message = invalid(&client, "tickets.set_kind", params.clone()).await;
        assert!(message.contains(says), "{params}: {message}");
    }
    let ev = answer(&client, "tickets.set_kind", json!({"id": "IDW-9", "kind": "gap"})).await;
    assert_eq!(ev, json!({"type": "not_found", "id": "IDW-9"}));
    // The kind it has: its answer, and nothing on the watch.
    let ev = answer(&client, "tickets.set_kind", json!({"id": "IDW-3", "kind": "build"})).await;
    assert_eq!((ev["type"].as_str(), ev["previous"].as_str()), (Some("kind_changed"), Some("build")));
    answer(&client, "tickets.amend", marker(3)).await;
    assert_eq!(kinds(&until_marker(&mut plain).await), ["amended"], "a no-op sends nothing");

    let ev = answer(&client, "tickets.get", json!({"id": "IDW-1"})).await;
    assert_eq!((ev["ticket"]["kind"].as_str(), ev["ticket"]["status"].as_str()), (Some("scope"), Some("Epic")));
}

/// FILTER-13 over the real protocol: `tree` — a parent's children and a
/// shortcode's top row that match or hold a match, each a `node { summary,
/// matched, below }`, then one `page` with no `first`; one address, or one
/// error; `under` taken everywhere — and `watch { filter }` asking `under`:
/// a re-parent, which moves no cell, enters and leaves the moved subtree,
/// as the write's last events.
#[tokio::test]
async fn tree_and_under_over_the_wire() {
    let port = start_server().await;
    let client = connect(port).await;
    let mk = |kind: &str, parent: Option<&str>| json!({"epic": "TW", "title": format!("a {kind}"), "kind": kind, "parent": parent});
    answer(&client, "tickets.create", mk("scope", None)).await; // TW-1
    answer(&client, "tickets.create", mk("execution", Some("TW-1"))).await; // TW-2
    answer(&client, "tickets.create", mk("execution", Some("TW-1"))).await; // TW-3
    answer(&client, "tickets.create", mk("build", Some("TW-2"))).await; // TW-4
    answer(&client, "tickets.create", mk("build", Some("TW-3"))).await; // TW-5
    answer(&client, "tickets.create", mk("doc", Some("TW-4"))).await; // TW-6
    answer(&client, "tickets.set_status", json!({"id": "TW-4", "status": "Ready"})).await;

    let nodes = |evs: &[Value]| -> Vec<(String, bool, u64)> {
        evs.iter()
            .map(|e| {
                assert_eq!(e["type"], "node", "{e}");
                (e["summary"]["id"].as_str().unwrap().to_string(), e["matched"].as_bool().unwrap(), e["below"].as_u64().unwrap())
            })
            .collect()
    };
    let (rows, page) = paged(call(&client, "tickets.nav.tree", json!({"parent": "TW-1", "filter": "status Ready"})).await).await;
    assert_eq!(nodes(&rows), [("TW-2".to_string(), false, 1)], "TW-2 holds the Ready build; TW-3 holds nothing");
    assert_eq!(page, json!({"type": "page", "next": null, "remaining": 0}), "no `first`");
    assert_eq!(rows[0]["summary"]["title"], "a execution", "the row is the summary");
    let (rows, _) = paged(call(&client, "tickets.nav.tree", json!({"epic": "tw", "filter": "status Ready"})).await).await;
    assert_eq!(nodes(&rows), [("TW-1".to_string(), false, 1)], "the shortcode's top row: its root");
    let (rows, page) = paged(call(&client, "tickets.nav.tree", json!({"parent": "TW-1", "filter": "", "limit": 1})).await).await;
    assert_eq!(nodes(&rows), [("TW-2".to_string(), true, 2)]);
    assert_eq!(page, json!({"type": "page", "next": "TW-2", "remaining": 1}));
    let (rows, _) = paged(call(&client, "tickets.nav.tree", json!({"parent": "TW-1", "filter": "kind doc and under TW-2"})).await).await;
    assert_eq!(nodes(&rows), [("TW-2".to_string(), false, 1)]);
    for bad in [json!({"filter": ""}), json!({"parent": "TW-1", "epic": "TW", "filter": ""}), json!({"parent": "TW-1", "filter": "text \"x\""})] {
        let evs = collect(call(&client, "tickets.nav.tree", bad.clone()).await).await;
        assert_eq!((evs.len(), evs[0]["type"].as_str()), (1, Some("error")), "{bad}: {evs:?}");
    }
    let counts = collect(call(&client, "tickets.nav.counts", json!({"filter": "under TW-2"})).await).await;
    assert_eq!(counts, [json!({"type": "count", "epic": "TW", "shortcode": "TW", "held": 3})], "TW-2, TW-4, TW-6");

    // `watch { filter }` asking `under`: TW-5 moved under TW-2 enters; back, it leaves.
    let mut watch = call(&client, "tickets.watch", json!({"filter": "under TW-2"})).await;
    let mut members = call(&client, "tickets.watch", json!({"filter": "under TW-2", "membership": true})).await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    answer(&client, "tickets.set_parent", json!({"id": "TW-5", "parent": "TW-2"})).await;
    answer(&client, "tickets.amend", json!({"id": "TW-1", "body": "marker"})).await;
    let evs = until_marker(&mut watch).await;
    let k = kinds(&evs);
    assert_eq!(k.first().map(String::as_str), Some("parent_changed"), "{evs:#?}");
    assert_eq!(&k[k.len() - 2..], ["entered", "amended"], "entered is the move's last event: {evs:#?}");
    assert_eq!(evs[k.len() - 2], json!({"type": "entered", "id": "TW-5", "epic": "TW", "shortcode": "TW", "seq": 5}));
    answer(&client, "tickets.set_parent", json!({"id": "TW-5", "parent": "TW-3"})).await;
    let evs = next_events(&mut members, 2).await;
    assert_eq!(evs, [json!({"type": "entered", "id": "TW-5", "epic": "TW", "shortcode": "TW", "seq": 5}), json!({"type": "left", "id": "TW-5", "epic": "TW", "shortcode": "TW", "seq": 5})]);
}

/// FILTER-23 over the real protocol: `tree { matches_only }` — the nav that
/// shows only matches. A ticket that does not match is no row: its matches
/// are answered at its nearest matching ancestor, or at the shortcode's top;
/// a hosted shortcode's row follows its host up; paged in the new place;
/// without the flag, FILTER-13's answer.
#[tokio::test]
async fn tree_matches_only_over_the_wire() {
    let port = start_server().await;
    let client = connect(port).await;
    let mk = |epic: &str, kind: &str, parent: Option<&str>| json!({"epic": epic, "title": format!("a {kind}"), "kind": kind, "parent": parent});
    answer(&client, "tickets.create", mk("TM", "scope", None)).await; // TM-1
    answer(&client, "tickets.create", mk("TM", "execution", Some("TM-1"))).await; // TM-2
    answer(&client, "tickets.create", mk("TM", "build", Some("TM-2"))).await; // TM-3
    answer(&client, "tickets.create", mk("TM", "build", Some("TM-3"))).await; // TM-4
    answer(&client, "tickets.create", mk("TN", "scope", Some("TM-2"))).await; // TN-1, hosted at TM-2
    answer(&client, "tickets.create", mk("TN", "build", Some("TN-1"))).await; // TN-2

    let nodes = |evs: &[Value]| -> Vec<(String, bool, u64)> {
        evs.iter()
            .map(|e| {
                assert_eq!(e["type"], "node", "{e}");
                (e["summary"]["id"].as_str().unwrap().to_string(), e["matched"].as_bool().unwrap(), e["below"].as_u64().unwrap())
            })
            .collect()
    };
    let row = |id: &str, m: bool, b: u64| (id.to_string(), m, b);
    // The heading TM: TM-1 and TM-2 are not builds; TM-3 moves up, TM-4
    // stays under it, and TN's row follows its host TM-2 up to TM's top.
    let (rows, page) = paged(call(&client, "tickets.nav.tree", json!({"epic": "TM", "filter": "kind build", "matches_only": true})).await).await;
    assert_eq!(nodes(&rows), [row("TM-3", true, 1), row("TN-1", false, 1)]);
    assert_eq!(page, json!({"type": "page", "next": null, "remaining": 0}));
    let (rows, _) = paged(call(&client, "tickets.nav.tree", json!({"parent": "TM-1", "filter": "kind build", "matches_only": true})).await).await;
    assert_eq!(nodes(&rows), [row("TM-3", true, 1), row("TN-1", false, 1)], "the root's address answers the same");
    let (rows, _) = paged(call(&client, "tickets.nav.tree", json!({"parent": "TM-3", "filter": "kind build", "matches_only": true})).await).await;
    assert_eq!(nodes(&rows), [row("TM-4", true, 0)]);
    let (rows, _) = paged(call(&client, "tickets.nav.tree", json!({"epic": "TN", "filter": "kind build", "matches_only": true})).await).await;
    assert_eq!(nodes(&rows), [row("TN-2", true, 0)]);
    // Paged across the matches and the shortcodes after them.
    let (rows, page) = paged(call(&client, "tickets.nav.tree", json!({"epic": "TM", "filter": "kind build", "matches_only": true, "limit": 1})).await).await;
    assert_eq!((nodes(&rows), page), (vec![row("TM-3", true, 1)], json!({"type": "page", "next": "TM-3", "remaining": 1})));
    let (rows, page) = paged(call(&client, "tickets.nav.tree", json!({"epic": "TM", "filter": "kind build", "matches_only": true, "after": "TM-3"})).await).await;
    assert_eq!((nodes(&rows), page), (vec![row("TN-1", false, 1)], json!({"type": "page", "next": null, "remaining": 0})));
    // Without the flag (or with it false), FILTER-13's answer: the holders.
    for off in [json!({"parent": "TM-1", "filter": "kind build"}), json!({"parent": "TM-1", "filter": "kind build", "matches_only": false})] {
        let (rows, _) = paged(call(&client, "tickets.nav.tree", off.clone()).await).await;
        assert_eq!(nodes(&rows), [row("TM-2", false, 3)], "{off}");
    }
    // TM-2 made a build: it is drawn, and TM-3 and TN's row are under it again.
    answer(&client, "tickets.set_kind", json!({"id": "TM-2", "kind": "build"})).await;
    let (rows, _) = paged(call(&client, "tickets.nav.tree", json!({"epic": "TM", "filter": "kind build", "matches_only": true})).await).await;
    assert_eq!(nodes(&rows), [row("TM-2", true, 3)]);
    let (rows, _) = paged(call(&client, "tickets.nav.tree", json!({"parent": "TM-2", "filter": "kind build", "matches_only": true})).await).await;
    assert_eq!(nodes(&rows), [row("TM-3", true, 1), row("TN-1", false, 1)]);
    let evs = collect(call(&client, "tickets.nav.tree", json!({"parent": "TM-1", "epic": "TM", "filter": "", "matches_only": true})).await).await;
    assert_eq!((evs.len(), evs[0]["type"].as_str()), (1, Some("error")), "one address: {evs:?}");
}

/// FILTER-24 over the real protocol: `keep` — the nav's open ticket, drawn
/// whatever the filter. `tree { matches_only, keep }` draws it where a match
/// would be, its matches and a shortcode hosted at it under it, and says it
/// does not match; `list { filter, keep }` pages it in its place and counts
/// it; `counts { filter, keep }` counts it. A match, a root, a ticket that is
/// none: as without it.
#[tokio::test]
async fn keep_over_the_wire() {
    let port = start_server().await;
    let client = connect(port).await;
    let mk = |epic: &str, kind: &str, parent: Option<&str>| json!({"epic": epic, "title": format!("a {kind}"), "kind": kind, "parent": parent});
    answer(&client, "tickets.create", mk("TK", "scope", None)).await; // TK-1
    answer(&client, "tickets.create", mk("TK", "execution", Some("TK-1"))).await; // TK-2
    answer(&client, "tickets.create", mk("TK", "build", Some("TK-2"))).await; // TK-3
    answer(&client, "tickets.create", mk("TK", "build", Some("TK-1"))).await; // TK-4
    answer(&client, "tickets.create", mk("TL", "scope", Some("TK-2"))).await; // TL-1, hosted at TK-2
    answer(&client, "tickets.create", mk("TL", "build", Some("TL-1"))).await; // TL-2

    let nodes = |evs: &[Value]| -> Vec<(String, bool, u64)> {
        evs.iter()
            .map(|e| (e["summary"]["id"].as_str().unwrap().to_string(), e["matched"].as_bool().unwrap(), e["below"].as_u64().unwrap()))
            .collect()
    };
    let row = |id: &str, m: bool, b: u64| (id.to_string(), m, b);
    let tree = |extra: Value| {
        let mut p = json!({"epic": "TK", "filter": "kind build", "matches_only": true});
        p.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
        p
    };
    let (rows, _) = paged(call(&client, "tickets.nav.tree", tree(json!({}))).await).await;
    assert_eq!(nodes(&rows), [row("TK-3", true, 0), row("TK-4", true, 0), row("TL-1", false, 1)], "without keep");
    let (rows, page) = paged(call(&client, "tickets.nav.tree", tree(json!({"keep": "TK-2"}))).await).await;
    assert_eq!(nodes(&rows), [row("TK-2", false, 2), row("TK-4", true, 0)], "TK-2 in TK-3's place");
    assert_eq!(page, json!({"type": "page", "next": null, "remaining": 0}));
    let (rows, _) = paged(call(&client, "tickets.nav.tree", json!({"parent": "TK-2", "filter": "kind build", "matches_only": true, "keep": "tk-2"})).await).await;
    assert_eq!(nodes(&rows), [row("TK-3", true, 0), row("TL-1", false, 1)], "its build and TL's row under it");
    // ACCESS-8: a blank `keep` is no ticket — refused at dispatch (it was none).
    assert!(invalid(&client, "tickets.nav.tree", tree(json!({"keep": ""}))).await.contains("keep: "));
    for keep in ["TK-3", "TK-1", "NONE-1"] {
        let (rows, _) = paged(call(&client, "tickets.nav.tree", tree(json!({"keep": keep}))).await).await;
        assert_eq!(nodes(&rows), [row("TK-3", true, 0), row("TK-4", true, 0), row("TL-1", false, 1)], "keep {keep:?}");
    }

    let ids = |evs: &[Value]| evs.iter().map(|e| e["summary"]["id"].as_str().unwrap().to_string()).collect::<Vec<_>>();
    let (rows, page) = paged(call(&client, "tickets.list", json!({"filter": "kind build", "keep": "TK-2", "summary": true})).await).await;
    assert_eq!(ids(&rows), ["TK-2", "TK-3", "TK-4", "TL-2"]);
    assert_eq!((page["first"].clone(), page["remaining"].clone()), (json!(0), json!(0)));
    let (rows, page) = paged(call(&client, "tickets.list", json!({"filter": "kind build", "keep": "TK-2", "summary": true, "after": "TK-2", "limit": 0})).await).await;
    assert_eq!((rows.len(), page["first"].clone()), (0, json!(1)), "TK-2 sits at 0");
    let (rows, _) = paged(call(&client, "tickets.list", json!({"filter": "kind build", "keep": "TK-2", "summary": true, "at": 1, "limit": 1})).await).await;
    assert_eq!(ids(&rows), ["TK-3"]);
    let (rows, _) = paged(call(&client, "tickets.list", json!({"filter": "kind build", "keep": "TK-2", "epic": "TL", "summary": true})).await).await;
    assert_eq!(ids(&rows), ["TL-2"], "outside the sugar: not kept");

    let counts = |evs: Vec<Value>| evs.into_iter().map(|e| (e["epic"].as_str().unwrap().to_string(), e["held"].as_u64().unwrap())).collect::<Vec<_>>();
    assert_eq!(counts(collect(call(&client, "tickets.nav.counts", json!({"filter": "kind build"})).await).await), [("TK".to_string(), 2), ("TL".to_string(), 1)]);
    assert_eq!(counts(collect(call(&client, "tickets.nav.counts", json!({"filter": "kind build", "keep": "TK-2"})).await).await), [("TK".to_string(), 3), ("TL".to_string(), 1)]);
    assert_eq!(counts(collect(call(&client, "tickets.nav.counts", json!({"filter": "kind build", "keep": "TK-3"})).await).await), [("TK".to_string(), 2), ("TL".to_string(), 1)], "a match once");
    assert_eq!(counts(collect(call(&client, "tickets.nav.counts", json!({"filter": "kind doc", "keep": "TL-1"})).await).await), [("TL".to_string(), 1)], "a shortcode with no match");
}

/// Acceptance 4: completing a blocker, under `watch { filter: "ready" }`, is
/// `status_changed` and then one `entered` per ticket it unblocked, in the
/// store's order.
#[tokio::test]
async fn completing_a_blocker_enters_what_it_unblocked_in_order() {
    let port = start_server().await;
    let client = connect(port).await;
    answer(&client, "tickets.create", json!({"epic": "UB", "title": "root", "kind": "scope"})).await;
    answer(&client, "tickets.create", json!({"epic": "UB", "title": "the blocker"})).await;
    for i in 3..=6 {
        answer(&client, "tickets.create", json!({"epic": "UB", "title": format!("waits {i}"), "blocked_by": ["UB-2"]})).await;
        answer(&client, "tickets.set_status", json!({"id": format!("UB-{i}"), "status": "Ready"})).await;
    }
    // UB-6 also waits on the root, which stays open.
    answer(&client, "tickets.create", json!({"epic": "UB", "title": "waits on two", "blocked_by": ["UB-2", "UB-1"]})).await;
    answer(&client, "tickets.set_status", json!({"id": "UB-7", "status": "Ready"})).await;
    let mut watch = call(&client, "tickets.watch", json!({"filter": "ready"})).await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    answer(&client, "tickets.set_status", json!({"id": "UB-2", "status": "Complete"})).await;
    answer(&client, "tickets.amend", json!({"id": "UB-1", "body": "marker"})).await;
    let evs = until_marker(&mut watch).await;
    let k = kinds(&evs);
    assert_eq!(k[0], "status_changed", "{evs:#?}");
    let entered: Vec<&Value> = evs.iter().filter(|e| e["type"] == "entered").collect();
    assert_eq!(
        entered.iter().map(|e| e["id"].as_str().unwrap()).collect::<Vec<_>>(),
        ["UB-3", "UB-4", "UB-5", "UB-6"],
        "each ticket it unblocked, in order — not UB-7, which waits on UB-1 too: {evs:#?}"
    );
    let first_entered = k.iter().position(|t| t == "entered").unwrap();
    assert!(k[first_entered..k.len() - 1].iter().all(|t| t == "entered"), "the write's last events: {k:?}");
    assert!(!k.contains(&"left".to_string()));
}

/// Acceptance 4: a crossing that unblocks 300 gives one `recount` per
/// shortcode it moved — the count there as it left it — and no `entered`.
#[tokio::test]
async fn a_crossing_that_unblocks_three_hundred_recounts() {
    let port = start_server().await;
    let client = connect(port).await;
    answer(&client, "tickets.create", json!({"epic": "RA", "title": "root", "kind": "scope"})).await;
    answer(&client, "tickets.create", json!({"epic": "RB", "title": "root", "kind": "scope"})).await;
    answer(&client, "tickets.create", json!({"epic": "RA", "title": "the service"})).await;
    for i in 0..300 {
        let epic = if i % 3 == 0 { "RB" } else { "RA" };
        let ev = answer(&client, "tickets.create", json!({"epic": epic, "title": format!("w{i}"), "blocked_by": ["RA-2"]})).await;
        let id = ev["ticket"]["id"].as_str().unwrap().to_string();
        answer(&client, "tickets.set_status", json!({"id": id, "status": "Ready"})).await;
    }
    let mut watch = call(&client, "tickets.watch", json!({"filter": "ready"})).await;
    let mut plain = call(&client, "tickets.watch", json!({})).await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    answer(&client, "tickets.set_status", json!({"id": "RA-2", "status": "Complete"})).await;
    answer(&client, "tickets.amend", json!({"id": "RA-1", "body": "marker"})).await;
    let evs = until_marker(&mut watch).await;
    let recounts: Vec<&Value> = evs.iter().filter(|e| e["type"] == "recount").collect();
    assert_eq!(recounts, [&json!({"type": "recount", "epic": "RA", "shortcode": "RA", "held": 200}), &json!({"type": "recount", "epic": "RB", "shortcode": "RB", "held": 100})], "{evs:#?}");
    assert!(!evs.iter().any(|e| e["type"] == "entered" || e["type"] == "left"), "no entered past 256");
    let counts = collect(call(&client, "tickets.nav.counts", json!({"filter": "ready"})).await).await;
    assert_eq!(counts, [json!({"type": "count", "epic": "RA", "shortcode": "RA", "held": 200}), json!({"type": "count", "epic": "RB", "shortcode": "RB", "held": 100})]);
    let plain_evs = until_marker(&mut plain).await;
    assert!(!plain_evs.iter().any(|e| matches!(e["type"].as_str(), Some("entered" | "left" | "recount"))), "watch {{}}");
}

/// The new reads' shapes over the wire: `counts`, `facets`, `list { filter,
/// at }` with `page.first` — and a page asked neither, which carries no
/// `first` at all; `at` with `after`, and a filter not taken, one error.
#[tokio::test]
async fn counts_facets_and_filtered_pages_over_the_wire() {
    let port = start_server().await;
    let client = connect(port).await;
    answer(&client, "tickets.create", json!({"epic": "FW", "title": "root", "kind": "scope"})).await;
    for i in 0..6 {
        answer(&client, "tickets.create", json!({"epic": "FW", "title": format!("t{i}"), "severity": if i % 2 == 0 { "High" } else { "Low" }})).await;
    }
    answer(&client, "tickets.set_status", json!({"id": "FW-3", "status": "Ready"})).await;

    // Neither filter nor at: the page as it always was — no `first`.
    let (_, page) = paged(call(&client, "tickets.list", json!({"summary": true, "limit": 2})).await).await;
    assert_eq!(page, json!({"type": "page", "next": "FW-2", "remaining": 5}));
    let (_, page) = paged(call(&client, "tickets.list", json!({"limit": 2})).await).await;
    assert!(page.get("first").is_none(), "{page}");

    let (rows, page) = paged(call(&client, "tickets.list", json!({"filter": "severity high", "summary": true, "limit": 2})).await).await;
    assert_eq!(rows.iter().map(|r| r["summary"]["id"].as_str().unwrap()).collect::<Vec<_>>(), ["FW-2", "FW-4"]);
    assert_eq!(page, json!({"type": "page", "next": "FW-4", "remaining": 1, "first": 0}));
    let (rows, page) = paged(call(&client, "tickets.list", json!({"filter": "severity high", "at": 2, "limit": 5})).await).await;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["ticket"]["id"], "FW-6", "whole tickets at a position");
    assert_eq!(page, json!({"type": "page", "next": null, "remaining": 0, "first": 2}));
    let (rows, page) = paged(call(&client, "tickets.list", json!({"at": 9, "summary": true})).await).await;
    assert!(rows.is_empty());
    assert_eq!(page, json!({"type": "page", "next": null, "remaining": 0, "first": 9}), "at past the end");
    let (_, page) = paged(call(&client, "tickets.list", json!({"filter": "not severity none", "after": "FW-3", "summary": true, "limit": 1})).await).await;
    // FW-2 and FW-3 match at or before the cursor: the page starts at 2.
    assert_eq!(page, json!({"type": "page", "next": "FW-4", "remaining": 3, "first": 2}));

    let both = collect(call(&client, "tickets.list", json!({"at": 1, "after": "FW-1"})).await).await;
    assert_eq!(both.len(), 1);
    assert_eq!(both[0]["type"], "error");
    // ACCESS-8: a filter the language does not read is no FilterText —
    // refused at dispatch, every bad chunk named.
    let message = invalid(&client, "tickets.list", json!({"filter": "tag x and ready and nonsense"})).await;
    assert!(message.contains("filter: ") && message.contains("`tag x`") && message.contains("`nonsense`"), "{message}");

    let counts = collect(call(&client, "tickets.nav.counts", json!({"filter": "severity in (high, low)"})).await).await;
    assert_eq!(counts, [json!({"type": "count", "epic": "FW", "shortcode": "FW", "held": 6})]);
    let all = collect(call(&client, "tickets.nav.counts", json!({})).await).await;
    assert_eq!(all, [json!({"type": "count", "epic": "FW", "shortcode": "FW", "held": 7})]);
    let none = collect(call(&client, "tickets.nav.counts", json!({"filter": "category active"})).await).await;
    assert!(none.is_empty(), "a shortcode with no match has no count");

    let facets = collect(call(&client, "tickets.nav.facets", json!({"filter": "severity high and status Draft"})).await).await;
    let of = |field: &str| -> Vec<(String, u64)> {
        facets
            .iter()
            .filter(|f| f["field"] == field)
            .map(|f| (f["value"].as_str().unwrap().to_string(), f["held"].as_u64().unwrap()))
            .collect()
    };
    // severity's menu: every Draft ticket's severity (FW-1 has none;
    // FW-2, 4, 6 are high; FW-5, 7 low — FW-3 is Ready); status's: every
    // high ticket's status (all three Draft); the rest: both groups.
    assert_eq!(of("severity"), [("high".to_string(), 3), ("low".to_string(), 2), ("none".to_string(), 1)]);
    assert_eq!(of("status"), [("Draft".to_string(), 3)]);
    assert_eq!(of("in"), [("FW".to_string(), 3)]);
    assert_eq!(of("blocked"), [("no".to_string(), 3)]);
    assert_eq!(facets[0], json!({"type": "facet", "field": "status", "value": "Draft", "held": 3}));
    let base = collect(call(&client, "tickets.nav.facets", json!({"base": "status Ready"})).await).await;
    assert!(base.iter().filter(|f| f["field"] == "status").all(|f| f["value"] == "Ready"), "base is never set aside: {base:?}");
}

/// FILTER-21 over the real protocol: `menger doc` admits exactly the tickets
/// whose path starts with `_MENGER/` — the ticket's own example, `list {
/// filter: "menger doc" }` answering the board note — and `not menger doc`
/// the rest, on every read that takes a filter: `list`, `counts`, `facets`
/// (a flag's `yes` / `no`), `tree`, `find`, `member`, and `watch { filter }`,
/// where a board note born is `entered`.
#[tokio::test]
async fn menger_doc_is_a_filter_on_every_read_over_the_wire() {
    let port = start_server().await;
    let client = connect(port).await;
    let ids = |rows: &[Value]| -> Vec<String> { rows.iter().map(|r| r["summary"]["id"].as_str().unwrap().to_string()).collect() };
    answer(&client, "tickets.create", json!({"epic": "PLEXUS", "title": "the platform", "kind": "scope"})).await; // PLEXUS-1
    let board = json!({"epic": "PLEXUS", "title": "board: PLEXUS", "kind": "doc", "path": "_MENGER/board", "body": "notes"});
    answer(&client, "tickets.create", board).await; // PLEXUS-2
    answer(&client, "tickets.create", json!({"epic": "PLEXUS", "title": "a board finding", "kind": "doc", "parent": "PLEXUS-1"})).await; // PLEXUS-3
    answer(&client, "tickets.create", json!({"epic": "PLEXUS", "title": "build the board", "parent": "PLEXUS-1"})).await; // PLEXUS-4

    // The ticket's example: the board note, and nothing else.
    let (rows, page) = paged(call(&client, "tickets.list", json!({"filter": "menger doc", "summary": true})).await).await;
    assert_eq!(ids(&rows), ["PLEXUS-2"]);
    assert_eq!((rows[0]["summary"]["title"].as_str(), rows[0]["summary"]["path"].as_str()), (Some("board: PLEXUS"), Some("_MENGER/board")));
    assert_eq!(page, json!({"type": "page", "next": null, "remaining": 0, "first": 0}));
    let (rows, _) = paged(call(&client, "tickets.list", json!({"filter": "not menger doc", "summary": true})).await).await;
    assert_eq!(ids(&rows), ["PLEXUS-1", "PLEXUS-3", "PLEXUS-4"]);
    let (rows, page) = paged(call(&client, "tickets.list", json!({"filter": "Not Menger Doc", "at": 1, "limit": 1, "summary": true})).await).await;
    assert_eq!((ids(&rows), page["first"].as_u64()), (vec!["PLEXUS-3".to_string()], Some(1)), "read whatever its case, paged by position");

    // counts, and facets: a flag category, yes / no.
    let counts = collect(call(&client, "tickets.nav.counts", json!({"filter": "menger doc"})).await).await;
    assert_eq!(counts, [json!({"type": "count", "epic": "PLEXUS", "shortcode": "PLEXUS", "held": 1})]);
    let counts = collect(call(&client, "tickets.nav.counts", json!({"filter": "not menger doc and kind doc"})).await).await;
    assert_eq!(counts, [json!({"type": "count", "epic": "PLEXUS", "shortcode": "PLEXUS", "held": 1})], "PLEXUS-3, the ordinary doc");
    let menu = |facets: &[Value], field: &str| -> Vec<(String, u64)> {
        facets
            .iter()
            .filter(|f| f["field"] == field)
            .map(|f| (f["value"].as_str().unwrap().to_string(), f["held"].as_u64().unwrap()))
            .collect()
    };
    let facets = collect(call(&client, "tickets.nav.facets", json!({})).await).await;
    assert_eq!(menu(&facets, "menger doc"), [("yes".to_string(), 1), ("no".to_string(), 3)]);
    let fields: Vec<&str> = facets.iter().map(|f| f["field"].as_str().unwrap()).collect();
    let at = |name: &str| fields.iter().position(|f| *f == name).unwrap();
    assert!(at("has commits") < at("menger doc") && at("menger doc") < at("in"), "after the other flags, before `in`: {fields:?}");
    let facets = collect(call(&client, "tickets.nav.facets", json!({"filter": "not menger doc"})).await).await;
    assert_eq!(menu(&facets, "menger doc"), [("yes".to_string(), 1), ("no".to_string(), 3)], "its own group set aside");
    assert_eq!(menu(&facets, "kind"), [("scope".to_string(), 1), ("build".to_string(), 1), ("doc".to_string(), 1)], "the rest under it");
    let facets = collect(call(&client, "tickets.nav.facets", json!({"base": "kind doc"})).await).await;
    assert_eq!(menu(&facets, "menger doc"), [("yes".to_string(), 1), ("no".to_string(), 1)]);

    // tree: the nav's default — the root and its work, the board note gone.
    let nodes = |evs: &[Value]| -> Vec<(String, bool, u64)> {
        evs.iter()
            .map(|e| (e["summary"]["id"].as_str().unwrap().to_string(), e["matched"].as_bool().unwrap(), e["below"].as_u64().unwrap()))
            .collect()
    };
    let (rows, _) = paged(call(&client, "tickets.nav.tree", json!({"epic": "plexus", "filter": "not menger doc"})).await).await;
    assert_eq!(nodes(&rows), [("PLEXUS-1".to_string(), true, 2)]);
    let (rows, _) = paged(call(&client, "tickets.nav.tree", json!({"parent": "PLEXUS-1", "filter": "not menger doc"})).await).await;
    assert_eq!(nodes(&rows), [("PLEXUS-3".to_string(), true, 0), ("PLEXUS-4".to_string(), true, 0)]);
    let (rows, _) = paged(call(&client, "tickets.nav.tree", json!({"parent": "PLEXUS-1", "filter": "menger doc"})).await).await;
    assert_eq!(nodes(&rows), [("PLEXUS-2".to_string(), true, 0)]);

    // find and member.
    let (rows, _) = paged(call(&client, "tickets.find", json!({"query": "board"})).await).await;
    assert_eq!(ids(&rows), ["PLEXUS-2", "PLEXUS-3", "PLEXUS-4"]);
    let (rows, _) = paged(call(&client, "tickets.find", json!({"query": "board", "filter": "not menger doc"})).await).await;
    assert_eq!(ids(&rows), ["PLEXUS-3", "PLEXUS-4"]);
    let members = collect(call(&client, "tickets.nav.member", json!({"ids": ["PLEXUS-2", "PLEXUS-3"], "filter": "menger doc"})).await).await;
    assert_eq!(
        members,
        [json!({"type": "member", "id": "PLEXUS-2", "matched": true}), json!({"type": "member", "id": "PLEXUS-3", "matched": false})]
    );

    // watch { filter }: another shortcode's board note born enters; its root does not.
    let mut watch = call(&client, "tickets.watch", json!({"filter": "menger doc"})).await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    answer(&client, "tickets.create", json!({"epic": "MNG", "title": "menger", "kind": "scope"})).await;
    answer(&client, "tickets.create", json!({"epic": "MNG", "title": "board: MNG", "kind": "doc", "path": "_MENGER/board"})).await;
    answer(&client, "tickets.amend", json!({"id": "PLEXUS-1", "body": "marker"})).await;
    let evs = until_marker(&mut watch).await;
    let entered: Vec<&str> = evs.iter().filter(|e| e["type"] == "entered").map(|e| e["id"].as_str().unwrap()).collect();
    assert_eq!(entered, ["MNG-2"], "{evs:#?}");
    assert!(!evs.iter().any(|e| e["type"] == "left"));
}

/// MENGER-12 over the real protocol: `tickets.label` moves a ticket's labels
/// (normalized, sorted, each once; a no-op answers empty and writes
/// nothing; a bad label is one in-band error), the row carries them, and
/// `label <name>` is a filter on every read — `list`, `counts`, `facets`
/// (`label` a count per label), `member` — and on `watch { filter }`, where
/// a label added or removed enters or leaves the Home panel's default
/// `label needs-you`.
#[tokio::test]
async fn labels_are_filters_over_the_wire() {
    let port = start_server().await;
    let client = connect(port).await;
    let ids = |rows: &[Value]| -> Vec<String> { rows.iter().map(|r| r["summary"]["id"].as_str().unwrap().to_string()).collect() };
    answer(&client, "tickets.create", json!({"epic": "HOME", "title": "home", "kind": "scope"})).await; // HOME-1
    let asks = "<m-input id=\"notes\" kind=\"text\"><m-question data-field=\"question\">Anything else?</m-question></m-input>";
    answer(&client, "tickets.create", json!({"epic": "HOME", "title": "asks", "parent": "HOME-1", "body": asks})).await; // HOME-2
    answer(&client, "tickets.create", json!({"epic": "HOME", "title": "labelled", "parent": "HOME-1"})).await; // HOME-3
    answer(&client, "tickets.create", json!({"epic": "HOME", "title": "quiet", "parent": "HOME-1"})).await; // HOME-4

    // label: normalized, sorted, each once; what moved, in the order given.
    let ev = answer(&client, "tickets.label", json!({"id": "home-3", "add": ["Needs-You", "p1", "p1"]})).await;
    assert_eq!(ev["type"], "labels_changed", "{ev}");
    assert_eq!((ev["added"].clone(), ev["removed"].clone()), (json!(["needs-you", "p1"]), json!([])));
    assert_eq!(ev["ticket"]["labels"], json!(["needs-you", "p1"]));
    let ev = answer(&client, "tickets.label", json!({"id": "HOME-3", "add": ["p1"], "remove": ["later"]})).await;
    assert_eq!((ev["type"].as_str(), ev["added"].clone(), ev["removed"].clone()), (Some("labels_changed"), json!([]), json!([])), "a no-op");
    let ev = answer(&client, "tickets.label", json!({"id": "HOME-3", "remove": ["P1"]})).await;
    assert_eq!((ev["removed"].clone(), ev["ticket"]["labels"].clone()), (json!(["p1"]), json!(["needs-you"])));
    // ACCESS-8: a label that is none is no `Label` — refused at dispatch.
    let message = invalid(&client, "tickets.label", json!({"id": "HOME-3", "add": ["two words"]})).await;
    assert!(message.contains("add: `two words` is not a label"), "{message}");
    let ev = answer(&client, "tickets.label", json!({"id": "HOME-99", "add": ["x"]})).await;
    assert_eq!(ev["type"], "not_found", "{ev}");
    let got = answer(&client, "tickets.get", json!({"id": "HOME-3"})).await;
    assert_eq!(got["ticket"]["labels"], json!(["needs-you"]), "kept");
    let (rows, _) = paged(call(&client, "tickets.list", json!({"summary": true, "epic": "HOME"})).await).await;
    assert_eq!(rows[2]["summary"]["labels"], json!(["needs-you"]), "on the row");
    assert_eq!(rows[3]["summary"]["labels"], json!([]), "none is []");

    // The filters, on every read. V2-4: `asking` went with the inputs —
    // refused by name (`a_dropped_cell_is_refused_over_the_wire`).
    for (filter, want) in [
        ("label needs-you", vec!["HOME-3"]),
        ("not label needs-you", vec!["HOME-1", "HOME-2", "HOME-4"]),
        ("label in (p1, needs-you)", vec!["HOME-3"]),
    ] {
        let (rows, _) = paged(call(&client, "tickets.list", json!({"filter": filter, "summary": true})).await).await;
        assert_eq!(ids(&rows), want, "{filter}");
    }
    let counts = collect(call(&client, "tickets.nav.counts", json!({"filter": "label needs-you"})).await).await;
    assert_eq!(counts, [json!({"type": "count", "epic": "HOME", "shortcode": "HOME", "held": 1})]);
    // FILTER-24: `keep` holds the open ticket in, under these cells too.
    let (rows, _) = paged(call(&client, "tickets.list", json!({"filter": "label needs-you", "keep": "HOME-4", "summary": true})).await).await;
    assert_eq!(ids(&rows), ["HOME-3", "HOME-4"], "kept beside the matches");
    let counts = collect(call(&client, "tickets.nav.counts", json!({"filter": "label needs-you", "keep": "HOME-4"})).await).await;
    assert_eq!(counts, [json!({"type": "count", "epic": "HOME", "shortcode": "HOME", "held": 2})]);
    let menu = |facets: &[Value], field: &str| -> Vec<(String, u64)> {
        facets
            .iter()
            .filter(|f| f["field"] == field)
            .map(|f| (f["value"].as_str().unwrap().to_string(), f["held"].as_u64().unwrap()))
            .collect()
    };
    let facets = collect(call(&client, "tickets.nav.facets", json!({})).await).await;
    assert_eq!(menu(&facets, "label"), [("needs-you".to_string(), 1)]);
    let fields: Vec<&str> = facets.iter().map(|f| f["field"].as_str().unwrap()).collect();
    let at = |name: &str| fields.iter().position(|f| *f == name).unwrap();
    assert!(at("menger doc") < at("asking") && at("asking") < at("label") && at("label") < at("in"), "{fields:?}");
    let members = collect(call(&client, "tickets.nav.member", json!({"ids": ["HOME-2", "HOME-3", "HOME-4"], "filter": "label needs-you"})).await).await;
    let matched: Vec<bool> = members.iter().map(|m| m["matched"].as_bool().unwrap()).collect();
    assert_eq!(matched, [false, true, false]);

    // watch { filter }: Home's panel, live.
    let mut watch = call(&client, "tickets.watch", json!({"filter": "label needs-you"})).await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    answer(&client, "tickets.label", json!({"id": "HOME-4", "add": ["needs-you"]})).await; // enters
    answer(&client, "tickets.label", json!({"id": "HOME-3", "remove": ["needs-you"]})).await; // leaves
    answer(&client, "tickets.amend", json!({"id": "HOME-1", "body": asks})).await; // a body moves no cell
    answer(&client, "tickets.amend", json!({"id": "HOME-4", "body": "marker"})).await;
    let mut evs: Vec<Value> = Vec::new();
    loop {
        let ev = next_events(&mut watch, 1).await.remove(0);
        let done = ev["type"] == "amended" && ev["ticket"]["id"] == "HOME-4";
        evs.push(ev);
        if done {
            break;
        }
    }
    let moves: Vec<(String, String)> = evs
        .iter()
        .filter(|e| e["type"] == "entered" || e["type"] == "left")
        .map(|e| (e["type"].as_str().unwrap().to_string(), e["id"].as_str().unwrap().to_string()))
        .collect();
    let want: Vec<(String, String)> = [("entered", "HOME-4"), ("left", "HOME-3")].iter().map(|(a, b)| (a.to_string(), b.to_string())).collect();
    assert_eq!(moves, want, "{evs:#?}");
    let labelled = evs.iter().filter(|e| e["type"] == "labels_changed").count();
    assert_eq!(labelled, 2, "watch carries each label that moved something");
    let (rows, _) = paged(call(&client, "tickets.list", json!({"filter": "label needs-you", "summary": true})).await).await;
    assert_eq!(ids(&rows), ["HOME-4"]);
}

/// FILTER-15 over the real protocol: `find`, `ready_set` and `inputs` take a
/// filter — the rows the filter admits, paged as without it, no `first`;
/// without one, or with one that narrows nothing, today's answer — and
/// `member` says which of some ids a filter admits, one event per id in the
/// order asked. A filter the language does not take, one asking `text`, and
/// more than 1,000 ids are each one in-band error and nothing else.
#[tokio::test]
async fn reads_take_the_lens_over_the_wire() {
    let port = start_server().await;
    let client = connect(port).await;
    let ids = |rows: &[Value], shape: &str| -> Vec<String> { rows.iter().map(|r| r[shape]["id"].as_str().unwrap().to_string()).collect() };
    answer(&client, "tickets.create", json!({"epic": "LN", "title": "the lens root", "kind": "scope"})).await;
    let decision = "<m-decision data-field=\"decision\" id=\"pick\">\n<m-question data-field=\"question\">Pick?</m-question>\n\
                    <m-option data-field=\"option\" name=\"A\"></m-option>\n<m-option data-field=\"option\" name=\"B\"></m-option>\n</m-decision>\n";
    for (title, severity, body) in [
        ("watch the board", "High", decision),
        ("watch the nav", "Low", ""),
        ("watch the palette", "High", ""),
        ("a gap to close", "High", ""),
        ("watch the inputs", "Low", ""),
    ] {
        answer(&client, "tickets.create", json!({"epic": "LN", "title": title, "severity": severity, "body": body})).await;
    }
    // LN-2, LN-4 and LN-6 are Ready, LN-3 Coding; LN-7 (below) is Ready but
    // waits on LN-5.
    answer(&client, "tickets.create", json!({"epic": "LX", "title": "watch elsewhere", "kind": "scope", "body": decision})).await;
    for (id, status) in [("LN-2", "Ready"), ("LN-4", "Ready"), ("LN-6", "Ready"), ("LN-3", "Coding"), ("LX-1", "Ready")] {
        answer(&client, "tickets.set_status", json!({"id": id, "status": status})).await;
    }
    let blocked = collect(call(&client, "tickets.create", json!({"epic": "LN", "title": "watch it wait", "blocked_by": ["LN-5"], "body": decision})).await).await;
    assert_eq!(blocked[0]["ticket"]["id"], "LN-7");
    answer(&client, "tickets.set_status", json!({"id": "LN-7", "status": "Ready"})).await;

    // find: without a filter, and with one that narrows nothing — the same
    // bytes; with one, the text matches it admits, in order, paged as ever.
    let (rows, page) = paged(call(&client, "tickets.find", json!({"query": "WATCH", "limit": 3})).await).await;
    assert_eq!(ids(&rows, "summary"), ["LN-2", "LN-3", "LN-4"]);
    assert_eq!(page, json!({"type": "page", "next": "LN-4", "remaining": 3}));
    for narrows_nothing in ["", "· flat"] {
        let same = collect(call(&client, "tickets.find", json!({"query": "WATCH", "limit": 3, "filter": narrows_nothing})).await).await;
        assert_eq!(same, collect(call(&client, "tickets.find", json!({"query": "WATCH", "limit": 3})).await).await, "{narrows_nothing:?}");
    }
    let (rows, page) = paged(call(&client, "tickets.find", json!({"query": "watch", "filter": "status Ready", "limit": 2})).await).await;
    assert_eq!(ids(&rows, "summary"), ["LN-2", "LN-4"]);
    assert_eq!(page, json!({"type": "page", "next": "LN-4", "remaining": 3}), "no first on a find page");
    let (rows, page) = paged(call(&client, "tickets.find", json!({"query": "watch", "filter": "status Ready", "after": "LN-4"})).await).await;
    assert_eq!(ids(&rows, "summary"), ["LN-6", "LN-7", "LX-1"]);
    assert_eq!(page, json!({"type": "page", "next": null, "remaining": 0}));
    let (rows, _) = paged(call(&client, "tickets.find", json!({"query": "watch", "filter": "status Ready and in LN and not blocked"})).await).await;
    assert_eq!(ids(&rows, "summary"), ["LN-2", "LN-4", "LN-6"]);

    // ready_set: the ready set and the filter — LN-7 waits on LN-5.
    let (rows, page) = paged(call(&client, "tickets.work.ready", json!({"summary": true})).await).await;
    assert_eq!(ids(&rows, "summary"), ["LN-2", "LN-4", "LN-6", "LX-1"]);
    assert_eq!(page, json!({"type": "page", "next": null, "remaining": 0}));
    let (rows, page) = paged(call(&client, "tickets.work.ready", json!({"summary": true, "filter": "severity low", "limit": 5})).await).await;
    assert_eq!(ids(&rows, "summary"), ["LN-6"]);
    assert_eq!(page, json!({"type": "page", "next": null, "remaining": 0}), "no first on a ready_set page");
    let (rows, page) = paged(call(&client, "tickets.work.ready", json!({"filter": "not severity low", "limit": 1})).await).await;
    assert_eq!(ids(&rows, "ticket"), ["LN-2"], "whole tickets");
    assert_eq!(rows[0]["ticket"]["body"], decision);
    assert_eq!(page, json!({"type": "page", "next": "LN-2", "remaining": 2}));
    let (rows, _) = paged(call(&client, "tickets.work.ready", json!({"summary": true, "epic": "lx", "filter": "kind scope"})).await).await;
    assert_eq!(ids(&rows, "summary"), ["LX-1"]);

    // member: one event per id, in the order asked, then done.
    let asked = json!(["LN-4", "LN-2", "NOPE-1", "ln-6", "LN-7", "LN-2"]);
    let members = collect(call(&client, "tickets.nav.member", json!({"ids": asked, "filter": "ready"})).await).await;
    assert_eq!(
        members,
        [
            json!({"type": "member", "id": "LN-4", "matched": true}),
            json!({"type": "member", "id": "LN-2", "matched": true}),
            json!({"type": "member", "id": "NOPE-1", "matched": false}),
            // ACCESS-8: an id is a `TicketRef`, folded at the wire.
            json!({"type": "member", "id": "LN-6", "matched": true}),
            json!({"type": "member", "id": "LN-7", "matched": false}),
            json!({"type": "member", "id": "LN-2", "matched": true}),
        ]
    );
    let all = collect(call(&client, "tickets.nav.member", json!({"ids": ["LN-3", "LX-1"], "filter": ""})).await).await;
    assert!(all.iter().all(|m| m["matched"] == true), "the empty filter admits every ticket: {all:?}");
    assert!(collect(call(&client, "tickets.nav.member", json!({"ids": [], "filter": "ready"})).await).await.is_empty());

    // The refusals: each one error, nothing else.
    let thousand: Vec<String> = (1..=1000).map(|i| format!("LN-{i}")).collect();
    assert_eq!(collect(call(&client, "tickets.nav.member", json!({"ids": thousand, "filter": "ready"})).await).await.len(), 1000);
    let more: Vec<String> = (1..=1001).map(|i| format!("LN-{i}")).collect();
    let too_many = collect(call(&client, "tickets.nav.member", json!({"ids": more, "filter": "ready"})).await).await;
    assert_eq!(too_many.len(), 1, "{too_many:?}");
    assert_eq!(too_many[0]["type"], "error");
    assert!(too_many[0]["message"].as_str().unwrap().contains("at most 1000 ids"), "{too_many:?}");
    // FILTER-13: `under` is answered by these reads too — LN-1 roots LN.
    let under = collect(call(&client, "tickets.work.ready", json!({"filter": "under LN-1", "summary": true})).await).await;
    let in_ln = collect(call(&client, "tickets.work.ready", json!({"filter": "in LN", "summary": true})).await).await;
    assert!(under.len() > 1 && under == in_ln, "{under:?}");
    for (method, params) in [
        ("tickets.find", json!({"query": "watch", "filter": "text \"watch\""})),
        ("tickets.work.ready", json!({"filter": "under LN-1 and text \"x\""})),
        ("tickets.nav.member", json!({"ids": ["LN-2"], "filter": "status Bogus"})),
        ("tickets.find", json!({"query": "x", "filter": "nonsense"})),
    ] {
        // ACCESS-8: a filter the language does not read is refused at
        // dispatch (`invalid_params`), one it reads but a read does not take
        // (`text`) in band — each read's refusal list's own.
        let first = |method: &'static str, params: Value| {
            let client = &client;
            async move {
                let mut sub = call(client, method, params).await;
                let mut item = sub.next().await.expect("an item").unwrap();
                if item["type"] == "data" {
                    assert_eq!(sub.next().await.expect("done").unwrap()["type"], "done", "one error, nothing else");
                    item = item["content"].clone();
                }
                item.as_object_mut().expect("an object").remove("metadata");
                item
            }
        };
        let refused = first(method, params.clone()).await;
        assert_eq!(refused["type"], "error", "{method} {params}: {refused}");
        let listed = first("tickets.list", json!({"filter": params["filter"]})).await;
        assert_eq!(refused, listed, "{method} {params}: list's own refusal");
    }
}

// ---------------------------------------------------------------------------
// SEARCH-2: search inside tickets over the real protocol
// ---------------------------------------------------------------------------

/// SEARCH-2 over the real protocol: `search { query }` yields one `hit` per
/// ticket, best first — its summary row (no body), its title and passage as
/// marked stretches, `corrected` — then one `searched { matched, next,
/// words }`. A typo matches through its correction, after what matched as
/// typed; a filter (`under` too) keeps what it admits; `offset` + `limit`
/// page; a filter asking `text` is one error, as `list`'s.
#[tokio::test]
async fn search_over_the_wire() {
    let port = start_server().await;
    let client = connect(port).await;
    for (title, kind, parent, body) in [
        ("Search", "scope", None, "the root"),
        ("Placements kept", "build", None, "every non-doc row carries its <strong>placement</strong>, equal at every moment"),
        ("The order of work", "build", None, "each ticket's wave, and its placement among them"),
        ("Rows under the order", "build", Some("SR-3"), "a placement for the child as well"),
        ("Nothing", "build", None, "<m-option name=\"Elsewhere\"></m-option> no match here"),
    ] {
        let created = answer(&client, "tickets.create", json!({"epic": "SR", "title": title, "kind": kind, "parent": parent, "body": body})).await;
        assert_eq!(created["type"], "created", "{created}");
    }
    answer(&client, "tickets.set_status", json!({"id": "SR-3", "status": "Ready"})).await;

    let mut evs = collect(call(&client, "tickets.search", json!({"query": "placement"})).await).await;
    let end = evs.pop().unwrap();
    assert_eq!(
        end,
        json!({"type": "searched", "matched": 3, "next": null, "words": [{"word": "placement", "also": []}]})
    );
    let order: Vec<String> = evs.iter().map(|e| e["summary"]["id"].as_str().unwrap().to_string()).collect();
    assert_eq!(order[0], "SR-2", "the title match first: {order:?}");
    let mut all = order.clone();
    all.sort();
    assert_eq!(all, ["SR-2", "SR-3", "SR-4"]);
    let first = &evs[0];
    assert_eq!(first["type"], "hit");
    assert!(first["summary"].get("body").is_none(), "a summary row: {first}");
    assert_eq!(first["corrected"], false);
    assert_eq!(first["title"], json!([{"text": "Placements", "hit": true}, {"text": " kept", "hit": false}]));
    assert_eq!(
        first["snippet"],
        json!([
            {"text": "every non-doc row carries its ", "hit": false},
            {"text": "placement", "hit": true},
            {"text": ", equal at every moment", "hit": false}
        ])
    );

    // A typo: corrected, said, and every hit marked as corrected.
    let mut evs = collect(call(&client, "tickets.search", json!({"query": "wave plcement"})).await).await;
    let end = evs.pop().unwrap();
    assert_eq!(end["words"], json!([{"word": "wave", "also": []}, {"word": "plcement", "also": ["placement", "placements"]}]), "nearest, then commonest");
    assert_eq!(evs.len(), 1);
    assert_eq!((evs[0]["summary"]["id"].as_str(), evs[0]["corrected"].as_bool()), (Some("SR-3"), Some(true)));

    // Under a filter; and under a ticket's lineage.
    let evs = collect(call(&client, "tickets.search", json!({"query": "placement", "filter": "status Ready"})).await).await;
    assert_eq!((evs[0]["summary"]["id"].as_str(), evs.len(), &evs[1]["matched"]), (Some("SR-3"), 2, &json!(1)));
    let evs = collect(call(&client, "tickets.search", json!({"query": "placement", "filter": "under SR-3"})).await).await;
    let mut ids: Vec<&str> = evs.iter().filter_map(|e| e["summary"]["id"].as_str()).collect();
    ids.sort();
    assert_eq!(ids, ["SR-3", "SR-4"], "SR-3 and its child");

    // Pages: offset + limit, next the offset to ask for.
    let evs = collect(call(&client, "tickets.search", json!({"query": "placement", "limit": 2})).await).await;
    assert_eq!((evs.len(), &evs[2]["next"], &evs[2]["matched"]), (3, &json!(2), &json!(3)));
    assert_eq!((evs[0]["summary"]["id"].as_str(), evs[1]["summary"]["id"].as_str()), (Some(order[0].as_str()), Some(order[1].as_str())));
    let evs = collect(call(&client, "tickets.search", json!({"query": "placement", "limit": 2, "offset": 2})).await).await;
    assert_eq!((evs[0]["summary"]["id"].as_str(), &evs[1]["next"]), (Some(order[2].as_str()), &json!(null)));

    // A blank query: nothing, said so; a filter asking text: one error.
    let evs = collect(call(&client, "tickets.search", json!({"query": "  "})).await).await;
    assert_eq!(evs, [json!({"type": "searched", "matched": 0, "next": null, "words": []})]);
    let params = json!({"query": "placement", "filter": "text \"placement\""});
    let refused = collect(call(&client, "tickets.search", params.clone()).await).await;
    assert_eq!(refused.len(), 1, "{refused:?}");
    assert_eq!(refused, collect(call(&client, "tickets.list", json!({"filter": params["filter"]})).await).await);
}

/// ACT-5 acceptance 2: `delete` over the real protocol — a delete of a ticket
/// nothing points at, a refused delete naming its blocker and its children,
/// a reparenting delete, `get` on a deleted id answering its tombstone, the
/// `deleted` event on `watch`, the hole `extents` names, and `undelete`.
#[tokio::test]
async fn delete_over_the_wire() {
    let port = start_server().await;
    let client = connect(port).await;
    for (title, kind, parent, blocked_by) in [
        ("the root", "scope", None, vec![]),
        // ORG-5: an area inside the shortcode is work, not a scope.
        ("an area", "execution", None, vec![]),
        ("a child", "build", Some("DEL-2"), vec![]),
        ("waits", "build", None, vec!["DEL-2"]),
        ("a typo", "build", None, vec![]),
    ] {
        answer(&client, "tickets.create", json!({"epic": "DEL", "title": title, "kind": kind, "parent": parent, "blocked_by": blocked_by})).await;
    }
    let mut watch = call(&client, "tickets.watch", json!({})).await;
    tokio::time::sleep(Duration::from_millis(100)).await;

    let gone = answer(&client, "tickets.delete", json!({"ids": ["DEL-5"], "message": "filed twice"})).await;
    assert_eq!(gone["type"], "deleted", "{gone}");
    assert_eq!(gone["ids"], json!(["DEL-5"]));
    assert_eq!((&gone["tombstones"][0]["title"], &gone["tombstones"][0]["message"]), (&json!("a typo"), &json!("filed twice")));
    let heard = watch_until(&mut watch, "deleted").await;
    assert_eq!(heard.last().unwrap()["ids"], json!(["DEL-5"]), "{heard:?}");
    let got = answer(&client, "tickets.get", json!({"id": "DEL-5"})).await;
    assert_eq!((&got["type"], &got["tombstones"][0]["by"]), (&json!("deleted"), &json!(support::admin())), "{got}");
    let extents = collect(call(&client, "tickets.nav.extents", json!({})).await).await;
    assert_eq!(extents, [json!({"type": "extent", "epic": "DEL", "shortcode": "DEL", "last_seq": 5, "holes": [5]})], "the deleted last is a hole");

    let refused = answer(&client, "tickets.delete", json!({"ids": ["DEL-2"]})).await;
    assert_eq!(refused["type"], "error", "{refused}");
    let message = refused["message"].as_str().unwrap();
    assert!(message.contains("DEL-3") && message.contains("children reparent"), "{message}");
    assert!(message.contains("DEL-4 waits on it — unblock"), "{message}");
    assert_eq!(answer(&client, "tickets.get", json!({"id": "DEL-2"})).await["type"], "ticket", "nothing written");

    let moved = answer(&client, "tickets.delete", json!({"ids": ["DEL-2"], "children": "reparent", "unblock": true})).await;
    assert_eq!(moved["tombstones"][0]["reparented"], json!(["DEL-3"]), "{moved}");
    assert_eq!(moved["tombstones"][0]["unblocked"], json!(["DEL-4"]), "{moved}");
    let heard = watch_until(&mut watch, "deleted").await;
    let kinds: Vec<&str> = heard.iter().filter_map(|e| e["type"].as_str()).skip_while(|k| *k != "parent_changed").collect();
    assert_eq!(kinds, ["parent_changed", "children_changed", "blockers_changed", "child_removed", "deleted"], "{heard:?}");
    let child = answer(&client, "tickets.get", json!({"id": "DEL-3"})).await;
    assert_eq!(child["ticket"]["parent"], "DEL-1", "moved to its grandparent");
    let extents = collect(call(&client, "tickets.nav.extents", json!({})).await).await;
    assert_eq!(extents, [json!({"type": "extent", "epic": "DEL", "shortcode": "DEL", "last_seq": 5, "holes": [2, 5]})]);

    let back = answer(&client, "tickets.undelete", json!({"ids": ["DEL-2"]})).await;
    assert_eq!((&back["type"], &back["tickets"][0]["title"]), (&json!("undeleted"), &json!("an area")), "{back}");
    assert_eq!(answer(&client, "tickets.get", json!({"id": "DEL-3"})).await["ticket"]["parent"], "DEL-2", "back under it");
    let born = answer(&client, "tickets.create", json!({"epic": "DEL", "title": "after"})).await;
    assert_eq!(born["ticket"]["id"], "DEL-6", "DEL-5's number is not reused");
}

/// TICKETS-7 acceptance 2: revisions over the real protocol — amend twice,
/// `revisions list`, `get`, `diff`, `restore`; `watch { revisions: true }`
/// sees each `revised`, a plain `watch` none.
#[tokio::test]
async fn revisions_over_the_wire() {
    let port = start_server().await;
    let client = connect(port).await;
    let mut watch = call(&client, "tickets.watch", json!({"revisions": true})).await;
    let mut plain = call(&client, "tickets.watch", json!({})).await;
    collect(call(&client, "tickets.create", json!({"epic": "REV", "title": "root", "kind": "scope", "body": "one\ntwo\n"})).await).await;
    assert_eq!(watch_until(&mut watch, "revised").await.last().unwrap()["fields"], json!(["created"]));
    collect(call(&client, "tickets.amend", json!({"id": "REV-1", "body": "one\nTWO\nthree\n"})).await).await;
    let seen = watch_until(&mut watch, "revised").await;
    assert_eq!((seen[0]["type"].as_str(), seen.last().unwrap()["rev"].clone()), (Some("amended"), json!(2)), "{seen:?}");
    collect(call(&client, "tickets.amend", json!({"id": "REV-1", "title": "renamed"})).await).await;
    watch_until(&mut watch, "revised").await;

    // V2-37: the title, by the same hand within the window, folds into the
    // body's revision.
    let listed = collect(call(&client, "tickets.revisions.list", json!({"id": "rev-1"})).await).await;
    let revs: Vec<i64> = listed.iter().filter(|e| e["type"] == "revision").map(|e| e["revision"]["rev"].as_i64().unwrap()).collect();
    assert_eq!(revs, [2, 1], "{listed:?}");
    assert_eq!(listed[0]["revision"]["fields"], json!(["title", "body"]));
    assert_eq!((listed[0]["revision"]["added"].clone(), listed[0]["revision"]["removed"].clone()), (json!(2), json!(1)));
    assert_eq!((listed[2]["type"].as_str(), listed[2]["count"].clone(), listed[2]["latest"].clone()), (Some("revisions_end"), json!(2), json!(2)));

    let at = collect(call(&client, "tickets.revisions.get", json!({"id": "REV-1", "rev": 1})).await).await;
    assert_eq!((at[0]["type"].as_str(), at[0]["ticket"]["title"].as_str(), at[0]["ticket"]["body"].as_str()), (Some("revision_at"), Some("root"), Some("one\ntwo\n")));

    let d = collect(call(&client, "tickets.revisions.diff", json!({"id": "REV-1", "from": 1, "to": 2})).await).await;
    let diff = &d[0]["diff"];
    assert_eq!(d[0]["type"], "revision_diff");
    assert_eq!(diff["fields"].as_array().unwrap().iter().map(|f| f["field"].as_str().unwrap()).collect::<Vec<_>>(), ["title", "body"]);
    let lines = &diff["body"]["hunks"][0]["lines"];
    assert!(lines.as_array().unwrap().iter().any(|l| l["op"] == "add" && l["text"] == "TWO" && l["words"][0]["text"] == "TWO"), "{lines}");

    let restored = collect(call(&client, "tickets.revisions.restore", json!({"id": "REV-1", "rev": 1, "message": "back to the start"})).await).await;
    assert_eq!((restored[0]["type"].as_str(), restored[0]["changed"].clone(), restored[0]["rev"].clone()), (Some("revision_restored"), json!(["title", "body"]), json!(3)), "{restored:?}");
    assert_eq!(restored[0]["ticket"]["body"], "one\ntwo\n");
    let seen = watch_until(&mut watch, "revised").await;
    assert_eq!(seen.last().unwrap()["rev"], json!(3), "a restore says why: its own revision");
    let listed = collect(call(&client, "tickets.revisions.list", json!({"id": "REV-1", "limit": 1})).await).await;
    assert_eq!(listed[0]["revision"]["message"], "restored title, body from revision 1: back to the start");
    let message = invalid(&client, "tickets.revisions.restore", json!({"id": "REV-1", "rev": 1, "fields": ["commits"]})).await;
    assert!(message.contains("commits are linked, never restored"), "{message}");

    // A plain watch heard the writes and no revision.
    let mut heard = Vec::new();
    for _ in 0..3 {
        heard.extend(watch_until(&mut plain, "amended").await);
    }
    assert!(!heard.iter().any(|e| e["type"] == "revised"), "{heard:?}");
}

/// MACRO-31: under `typed_params` a key no parameter claims is refused at
/// dispatch, before the method runs — `-32602 invalid_params`, the message
/// naming the key and the method's parameters, sorted, with the words
/// `unknown field` synapse reads to know its cached schema is stale.
/// MACRO-29 measured the hole: `{sumary: true}` listed full rows, and
/// `create {kindd: "gap"}` made a build. Now neither runs: nothing is
/// created.
#[tokio::test]
async fn an_unknown_parameter_is_refused_before_the_method_runs() {
    let port = start_server().await;
    let client = connect(port).await;

    let m = invalid(&client, "tickets.list", json!({"sumary": true})).await;
    assert_eq!(
        m,
        "Invalid params: unknown field `sumary`; tickets.list takes: after, at, epic, fields, filter, frame, keep, limit, parent, status, summary, tz"
    );
    let m = invalid(&client, "tickets.list", json!({"summary": true, "limt": 1})).await;
    assert!(m.contains("unknown field `limt`"), "{m}");

    let m = invalid(&client, "tickets.create", json!({"epic": "WIRE", "title": "a gap, misspelled", "kindd": "gap"})).await;
    assert_eq!(
        m,
        "Invalid params: unknown field `kindd`; tickets.create takes: blocked_by, body, epic, kind, org, parent, path, severity, title"
    );
    // The store is untouched: the create never ran.
    let mut sub = call(&client, "tickets.health", json!({})).await;
    let ev = content(sub.next().await.unwrap().unwrap());
    assert_eq!(ev["type"], "health");
    assert_eq!(ev["ticket_count"], 0, "a refused create wrote nothing: {ev}");
    assert!(rows(call(&client, "tickets.list", json!({"summary": true})).await).await.is_empty());

    // A child's method is named by the child's own namespace — the
    // dispatch is not handed the path it is mounted at.
    let m = invalid(&client, "tickets.filters.save", json!({"nmae": "Open", "filter": "ready"})).await;
    assert_eq!(m, "Invalid params: unknown field `nmae`; filters.save takes: filter, name");

    // A call naming only its own parameters runs as before.
    // (A shortcode's first ticket is its scope.)
    let mut sub = call(&client, "tickets.create", json!({"epic": "WIRE", "title": "the root", "kind": "scope"})).await;
    let ev = content(sub.next().await.unwrap().unwrap());
    assert_eq!((ev["type"].as_str(), ev["ticket"]["kind"].as_str()), (Some("created"), Some("scope")), "{ev}");
}

// ---------------------------------------------------------------------------
// PERF-15: a page is one frame. Every paged read answers its rows as one
// `rows { items }` event — each item the per-row event it was (`summary`,
// `ticket`, `node`, `hit`) — then its one trailing event (`page`, or
// `searched`), then `done`: three frames, not one per row. The board's whole
// listings (`nav.groups`, `extents`, `counts`) the same, with nothing
// trailing. A watch still sends each change as its own event.

/// One call's frames as they crossed: every item of the subscription, whole
/// (envelope and all), through `done`. One item is one WebSocket message.
async fn frames(client: &WsClient, method: &str, params: Value) -> Vec<Value> {
    let mut sub = call(client, method, params).await;
    let mut out = Vec::new();
    while let Some(item) = tokio::time::timeout(Duration::from_secs(20), sub.next()).await.expect("the call answers").map(|i| i.unwrap()) {
        let end = item["type"] != "data";
        out.push(item);
        if end {
            break;
        }
    }
    out
}

/// What each frame is: `data:<event type>` or the item's own type.
fn frame_kinds(frames: &[Value]) -> Vec<String> {
    frames
        .iter()
        .map(|f| match f["type"].as_str() {
            Some("data") => format!("data:{}", f["content"]["type"].as_str().unwrap_or("?")),
            other => other.unwrap_or("?").to_string(),
        })
        .collect()
}

/// The one `rows` frame's items, the frames asserted to be `rows`, then
/// `trailing` (when given), then `done`.
fn one_rows_frame(what: &str, frames: &[Value], trailing: Option<&str>) -> Vec<Value> {
    let mut want = vec!["data:rows".to_string()];
    want.extend(trailing.map(|t| format!("data:{t}")));
    want.push("done".into());
    let kinds = frame_kinds(frames);
    assert_eq!(kinds.len(), want.len(), "{what}: a page is {want:?}, one frame for its rows — it crossed as {} frames ({} {:?} …)", kinds.len(), kinds.iter().filter(|k| *k == &kinds[0]).count(), kinds.first());
    assert_eq!(kinds, want, "{what}");
    frames[0]["content"]["items"].as_array().unwrap_or_else(|| panic!("{what}: rows carries items: {}", frames[0])).clone()
}

/// The keys of a build's summary row (a doc has no `placement`; the flags
/// and hidden counts are omitted when unset). TICKETS-35 (V2-42): its
/// `uid`, and `uid_of` for the parent it names.
const SUMMARY_KEYS: [&str; 20] = [
    "address", "blocked_by", "child_count", "commits", "created_unix", "epic", "id", "kind", "labels", "parent", "placement", "seq", "severity",
    "shortcode", "status", "title", "uid", "uid_of", "unlocks", "updated_unix",
];

/// `n` tickets in `epic`: its scope root, then `n - 1` builds under it,
/// titled `row 2` … `row n`.
async fn seed_rows(client: &WsClient, epic: &str, n: usize) {
    let root = answer(client, "tickets.create", json!({"epic": epic, "title": "the root", "kind": "scope"})).await;
    assert_eq!(root["type"], "created", "{root}");
    for i in 2..=n {
        let ev = answer(client, "tickets.create", json!({"epic": epic, "title": format!("row {i}"), "kind": "build"})).await;
        assert_eq!(ev["type"], "created", "{ev}");
    }
}

/// PERF-15: `list { summary, limit: 200 }` over 300 tickets — one `rows`
/// frame of 200 `summary` items in the store's order, then `page`, then
/// `done`; no per-row frame. Without `summary`, the same with `ticket`
/// items (bodies and all).
#[tokio::test]
async fn a_list_page_is_one_frame() {
    let port = start_server().await;
    let client = connect(port).await;
    seed_rows(&client, "ONE", 300).await;

    let fs = frames(&client, "tickets.list", json!({"summary": true, "limit": 200})).await;
    let items = one_rows_frame("list { summary, limit: 200 }", &fs, Some("page"));
    assert_eq!(items.len(), 200, "the page's 200 rows in the one frame");
    assert!(items.iter().all(|i| i["type"] == "summary" && i["summary"].get("body").is_none()), "each item a summary row, no body: {}", items[0]);
    assert_eq!((items[0]["summary"]["id"].as_str(), items[199]["summary"]["id"].as_str()), (Some("ONE-1"), Some("ONE-200")), "in the store's order");
    assert_eq!(fs[1]["content"], json!({"type": "page", "next": "ONE-200", "remaining": 100}), "the page after the rows, as it was");
    assert!(!frame_kinds(&fs).iter().any(|k| k == "data:summary" || k == "data:ticket"), "no per-row frame");
    let keys: std::collections::BTreeSet<&str> = items[1]["summary"].as_object().unwrap().keys().map(String::as_str).collect();
    assert_eq!(keys, SUMMARY_KEYS.iter().copied().collect(), "a build's summary row carries every field it did: {}", items[1]);

    // An empty page: one empty `rows`, then the page. A refusal: the error
    // alone, no `rows`.
    let fs = frames(&client, "tickets.list", json!({"summary": true, "epic": "NONE"})).await;
    assert!(one_rows_frame("list of an empty shortcode", &fs, Some("page")).is_empty());
    assert_eq!(fs[1]["content"], json!({"type": "page", "next": null, "remaining": 0}));
    let fs = frames(&client, "tickets.list", json!({"summary": true, "after": "ZZZ-9"})).await;
    assert_eq!(frame_kinds(&fs), ["data:error", "done"], "a cursor naming no ticket: its error, no rows");

    let fs = frames(&client, "tickets.list", json!({"limit": 50, "after": "ONE-200"})).await;
    let items = one_rows_frame("list { limit: 50, after }", &fs, Some("page"));
    assert_eq!(items.len(), 50);
    assert!(items.iter().all(|i| i["type"] == "ticket" && i["ticket"]["body"].is_string()), "without summary: whole tickets: {}", items[0]);
    assert_eq!(items[0]["ticket"]["id"], "ONE-201");
    assert_eq!(fs[1]["content"], json!({"type": "page", "next": "ONE-250", "remaining": 50}));

    // A filter (FILTER-4's cells) pages the same way, `first` on its page.
    let fs = frames(&client, "tickets.list", json!({"summary": true, "limit": 200, "filter": "kind build", "at": 0})).await;
    let items = one_rows_frame("list { filter, at }", &fs, Some("page"));
    assert_eq!(items.len(), 200);
    assert_eq!(fs[1]["content"]["first"], 0);
}

/// PERF-15: `work.ready { summary, limit: 200 }` — one `rows` frame, then
/// `page`, then `done`.
#[tokio::test]
async fn a_ready_page_is_one_frame() {
    let port = start_server().await;
    let client = connect(port).await;
    seed_rows(&client, "RDY", 300).await;
    let moved = answer(&client, "tickets.update_many", json!({"filter": "kind build", "status": "Ready", "max": 500})).await;
    assert_eq!(moved["type"], "updated_many", "{moved}");

    let fs = frames(&client, "tickets.work.ready", json!({"summary": true, "limit": 200})).await;
    let items = one_rows_frame("work.ready { summary, limit: 200 }", &fs, Some("page"));
    assert_eq!(items.len(), 200);
    assert!(items.iter().all(|i| i["type"] == "summary" && i["summary"]["status"] == "Ready"), "{}", items[0]);
    assert_eq!(fs[1]["content"], json!({"type": "page", "next": "RDY-201", "remaining": 99}), "the page after the rows, as it was");
}

/// PERF-15: `nav.tree { parent, filter, limit: 200 }` — one `rows` frame of
/// `node` items (`summary`, `matched`, `below`, as each node was), then
/// `page`, then `done`.
#[tokio::test]
async fn a_tree_page_is_one_frame() {
    let port = start_server().await;
    let client = connect(port).await;
    seed_rows(&client, "TRE", 300).await;

    let fs = frames(&client, "tickets.nav.tree", json!({"parent": "TRE-1", "filter": "kind build", "limit": 200})).await;
    let items = one_rows_frame("nav.tree { parent, filter, limit: 200 }", &fs, Some("page"));
    assert_eq!(items.len(), 200);
    // The node's summary is TRE-2's whole summary row, as `list` gives it.
    let one = frames(&client, "tickets.list", json!({"summary": true, "after": "TRE-1", "limit": 1})).await;
    let listed = one_rows_frame("list after TRE-1", &one, Some("page"));
    assert_eq!(items[0]["summary"], listed[0]["summary"], "the node's row is the summary row, every field");
    assert_eq!(items[0]["summary"]["id"], "TRE-2");
    assert_eq!(items[0]["matched"], true);
    assert_eq!(items[0]["below"], 0);
    assert_eq!(items[0].as_object().unwrap().len(), 4, "a node item is type, summary, matched, below: {}", items[0]);
    assert_eq!(fs[1]["content"], json!({"type": "page", "next": "TRE-201", "remaining": 99}));
}

/// PERF-15: `find { query, limit: 200 }` — one `rows` frame of `summary`
/// items, then `page`, then `done`.
#[tokio::test]
async fn a_find_page_is_one_frame() {
    let port = start_server().await;
    let client = connect(port).await;
    seed_rows(&client, "FND", 300).await;

    let fs = frames(&client, "tickets.find", json!({"query": "row", "limit": 200})).await;
    let items = one_rows_frame("find { query, limit: 200 }", &fs, Some("page"));
    assert_eq!(items.len(), 200);
    assert!(items.iter().all(|i| i["type"] == "summary"), "{}", items[0]);
    assert_eq!((items[0]["summary"]["id"].as_str(), items[199]["summary"]["id"].as_str()), (Some("FND-2"), Some("FND-201")), "the root's title has no `row`");
    assert_eq!(fs[1]["content"], json!({"type": "page", "next": "FND-201", "remaining": 99}), "the page after the rows, as it was");
}

/// PERF-15: `search { query, limit: 100 }` — one `rows` frame of `hit`
/// items (`summary`, `title`, `snippet`, `corrected`), then `searched`,
/// then `done`.
#[tokio::test]
async fn a_search_page_is_one_frame() {
    let port = start_server().await;
    let client = connect(port).await;
    seed_rows(&client, "SRC", 300).await;

    let fs = frames(&client, "tickets.search", json!({"query": "row", "limit": 100})).await;
    let items = one_rows_frame("search { query, limit: 100 }", &fs, Some("searched"));
    assert_eq!(items.len(), 100);
    assert!(items.iter().all(|i| i["type"] == "hit" && i["summary"]["id"].is_string() && i.get("snippet").is_some()), "{}", items[0]);
    assert_eq!(fs[1]["content"], json!({"type": "searched", "matched": 299, "next": 100, "words": [{"word": "row", "also": []}]}), "searched after the rows, as it was");

    // No hit: one empty `rows`, then `searched`.
    let fs = frames(&client, "tickets.search", json!({"query": "zebra"})).await;
    assert!(one_rows_frame("search with no hit", &fs, Some("searched")).is_empty());
    assert_eq!(fs[1]["content"]["matched"], 0);
}

/// PERF-15: the board's whole listings — `nav.groups`, `nav.extents`,
/// `nav.counts` — are one `rows` frame each (one item per shortcode, as
/// each event was), then `done`: on app-test they were 84 frames apiece
/// (83 shortcodes), 97 KB of the board's 247.
#[tokio::test]
async fn the_boards_listings_are_one_frame() {
    let port = start_server().await;
    let client = connect(port).await;
    for epic in ["LA", "LB", "LC"] {
        seed_rows(&client, epic, 4).await;
    }
    for (method, kind) in [("tickets.nav.groups", "group"), ("tickets.nav.extents", "extent"), ("tickets.nav.counts", "count")] {
        let fs = frames(&client, method, json!({})).await;
        let items = one_rows_frame(method, &fs, None);
        assert_eq!(items.len(), 3, "{method}: one item per shortcode");
        assert!(items.iter().all(|i| i["type"] == kind), "{method}: {kind} items: {}", items[0]);
    }
    let fs = frames(&client, "tickets.nav.counts", json!({})).await;
    assert_eq!(fs[0]["content"]["items"][0], json!({"type": "count", "epic": "LA", "shortcode": "LA", "held": 4}), "a count item as the count event was");
}

/// PERF-15: a watch is not a page — each change is still its own frame:
/// three creates, a status change and a delete are `created` ×3,
/// `status_changed`, `deleted`, one frame each (with the transitions and
/// roll-ups they always brought), and nothing on a watch is ever a `rows`.
/// (The watch is opened and given a moment, as every watch test here does:
/// its subscription answers before its store subscription is polled.)
#[tokio::test]
async fn a_watch_still_sends_one_ticket_per_change() {
    let port = start_server().await;
    let client = connect(port).await;
    answer(&client, "tickets.create", json!({"epic": "WCH", "title": "the root", "kind": "scope"})).await;
    let mut watch = call(&client, "tickets.watch", json!({})).await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    for i in 2..=4 {
        answer(&client, "tickets.create", json!({"epic": "WCH", "title": format!("row {i}"), "kind": "build"})).await;
    }
    answer(&client, "tickets.set_status", json!({"id": "WCH-2", "status": "Ready"})).await;
    let gone = answer(&client, "tickets.delete", json!({"ids": ["WCH-4"]})).await;
    assert_eq!(gone["type"], "deleted", "{gone}");
    let mut seen = Vec::new();
    while !seen.iter().any(|(k, _): &(String, Value)| k == "deleted") {
        let item = tokio::time::timeout(Duration::from_secs(10), watch.next()).await.unwrap_or_else(|_| panic!("no deleted after {seen:?}")).expect("open").unwrap();
        assert_eq!(item["type"], "data", "{item}");
        let ev = item["content"].clone();
        seen.push((ev["type"].as_str().unwrap_or("?").to_string(), ev));
    }
    assert!(!seen.iter().any(|(k, _)| k == "rows"), "a watch never batches: {seen:?}");
    let changes: Vec<(String, String)> = seen
        .iter()
        .filter(|(k, _)| matches!(k.as_str(), "created" | "status_changed" | "deleted"))
        .map(|(k, ev)| (k.clone(), ev["ticket"]["id"].as_str().or(ev["ids"][0].as_str()).unwrap_or("?").to_string()))
        .collect();
    let want = [("created", "WCH-2"), ("created", "WCH-3"), ("created", "WCH-4"), ("status_changed", "WCH-2"), ("deleted", "WCH-4")];
    assert_eq!(changes, want.map(|(k, id)| (k.to_string(), id.to_string())), "one frame per change, in order");
}

/// PERF-15: thin rows — `fields` keeps those keys of each summary row and
/// its `id`, nothing else; without it every row is whole, as before; and
/// `fields` alone asks for summary rows. A name not a summary row's field is
/// refused by its type, before the method runs.
#[tokio::test]
async fn a_summary_page_carries_only_the_fields_asked() {
    let port = start_server().await;
    let client = connect(port).await;
    seed_rows(&client, "THN", 300).await;

    let fs = frames(&client, "tickets.list", json!({"summary": true, "limit": 200, "fields": ["id", "title", "kind", "status"]})).await;
    let items = one_rows_frame("list { summary, limit: 200, fields }", &fs, Some("page"));
    assert_eq!(items.len(), 200);
    for i in &items {
        let keys: std::collections::BTreeSet<&str> = i["summary"].as_object().unwrap().keys().map(String::as_str).collect();
        assert_eq!(keys, ["id", "kind", "status", "title"].into_iter().collect(), "the four and nothing else: {i}");
    }
    assert_eq!(items[1], json!({"type": "summary", "summary": {"id": "THN-2", "title": "row 2", "kind": "build", "status": "Draft"}}));
    assert_eq!(fs[1]["content"], json!({"type": "page", "next": "THN-200", "remaining": 100}), "the page as without fields");

    let thin = frames(&client, "tickets.list", json!({"limit": 3, "fields": ["title"]})).await;
    assert_eq!(one_rows_frame("list { fields } without summary", &thin, Some("page"))[2], json!({"type": "summary", "summary": {"id": "THN-3", "title": "row 3"}}), "fields asks for summary rows; the id is always kept");

    let whole = frames(&client, "tickets.list", json!({"summary": true, "limit": 3})).await;
    let whole = one_rows_frame("list { summary } without fields", &whole, Some("page"));
    let keys: std::collections::BTreeSet<&str> = whole[1]["summary"].as_object().unwrap().keys().map(String::as_str).collect();
    assert_eq!(keys, SUMMARY_KEYS.iter().copied().collect(), "without fields: the whole row");

    for (method, params) in [("tickets.work.ready", json!({"fields": ["id"]})), ("tickets.find", json!({"query": "row", "limit": 2, "fields": ["id", "status"]}))] {
        let fs = frames(&client, method, params).await;
        let items = one_rows_frame(method, &fs, Some("page"));
        assert!(items.iter().all(|i| i["summary"].as_object().unwrap().keys().all(|k| k == "id" || k == "status")), "{method}: thinned: {items:?}");
    }

    let m = invalid(&client, "tickets.list", json!({"summary": true, "fields": ["id", "bogus"]})).await;
    assert!(m.contains("fields") && m.contains("bogus"), "refused naming the parameter and the name: {m}");
}

/// PERF-15: a page whose rows are past [`ROWS_BUDGET`] (1 MiB) is several
/// `rows` frames, then the page, then `done` — so three 4 MiB bodies
/// (12 MiB: past the transport's 10 MB message cap as one frame) cross,
/// each ticket in one frame, every id once and in order; and six 300 KiB
/// bodies cross in frames each within the budget.
#[tokio::test]
async fn a_page_over_the_frame_budget_is_several_rows_frames() {
    const BUDGET: usize = menger_tickets_api::activation::ROWS_BUDGET;
    assert_eq!(BUDGET, 1 << 20);
    let port = start_server().await;
    let client = WsClientBuilder::default()
        .set_headers(support::admin_headers(port))
        .max_request_size(64 << 20)
        .max_response_size(64 << 20)
        .build(format!("ws://127.0.0.1:{port}"))
        .await;
    let client = match client {
        Ok(c) => c,
        Err(_) => connect(port).await,
    };
    for (epic, n, body) in [("BIG", 3usize, 4 << 20), ("MID", 6, 300 << 10)] {
        answer(&client, "tickets.create", json!({"epic": epic, "title": "the root", "kind": "scope"})).await;
        for i in 2..=n + 1 {
            let ev = answer(&client, "tickets.create", json!({"epic": epic, "title": format!("big {i}"), "kind": "build", "body": "x".repeat(body)})).await;
            assert_eq!(ev["type"], "created", "{}", ev["type"]);
        }
    }
    for (epic, n) in [("BIG", 4usize), ("MID", 7)] {
        let fs = frames(&client, "tickets.list", json!({"epic": epic})).await;
        let kinds = frame_kinds(&fs);
        let rows: Vec<&Value> = fs.iter().filter(|f| f["content"]["type"] == "rows").collect();
        assert!(rows.len() >= 2, "{epic}: several rows frames: {kinds:?}");
        assert_eq!(&kinds[rows.len()..], ["data:page", "done"], "{epic}: the rows, then the page, then done");
        let mut ids = Vec::new();
        for f in &rows {
            let items = f["content"]["items"].as_array().unwrap();
            let bytes: usize = items.iter().map(|i| serde_json::to_vec(i).unwrap().len()).sum();
            assert!(items.len() == 1 || bytes <= BUDGET, "{epic}: a frame of {} items is {bytes} B, over the budget", items.len());
            assert!(serde_json::to_vec(f).unwrap().len() < 10_000_000, "{epic}: every frame under the transport's cap");
            ids.extend(items.iter().map(|i| i["ticket"]["id"].as_str().unwrap().to_string()));
        }
        assert_eq!(ids, (1..=n).map(|i| format!("{epic}-{i}")).collect::<Vec<_>>(), "{epic}: every id once, in order");
        if epic == "MID" {
            assert!(rows.iter().all(|f| f["content"]["items"].as_array().unwrap().iter().map(|i| serde_json::to_vec(i).unwrap().len()).sum::<usize>() <= BUDGET), "MID: every frame within the budget");
        }
    }
}

/// PLEXUS-12's test, kept by PERF-20 (a call is its own task, so calls on
/// one socket overlap): one socket asks for a long list and then for
/// `health`; `health` is done before the list is, and each call's items
/// come in the order they were sent, `done` last.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_second_call_on_one_socket_overlaps_the_first_s_stream() {
    let port = start_server().await;
    let client = connect(port).await;
    answer(&client, "tickets.create", json!({"epic": "OV", "title": "root", "kind": "scope"})).await;
    for i in 0..1000 {
        answer(&client, "tickets.create", json!({"epic": "OV", "title": format!("t{i}")})).await;
    }
    let began = std::time::Instant::now();
    let drain = move |mut sub: Subscription<Value>| async move {
        let mut items = Vec::new();
        while let Some(item) = sub.next().await {
            let item = item.unwrap();
            let done = item["type"] == "done";
            items.push(item);
            if done {
                break;
            }
        }
        (began.elapsed(), items)
    };
    let list = tokio::spawn(drain(call(&client, "tickets.list", json!({"limit": 10000})).await));
    let health = tokio::spawn(drain(call(&client, "tickets.health", json!({})).await));
    let ((list_at, list), (health_at, health)) = (list.await.unwrap(), health.await.unwrap());
    println!("one socket: health done at {health_at:?}, the 1,001-row list done at {list_at:?}");
    assert!(health_at < list_at, "health ({health_at:?}) waited for the list ({list_at:?})");
    for items in [&list, &health] {
        let (done, rest) = items.split_last().unwrap();
        assert_eq!(done["type"], "done", "done last");
        assert!(rest.iter().all(|i| i["type"] == "data"), "{rest:?}");
    }
    // v2: a page comes as `rows { items }` events (PERF-15), each item a ticket.
    let seqs: Vec<u64> = list
        .iter()
        .flat_map(|i| i["content"]["items"].as_array().cloned().unwrap_or_else(|| vec![i["content"].clone()]))
        .filter_map(|c| c.get("ticket").and_then(|t| t["seq"].as_u64()).or_else(|| c["summary"]["seq"].as_u64()))
        .collect();
    assert_eq!(seqs.len(), 1001, "every row");
    assert!(seqs.windows(2).all(|w| w[0] < w[1]), "the rows in the order they were sent");
}

/// TICKETS-35: a client signed in as the key `seed` (not the operator) on
/// `port` — a principal with no grant until one is given.
async fn connect_as(port: u16, seed: &[u8; 32]) -> WsClient {
    let url = format!("ws://127.0.0.1:{port}");
    let mut h = jsonrpsee::ws_client::HeaderMap::new();
    h.insert("authorization", jsonrpsee::ws_client::HeaderValue::from_str(&format!("Bearer {}", support::proof_of(seed, port))).unwrap());
    WsClientBuilder::default().set_headers(h).build(&url).await.unwrap_or_else(|e| panic!("{url}: {e}"))
}

/// TICKETS-35: the one event a finite call answers.
async fn one(client: &WsClient, method: &str, params: Value) -> Value {
    let evs = collect(call(client, method, params.clone()).await).await;
    assert_eq!(evs.len(), 1, "{method} {params}: {evs:?}");
    evs.into_iter().next().unwrap()
}

/// TICKETS-35: a uid as the store mints one — 8 characters of Crockford's
/// base32, lower case.
fn is_uid(v: &Value) -> bool {
    v.as_str().is_some_and(|u| u.len() == 8 && u.bytes().all(|b| b"0123456789abcdefghjkmnpqrstvwxyz".contains(&b)))
}

/// TICKETS-35 (TICKETS-32's case): two shortcodes named PERF in two
/// namespaces, the original's ticket keyed `PERF-2` beside a namesake's
/// `PERF.N-2`. A write by either's full address lands on its own (TICKETS-32:
/// the original's was refused as ambiguous, its key read again as a name);
/// every ticket carries a uid, and `get`, `amend` and `update` take it; the
/// bare id answers `candidates` and `choices`, each choice with its parent;
/// a reader of one organization sees its own choices alone, and a choice
/// its caller may not read carries its address alone.
#[tokio::test]
async fn a_ticket_is_read_and_written_by_its_address_or_its_uid_in_both_namespaces_over_the_wire() {
    let port = start_server().await;
    let client = connect(port).await;
    let made = |evs: Vec<Value>| -> Value {
        assert_eq!(evs[0]["type"], "created", "{evs:?}");
        evs[0]["ticket"].clone()
    };
    let create = |p: Value| {
        let client = &client;
        async move { made(collect(call(client, "tickets.create", p).await).await) }
    };
    create(json!({"epic": "MENGER", "title": "menger", "kind": "scope"})).await;
    create(json!({"epic": "STORE", "title": "the store and the server", "kind": "scope", "parent": "MENGER-1"})).await;
    let perf1 = create(json!({"epic": "PERF", "title": "Magnitude", "kind": "scope", "parent": "STORE-1"})).await;
    let original = create(json!({"epic": "PERF", "title": "Compression on the WebSocket", "parent": "PERF-1"})).await;
    create(json!({"epic": "PLEXUS", "title": "the platform", "kind": "scope"})).await;
    let theirs1 = create(json!({"epic": "PERF", "title": "Self-description cached", "kind": "scope", "parent": "PLEXUS-1"})).await;
    let namesake = create(json!({"epic": "PLEXUS/PERF", "title": "the schema answers 3.24 MB", "kind": "gap", "parent": theirs1["id"]})).await;
    assert_eq!((original["id"].as_str(), original["address"].as_str()), (Some("PERF-2"), Some("hypermemetic:MENGER/STORE/PERF-2")));
    assert_eq!(namesake["address"], "hypermemetic:PLEXUS/PERF-2");
    let nid = namesake["id"].as_str().unwrap().to_string();
    assert!(nid.starts_with("PERF.") && nid.ends_with("-2"), "the namesake's key: {nid}");

    // TICKETS-32: a write by the full address lands on its own ticket — in
    // both namespaces, and by a short form that fits one.
    let amended = one(&client, "tickets.amend", json!({"id": "hypermemetic:MENGER/STORE/PERF-2", "body": "by its address"})).await;
    assert_eq!((amended["type"].as_str(), amended["ticket"]["id"].as_str()), (Some("amended"), Some("PERF-2")), "{amended}");
    let amended = one(&client, "tickets.amend", json!({"id": "hypermemetic:PLEXUS/PERF-2", "body": "by its address"})).await;
    assert_eq!((amended["type"].as_str(), amended["ticket"]["id"].as_str()), (Some("amended"), Some(nid.as_str())), "{amended}");
    let updated = one(&client, "tickets.update", json!({"id": "STORE/PERF-2", "severity": "High"})).await;
    assert_eq!((updated["type"].as_str(), updated["ticket"]["severity"].as_str()), (Some("updated"), Some("High")), "{updated}");
    let moved = one(&client, "tickets.set_status", json!({"id": "hypermemetic:MENGER/STORE/PERF-2", "status": "Ready"})).await;
    assert_eq!((moved["type"].as_str(), moved["ticket"]["id"].as_str()), (Some("status_changed"), Some("PERF-2")), "{moved}");
    let linked = one(&client, "tickets.commits.link", json!({"id": "hypermemetic:MENGER/STORE/PERF-2", "commits": ["77df9a0"]})).await;
    assert_eq!(linked["type"], "commits_linked", "{linked}");
    let labelled = one(&client, "tickets.label", json!({"id": "hypermemetic:MENGER/STORE/PERF-2", "add": ["needs-you"]})).await;
    assert_eq!(labelled["type"], "labels_changed", "{labelled}");

    // Every ticket carries a uid, minted at create; no two alike.
    let all = rows(call(&client, "tickets.list", json!({"summary": true})).await).await;
    let uids: std::collections::BTreeSet<String> = all.iter().map(|r| r["summary"]["uid"].as_str().unwrap_or_default().to_string()).collect();
    assert_eq!(uids.len(), 7, "seven tickets, seven uids: {all:?}");
    for t in [&perf1, &original, &theirs1, &namesake] {
        assert!(is_uid(&t["uid"]), "created answers a uid: {t}");
        assert!(uids.contains(t["uid"].as_str().unwrap()), "the uid it was born with: {t}");
    }
    let (ouid, nuid) = (original["uid"].as_str().unwrap(), namesake["uid"].as_str().unwrap());

    // `get`, `amend` and `update` by the uid, in both namespaces.
    for (uid, id) in [(ouid, "PERF-2"), (nuid, nid.as_str())] {
        let got = one(&client, "tickets.get", json!({"id": uid})).await;
        assert_eq!((got["type"].as_str(), got["ticket"]["id"].as_str(), got["ticket"]["uid"].as_str()), (Some("ticket"), Some(id), Some(uid)), "{got}");
        let upper = one(&client, "tickets.get", json!({"id": uid.to_ascii_uppercase()})).await;
        assert_eq!(upper["ticket"]["id"].as_str(), Some(id), "a uid reads case-blind: {upper}");
        let amended = one(&client, "tickets.amend", json!({"id": uid, "title": format!("{id}, by its uid")})).await;
        assert_eq!((amended["type"].as_str(), amended["ticket"]["id"].as_str()), (Some("amended"), Some(id)), "{amended}");
        let updated = one(&client, "tickets.update", json!({"id": uid, "severity": "Low"})).await;
        assert_eq!((updated["type"].as_str(), updated["ticket"]["id"].as_str(), updated["ticket"]["uid"].as_str()), (Some("updated"), Some(id), Some(uid)), "{updated}");
    }
    for (address, id) in [("hypermemetic:MENGER/STORE/PERF-2", "PERF-2"), ("hypermemetic:PLEXUS/PERF-2", nid.as_str())] {
        let got = one(&client, "tickets.get", json!({"id": address})).await;
        assert_eq!(got["ticket"]["id"].as_str(), Some(id), "{got}");
    }

    // The bare id fits both: `candidates` as before, and a choice each —
    // uid, id, address, org, epic, kind, status, title and its parent.
    let both = one(&client, "tickets.get", json!({"id": "PERF-2"})).await;
    assert_eq!(both["type"], "ambiguous", "{both}");
    assert_eq!(both["candidates"], json!(["hypermemetic:MENGER/STORE/PERF-2", "hypermemetic:PLEXUS/PERF-2"]), "{both}");
    let choices = both["choices"].as_array().unwrap_or_else(|| panic!("choices: {both}"));
    assert_eq!(choices.len(), 2, "{both}");
    let (a, b) = (&choices[0], &choices[1]);
    assert_eq!(
        (a["uid"].as_str(), a["id"].as_str(), a["address"].as_str(), a["org"].as_str(), a["epic"].as_str(), a["kind"].as_str(), a["status"].as_str(), a["title"].as_str()),
        (Some(ouid), Some("PERF-2"), Some("hypermemetic:MENGER/STORE/PERF-2"), Some("hypermemetic"), Some("genesis"), Some("build"), Some("Ready"), Some("PERF-2, by its uid")),
        "{a}"
    );
    assert_eq!(
        (a["parent"]["uid"].as_str(), a["parent"]["id"].as_str(), a["parent"]["address"].as_str(), a["parent"]["title"].as_str()),
        (perf1["uid"].as_str(), Some("PERF-1"), Some("hypermemetic:MENGER/STORE/PERF-1"), Some("Magnitude")),
        "{a}"
    );
    assert_eq!((b["uid"].as_str(), b["id"].as_str(), b["kind"].as_str()), (Some(nuid), Some(nid.as_str()), Some("gap")), "{b}");
    assert_eq!((b["parent"]["uid"].as_str(), b["parent"]["address"].as_str()), (theirs1["uid"].as_str(), Some("hypermemetic:PLEXUS/PERF-1")), "{b}");
    // A write by the bare id stays ambiguous, with the same choices.
    let refused = one(&client, "tickets.amend", json!({"id": "PERF-2", "body": "which one?"})).await;
    assert_eq!((refused["type"].as_str(), refused["choices"].as_array().map(Vec::len)), (Some("ambiguous"), Some(2)), "{refused}");

    // A reader of one organization: acme holds two PERFs of its own. Its
    // choices are acme's alone; another organization's uid is not found.
    const CAROL: [u8; 32] = [0xc4; 32];
    let carol_key = support::principal_of(&CAROL);
    let acme = one(&client, "orgs.create", json!({"name": "acme", "admin": carol_key})).await;
    assert_eq!(acme["type"], "created", "{acme}");
    let carol = connect_as(port, &CAROL).await;
    let made_by_carol = |evs: Vec<Value>| -> Value {
        assert_eq!(evs[0]["type"], "created", "{evs:?}");
        evs[0]["ticket"].clone()
    };
    made_by_carol(collect(call(&carol, "tickets.create", json!({"epic": "PERF", "title": "acme's perf", "kind": "scope"})).await).await);
    made_by_carol(collect(call(&carol, "tickets.create", json!({"epic": "acme:PERF", "title": "acme's PERF-2"})).await).await);
    let ops = made_by_carol(collect(call(&carol, "tickets.create", json!({"epic": "OPS", "title": "acme's ops", "kind": "scope"})).await).await);
    let ops_perf = made_by_carol(collect(call(&carol, "tickets.create", json!({"epic": "PERF", "title": "ops perf", "kind": "scope", "parent": ops["id"]})).await).await);
    made_by_carol(collect(call(&carol, "tickets.create", json!({"epic": "acme:OPS/PERF", "title": "ops PERF-2", "parent": ops_perf["id"]})).await).await);
    let hers = one(&carol, "tickets.get", json!({"id": "PERF-2"})).await;
    assert_eq!(hers["type"], "ambiguous", "{hers}");
    assert_eq!(hers["candidates"], json!(["acme:PERF-2", "acme:OPS/PERF-2"]), "acme's alone: {hers}");
    let titles: Vec<&str> = hers["choices"].as_array().unwrap().iter().map(|c| c["title"].as_str().unwrap_or_default()).collect();
    assert_eq!(titles, ["acme's PERF-2", "ops PERF-2"], "{hers}");
    assert!(hers["choices"].as_array().unwrap().iter().all(|c| c["org"] == "acme" && is_uid(&c["uid"]) && c["parent"]["address"].as_str().is_some_and(|p| p.starts_with("acme:"))), "{hers}");
    assert_eq!(one(&carol, "tickets.get", json!({"id": ouid})).await["type"], "not_found", "another organization's uid");
    // The operator, of `*`, is told all four.
    let four = one(&client, "tickets.get", json!({"id": "PERF-2"})).await;
    assert_eq!(four["choices"].as_array().map(Vec::len), Some(4), "{four}");

    // A choice the caller may not read carries its address alone: Bob edits
    // MENGER's tree; the bare id is the original's key, so the gate admits
    // his write, and the store says which two it could be.
    const BOB: [u8; 32] = [0xb0; 32];
    let granted = one(&client, "members.grant", json!({"principal": support::principal_of(&BOB), "scope": "hypermemetic:MENGER", "role": "editor"})).await;
    assert_eq!(granted["type"], "grant", "{granted}");
    let bob = connect_as(port, &BOB).await;
    let his = one(&bob, "tickets.amend", json!({"id": "PERF-2", "body": "which one?"})).await;
    assert_eq!(his["type"], "ambiguous", "{his}");
    assert_eq!(his["candidates"], json!(["hypermemetic:MENGER/STORE/PERF-2", "hypermemetic:PLEXUS/PERF-2"]), "{his}");
    let seen = &his["choices"][0];
    assert_eq!((seen["uid"].as_str(), seen["title"].as_str()), (Some(ouid), Some("PERF-2, by its uid")), "the one he reads: {his}");
    assert_eq!(his["choices"][1], json!({"address": "hypermemetic:PLEXUS/PERF-2"}), "the one he may not read: its address alone");
    let landed = one(&bob, "tickets.amend", json!({"id": ouid, "body": "Bob, by its uid"})).await;
    assert_eq!(landed["type"], "amended", "{landed}");
}

/// TICKETS-35 (rows 4 and 9): a uid names its ticket wherever a ticket is
/// named — `get` by the bare uid, by `/t/<uid>/…` and by the whole link; the
/// filter words `under` and `ticket`; a deleted ticket's uid answers its
/// tombstone — and a row names the uids of the tickets it names (`uid_of`).
#[tokio::test]
async fn a_uid_names_its_ticket_in_get_and_in_the_filters_over_the_wire() {
    let port = start_server().await;
    let client = connect(port).await;
    let mut made = Vec::new();
    for p in [
        json!({"epic": "KILN", "title": "the kiln", "kind": "scope"}),
        json!({"epic": "KILN", "title": "a firing log", "parent": "KILN-1"}),
        json!({"epic": "KILN", "title": "the glaze", "parent": "KILN-2", "blocked_by": ["KILN-2"]}),
        json!({"epic": "KILN", "title": "a shelf", "parent": "KILN-1", "blocked_by": ["KILN-5"]}),
    ] {
        let evs = collect(call(&client, "tickets.create", p).await).await;
        assert_eq!(evs[0]["type"], "created", "{evs:?}");
        made.push(evs[0]["ticket"].clone());
    }
    let uid = |i: usize| made[i]["uid"].as_str().unwrap_or_else(|| panic!("a uid: {}", made[i])).to_string();
    let (k2, k3) = (uid(1), uid(2));

    // `get` by the bare uid, by `/t/<uid>/<anything>`, by the whole link.
    for name in [k2.clone(), format!("/t/{k2}/KILN-2-a-firing-log"), format!("https://app.menger.sh/t/{k2}/KILN-2-a-firing-log"), format!("/t/{k2}")] {
        let got = one(&client, "tickets.get", json!({"id": name})).await;
        assert_eq!((got["type"].as_str(), got["ticket"]["id"].as_str()), (Some("ticket"), Some("KILN-2")), "{name}: {got}");
    }
    assert_eq!(one(&client, "tickets.get", json!({"id": "zzzzzzzz"})).await["type"], "not_found", "a uid no ticket has");

    // `uid_of`: the parent's, a blocker's, an unlock's — keyed as the row
    // writes them; a forward reference has none.
    let k3got = one(&client, "tickets.get", json!({"id": k3})).await["ticket"].clone();
    assert_eq!(k3got["uid_of"], json!({"KILN-2": k2}), "parent and blocker are one: {k3got}");
    let k2got = one(&client, "tickets.get", json!({"id": "KILN-2"})).await["ticket"].clone();
    assert_eq!(k2got["uid_of"], json!({"KILN-1": uid(0), "KILN-3": k3}), "its parent and what it unlocks: {k2got}");
    let k4 = one(&client, "tickets.get", json!({"id": "KILN-4"})).await["ticket"].clone();
    assert_eq!(k4["blocked_by"], json!(["KILN-5"]), "{k4}");
    assert_eq!(k4["uid_of"], json!({"KILN-1": uid(0)}), "a forward reference has none: {k4}");
    let summaries = rows(call(&client, "tickets.list", json!({"summary": true, "parent": k2})).await).await;
    assert_eq!(summaries[0]["summary"]["uid_of"], json!({"KILN-2": k2}), "a summary names them too: {summaries:?}");
    let k1 = one(&client, "tickets.get", json!({"id": "KILN-1"})).await["ticket"].clone();
    assert!(k1.get("uid_of").is_none(), "omitted when empty: {k1}");

    // The filter words take a uid: `under` the ticket and its descendants,
    // `ticket` the one.
    let ids = |evs: Vec<Value>| -> Vec<String> { evs.iter().filter_map(|e| e["summary"]["id"].as_str().map(str::to_string)).collect() };
    let under = ids(rows(call(&client, "tickets.list", json!({"summary": true, "filter": format!("under {k2}")})).await).await);
    assert_eq!(under, ["KILN-2", "KILN-3"]);
    let upper = ids(rows(call(&client, "tickets.list", json!({"summary": true, "filter": format!("under {}", k2.to_ascii_uppercase())})).await).await);
    assert_eq!(upper, ["KILN-2", "KILN-3"], "case-blind");
    let alone = ids(rows(call(&client, "tickets.list", json!({"summary": true, "filter": format!("ticket {k3}")})).await).await);
    assert_eq!(alone, ["KILN-3"]);

    // Every parameter that names a ticket takes it: a parent, a blocker.
    let child = one(&client, "tickets.create", json!({"epic": "KILN", "title": "by uid", "parent": k2, "blocked_by": [k3]})).await;
    assert_eq!((child["ticket"]["parent"].as_str(), child["ticket"]["blocked_by"].clone()), (Some("KILN-2"), json!(["KILN-3"])), "{child}");
    let moved = one(&client, "tickets.set_parent", json!({"id": child["ticket"]["uid"], "parent": made[0]["uid"]})).await;
    assert_eq!((moved["type"].as_str(), moved["ticket"]["parent"].as_str()), (Some("parent_changed"), Some("KILN-1")), "{moved}");

    // A deleted ticket's uid answers its tombstone; undeleted, the same uid.
    let gone = one(&client, "tickets.delete", json!({"ids": [child["ticket"]["uid"]], "unblock": true})).await;
    assert_eq!(gone["type"], "deleted", "{gone}");
    let tomb = one(&client, "tickets.get", json!({"id": child["ticket"]["uid"]})).await;
    assert_eq!((tomb["type"].as_str(), tomb["tombstones"][0]["uid"].clone()), (Some("deleted"), child["ticket"]["uid"].clone()), "{tomb}");
    let back = one(&client, "tickets.undelete", json!({"ids": [child["ticket"]["id"]]})).await;
    assert_eq!(back["tickets"][0]["uid"], child["ticket"]["uid"], "{back}");
}
