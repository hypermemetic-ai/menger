//! MFE-11: menger's wire test, re-expressed through the **generated
//! client** — `TicketsClient<&WsTransport>`, emitted by
//! `#[plexus_macros::activation(client, …)]` on the activation's own `impl`
//! in `menger-tickets-api` — against the in-process server on the real
//! wire.
//!
//! `tests/wire.rs` drives a hand-written jsonrpsee subscription and asserts
//! on `serde_json::Value` (`ev["ticket"]["status"] == "Pending"`). This file
//! asserts on `TicketsEvent::StatusChanged { previous: Status::Pending, .. }`
//! — the same wire, the same server, typed. Everything between the socket
//! and the assertion is MACRO-2's: `WsTransport` → `StreamItem` → `decode`
//! → the activation's own event enum. And because the client is generated
//! from the one declaration the server runs, a renamed parameter or event
//! field fails *this file's* compile — there is no mirror to fall out of
//! step.
//!
//! MACRO-6 first ran this shape from a fixture in plexus-macros that
//! mirrored menger's declarations by hand. The mirror is gone; the test
//! lives where the declarations do.

use futures::StreamExt;
use menger::build_rpc;
use menger_tickets_api::{Group, Status, Ticket, TicketsClient, TicketsEvent};
use plexus_client::{ClientEvent, MethodRef, StreamItem, Transport, WsTransport};
use plexus_core::plexus::DynamicHub;
use plexus_transport::TransportServer;
use serde_json::json;
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use std::time::Duration;

const HUB: &str = "menger";

async fn free_port() -> u16 {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    listener.local_addr().unwrap().port()
}

/// The same in-process server `tests/wire.rs` starts, on a throwaway
/// database. Production (4455–4457) is never touched.
async fn start_server() -> u16 {
    let dir = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).expect("tempdir");
    let db_path = dir.path().join("tickets.db");
    let port = free_port().await;
    let hub = build_rpc(&db_path).await.expect("open database");
    let conv = |arc: Arc<DynamicHub>| {
        DynamicHub::arc_into_rpc_module(arc).map_err(|e| anyhow::anyhow!("{e}"))
    };
    tokio::spawn(async move {
        let _keep = dir;
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

async fn connect(port: u16) -> WsTransport {
    let url = format!("ws://127.0.0.1:{port}");
    for _ in 0..100 {
        if let Ok(transport) = WsTransport::connect(&url, HUB).await {
            return transport;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("server never came up on {url}");
}

// ---------------------------------------------------------------------------
// The watch-only mirror — `tests/wire.rs::apply`, typed
// ---------------------------------------------------------------------------

/// Every mutation event carries the full affected ticket, so applying one
/// is a single upsert. In `wire.rs` this is a `match` on a type string with
/// a `panic!` arm; here it is a `match` over the event enum, so an event
/// kind the mirror does not handle is a compile-time question, not a
/// runtime one. MNG-10's `GroupChanged` carries the whole group, so it is
/// one upsert too — into the client's copy of the nav's roll-ups.
fn apply(
    mirror: &mut HashMap<String, Ticket>,
    groups: &mut BTreeMap<String, Group>,
    event: &TicketsEvent,
) -> &'static str {
    let (ticket, kind) = match event {
        TicketsEvent::Created { ticket } => (ticket, "created"),
        TicketsEvent::StatusChanged { ticket, .. } => (ticket, "status_changed"),
        TicketsEvent::UnlocksChanged { ticket, .. } => (ticket, "unlocks_changed"),
        TicketsEvent::ChildrenChanged { ticket, .. } => (ticket, "children_changed"),
        TicketsEvent::ParentChanged { ticket, .. } => (ticket, "parent_changed"),
        TicketsEvent::ChildRemoved { ticket, .. } => (ticket, "child_removed"),
        TicketsEvent::GroupChanged { group } => {
            groups.insert(group.epic.clone(), group.clone());
            return "group_changed";
        }
        other => panic!("unexpected watch event {other:?}"),
    };
    mirror.insert(ticket.id.clone(), ticket.clone());
    kind
}

/// The one event a single-event call answers with.
fn one(events: Vec<TicketsEvent>) -> TicketsEvent {
    assert_eq!(events.len(), 1, "expected exactly one event, got {events:?}");
    events.into_iter().next().unwrap()
}

fn created(events: Vec<TicketsEvent>) -> Ticket {
    match one(events) {
        TicketsEvent::Created { ticket } => ticket,
        other => panic!("expected `created`, got {other:?}"),
    }
}

/// The rows of a paged call (MNG-9): every event but the trailing `page`,
/// which is asserted to be there exactly once.
fn rows(events: Vec<TicketsEvent>) -> Vec<TicketsEvent> {
    let pages = events
        .iter()
        .filter(|e| matches!(e, TicketsEvent::Page { .. }))
        .count();
    assert_eq!(pages, 1, "one page event per paged call: {events:?}");
    assert!(
        matches!(events.last(), Some(TicketsEvent::Page { .. })),
        "the page event is last: {events:?}"
    );
    events
        .into_iter()
        .filter(|e| !matches!(e, TicketsEvent::Page { .. }))
        .collect()
}

fn ids(events: Vec<TicketsEvent>) -> Vec<String> {
    events
        .into_iter()
        .map(|e| match e {
            TicketsEvent::Ticket { ticket } => ticket.id,
            other => panic!("expected `ticket`, got {other:?}"),
        })
        .collect()
}

// ---------------------------------------------------------------------------
// The test
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_wire_assertions_through_the_generated_client() {
    let port = start_server().await;
    let transport = connect(port).await;
    let client = TicketsClient::new(&transport);

    // -- health -------------------------------------------------------------
    match one(client.health_all().await.expect("health")) {
        TicketsEvent::Health {
            ticket_count,
            version,
            ..
        } => {
            assert_eq!(ticket_count, 0, "a fresh database");
            assert!(!version.is_empty());
        }
        other => panic!("expected `health`, got {other:?}"),
    }

    // -- the full envelope --------------------------------------------------
    // The RPC-level client, on the same connection: the WebSocket is the
    // transport that carries `StreamMetadata`.
    let mut raw = transport.call(MethodRef::new("tickets", "health"), json!({}));
    let item = raw.next().await.expect("an item").expect("no transport error");
    match &item {
        StreamItem::Data { metadata, .. } => {
            let metadata = metadata.as_ref().expect("WebSocket carries metadata");
            assert!(
                metadata.provenance.contains(&"tickets".to_string()),
                "provenance names the activation: {:?}",
                metadata.provenance
            );
            assert!(!metadata.plexus_hash.is_empty(), "and the config hash");
            assert!(metadata.timestamp > 0, "and when it was wrapped");
        }
        other => panic!("expected a data item, got {other:?}"),
    }
    drop(raw);

    // -- watch, subscribed before any mutation ------------------------------
    // The subscribe frame is written by `call`, not by the first poll; the
    // round trip flushes it past the server's dispatcher.
    let mut watch = client.watch(None);
    let _ = client.health_all().await.expect("round trip");
    tokio::time::sleep(Duration::from_millis(250)).await;

    // -- create -------------------------------------------------------------
    // wire.rs: created, WIRE-1, Pending. MNG-12: a prefix's first ticket is
    // its scope; MNG-14: the root is top-level.
    let ticket = created(
        client
            .create_all(
                "WIRE".into(),
                "first over the wire".into(),
                Some("scope".into()),
                None,
                None,
                None,
                None,
            )
            .await
            .expect("create"),
    );
    assert_eq!(ticket.id, "WIRE-1");
    assert_eq!(ticket.status, Status::Pending);
    assert_eq!(ticket.seq, 1);
    assert!(ticket.parent.is_none(), "the root is top-level");
    assert!(ticket.children.is_empty(), "and childless");

    // The rule itself, typed: a build first is the activation's own Error.
    match one(
        client
            .create_all("FRESH".into(), "a build first".into(), None, None, None, None, None)
            .await
            .expect("the call succeeds; the activation says no"),
    ) {
        TicketsEvent::Error { message } => assert!(message.contains("scope"), "{message}"),
        other => panic!("expected the in-band `error`, got {other:?}"),
    }

    // -- set_status ---------------------------------------------------------
    match one(
        client
            .set_status_all("WIRE-1".into(), "Ready".into())
            .await
            .expect("set_status"),
    ) {
        TicketsEvent::StatusChanged { ticket, previous } => {
            assert_eq!(previous, Status::Pending);
            assert_eq!(ticket.status, Status::Ready);
        }
        other => panic!("expected `status_changed`, got {other:?}"),
    }

    // -- list replays current state, paged (MNG-9) ---------------------------
    let listed = rows(
        client
            .list_all(Some("WIRE".into()), None, None, None, None, None)
            .await
            .expect("list"),
    );
    assert_eq!(ids(listed), vec!["WIRE-1".to_string()]);

    // -- a dangling blocker -------------------------------------------------
    created(
        client
            .create_all(
                "DG".into(),
                "C".into(),
                Some("scope".into()),
                None,
                Some(vec!["DG-99".into()]),
                None,
                None,
            )
            .await
            .expect("create DG-1"),
    );
    client
        .set_status_all("DG-1".into(), "Ready".into())
        .await
        .expect("promote DG-1");
    let ready = rows(
        client
            .ready_set_all(Some("DG".into()), None, None, None)
            .await
            .expect("ready_set"),
    );
    assert!(ready.is_empty(), "DG-1 must not be ready: {ready:?}");
    match one(client.blockers_all("DG-1".into()).await.expect("blockers")) {
        TicketsEvent::DanglingBlocker { id } => assert_eq!(id, "DG-99"),
        other => panic!("expected `dangling_blocker`, got {other:?}"),
    }

    // -- parent / children (AGS-19, MNG-14, MNG-12's set_parent) ------------
    let parent = created(
        client
            .create_all("PA".into(), "P".into(), Some("scope".into()), None, None, None, None)
            .await
            .expect("create PA-1"),
    );
    assert_eq!(parent.id, "PA-1");
    let child = created(
        client
            .create_all(
                "PA".into(),
                "C".into(),
                Some("execution".into()),
                None,
                None,
                None,
                Some("PA-1".into()),
            )
            .await
            .expect("create PA-2"),
    );
    assert_eq!(child.id, "PA-2");
    assert_eq!(child.parent.as_deref(), Some("PA-1"));
    // No parent named: the shortcode root (MNG-14).
    let third = created(
        client
            .create_all("PA".into(), "loose".into(), None, None, None, None, None)
            .await
            .expect("create PA-3"),
    );
    assert_eq!(third.parent.as_deref(), Some("PA-1"), "defaulted to the root");

    match one(client.get_all("PA-1".into()).await.expect("get PA-1")) {
        TicketsEvent::Ticket { ticket } => {
            assert_eq!(ticket.children, vec!["PA-2".to_string(), "PA-3".to_string()])
        }
        other => panic!("expected `ticket`, got {other:?}"),
    }
    let children = ids(rows(
        client
            .list_all(None, None, Some("PA-1".into()), None, None, None)
            .await
            .expect("list by parent"),
    ));
    assert_eq!(children, vec!["PA-2".to_string(), "PA-3".to_string()]);

    // The MNG-12 verb through the generated client: move PA-3 under PA-2.
    match one(
        client
            .set_parent_all("PA-3".into(), "PA-2".into())
            .await
            .expect("set_parent"),
    ) {
        TicketsEvent::ParentChanged { ticket, previous } => {
            assert_eq!(ticket.parent.as_deref(), Some("PA-2"));
            assert_eq!(previous.as_deref(), Some("PA-1"));
        }
        other => panic!("expected `parent_changed`, got {other:?}"),
    }
    match one(client.get_all("PA-2".into()).await.expect("get PA-2")) {
        TicketsEvent::Ticket { ticket } => assert_eq!(ticket.children, vec!["PA-3".to_string()]),
        other => panic!("expected `ticket`, got {other:?}"),
    }

    // -- an unlocks edge ----------------------------------------------------
    created(
        client
            .create_all("RS".into(), "A".into(), Some("scope".into()), None, None, None, None)
            .await
            .expect("create RS-1"),
    );
    created(
        client
            .create_all(
                "RS".into(),
                "B".into(),
                None,
                None,
                Some(vec!["RS-1".into()]),
                None,
                None,
            )
            .await
            .expect("create RS-2"),
    );

    // -- the watch-only mirror ---------------------------------------------
    // Every mutation event kind, decoded into the activation's own types.
    let mut mirror: HashMap<String, Ticket> = HashMap::new();
    let mut group_copy: BTreeMap<String, Group> = BTreeMap::new();
    let mut kinds: Vec<&'static str> = Vec::new();
    let mut first_transition: Option<Status> = None;
    loop {
        match tokio::time::timeout(Duration::from_millis(1500), watch.next()).await {
            Err(_) => break, // quiet: every change has arrived
            Ok(None) => panic!("the watch stream ended"),
            Ok(Some(event)) => match event.expect("no client error on watch") {
                ClientEvent::Data(event) => {
                    if let TicketsEvent::StatusChanged { previous, .. } = &event {
                        first_transition.get_or_insert(*previous);
                    }
                    kinds.push(apply(&mut mirror, &mut group_copy, &event));
                }
                ClientEvent::Progress { .. } => {}
            },
        }
    }
    for expected in [
        "created",
        "status_changed",
        "unlocks_changed",
        "children_changed",
        "parent_changed",
        "child_removed",
        "group_changed",
    ] {
        assert!(kinds.contains(&expected), "the watch mirror saw no `{expected}`: {kinds:?}");
    }
    assert_eq!(first_transition, Some(Status::Pending));
    assert_eq!(mirror["WIRE-1"].status, Status::Ready, "the watcher saw the promotion");
    assert_eq!(mirror["PA-1"].children, vec!["PA-2".to_string()], "child_removed carried the full ex-parent");
    assert_eq!(mirror["PA-2"].children, vec!["PA-3".to_string()], "children_changed carried the full parent");
    assert_eq!(mirror["PA-3"].parent.as_deref(), Some("PA-2"), "parent_changed carried the full child");
    assert_eq!(mirror["RS-1"].unlocks, vec!["RS-2".to_string()], "unlocks_changed carried the full ticket");
    assert_eq!(mirror["RS-1"].children, vec!["RS-2".to_string()], "RS-2 hangs under its root");
    assert_eq!(mirror["DG-1"].blocked_by, vec!["DG-99".to_string()]);

    // Server and watch-only mirror agree on the ready set, computed two ways.
    let server_ready = ids(rows(
        client
            .ready_set_all(Some("WIRE".into()), None, None, None)
            .await
            .expect("ready_set"),
    ));
    let local_ready: Vec<String> = {
        let mut ids: Vec<String> = mirror
            .values()
            .filter(|t| t.epic == "WIRE" && t.status == Status::Ready)
            .filter(|t| {
                t.blocked_by.iter().all(|b| {
                    mirror
                        .get(b)
                        .is_some_and(|blocker| matches!(blocker.status, Status::Complete | Status::Superseded))
                })
            })
            .map(|t| t.id.clone())
            .collect();
        ids.sort();
        ids
    };
    assert_eq!(server_ready, vec!["WIRE-1".to_string()]);
    assert_eq!(server_ready, local_ready, "watch-derivable, as wire.rs claims");

    // -- groups (MNG-10) ----------------------------------------------------
    // One typed `Group` per shortcode row — and a client that only watched
    // holds the same roll-ups, built from `GroupChanged` alone.
    let groups: Vec<Group> = client
        .groups_all()
        .await
        .expect("groups")
        .into_iter()
        .map(|event| match event {
            TicketsEvent::Group { group } => group,
            other => panic!("groups yields only `group`, got {other:?}"),
        })
        .collect();
    assert_eq!(
        groups
            .iter()
            .map(|g| (g.epic.as_str(), g.total, g.done, g.root.as_deref(), g.host.as_deref()))
            .collect::<Vec<_>>(),
        [
            ("DG", 1, 0, Some("DG-1"), None),
            ("PA", 3, 0, Some("PA-1"), None),
            ("RS", 2, 0, Some("RS-1"), None),
            ("WIRE", 1, 0, Some("WIRE-1"), None),
        ]
    );
    assert_eq!(
        group_copy.into_values().collect::<Vec<_>>(),
        groups,
        "the roll-ups are watch-derivable too"
    );

    // -- unsubscribe --------------------------------------------------------
    // Dropping the stream sends `menger.call_unsub`; the connection survives
    // it and keeps serving.
    drop(watch);
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(!transport.is_closed(), "the unsubscribe did not disturb the socket");
    match one(client.health_all().await.expect("health after unsubscribe")) {
        TicketsEvent::Health { ticket_count, .. } => assert_eq!(ticket_count, 7),
        other => panic!("expected `health`, got {other:?}"),
    }
}
