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
use menger_tickets_api::vocab::{CommentId, TicketRef};
use menger_tickets_api::{Anchor, Comment, CommentState, CommentsClient, CommentsEvent, Group, Kind, Status, Ticket, TicketsClient, TicketsEvent};
use plexus_client::{ClientEvent, MethodRef, StreamItem, Transport, WsTransport};
use plexus_core::plexus::DynamicHub;
use plexus_transport::TransportServer;
use serde_json::json;
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use std::time::Duration;

const HUB: &str = "menger";

/// A port no other test in this binary has been handed (MNG-19). Binding
/// `:0` and dropping the listener only *suggests* a port: the kernel can
/// suggest the same one to a second test before the first test's server
/// has bound it — one server then fails to start and both tests talk to
/// the other, each seeing a store that is not its own.
async fn free_port() -> u16 {
    static HANDED: std::sync::Mutex<Vec<u16>> = std::sync::Mutex::new(Vec::new());
    loop {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let mut handed = HANDED.lock().unwrap();
        if !handed.contains(&port) {
            handed.push(port);
            return port;
        }
    }
}

/// The same in-process server `tests/wire.rs` starts, on a throwaway
/// database. Production (4455–4457) is never touched.
async fn start_server() -> u16 {
    let dir = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).expect("tempdir");
    let db_path = dir.path().join("tickets.db");
    let port = free_port().await;
    let hub = build_rpc(&db_path).await.expect("open database");
    let conv = |arc: Arc<DynamicHub>| {
        menger::calls::rpc_module(arc, Default::default()).map_err(|e| anyhow::anyhow!("{e}"))
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
        // SDLC-13: a kind changed — the whole ticket, one upsert.
        TicketsEvent::KindChanged { ticket, .. } => (ticket, "kind_changed"),
        TicketsEvent::UnlocksChanged { ticket, .. } => (ticket, "unlocks_changed"),
        TicketsEvent::ParentChanged { ticket, .. } => (ticket, "parent_changed"),
        // SCALE-4: a parent's side is its id and new count — set on the
        // row the mirror holds.
        TicketsEvent::ChildrenChanged { id, child_count, .. } => {
            mirror.entry(id.clone()).and_modify(|row| row.child_count = *child_count);
            return "children_changed";
        }
        TicketsEvent::ChildRemoved { id, child_count, .. } => {
            mirror.entry(id.clone()).and_modify(|row| row.child_count = *child_count);
            return "child_removed";
        }
        TicketsEvent::GroupChanged { group } => {
            groups.insert(group.epic.to_string(), group.clone());
            return "group_changed";
        }
        // WAVE-6: each kept placement a write moved, on the row the mirror
        // holds — SDLC-13: a doc made a scope is placed.
        TicketsEvent::PlacementsChanged { changes } => {
            for c in changes {
                mirror.entry(c.id.clone()).and_modify(|row| row.placement = Some(c.placement));
            }
            return "placements_changed";
        }
        // MENGER-13: a transition says what its ticket's own event said —
        // nothing for the mirror to apply.
        TicketsEvent::Transition { .. } => return "transition",
        other => panic!("unexpected watch event {other:?}"),
    };
    mirror.insert(ticket.id.to_string(), ticket.clone());
    kind
}

/// The one event a single-event call answers with.
/// ACCESS-8: a literal as the parameter's type — a `TicketRef`, a `Kind`,
/// a `Title` — parsed as the app parses one at its edge.
fn p<T: std::str::FromStr>(text: &str) -> T
where
    T::Err: std::fmt::Debug,
{
    text.parse().unwrap_or_else(|e| panic!("{text:?}: {e:?}"))
}

fn one<E: std::fmt::Debug>(events: Vec<E>) -> E {
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
            TicketsEvent::Ticket { ticket } => ticket.id.into_string(),
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
    let mut watch = client.watch(None, None, None, None);
    let _ = client.health_all().await.expect("round trip");
    tokio::time::sleep(Duration::from_millis(250)).await;

    // -- create -------------------------------------------------------------
    // wire.rs: created, WIRE-1, Pending. MNG-12: a prefix's first ticket is
    // its scope; MNG-14: the root is top-level.
    let ticket = created(
        client
            .create_all(
                p("WIRE"),
                p("first over the wire"),
                Some(p("scope")),
                None,
                None,
                None,
                None, None,
                None,
            )
            .await
            .expect("create"),
    );
    assert_eq!(ticket.id, "WIRE-1");
    assert_eq!(ticket.status, Status::Pending);
    assert_eq!(ticket.seq, 1);
    assert!(ticket.parent.is_none(), "the root is top-level");
    assert_eq!(ticket.child_count, 0, "and childless");

    // The rule itself, typed: a build first is the activation's own Error.
    match one(
        client
            .create_all(p("FRESH"), p("a build first"), None, None, None, None, None, None, None)
            .await
            .expect("the call succeeds; the activation says no"),
    ) {
        TicketsEvent::Error { message } => assert!(message.contains("scope"), "{message}"),
        other => panic!("expected the in-band `error`, got {other:?}"),
    }

    // -- set_status ---------------------------------------------------------
    match one(
        client
            .set_status_all(p("WIRE-1"), p("Ready"), None, None, None)
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
            .list_all(Some(p("WIRE")), None, None, None, None, None, None, None, None, None)
            .await
            .expect("list"),
    );
    assert_eq!(ids(listed), vec!["WIRE-1".to_string()]);

    // -- find (SCALE-6) -----------------------------------------------------
    // The generated client's `find`: typed `summary` rows, then the page.
    let found = client
        .find_all(p(" OVER the "), None, None, None, None)
        .await
        .expect("find");
    match found.as_slice() {
        [TicketsEvent::Summary { summary }, TicketsEvent::Page { next: None, remaining: 0, first: None }] => {
            assert_eq!(summary.id, "WIRE-1");
            assert_eq!(summary.title, "first over the wire");
        }
        other => panic!("expected one `summary` and the page, got {other:?}"),
    }
    assert!(
        rows(client.find_all(p("zzz"), None, Some(2), None, None).await.expect("find")).is_empty(),
        "nothing matches"
    );

    // -- FILTER-15: find under a filter, and member -------------------------
    // WIRE-1 is Ready: a filter that does not admit it hides it from find.
    assert!(
        rows(client.find_all(p(" OVER the "), None, None, Some(p("status Pending")), None).await.expect("find")).is_empty(),
        "the filter does not admit it"
    );
    assert_eq!(rows(client.find_all(p(" OVER the "), None, None, Some(p("ready")), None).await.expect("find")).len(), 1);
    let members = client
        .member_all(vec![p("WIRE-1"), p("NOPE-1"), p("wire-1")], p("status Ready"), None)
        .await
        .expect("member");
    match members.as_slice() {
        [TicketsEvent::Member { id: a, matched: true }, TicketsEvent::Member { id: b, matched: false }, TicketsEvent::Member { id: c, matched: true }] => {
            assert_eq!((a.as_str(), b.as_str(), c.as_str()), ("WIRE-1", "NOPE-1", "WIRE-1"), "in the order asked, folded (ACCESS-8: a TicketRef)");
        }
        other => panic!("expected three `member` events, got {other:?}"),
    }

    // -- a dangling blocker -------------------------------------------------
    created(
        client
            .create_all(
                p("DG"),
                p("C"),
                Some(p("scope")),
                None,
                Some(vec![p("DG-99")]),
                None,
                None, None,
                None,
            )
            .await
            .expect("create DG-1"),
    );
    client
        .set_status_all(p("DG-1"), p("Ready"), None, None, None)
        .await
        .expect("promote DG-1");
    let ready = rows(
        client
            .ready_set_all(Some(p("DG")), None, None, None, None, None)
            .await
            .expect("ready_set"),
    );
    assert!(ready.is_empty(), "DG-1 must not be ready: {ready:?}");
    match one(client.blockers_all(p("DG-1")).await.expect("blockers")) {
        TicketsEvent::DanglingBlocker { id } => assert_eq!(id, "DG-99"),
        other => panic!("expected `dangling_blocker`, got {other:?}"),
    }

    // -- parent / children (AGS-19, MNG-14, MNG-12's set_parent) ------------
    let parent = created(
        client
            .create_all(p("PA"), p("P"), Some(p("scope")), None, None, None, None, None, None)
            .await
            .expect("create PA-1"),
    );
    assert_eq!(parent.id, "PA-1");
    let child = created(
        client
            .create_all(
                p("PA"),
                p("C"),
                Some(p("execution")),
                None,
                None,
                None,
                Some(p("PA-1")), None,
                None,
            )
            .await
            .expect("create PA-2"),
    );
    assert_eq!(child.id, "PA-2");
    assert_eq!(child.parent.as_deref(), Some("PA-1"));
    // No parent named: the shortcode root (MNG-14).
    let third = created(
        client
            .create_all(p("PA"), p("loose"), None, None, None, None, None, None, None)
            .await
            .expect("create PA-3"),
    );
    assert_eq!(third.parent.as_deref(), Some("PA-1"), "defaulted to the root");

    match one(client.get_all(p("PA-1")).await.expect("get PA-1")) {
        TicketsEvent::Ticket { ticket } => assert_eq!(ticket.child_count, 2),
        other => panic!("expected `ticket`, got {other:?}"),
    }
    let children = ids(rows(
        client
            .list_all(None, None, Some(p("PA-1")), None, None, None, None, None, None, None)
            .await
            .expect("list by parent"),
    ));
    assert_eq!(children, vec!["PA-2".to_string(), "PA-3".to_string()]);

    // The MNG-12 verb through the generated client: move PA-3 under PA-2.
    match one(
        client
            .set_parent_all(p("PA-3"), p("PA-2"))
            .await
            .expect("set_parent"),
    ) {
        TicketsEvent::ParentChanged { ticket, previous } => {
            assert_eq!(ticket.parent.as_deref(), Some("PA-2"));
            assert_eq!(previous.as_deref(), Some("PA-1"));
        }
        other => panic!("expected `parent_changed`, got {other:?}"),
    }
    match one(client.get_all(p("PA-2")).await.expect("get PA-2")) {
        TicketsEvent::Ticket { ticket } => assert_eq!(ticket.child_count, 1),
        other => panic!("expected `ticket`, got {other:?}"),
    }

    // -- an unlocks edge ----------------------------------------------------
    created(
        client
            .create_all(p("RS"), p("A"), Some(p("scope")), None, None, None, None, None, None)
            .await
            .expect("create RS-1"),
    );
    created(
        client
            .create_all(
                p("RS"),
                p("B"),
                None,
                None,
                Some(vec![p("RS-1")]),
                None,
                None, None,
                None,
            )
            .await
            .expect("create RS-2"),
    );

    // -- set_kind (SDLC-13) -------------------------------------------------
    // A doc collection's root made a scope, through the emitted client.
    created(
        client
            .create_all(p("KD"), p("identity"), Some(p("doc")), None, None, None, None, None, None)
            .await
            .expect("create KD-1"),
    );
    match one(client.set_kind_all(p("KD-1"), p("scope"), None, None, None).await.expect("set_kind")) {
        TicketsEvent::KindChanged { ticket, previous } => {
            assert_eq!((ticket.kind, ticket.status, previous), (Kind::Scope, Status::Epic, Kind::Doc));
        }
        other => panic!("expected `kind_changed`, got {other:?}"),
    }
    match one(client.set_kind_all(p("KD-1"), p("build"), Some(p("Ready")), None, None).await.expect("set_kind")) {
        TicketsEvent::Error { message } => assert!(message.contains("'KD-1' is the root of KD"), "{message}"),
        other => panic!("expected the in-band `error`, got {other:?}"),
    }

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
        "kind_changed",
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
    assert_eq!(mirror["PA-1"].child_count, 1, "child_removed carried the ex-parent's count");
    assert_eq!(mirror["PA-2"].child_count, 1, "children_changed carried the parent's count");
    assert_eq!(mirror["PA-3"].parent.as_deref(), Some("PA-2"), "parent_changed carried the full child");
    assert_eq!(mirror["RS-1"].unlocks, vec!["RS-2".to_string()], "unlocks_changed carried the full ticket");
    assert_eq!(mirror["RS-1"].child_count, 1, "RS-2 hangs under its root");
    assert_eq!(mirror["DG-1"].blocked_by, vec!["DG-99".to_string()]);

    // Server and watch-only mirror agree on the ready set, computed two ways.
    let server_ready = ids(rows(
        client
            .ready_set_all(Some(p("WIRE")), None, None, None, None, None)
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
                        .get(b.as_str())
                        .is_some_and(|blocker| matches!(blocker.status, Status::Complete | Status::Superseded | Status::Canceled))
                })
            })
            .map(|t| t.id.to_string())
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
            // SDLC-13: the doc root made a scope — one piece of work.
            ("KD", 1, 0, Some("KD-1"), None),
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

    // -- extents (SCALE-16) -------------------------------------------------
    // The generated client's `extents`: each shortcode's last sequence,
    // typed, in shortcode order — the same numbers the watch mirror holds.
    let extents: Vec<(String, u32)> = client
        .extents_all()
        .await
        .expect("extents")
        .into_iter()
        .map(|event| match event {
            TicketsEvent::Extent { epic, last_seq, .. } => (epic.to_string(), last_seq),
            other => panic!("extents yields only `extent`, got {other:?}"),
        })
        .collect();
    let mut from_mirror: BTreeMap<String, u32> = BTreeMap::new();
    for t in mirror.values() {
        let last = from_mirror.entry(t.epic.to_string()).or_default();
        *last = (*last).max(t.seq);
    }
    assert_eq!(
        extents,
        [("DG".to_string(), 1), ("KD".to_string(), 1), ("PA".to_string(), 3), ("RS".to_string(), 2), ("WIRE".to_string(), 1)]
    );
    assert_eq!(extents, from_mirror.into_iter().collect::<Vec<_>>(), "watch-derivable from `created`");

    // -- unsubscribe --------------------------------------------------------
    // Dropping the stream sends `menger.call_unsub`; the connection survives
    // it and keeps serving.
    drop(watch);
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(!transport.is_closed(), "the unsubscribe did not disturb the socket");
    match one(client.health_all().await.expect("health after unsubscribe")) {
        TicketsEvent::Health { ticket_count, .. } => assert_eq!(ticket_count, 8),
        other => panic!("expected `health`, got {other:?}"),
    }
}

/// WAVE-4: the `waves` events through the generated client — the call the
/// board makes — decode to the activation's own types: `Placed` with its
/// internally tagged `Placement` nested inside the tagged event, then one
/// `WaveCounts`.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn waves_decode_typed_through_the_generated_client() {
    use menger_tickets_api::Placement;
    let port = start_server().await;
    let transport = connect(port).await;
    let client = TicketsClient::new(&transport);
    for (epic, kind, blocked_by) in [
        ("TY", "scope", None),
        ("TY", "build", None),
        ("TY", "build", Some(vec![p("TY-2")])),
        ("TY", "gap", Some(vec![p("TY-77")])),
    ] {
        created(
            client
                .create_all(p(epic), p("t"), Some(p(kind)), None, blocked_by, None, None, None, None)
                .await
                .expect("create"),
        );
    }
    let events = client.waves_all(vec![p("ty")], None).await.expect("waves");
    let placed: Vec<(String, Placement, Vec<String>)> = events
        .iter()
        .filter_map(|e| match e {
            TicketsEvent::Placed { summary, placement, waiting_on } => {
                Some((summary.id.to_string(), *placement, waiting_on.iter().map(|w| w.to_string()).collect()))
            }
            _ => None,
        })
        .collect();
    assert_eq!(
        placed,
        vec![
            ("TY-2".to_string(), Placement::Wave { n: 1 }, vec![]),
            ("TY-3".to_string(), Placement::Wave { n: 2 }, vec![p("TY-2")]),
            ("TY-4".to_string(), Placement::MissingBlocker, vec![p("TY-77")]),
        ]
    );
    match events.last() {
        Some(TicketsEvent::WaveCounts { waves, missing, done }) => {
            assert_eq!((waves.as_slice(), *missing, *done), (&[1u64, 1][..], 1, 0));
        }
        other => panic!("expected wave_counts last, got {other:?}"),
    }
}

/// WAVE-6 and WAVE-10: what the board and the annotation read, through the
/// generated client — `placement` on a created ticket and on a summary row,
/// a `placements_changed` from `watch` decoding to its typed changes, and
/// `get` by a path address — the activation's own types, no JSON in hand.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn placements_and_paths_decode_typed_through_the_generated_client() {
    use futures::StreamExt;
    use menger_tickets_api::{Placement, PlacementChange};
    let port = start_server().await;
    let transport = connect(port).await;
    let client = TicketsClient::new(&transport);
    let mut watch = client.watch(None, None, None, None);
    let make = |kind: &str, blocked_by: Option<Vec<TicketRef>>, path: Option<&str>| {
        client.create_all(p("PT"), p("t"), Some(p(kind)), None, blocked_by, None, None, path.map(p), None)
    };
    created(make("scope", None, None).await.expect("create")); // PT-1
    let y = created(make("build", None, None).await.expect("create")); // PT-2
    let x = created(make("build", Some(vec![p("PT-2")]), None).await.expect("create")); // PT-3
    let doc = created(make("doc", None, Some("_MENGER/board")).await.expect("create")); // PT-4
    assert_eq!((y.placement, x.placement, doc.placement), (Some(Placement::Wave { n: 1 }), Some(Placement::Wave { n: 2 }), None));
    assert_eq!(doc.path.as_deref(), Some("_MENGER/board"));

    match one(client.get_all(p("pt/_MENGER/board")).await.expect("get")) {
        TicketsEvent::Ticket { ticket } => assert_eq!(ticket.id, "PT-4"),
        other => panic!("expected the ticket, got {other:?}"),
    }
    client.set_status_all(p("PT-2"), p("Complete"), None, None, None).await.expect("set_status");
    let mut changes = None;
    while changes.is_none() {
        let ev = tokio::time::timeout(Duration::from_secs(5), watch.next()).await.expect("watch").expect("open");
        if let Ok(plexus_client::ClientEvent::Data(TicketsEvent::PlacementsChanged { changes: c })) = ev {
            changes = Some(c);
        }
    }
    assert_eq!(
        changes.unwrap(),
        vec![
            PlacementChange { id: p("PT-2"), placement: Placement::Done },
            PlacementChange { id: p("PT-3"), placement: Placement::Wave { n: 1 } },
        ]
    );
    let rows: Vec<_> = client
        .list_all(Some(p("PT")), None, None, None, None, Some(true), None, None, None, None)
        .await
        .expect("list")
        .into_iter()
        .filter_map(|e| match e {
            TicketsEvent::Summary { summary } => Some((summary.id.into_string(), summary.placement)),
            _ => None,
        })
        .collect();
    assert_eq!(rows[2], ("PT-3".to_string(), Some(Placement::Wave { n: 1 })), "on the row");
    assert_eq!(rows[3], ("PT-4".to_string(), None), "a doc is placed nowhere");
}

/// ACT-6: `reshortcode` on the real wire — three tickets moved with a
/// blocker between them, each reached by its old id afterwards: `get`
/// answers the new ticket with `moved_from` (what the app's old URL asks,
/// and what a chip in a body asks for the ref it names), the blocker named
/// anew, and `watch` saying `reshortcoded`.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn reshortcode_moves_three_and_every_old_id_still_reaches_its_ticket() {
    let port = start_server().await;
    let transport = connect(port).await;
    let client = TicketsClient::new(&transport);
    let mut watch = client.watch(None, None, None, None);
    let make = |epic: &str, kind: &str, blocked_by: Option<Vec<TicketRef>>, body: Option<&str>| {
        client.create_all(p(epic), p("t"), Some(p(kind)), None, blocked_by, body.map(p), None, None, None)
    };
    created(make("RS", "scope", None, None).await.expect("create")); // RS-1
    created(make("RS", "build", None, None).await.expect("create")); // RS-2
    created(make("RS", "build", Some(vec![p("RS-2")]), None).await.expect("create")); // RS-3
    created(make("RS", "build", None, None).await.expect("create")); // RS-4
    created(make("TO", "scope", None, None).await.expect("create")); // TO-1
    // A body that cites one of them by the id it has now.
    let citing = created(make("RS", "doc", None, Some(r#"See <m-ticket ref="RS-3"></m-ticket>."#)).await.expect("create"));
    let moved = client
        .reshortcode_all(vec![p("RS-2"), p("RS-3"), p("RS-4")], p("TO"), None, None, None, Some(p("wire")))
        .await
        .expect("reshortcode");
    match one(moved) {
        TicketsEvent::Reshortcoded { moves, tickets } => {
            let pairs: Vec<(String, String)> = moves.into_iter().map(|m| (m.from, m.to)).collect();
            assert_eq!(pairs, [("RS-2", "TO-2"), ("RS-3", "TO-3"), ("RS-4", "TO-4")].map(|(a, b)| (a.to_string(), b.to_string())));
            assert_eq!(tickets[1].blocked_by, ["TO-2"], "the block between them, by identity");
        }
        other => panic!("expected reshortcoded, got {other:?}"),
    }
    for (old, new) in [("RS-2", "TO-2"), ("RS-3", "TO-3"), ("rs-4", "TO-4")] {
        match one(client.get_all(p(old)).await.expect("get")) {
            TicketsEvent::Ticket { ticket } => {
                assert_eq!(ticket.id, new, "{old}");
                assert_eq!(ticket.moved_from, [old.to_ascii_uppercase()], "{old}");
                assert_eq!(ticket.parent.as_deref(), Some("TO-1"));
            }
            other => panic!("expected the ticket for {old}, got {other:?}"),
        }
    }
    // The chip's ref, as the body still writes it, reaches the moved ticket.
    let body = match one(client.get_all(p(&citing.id)).await.expect("get")) {
        TicketsEvent::Ticket { ticket } => ticket.body,
        other => panic!("{other:?}"),
    };
    let cited = body.split("ref=\"").nth(1).and_then(|r| r.split('"').next()).expect("a ref").to_string();
    match one(client.get_all(p(&cited)).await.expect("get")) {
        TicketsEvent::Ticket { ticket } => assert_eq!(ticket.id, "TO-3"),
        other => panic!("{other:?}"),
    }
    let mut heard = false;
    while !heard {
        let ev = tokio::time::timeout(Duration::from_secs(5), watch.next()).await.expect("watch").expect("open");
        heard = matches!(ev, Ok(ClientEvent::Data(TicketsEvent::Reshortcoded { .. })));
    }
}

/// COMMENT-2: the `comments` activation through its emitted client —
/// `CommentsClient<&WsTransport>`, from the activation's own `impl` in
/// `menger-tickets-api`, as the browser app takes it: `add` with a typed
/// [`Anchor`], `list` with its filters and paging, `resolve`, `reopen`, and
/// `watch`, every answer a [`CommentsEvent`] — the comment's fields beside
/// its type ([`CommentsEvent::into_comment`]).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_comments_through_the_generated_client() {
    let port = start_server().await;
    let transport = connect(port).await;
    let tickets = TicketsClient::new(&transport);
    let comments = CommentsClient::new(&transport);
    tickets.create_all(p("DEMAND"), p("demands"), Some(p("scope")), None, None, None, None, None, None).await.expect("create");
    tickets.create_all(p("DEMAND"), p("the chip"), None, None, None, None, None, None, None).await.expect("create");
    let version = match one(tickets.get_all(p("DEMAND-2")).await.expect("get")) {
        TicketsEvent::Ticket { ticket } => ticket.updated_unix,
        other => panic!("expected `ticket`, got {other:?}"),
    };
    let mut watch = comments.watch();
    let _ = comments.list_all(None, None, None, None, None, None, None, None, None, Some(1)).await.expect("round trip");
    tokio::time::sleep(Duration::from_millis(250)).await;

    let anchor = Anchor {
        route: p("#/ticket/DEMAND-2"),
        path: p("[data-tag=\"ticket DEMAND-1\"]"),
        label: p("Parent chip"),
        excerpt: p("DEMAND-1"),
        ticket: Some(p("DEMAND-2")),
        version: None,
    };
    let added = one(comments.add_all(anchor.clone(), p("show the parent's title too"), None).await.expect("add"))
        .into_comment()
        .expect("a comment");
    assert_eq!(
        added,
        Comment {
            id: CommentId::new(1),
            state: CommentState::Open,
            author: p("owner"),
            at: added.at,
            anchor: Anchor { version: Some(version), ..anchor.clone() },
            body: p("show the parent's title too"),
            principal: None,
            intent: menger_tickets_api::CommentIntent::Comment,
        }
    );
    let second = one(comments.add_all(Anchor { ticket: None, ..anchor.clone() }, p("on no ticket"), Some(p("agent"))).await.expect("add"))
        .into_comment()
        .expect("a comment");

    let listed = comments
        .list_all(Some(p("DEMAND-2")), None, None, None, Some(p("open")), None, None, None, None, None)
        .await
        .expect("list");
    assert_eq!(listed, [CommentsEvent::from(added.clone()), CommentsEvent::Page { next: None }]);
    let page = comments.list_all(None, None, None, None, None, None, None, None, None, Some(1)).await.expect("list");
    assert_eq!(page, [CommentsEvent::from(second.clone()), CommentsEvent::Page { next: Some(2) }]);
    let rest = comments.list_all(None, Some(p("DEMAND")), None, None, None, None, None, None, Some(CommentId::new(2)), Some(1)).await.expect("list");
    assert_eq!(rest, [CommentsEvent::from(added.clone()), CommentsEvent::Page { next: None }]);

    let resolved = one(comments.resolve_all(CommentId::new(1)).await.expect("resolve")).into_comment().expect("a comment");
    assert_eq!(resolved.state, CommentState::Resolved);
    let reopened = one(comments.reopen_all(CommentId::new(1)).await.expect("reopen")).into_comment().expect("a comment");
    assert_eq!(reopened, added);
    match one(comments.resolve_all(CommentId::new(9)).await.expect("resolve")) {
        CommentsEvent::Error { message } => assert!(message.starts_with("no comment 9"), "{message}"),
        other => panic!("expected `error`, got {other:?}"),
    }

    let mut heard = Vec::new();
    while heard.len() < 4 {
        let ev = tokio::time::timeout(Duration::from_secs(20), watch.next())
            .await
            .expect("an event in time")
            .expect("the watch goes on")
            .expect("no transport error");
        if let ClientEvent::Data(ev) = ev {
            heard.push(ev);
        }
    }
    assert_eq!(
        heard,
        [
            CommentsEvent::Added { comment: added.clone() },
            CommentsEvent::Added { comment: second },
            CommentsEvent::Resolved { comment: resolved },
            CommentsEvent::Reopened { comment: added },
        ]
    );
}
