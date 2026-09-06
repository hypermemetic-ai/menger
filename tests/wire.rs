//! Wire-level tests: a real TransportServer, a real WebSocket client,
//! the same envelope web/index.html speaks — `menger.call` subscriptions
//! carrying `{method, params}`, events arriving as PlexusStreamItems.

use jsonrpsee::core::client::{Subscription, SubscriptionClientT};
use jsonrpsee::core::params::ObjectParams;
use jsonrpsee::ws_client::{WsClient, WsClientBuilder};
use menger::build_rpc;
use plexus_core::plexus::DynamicHub;
use plexus_transport::TransportServer;
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::Duration;

async fn free_port() -> u16 {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    listener.local_addr().unwrap().port()
}

/// Starts a server whose database lives at `db_path`. The caller owns the
/// location's lifetime; `keep` is dropped (cleaning up) when the test's
/// runtime tears the server task down.
async fn start_server_at(db_path: std::path::PathBuf, keep: Option<tempfile::TempDir>) -> u16 {
    let port = free_port().await;
    let hub = build_rpc(&db_path).await.expect("open database");
    let conv = |arc: Arc<DynamicHub>| {
        DynamicHub::arc_into_rpc_module(arc).map_err(|e| anyhow::anyhow!("{e}"))
    };
    tokio::spawn(async move {
        let _keep = keep;
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
    client
        .subscribe("menger.call", p, "menger.call_unsub")
        .await
        .expect("subscribe")
}

/// Unwraps a PlexusStreamItem, returning the activation event inside `data`.
fn content(item: Value) -> Value {
    assert_eq!(item["type"], "data", "expected data item, got {item}");
    item["content"].clone()
}

/// Drains a finite call: collects every `data` item's content until `done`.
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
    assert_eq!(ev["ticket"]["status"], "Pending");

    // the watch subscriber saw it pushed, unprompted
    let ev = content(watch.next().await.unwrap().unwrap());
    assert_eq!(ev["type"], "created");
    assert_eq!(ev["ticket"]["id"], "WIRE-1");

    // set_status → watch sees the transition too
    let mut sub = call(
        &client,
        "tickets.set_status",
        json!({"id": "WIRE-1", "status": "Ready"}),
    )
    .await;
    let ev = content(sub.next().await.unwrap().unwrap());
    assert_eq!(ev["type"], "status_changed");
    assert_eq!(ev["previous"], "Pending");

    let ev = content(watch.next().await.unwrap().unwrap());
    assert_eq!(ev["type"], "status_changed");
    assert_eq!(ev["ticket"]["status"], "Ready");

    // list replays current state
    let mut sub = call(&client, "tickets.list", json!({})).await;
    let ev = content(sub.next().await.unwrap().unwrap());
    assert_eq!(ev["type"], "ticket");
    assert_eq!(ev["ticket"]["id"], "WIRE-1");
}

#[tokio::test]
async fn in_band_errors_cross_the_wire() {
    let port = start_server().await;
    let client = connect(port).await;

    let mut sub = call(
        &client,
        "tickets.set_status",
        json!({"id": "X-1", "status": "wibble"}),
    )
    .await;
    let ev = content(sub.next().await.unwrap().unwrap());
    assert_eq!(ev["type"], "error");
    assert!(ev["message"].as_str().unwrap().contains("wibble"));
}

/// A watch-only client's local mirror: every mutation event carries the
/// full affected ticket, so applying it is one upsert.
fn apply(mirror: &mut std::collections::HashMap<String, Value>, ev: &Value) {
    match ev["type"].as_str() {
        Some("created")
        | Some("status_changed")
        | Some("unlocks_changed")
        | Some("children_changed")
        | Some("parent_changed")
        | Some("child_removed") => {
            let ticket = ev["ticket"].clone();
            let id = ticket["id"].as_str().expect("ticket id").to_string();
            mirror.insert(id, ticket);
        }
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
                    .is_some_and(|bt| bt["status"] == "Complete" || bt["status"] == "Superseded")
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

    // A exists before the client arrives.
    let evs = collect(call(&client, "tickets.create", json!({"epic": "RS", "title": "A", "kind": "scope"})).await).await;
    assert_eq!(evs[0]["type"], "created");

    // The client fetches the full set once...
    for ev in collect(call(&client, "tickets.list", json!({})).await).await {
        assert_eq!(ev["type"], "ticket");
        let ticket = ev["ticket"].clone();
        mirror.insert(ticket["id"].as_str().unwrap().to_string(), ticket);
    }
    assert_eq!(mirror.len(), 1);

    // ...then consumes only watch events.
    let mut watch = call(&client, "tickets.watch", json!({})).await;

    // create B blocked_by [A]; promote both to Ready.
    collect(call(&client, "tickets.create",
        json!({"epic": "RS", "title": "B", "blocked_by": ["RS-1"]})).await).await;
    collect(call(&client, "tickets.set_status", json!({"id": "RS-1", "status": "Ready"})).await).await;
    collect(call(&client, "tickets.set_status", json!({"id": "RS-2", "status": "Ready"})).await).await;

    // Watch delivered: created(RS-2), children_changed(RS-1) — RS-2 named
    // no parent and hangs under the root (MNG-14) — unlocks_changed(RS-1),
    // 2x status_changed.
    for _ in 0..5 {
        let ev = content(watch.next().await.unwrap().unwrap());
        apply(&mut mirror, &ev);
    }
    assert_eq!(mirror["RS-1"]["unlocks"], json!(["RS-2"]));

    // Checkpoint 1: A is ready, B is not — server and mirror agree.
    let server = ready_ids(collect(call(&client, "tickets.ready_set", json!({})).await).await);
    assert_eq!(server, vec!["RS-1"]);
    assert_eq!(server, local_ready(&mirror));

    // A completes; B unblocks.
    collect(call(&client, "tickets.set_status", json!({"id": "RS-1", "status": "Complete"})).await).await;
    let ev = content(watch.next().await.unwrap().unwrap());
    apply(&mut mirror, &ev);

    let server = ready_ids(collect(call(&client, "tickets.ready_set", json!({})).await).await);
    assert_eq!(server, vec!["RS-2"]);
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

    let ready = collect(call(&client, "tickets.ready_set", json!({})).await).await;
    assert!(ready.is_empty(), "DG-1 must not be ready: {ready:?}");

    let evs = collect(call(&client, "tickets.blockers", json!({"id": "DG-1"})).await).await;
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
/// parent at create, `get` shows both halves, the watch-only mirror sees
/// the parent's update as `children_changed` carrying the full ticket, a
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
    assert_eq!(evs[0]["ticket"]["children"], json!([]), "born childless");
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

    // The watcher saw the birth and the parent's update, full ticket.
    for _ in 0..2 {
        let ev = content(watch.next().await.unwrap().unwrap());
        if ev["type"] == "children_changed" {
            assert_eq!(ev["ticket"]["id"], "PA-1");
            assert_eq!(ev["added"], "PA-2");
            assert_eq!(ev["ticket"]["children"], json!(["PA-2"]));
            assert_eq!(ev["ticket"]["title"], "P", "the full parent, not a delta");
        }
        apply(&mut mirror, &ev);
    }
    assert_eq!(mirror["PA-1"]["children"], json!(["PA-2"]),
        "a watch-only client's mirror stays coherent");

    // Both ends agree when asked directly.
    let evs = collect(call(&client, "tickets.get", json!({"id": "PA-1"})).await).await;
    assert_eq!(evs[0]["ticket"]["children"], json!(["PA-2"]));
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
        assert_eq!(evs[0]["ticket"]["children"], json!([child]), "{parent}");
    }
    let evs = collect(call(&client, "tickets.get", json!({"id": "PB-2"})).await).await;
    assert_eq!(evs[0]["ticket"]["children"], json!([]), "the leaf has none");

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
        ids(collect(call(&client, "tickets.list", json!({"parent": "PA-1"})).await).await),
        vec!["PA-2", "PA-3", "PC-1"]
    );
    assert_eq!(
        ids(collect(call(&client, "tickets.list", json!({"parent": "PA-3"})).await).await),
        Vec::<String>::new()
    );
    assert_eq!(
        ids(collect(call(&client, "tickets.list", json!({})).await).await).len(),
        6,
        "the unfiltered list is unchanged"
    );
}

#[tokio::test]
async fn shell_serves_the_page() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let shell_port = free_port().await;
    tokio::spawn(async move {
        // Same call, MFE-2 signature: config struct instead of two ports.
        // No REST upstream and an empty app dist — this test only cares
        // that the vanilla page at `/` still serves, unchanged.
        let _ = menger::shell::serve(menger::shell::ShellConfig {
            port: shell_port,
            ws_port: 4455,
            rest_port: None,
            app_dist: std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR")),
        })
        .await;
    });

    let mut resp = Vec::new();
    for _ in 0..50 {
        match tokio::net::TcpStream::connect(("127.0.0.1", shell_port)).await {
            Ok(mut s) => {
                s.write_all(b"GET / HTTP/1.0\r\nHost: localhost\r\n\r\n")
                    .await
                    .unwrap();
                s.read_to_end(&mut resp).await.unwrap();
                break;
            }
            Err(_) => tokio::time::sleep(Duration::from_millis(50)).await,
        }
    }
    let text = String::from_utf8_lossy(&resp);
    assert!(text.starts_with("HTTP/1.0 200") || text.starts_with("HTTP/1.1 200"));
    assert!(text.contains("<main id=\"main\">"));
    assert!(text.contains("menger.call"), "shell speaks the envelope");
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
    let ev = content(sub.next().await.unwrap().unwrap());
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
                let ev = content(sub.next().await.unwrap().unwrap());
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
        // Each create after the root also updates the root's `children`
        // (MNG-14), so: 1 + (MUTATIONS - 1) × (created, children_changed,
        // status_changed).
        let mut next_seq = 1u64;
        for _ in 0..(3 * MUTATIONS - 2) {
            let ev = content(watch.next().await.unwrap().unwrap());
            match ev["type"].as_str().unwrap() {
                "created" => {
                    assert_eq!(ev["ticket"]["seq"], next_seq);
                    next_seq += 1;
                }
                "children_changed" => assert_eq!(ev["ticket"]["id"], "LOAD-1"),
                "status_changed" => {}
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
/// watch-only mirror sees the child (`parent_changed`) and both parents
/// (`children_changed`, `child_removed`) as full tickets; `list {parent}`
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
    for ev in collect(call(&client, "tickets.list", json!({})).await).await {
        mirror.insert(ev["ticket"]["id"].as_str().unwrap().to_string(), ev["ticket"].clone());
    }
    assert_eq!(mirror["SC-1"]["children"], json!(["SC-2", "SC-3", "SD-1"]));
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
                assert_eq!(ev["ticket"]["id"], "SC-2");
                assert_eq!(ev["added"], "SC-3");
                assert_eq!(ev["ticket"]["title"], "a", "the full parent, not a delta");
            }
            "child_removed" => {
                assert_eq!(ev["ticket"]["id"], "SC-1");
                assert_eq!(ev["removed"], "SC-3");
            }
            _ => {}
        }
        apply(&mut mirror, &ev);
    }
    assert_eq!(mirror["SC-3"]["parent"], "SC-2");
    assert_eq!(mirror["SC-2"]["children"], json!(["SC-3"]));
    assert_eq!(mirror["SC-1"]["children"], json!(["SC-2", "SD-1"]));

    let evs = collect(call(&client, "tickets.list", json!({"parent": "SC-2"})).await).await;
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
