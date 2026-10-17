//! MENGER-29: the tickets activation's tests, moved out of `activation.rs`.

use crate::store::SYS;
use super::*;
use futures::StreamExt;

// ACCESS-15, ACCESS-16, ACCESS-17: the children's tests, each in a file of its own.
mod commits;
mod nav;
mod work;

fn hub() -> TicketsActivation {
    TicketsActivation::new(Arc::new(Store::new()))
}

/// ACCESS-13: the caller every method takes — with login off, anyone, as
/// the frame's resolver verifies it.
fn anyone() -> VerifiedUser {
    Access::open().verify_now(&plexus_core::plexus::AuthContext::anonymous()).expect("login off admits anyone")
}

/// MENGER-13: a watch as the tests before the transition log read it —
/// each `transition` (right after its ticket's event) set aside.
fn untransitioned(s: impl Stream<Item = TicketsEvent> + Send + 'static) -> impl Stream<Item = TicketsEvent> + Send + 'static {
    s.filter(|e| futures::future::ready(!matches!(e, TicketsEvent::Transition { .. })))
}

async fn one(s: impl Stream<Item = TicketsEvent> + Send + 'static) -> TicketsEvent {
    Box::pin(s).next().await.expect("event")
}

#[test]
fn a_refusal_names_only_the_tickets_the_caller_can_see() {
    let cycle = StoreError::Cycle { id: "A-3".into(), path: vec!["A-2".into(), "B-7".into(), "A-3".into()] };
    let said = redacted(cycle, |id| id.starts_with("A-"));
    assert!(said.contains("A-2 -> (a ticket out of your reach) -> A-3"), "{said}");
    assert!(!said.contains("B-7"));
    let awaited = StoreError::DocAwaited { id: "A-4".into(), by: vec!["B-1".into()] };
    assert!(!redacted(awaited, |id| id.starts_with("A-")).contains("B-1"));
}

#[tokio::test]
async fn health_reports_count() {
    let hub = hub();
    match one(hub.health(anyone()).await).await {
        TicketsEvent::Health { ticket_count, .. } => assert_eq!(ticket_count, 0),
        other => panic!("expected Health, got {other:?}"),
    }
}

#[tokio::test]
async fn create_allocates_sequential_ids() {
    let hub = hub();
    let ev = one(hub.create(anyone(), "mng".into(), "first".into(), Some("scope".into()), None, None, None, None, None, None).await).await;
    match ev {
        TicketsEvent::Created { ticket } => {
            assert_eq!(ticket.id, "MNG-1");
            assert_eq!(ticket.status, Status::Pending);
        }
        other => panic!("expected Created, got {other:?}"),
    }
    let ev = one(hub.create(anyone(), "MNG".into(), "second".into(), None, None, None, None, None, None, None).await).await;
    match ev {
        TicketsEvent::Created { ticket } => assert_eq!(ticket.id, "MNG-2"),
        other => panic!("expected Created, got {other:?}"),
    }
}

/// AGS-13 criterion 2: no kind → `build`; `gap` is accepted; a kind
/// outside the five (`bug` — rim's name for gap) is the in-band error
/// that names all five, and nothing is created.
#[tokio::test]
async fn create_defaults_kind_to_build_accepts_gap_and_rejects_unknown_naming_the_six() {
    let hub = hub();
    // MNG-12: the prefix's scope root first; the default is measured
    // on the ticket after it.
    one(hub.create(anyone(), "MNG".into(), "root".into(), Some("scope".into()), None, None, None, None, None, None).await).await;
    match one(hub.create(anyone(), "MNG".into(), "no kind".into(), None, None, None, None, None, None, None).await).await {
        TicketsEvent::Created { ticket } => assert_eq!(ticket.kind, Kind::Build),
        other => panic!("expected Created, got {other:?}"),
    }
    match one(hub.create(anyone(), "MNG".into(), "a gap".into(), Some("gap".into()), None, None, None, None, None, None).await)
        .await
    {
        TicketsEvent::Created { ticket } => assert_eq!(ticket.kind, Kind::Gap),
        other => panic!("expected Created, got {other:?}"),
    }
    // ACCESS-8: `bug` is no Kind — refused at dispatch (`invalid_params`
    // naming `kind`, tests/wire.rs), naming the six, before anything runs.
    let message = "bug".parse::<Kind>().unwrap_err();
    assert!(message.contains("'bug'"), "{message}");
    for name in ["scope", "execution", "build", "gap", "spike", "doc"] {
        assert!(message.contains(name), "names {name}: {message}");
    }
    assert!(matches!(
        one(hub.get(anyone(), "MNG-4".into()).await).await,
        TicketsEvent::NotFound { .. }
    ));
}

/// SDLC-11 over the activation: a doc is created in `Doc` (and may root
/// a shortcode); `set_status` on it is one in-band `error` saying it has
/// no lifecycle; `Doc` on work is one in-band `error`; a doc with a
/// `blocked_by`, and a `blocked_by` naming a doc, are in-band `error`s;
/// `list { status: Doc }` finds the doc.
#[tokio::test]
async fn a_doc_over_the_activation_refuses_every_lifecycle_write_in_band() {
    let hub = hub();
    match one(hub.create(anyone(), "DOCS".into(), "the manifest".into(), Some("doc".into()), None, None, None, None, None, None).await).await {
        TicketsEvent::Created { ticket } => {
            assert_eq!((ticket.id.as_str(), ticket.kind, ticket.status), ("DOCS-1", Kind::Doc, Status::Doc));
        }
        other => panic!("expected Created, got {other:?}"),
    }
    one(hub.create(anyone(), "DOCS".into(), "work".into(), None, None, None, None, None, None, None).await).await; // DOCS-2
    for status in ["Ready", "Complete", "Doc", "active"] {
        match one(hub.set_status(anyone(), "DOCS-1".into(), status.into(), None, None, None).await).await {
            TicketsEvent::Error { message } => assert!(message.contains("a doc has no lifecycle"), "{message}"),
            other => panic!("{status}: expected Error, got {other:?}"),
        }
    }
    match one(hub.set_status(anyone(), "DOCS-2".into(), "doc".into(), None, None, None).await).await {
        TicketsEvent::Error { message } => assert!(message.contains("Doc is a doc's own status"), "{message}"),
        other => panic!("expected Error, got {other:?}"),
    }
    let blocked = Some(vec!["DOCS-2".into()]);
    match one(hub.create(anyone(), "DOCS".into(), "x".into(), Some("doc".into()), None, blocked, None, None, None, None).await).await {
        TicketsEvent::Error { message } => assert!(message.contains("a doc is never blocked"), "{message}"),
        other => panic!("expected Error, got {other:?}"),
    }
    let on_doc = Some(vec!["DOCS-1".into()]);
    match one(hub.create(anyone(), "DOCS".into(), "x".into(), None, None, on_doc, None, None, None, None).await).await {
        TicketsEvent::Error { message } => assert!(message.contains("'DOCS-1' is a doc"), "{message}"),
        other => panic!("expected Error, got {other:?}"),
    }
    match one(hub.list(anyone(), None, Some("Doc".into()), None, None, None, Some(true), None, None, None, None).await).await {
        TicketsEvent::Summary { summary } => assert_eq!((summary.id.as_str(), summary.status), ("DOCS-1", Status::Doc)),
        other => panic!("expected Summary, got {other:?}"),
    }
    match one(hub.get(anyone(), "DOCS-1".into()).await).await {
        TicketsEvent::Ticket { ticket } => assert_eq!(ticket.status, Status::Doc, "nothing moved it"),
        other => panic!("expected Ticket, got {other:?}"),
    }
}

#[tokio::test]
async fn get_distinguishes_found_from_missing() {
    let hub = hub();
    one(hub.create(anyone(), "MNG".into(), "x".into(), Some("scope".into()), None, None, None, None, None, None).await).await;
    assert!(matches!(
        one(hub.get(anyone(), "MNG-1".into()).await).await,
        TicketsEvent::Ticket { .. }
    ));
    assert!(matches!(
        one(hub.get(anyone(), "MNG-99".into()).await).await,
        TicketsEvent::NotFound { .. }
    ));
}

#[tokio::test]
async fn list_filters_by_status() {
    let hub = hub();
    one(hub.create(anyone(), "MNG".into(), "a".into(), Some("scope".into()), None, None, None, None, None, None).await).await;
    one(hub.create(anyone(), "MNG".into(), "b".into(), None, None, None, None, None, None, None).await).await;
    one(hub.set_status(anyone(), "MNG-1".into(), "Ready".into(), None, None, None).await).await;

    let (ids, page) =
        ids_and_page(hub.list(anyone(), None, Some("Ready".into()), None, None, None, None, None, None, None, None).await).await;
    assert_eq!(ids, vec!["MNG-1"]);
    assert_eq!(page, Some((None, 0)), "one match, nothing beyond it");
}

#[tokio::test]
async fn set_status_reports_previous() {
    let hub = hub();
    one(hub.create(anyone(), "MNG".into(), "a".into(), Some("scope".into()), None, None, None, None, None, None).await).await;
    match one(hub.set_status(anyone(), "MNG-1".into(), "ready".into(), None, None, None).await).await {
        TicketsEvent::StatusChanged { ticket, previous } => {
            assert_eq!(ticket.status, Status::Ready);
            assert_eq!(previous, Status::Pending);
        }
        other => panic!("expected StatusChanged, got {other:?}"),
    }
}

/// SDLC-13, over the activation: `set_kind` answers `kind_changed` with
/// the kind it had, its kind and status as the call and the rules leave
/// them (case-insensitive, `active` for `Coding`); an unknown id is
/// `not_found`; an unknown kind or status, and each rule's refusal, one
/// in-band `error` naming what it refuses.
#[tokio::test]
async fn set_kind_reports_previous_and_refuses_in_band() {
    let hub = hub();
    one(hub.create(anyone(), "IDN".into(), "identity".into(), Some("doc".into()), None, None, None, None, None, None).await).await;
    one(hub.create(anyone(), "IDN".into(), "a".into(), None, None, None, None, None, None, None).await).await;
    match one(hub.set_kind(anyone(), "IDN-1".into(), "Scope".into(), None, None, None).await).await {
        TicketsEvent::KindChanged { ticket, previous } => {
            assert_eq!((ticket.kind, ticket.status, previous), (Kind::Scope, Status::Epic, Kind::Doc));
        }
        other => panic!("expected KindChanged, got {other:?}"),
    }
    match one(hub.set_kind(anyone(), "IDN-2".into(), "gap".into(), Some("active".into()), None, None).await).await {
        TicketsEvent::KindChanged { ticket, previous } => {
            assert_eq!((ticket.kind, ticket.status, previous), (Kind::Gap, Status::Coding, Kind::Build));
        }
        other => panic!("expected KindChanged, got {other:?}"),
    }
    assert!(matches!(
        one(hub.set_kind(anyone(), "IDN-9".into(), "gap".into(), None, None, None).await).await,
        TicketsEvent::NotFound { id } if id == "IDN-9"
    ));
    // ACCESS-8: an unknown kind or status is no Kind or Status — refused
    // at dispatch, naming the parameter (tests/wire.rs); the rest in band.
    assert!("bug".parse::<Kind>().unwrap_err().contains("unknown kind 'bug'"));
    assert!("Shipped".parse::<Status>().unwrap_err().contains("unknown status 'shipped'"));
    for (id, kind, status, says) in [
        ("IDN-1", "build", None, "'IDN-1' is the root of IDN"),
        ("IDN-2", "doc", Some("Ready"), "a doc's status is Doc"),
        ("IDN-2", "spike", Some("Doc"), "Doc is a doc's own status"),
    ] {
        match one(hub.set_kind(anyone(), id.into(), kind.into(), status.map(Into::into), None, None).await).await {
            TicketsEvent::Error { message } => assert!(message.contains(says), "{kind} {status:?}: {message}"),
            other => panic!("{kind} {status:?}: expected Error, got {other:?}"),
        }
    }
}

/// SDLC-7 criterion 1, over the activation: `set_status` takes both
/// worker states — the canonical spelling and a hand-typed one —
/// answers `status_changed`, `get` returns `In Review` byte-exact on
/// the wire, and `list { status }` filters on each.
#[tokio::test]
async fn set_status_takes_the_worker_states_and_list_filters_on_them() {
    let hub = hub();
    one(hub.create(anyone(), "WK".into(), "root".into(), Some("scope".into()), None, None, None, None, None, None).await).await;
    one(hub.create(anyone(), "WK".into(), "a".into(), None, None, None, None, None, None, None).await).await;
    one(hub.create(anyone(), "WK".into(), "b".into(), None, None, None, None, None, None, None).await).await;

    for (id, spelling, want, previous) in [
        ("WK-2", "Coding", Status::Coding, Status::Pending),
        ("WK-3", "active", Status::Coding, Status::Pending),
        ("WK-3", "In Review", Status::InReview, Status::Coding),
    ] {
        match one(hub.set_status(anyone(), id.into(), spelling.into(), None, None, None).await).await {
            TicketsEvent::StatusChanged { ticket, previous: was } => {
                assert_eq!((ticket.id.as_str(), ticket.status, was), (id, want, previous), "{spelling}");
            }
            other => panic!("{spelling}: expected StatusChanged, got {other:?}"),
        }
    }
    let wire = serde_json::to_value(one(hub.get(anyone(), "WK-3".into()).await).await).unwrap();
    assert_eq!(wire["ticket"]["status"], "In Review");

    let (ids, _) = ids_and_page(hub.list(anyone(), None, Some("coding".into()), None, None, None, None, None, None, None, None).await).await;
    assert_eq!(ids, ["WK-2"]);
    let (ids, _) = ids_and_page(hub.list(anyone(), None, Some("In Review".into()), None, None, None, None, None, None, None, None).await).await;
    assert_eq!(ids, ["WK-3"]);
}

/// MNG-16 over the activation: `amend` answers `amended { ticket,
/// changed }` on the wire (and `changed: []` for a no-op), `not_found`
/// for a missing id, an in-band `error` for a blank title; `watch`
/// carries one `amended` per amend that changed something and none for
/// the no-op or the refusals — so the next event after them is the next
/// real change.
#[tokio::test]
async fn a_body_past_the_cap_is_the_typed_body_too_large() {
    let hub = TicketsActivation::new(Arc::new(Store::new().with_body_max(10)));
    one(hub.create(anyone(), "BIG".into(), "root".into(), Some("scope".into()), None, None, None, None, None, None).await).await;
    let ev = one(hub.create(anyone(), "BIG".into(), "page".into(), Some("doc".into()), None, None, Some("x".repeat(11).as_str().into()), None, None, None).await).await;
    let wire = serde_json::to_value(&ev).unwrap();
    assert_eq!((wire["type"].as_str(), wire["bytes"].as_u64(), wire["max"].as_u64()), (Some("body_too_large"), Some(11), Some(10)), "{wire}");
    assert!(wire["id"].is_null() && wire["message"].as_str().unwrap().contains("past the 10-byte cap"), "{wire}");
    let ev = one(hub.amend(anyone(), "BIG-1".into(), None, Some("y".repeat(12).as_str().into())).await).await;
    let wire = serde_json::to_value(&ev).unwrap();
    assert_eq!((wire["type"].as_str(), wire["id"].as_str(), wire["bytes"].as_u64()), (Some("body_too_large"), Some("BIG-1"), Some(12)), "{wire}");
    // At the cap, kept.
    let ev = one(hub.amend(anyone(), "BIG-1".into(), None, Some("z".repeat(10).as_str().into())).await).await;
    assert!(matches!(ev, TicketsEvent::Amended { .. }), "{ev:?}");
}

#[tokio::test]
async fn amend_yields_amended_and_watch_carries_only_what_changed() {
    let hub = hub();
    one(hub.create(anyone(), "AM".into(), "root".into(), Some("scope".into()), None, None, None, None, None, None).await).await;
    let mut watch = Box::pin(hub.watch(anyone(), None, None, None, None).await);

    let ev = one(hub.amend(anyone(), "am-1".into(), None, Some("the body, now".into())).await).await;
    let wire = serde_json::to_value(&ev).unwrap();
    assert_eq!(wire["type"], "amended");
    assert_eq!(wire["changed"], serde_json::json!(["body"]));
    assert_eq!(wire["ticket"]["id"], "AM-1");
    assert_eq!(wire["ticket"]["body"], "the body, now");
    match watch.next().await.expect("change") {
        TicketsEvent::Amended { ticket, changed } => {
            assert_eq!((ticket.id.as_str(), ticket.body.as_str()), ("AM-1", "the body, now"));
            assert_eq!(changed, ["body"]);
        }
        other => panic!("expected Amended, got {other:?}"),
    }

    match one(hub.amend(anyone(), "AM-1".into(), None, Some("the body, now".into())).await).await {
        TicketsEvent::Amended { changed, .. } => assert!(changed.is_empty(), "{changed:?}"),
        other => panic!("expected Amended, got {other:?}"),
    }
    assert!(matches!(
        one(hub.amend(anyone(), "AM-7".into(), Some("t".into()), None).await).await,
        TicketsEvent::NotFound { id } if id == "AM-7"
    ));
    // ACCESS-8: a blank title is no Title — refused at dispatch.
    assert!("  ".parse::<crate::vocab::Title>().unwrap_err().contains("not blank"));

    one(hub.amend(anyone(), "AM-1".into(), Some("a better title".into()), None).await).await;
    match watch.next().await.expect("change") {
        TicketsEvent::Amended { ticket, changed } => {
            assert_eq!(ticket.title, "a better title");
            assert_eq!(changed, ["title"], "the no-op and the refusals said nothing");
        }
        other => panic!("expected Amended, got {other:?}"),
    }
}

#[tokio::test]
async fn watch_sees_creations_and_status_changes() {
    let hub = hub();
    let mut watch = Box::pin(hub.watch(anyone(), None, None, None, None).await);

    one(hub.create(anyone(), "MNG".into(), "a".into(), Some("scope".into()), None, None, None, None, None, None).await).await;
    match watch.next().await.expect("change") {
        TicketsEvent::Created { ticket } => assert_eq!(ticket.id, "MNG-1"),
        other => panic!("expected Created, got {other:?}"),
    }
    // MENGER-13: its first status, a transition from nothing, right after.
    match watch.next().await.expect("change") {
        TicketsEvent::Transition { transition } => {
            assert_eq!((transition.ticket.as_str(), transition.from.as_deref(), transition.to.as_str()), ("MNG-1", None, "Pending"));
            assert_eq!((transition.actor.as_str(), transition.source), ("anonymous", crate::types::TransitionSource::Api));
        }
        other => panic!("expected Transition, got {other:?}"),
    }
    // MNG-10: a new shortcode's row appears after the ticket that made it.
    match watch.next().await.expect("change") {
        TicketsEvent::GroupChanged { group } => assert_eq!((group.epic.as_str(), group.total), ("MNG", 1)),
        other => panic!("expected GroupChanged, got {other:?}"),
    }

    one(hub.set_status(anyone(), "MNG-1".into(), "Ready".into(), None, None, None).await).await;
    match watch.next().await.expect("change") {
        TicketsEvent::StatusChanged { previous, .. } => assert_eq!(previous, Status::Pending),
        other => panic!("expected StatusChanged, got {other:?}"),
    }
    match watch.next().await.expect("change") {
        TicketsEvent::Transition { transition } => {
            assert_eq!((transition.from.as_deref(), transition.to.as_str(), transition.cause), (Some("Pending"), "Ready", None));
        }
        other => panic!("expected Transition, got {other:?}"),
    }
}

/// Ids of a `list` / `work ready` stream, in order — asserting the
/// MNG-9 shape on the way through: rows, then exactly one closing
/// `page` event.
async fn ticket_ids(s: impl Stream<Item = TicketsEvent> + Send + 'static) -> Vec<String> {
    let (ids, page) = ids_and_page(s).await;
    assert!(page.is_some(), "every list / work ready call ends with a page");
    ids
}

/// The same, keeping the page event: `(ids, Some((next, remaining)))`.
#[allow(clippy::type_complexity)]
async fn ids_and_page(
    s: impl Stream<Item = TicketsEvent> + Send + 'static,
) -> (Vec<String>, Option<(Option<String>, u64)>) {
    let evs: Vec<TicketsEvent> = Box::pin(s).collect().await;
    let mut ids = Vec::new();
    let mut page = None;
    for ev in evs {
        assert!(page.is_none(), "the page event is last: {ev:?}");
        match ev {
            TicketsEvent::Ticket { ticket } => ids.push(ticket.id),
            TicketsEvent::Summary { summary } => ids.push(summary.id),
            TicketsEvent::Page { next, remaining, .. } => page = Some((next, remaining)),
            other => panic!("expected a row or the page, got {other:?}"),
        }
    }
    (ids.into_iter().map(String::from).collect::<Vec<String>>(), page)
}

#[tokio::test]
async fn create_adds_new_id_to_blockers_unlocks_and_watch_carries_it() {
    let hub = hub();
    one(hub.create(anyone(), "DAG".into(), "a".into(), Some("scope".into()), None, None, None, None, None, None).await).await;
    let mut watch = Box::pin(untransitioned(hub.watch(anyone(), None, None, None, None).await));

    one(hub
        .create(anyone(), "DAG".into(), "b".into(), None, None, Some(vec!["DAG-1".into()]), None, None, None, None)
        .await)
    .await;

    match watch.next().await.expect("change") {
        TicketsEvent::Created { ticket } => assert_eq!(ticket.id, "DAG-2"),
        other => panic!("expected Created, got {other:?}"),
    }
    // MNG-14: DAG-2 named no parent, so it hangs under the root — which
    // is also its blocker: one updated ticket, both edges, two events.
    // SCALE-4: the first is the parent's id and count; the second the
    // whole ticket, carrying both edges.
    match watch.next().await.expect("change") {
        TicketsEvent::ChildrenChanged { id, added, child_count } => {
            assert_eq!(id, "DAG-1");
            assert_eq!(child_count, 1);
            assert_eq!(added, "DAG-2");
        }
        other => panic!("expected ChildrenChanged, got {other:?}"),
    }
    match watch.next().await.expect("change") {
        TicketsEvent::UnlocksChanged { ticket, added } => {
            assert_eq!(ticket.id, "DAG-1");
            assert_eq!(ticket.unlocks, vec!["DAG-2"]);
            assert_eq!(ticket.child_count, 1, "both edges on the one ticket");
            assert_eq!(added, "DAG-2");
        }
        other => panic!("expected UnlocksChanged, got {other:?}"),
    }

    match one(hub.get(anyone(), "DAG-1".into()).await).await {
        TicketsEvent::Ticket { ticket } => assert_eq!(ticket.unlocks, vec!["DAG-2"]),
        other => panic!("expected Ticket, got {other:?}"),
    }
}

/// AGS-19 acceptance 1: a child's birth is visible from both ends —
/// `get C` shows the parent, `get P` counts the child and `list
/// { parent: P }` names it (SCALE-4) — and a watcher sees the parent's
/// update as `children_changed`: its id, the child and its new count.
#[tokio::test]
async fn create_with_parent_updates_children_and_watch_carries_the_parent() {
    let hub = hub();
    one(hub.create(anyone(), "FAM".into(), "P".into(), Some("scope".into()), None, None, None, None, None, None).await).await;
    let mut watch = Box::pin(untransitioned(hub.watch(anyone(), None, None, None, None).await));

    match one(
        hub.create(anyone(), 
            "FAM".into(),
            "C".into(),
            Some("execution".into()),
            None,
            None,
            None,
            Some("fam-1".into()), None,
            None,
        )
        .await,
    )
    .await
    {
        TicketsEvent::Created { ticket } => {
            assert_eq!(ticket.id, "FAM-2");
            assert_eq!(ticket.parent.as_deref(), Some("FAM-1"), "case-insensitive");
            assert_eq!(ticket.child_count, 0);
        }
        other => panic!("expected Created, got {other:?}"),
    }

    match watch.next().await.expect("change") {
        TicketsEvent::Created { ticket } => assert_eq!(ticket.id, "FAM-2"),
        other => panic!("expected Created, got {other:?}"),
    }
    match watch.next().await.expect("change") {
        TicketsEvent::ChildrenChanged { id, added, child_count } => {
            assert_eq!(id, "FAM-1");
            assert_eq!(added, "FAM-2");
            assert_eq!(child_count, 1, "the count, not the parent");
        }
        other => panic!("expected ChildrenChanged, got {other:?}"),
    }
    let wire = serde_json::to_value(TicketsEvent::ChildrenChanged {
        id: "FAM-1".into(),
        added: "FAM-2".into(),
        child_count: 1,
    })
    .unwrap();
    assert_eq!(
        wire,
        serde_json::json!({"type": "children_changed", "id": "FAM-1", "added": "FAM-2", "child_count": 1})
    );

    match one(hub.get(anyone(), "FAM-1".into()).await).await {
        TicketsEvent::Ticket { ticket } => {
            assert_eq!(ticket.child_count, 1);
            assert_eq!(ticket.parent, None);
        }
        other => panic!("expected Ticket, got {other:?}"),
    }
    assert_eq!(
        ticket_ids(hub.list(anyone(), None, None, Some("FAM-1".into()), None, None, None, None, None, None, None).await).await,
        vec!["FAM-2"]
    );
    match one(hub.get(anyone(), "FAM-2".into()).await).await {
        TicketsEvent::Ticket { ticket } => assert_eq!(ticket.parent.as_deref(), Some("FAM-1")),
        other => panic!("expected Ticket, got {other:?}"),
    }
}

/// AGS-19 acceptance 2: a parent that names no ticket is an in-band
/// error naming the reason, and nothing is created.
#[tokio::test]
async fn create_rejects_a_nonexistent_parent_in_band() {
    let hub = hub();
    let ev = one(
        hub.create(anyone(), 
            "ORPH".into(),
            "no such parent".into(),
            Some("scope".into()),
            None,
            None,
            None,
            Some("ORPH-9".into()), None,
            None,
        )
        .await,
    )
    .await;
    match ev {
        TicketsEvent::Error { message } => {
            assert!(message.contains("ORPH-9"), "names the parent: {message}");
            assert!(message.contains("must exist"), "names the reason: {message}");
        }
        other => panic!("expected Error, got {other:?}"),
    }
    assert!(matches!(
        one(hub.get(anyone(), "ORPH-1".into()).await).await,
        TicketsEvent::NotFound { .. }
    ));
}

/// AGS-19 acceptance 4: `list {parent: P}` yields exactly P's
/// children, in (epic, seq) order.
#[tokio::test]
async fn list_filters_by_parent() {
    let hub = hub();
    one(hub.create(anyone(), "LP".into(), "P".into(), Some("scope".into()), None, None, None, None, None, None).await).await;
    one(hub.create(anyone(), "LP".into(), "unrelated".into(), None, None, None, None, None, None, None).await).await;
    // ZZ's first ticket is its scope root (MNG-12), filed under LP-1.
    for (epic, title, kind) in [("LP", "child a", None), ("ZZ", "child b", Some("scope"))] {
        one(hub
            .create(anyone(), 
                epic.into(),
                title.into(),
                kind.map(Into::into),
                None,
                None,
                None,
                Some("LP-1".into()), None,
                None,
            )
            .await)
        .await;
    }

    // MNG-14: "unrelated" (LP-2) named no parent and hangs under the root.
    assert_eq!(
        ticket_ids(hub.list(anyone(), None, None, Some("LP-1".into()), None, None, None, None, None, None, None).await).await,
        vec!["LP-2", "LP-3", "ZZ-1"]
    );
    assert!(ticket_ids(hub.list(anyone(), None, None, Some("LP-2".into()), None, None, None, None, None, None, None).await)
        .await
        .is_empty());
    assert_eq!(
        ticket_ids(hub.list(anyone(), None, None, None, None, None, None, None, None, None, None).await).await.len(),
        4,
        "unfiltered list is unchanged"
    );
}

/// MNG-9: `summary: true` swaps the row shape — every field of the
/// ticket except the body, and the serialized row carries no `body`
/// key at all (what a board renders; the body arrives via `get`).
#[tokio::test]
async fn summary_rows_carry_every_field_but_the_body() {
    let hub = hub();
    one(hub
        .create(anyone(), 
            "SUM".into(),
            "with a body".into(),
            Some("scope".into()), // MNG-12: a prefix's first ticket is its scope
            Some("High".into()),
            None,
            Some("## Problem\n\nthe weight of a list".into()),
            None, None,
            None,
        )
        .await)
    .await;

    let evs: Vec<_> = Box::pin(hub.list(anyone(), None, None, None, None, None, Some(true), None, None, None, None).await)
        .collect()
        .await;
    assert_eq!(evs.len(), 2, "one row, one page: {evs:?}");
    let row = serde_json::to_value(&evs[0]).unwrap();
    assert_eq!(row["type"], "summary");
    let summary = &row["summary"];
    assert!(summary.get("body").is_none(), "no body on a summary: {summary}");
    assert_eq!(summary["id"], "SUM-1");
    assert_eq!(summary["kind"], "scope");
    assert_eq!(summary["severity"], "High");
    assert!(summary["parent"].is_null(), "an absent parent is null, not missing");
    assert_eq!(summary["child_count"], 0);
    assert!(summary.get("children").is_none(), "SCALE-4: no list of children: {summary}");

    // The full ticket still carries it, unchanged.
    let evs: Vec<_> = Box::pin(hub.list(anyone(), None, None, None, None, None, None, None, None, None, None).await)
        .collect()
        .await;
    let row = serde_json::to_value(&evs[0]).unwrap();
    assert_eq!(row["type"], "ticket");
    assert!(row["ticket"]["body"].as_str().unwrap().contains("the weight of a list"));
}

/// MNG-9: `after` naming no ticket is one in-band error naming it,
/// and nothing else — no rows, no page (there is nowhere to resume).
#[tokio::test]
async fn an_after_that_names_no_ticket_is_one_in_band_error() {
    let hub = hub();
    one(hub.create(anyone(), "CUR".into(), "a".into(), None, None, None, None, None, None, None).await).await;

    for evs in [
        Box::pin(hub.list(anyone(), None, None, None, Some("ZZZ-9".into()), None, None, None, None, None, None).await)
            .collect::<Vec<_>>()
            .await,
        Box::pin(hub.work().ready(anyone(), None, Some("ZZZ-9".into()), None, None, None, None).await)
            .collect::<Vec<_>>()
            .await,
    ] {
        assert_eq!(evs.len(), 1, "one event, no page: {evs:?}");
        match &evs[0] {
            TicketsEvent::Error { message } => {
                assert_eq!(message, "after: no ticket ZZZ-9");
            }
            other => panic!("expected Error, got {other:?}"),
        }
    }
}

/// SCALE-6: `find` yields `summary` rows — never full tickets — then
/// one `page`; with no `limit` it answers the palette's 50, and
/// `remaining` is the rest of the matches. The query is trimmed and
/// case-folded; `after` is `list`'s cursor (a position: FND-3 does not
/// match "ticket page"), and one naming no ticket is the one in-band
/// error.
#[tokio::test]
async fn find_yields_summary_rows_then_one_page_fifty_by_default() {
    let hub = hub();
    for i in 1..=60 {
        let kind = (i == 1).then_some(crate::types::Kind::Scope); // MNG-12: the root first
        one(hub.create(anyone(), "FND".into(), format!("row {i}").as_str().into(), kind, None, None, Some("a body".into()), None, None, None).await).await;
    }
    one(hub.create(anyone(), "OTH".into(), "The Ticket Page".into(), Some("scope".into()), None, None, None, None, None, None).await).await;

    let evs: Vec<TicketsEvent> = Box::pin(hub.find(anyone(), "".into(), None, None, None, None).await).collect().await;
    assert_eq!(evs.len(), 51, "50 rows and the page");
    let row = serde_json::to_value(&evs[0]).unwrap();
    assert_eq!((row["type"].as_str(), row["summary"]["id"].as_str()), (Some("summary"), Some("FND-1")));
    assert!(row["summary"].get("body").is_none(), "a summary carries no body: {row}");
    let (ids, page) = ids_and_page(hub.find(anyone(), "".into(), None, None, None, None).await).await;
    assert_eq!(ids.len(), 50, "the palette's 50 when no limit is asked");
    assert_eq!(page, Some((Some("FND-50".into()), 11)));

    let (ids, page) = ids_and_page(hub.find(anyone(), "  fnd-5 ".into(), None, None, None, None).await).await;
    let expected: Vec<String> =
        std::iter::once(5).chain(50..=59).map(|n| format!("FND-{n}")).collect();
    assert_eq!(ids, expected, "trimmed, case-folded, in (epic, seq) order");
    assert_eq!(page, Some((None, 0)));

    let (ids, page) =
        ids_and_page(hub.find(anyone(), "ticket page".into(), Some("fnd-3".into()), Some(1), None, None).await).await;
    assert_eq!((ids, page), (vec!["OTH-1".to_string()], Some((None, 0))));

    let evs: Vec<TicketsEvent> = Box::pin(hub.find(anyone(), "row".into(), Some("ZZZ-9".into()), None, None, None).await)
        .collect()
        .await;
    match evs.as_slice() {
        [TicketsEvent::Error { message }] => assert_eq!(message, "after: no ticket ZZZ-9"),
        other => panic!("one in-band error and nothing else, got {other:?}"),
    }
}

#[tokio::test]
async fn dangling_forward_reference_resolves_at_birth() {
    let hub = hub();
    one(hub
        .create(anyone(), "DAG".into(), "d1".into(), Some("scope".into()), None, Some(vec!["DAG-2".into()]), None, None, None, None)
        .await)
    .await;
    // The ticket that becomes DAG-2 has DAG-1 in unlocks from birth.
    match one(hub.create(anyone(), "DAG".into(), "d2".into(), None, None, None, None, None, None, None).await).await {
        TicketsEvent::Created { ticket } => {
            assert_eq!(ticket.id, "DAG-2");
            assert_eq!(ticket.unlocks, vec!["DAG-1"]);
        }
        other => panic!("expected Created, got {other:?}"),
    }
    match one(hub.get(anyone(), "DAG-2".into()).await).await {
        TicketsEvent::Ticket { ticket } => assert_eq!(ticket.unlocks, vec!["DAG-1"]),
        other => panic!("expected Ticket, got {other:?}"),
    }
}

#[tokio::test]
async fn create_rejects_self_reference() {
    let hub = hub();
    // First create in the epic allocates DAG-1; naming it is a self-cycle.
    let ev = one(hub
        .create(anyone(), "DAG".into(), "me".into(), Some("scope".into()), None, Some(vec!["DAG-1".into()]), None, None, None, None)
        .await)
    .await;
    match ev {
        TicketsEvent::Error { message } => assert!(message.contains("cycle"), "{message}"),
        other => panic!("expected Error, got {other:?}"),
    }
    assert!(matches!(
        one(hub.get(anyone(), "DAG-1".into()).await).await,
        TicketsEvent::NotFound { .. }
    ));
}

#[tokio::test]
async fn create_rejects_cycle_through_dangling_forward_reference() {
    let hub = hub();
    // DAG-1 blocked_by the not-yet-existing DAG-2.
    one(hub
        .create(anyone(), "DAG".into(), "d1".into(), Some("scope".into()), None, Some(vec!["DAG-2".into()]), None, None, None, None)
        .await)
    .await;
    // The create that would become DAG-2, blocked_by DAG-1, closes the loop.
    let ev = one(hub
        .create(anyone(), "DAG".into(), "d2".into(), None, None, Some(vec!["DAG-1".into()]), None, None, None, None)
        .await)
    .await;
    match ev {
        TicketsEvent::Error { message } => assert!(message.contains("cycle"), "{message}"),
        other => panic!("expected Error, got {other:?}"),
    }
    // Nothing was stored.
    assert!(matches!(
        one(hub.get(anyone(), "DAG-2".into()).await).await,
        TicketsEvent::NotFound { .. }
    ));
}

#[tokio::test]
async fn create_rejects_longer_cycles() {
    let hub = hub();
    one(hub
        .create(anyone(), "DAG".into(), "p1".into(), Some("scope".into()), None, Some(vec!["DAG-3".into()]), None, None, None, None)
        .await)
    .await;
    one(hub
        .create(anyone(), "DAG".into(), "p2".into(), None, None, Some(vec!["DAG-1".into()]), None, None, None, None)
        .await)
    .await;
    // Would become DAG-3: DAG-3 -> DAG-2 -> DAG-1 -> DAG-3.
    let ev = one(hub
        .create(anyone(), "DAG".into(), "p3".into(), None, None, Some(vec!["DAG-2".into()]), None, None, None, None)
        .await)
    .await;
    assert!(matches!(ev, TicketsEvent::Error { .. }), "got {ev:?}");
    assert!(matches!(
        one(hub.get(anyone(), "DAG-3".into()).await).await,
        TicketsEvent::NotFound { .. }
    ));
}

#[tokio::test]
async fn watch_epic_filter_drops_other_epics() {
    let hub = hub();
    let mut watch = Box::pin(hub.watch(anyone(), Some("mng".into()), None, None, None).await);

    one(hub.create(anyone(), "OTHER".into(), "x".into(), Some("scope".into()), None, None, None, None, None, None).await).await;
    one(hub.create(anyone(), "MNG".into(), "y".into(), Some("scope".into()), None, None, None, None, None, None).await).await;

    match watch.next().await.expect("change") {
        TicketsEvent::Created { ticket } => assert_eq!(ticket.epic, "MNG"),
        other => panic!("expected Created, got {other:?}"),
    }
}

/// One line per event, unique within this test's burst — so "the
/// newest changes, none twice" is a comparison of lists.
fn label(ev: &TicketsEvent) -> String {
    match ev {
        TicketsEvent::Created { ticket } => format!("created {}", ticket.id),
        TicketsEvent::ChildrenChanged { id, added, .. } => format!("children {id} +{added}"),
        TicketsEvent::GroupChanged { group } => format!("group {} {}", group.epic, group.total),
        TicketsEvent::Lagged { missed } => format!("lagged {missed}"),
        TicketsEvent::Transition { transition } => format!("transition {}", transition.ticket),
        other => format!("{other:?}"),
    }
}

/// SCALE-5 criterion 1, against the store's real channel (capacity 256,
/// untouched): a `watch` subscribed and then left unread while 200
/// creates broadcast 800 changes — each create is `created`, its
/// `transition` (MENGER-13), the root's `children_changed`, the
/// shortcode's `group_changed` — reads exactly
/// one `lagged { missed: 544 }` (800 - 256, the count the channel
/// reports), then precisely the newest 256 changes in acknowledgment
/// order, none twice, then later changes as usual. A `watch { epic }`
/// on another shortcode is told too, and then hears only its own.
#[tokio::test]
async fn a_watch_that_falls_behind_reads_one_lagged_then_the_newest_changes() {
    const CREATES: u64 = 200;
    const HELD: u64 = 256; // Store::with_connection's broadcast capacity
    let hub = hub();
    one(hub.create(anyone(), "LAG".into(), "root".into(), Some("scope".into()), None, None, None, None, None, None).await).await;
    let mut watch = Box::pin(hub.watch(anyone(), None, None, None, None).await);
    let mut other = Box::pin(hub.watch(anyone(), Some("other".into()), None, None, None).await);

    let mut broadcast: Vec<String> = Vec::new();
    for k in 2..=CREATES + 1 {
        match one(hub.create(anyone(), "LAG".into(), format!("t{k}").as_str().into(), None, None, None, None, None, None, None).await).await {
            TicketsEvent::Created { ticket } => assert_eq!(ticket.id, format!("LAG-{k}")),
            ev => panic!("expected Created, got {ev:?}"),
        }
        broadcast.push(format!("created LAG-{k}"));
        // MENGER-13: its first status's transition, right after it.
        broadcast.push(format!("transition LAG-{k}"));
        broadcast.push(format!("children LAG-1 +LAG-{k}"));
        broadcast.push(format!("group LAG {k}"));
    }
    let missed = broadcast.len() as u64 - HELD;
    assert_eq!(missed, 544);

    // The filtered subscriber is told as well; the 256 LAG changes still
    // held are then read and filtered out, and nothing else is ready.
    assert_eq!(label(&other.next().await.expect("an event")), format!("lagged {missed}"), "past the epic filter");
    assert!(futures::FutureExt::now_or_never(other.next()).is_none(), "no LAG change passes the filter");

    let mut read: Vec<String> = Vec::new();
    for _ in 0..=HELD {
        read.push(label(&watch.next().await.expect("an event")));
    }
    assert_eq!(read[0], format!("lagged {missed}"), "one lagged, first, with the channel's count");
    assert_eq!(read[1..], broadcast[missed as usize..], "then the newest {HELD}, in order, none twice");
    assert_eq!(read.iter().filter(|l| l.starts_with("lagged")).count(), 1);
    assert!(futures::FutureExt::now_or_never(watch.next()).is_none(), "and nothing more");

    one(hub.create(anyone(), "LAG".into(), "after".into(), None, None, None, None, None, None, None).await).await;
    assert_eq!(label(&watch.next().await.expect("an event")), format!("created LAG-{}", CREATES + 2), "and on, as usual");

    one(hub.create(anyone(), "OTHER".into(), "its own".into(), Some("scope".into()), None, None, None, None, None, None).await).await;
    assert_eq!(label(&other.next().await.expect("an event")), "created OTHER-1", "then only its own epic");

    let wire = serde_json::to_value(TicketsEvent::Lagged { missed: 3 }).unwrap();
    assert_eq!(wire, serde_json::json!({"type": "lagged", "missed": 3}));
}
/// MNG-12/14: `set_parent` over the activation — `ParentChanged` on a
/// move, `NotFound` for a missing child, in-band errors for a missing
/// parent, a parent outside the shortcode, and ancestry; `watch`
/// carries both parents' sides.
#[tokio::test]
async fn set_parent_yields_parent_changed_and_maps_errors_in_band() {
    let hub = hub();
    one(hub.create(anyone(), "SP".into(), "root".into(), Some("scope".into()), None, None, None, None, None, None).await).await;
    one(hub.create(anyone(), "SP".into(), "a".into(), None, None, None, None, None, None, None).await).await; // SP-2 → SP-1
    one(hub.create(anyone(), "SP".into(), "b".into(), None, None, None, None, None, None, None).await).await; // SP-3 → SP-1
    one(hub.create(anyone(), "OT".into(), "other".into(), Some("scope".into()), None, None, None, None, None, None).await).await;
    let mut watch = Box::pin(hub.watch(anyone(), None, None, None, None).await);

    match one(hub.set_parent(anyone(), "sp-3".into(), "sp-2".into()).await).await {
        TicketsEvent::ParentChanged { ticket, previous } => {
            assert_eq!(ticket.id, "SP-3");
            assert_eq!(ticket.parent.as_deref(), Some("SP-2"));
            assert_eq!(previous.as_deref(), Some("SP-1"));
        }
        other => panic!("expected ParentChanged, got {other:?}"),
    }
    assert!(matches!(watch.next().await, Some(TicketsEvent::ParentChanged { .. })));
    match watch.next().await {
        Some(TicketsEvent::ChildrenChanged { id, added, child_count }) => {
            assert_eq!(id, "SP-2");
            assert_eq!(added, "SP-3");
            assert_eq!(child_count, 1);
        }
        other => panic!("expected ChildrenChanged, got {other:?}"),
    }
    match watch.next().await {
        Some(TicketsEvent::ChildRemoved { id, removed, child_count }) => {
            assert_eq!(id, "SP-1");
            assert_eq!(removed, "SP-3");
            assert_eq!(child_count, 1, "SP-2 is left");
        }
        other => panic!("expected ChildRemoved, got {other:?}"),
    }

    assert!(matches!(
        one(hub.set_parent(anyone(), "SP-9".into(), "SP-1".into()).await).await,
        TicketsEvent::NotFound { .. }
    ));
    match one(hub.set_parent(anyone(), "SP-2".into(), "SP-9".into()).await).await {
        TicketsEvent::Error { message } => {
            assert!(message.contains("SP-9") && message.contains("must exist"), "{message}")
        }
        other => panic!("expected Error, got {other:?}"),
    }
    match one(hub.set_parent(anyone(), "SP-2".into(), "OT-1".into()).await).await {
        TicketsEvent::Error { message } => {
            assert!(message.contains("OT-1") && message.contains("outside"), "{message}")
        }
        other => panic!("expected Error, got {other:?}"),
    }
    match one(hub.set_parent(anyone(), "SP-1".into(), "SP-3".into()).await).await {
        TicketsEvent::Error { message } => assert!(message.contains("own ancestor"), "{message}"),
        other => panic!("expected Error, got {other:?}"),
    }
    // The root may be hosted by another shortcode.
    match one(hub.set_parent(anyone(), "SP-1".into(), "OT-1".into()).await).await {
        TicketsEvent::ParentChanged { ticket, .. } => assert_eq!(ticket.parent.as_deref(), Some("OT-1")),
        other => panic!("expected ParentChanged, got {other:?}"),
    }
}

/// MNG-12: the first ticket of a prefix must be a scope — over the
/// activation the refusal is the in-band error naming the rule.
#[tokio::test]
async fn first_ticket_of_a_prefix_must_be_a_scope_in_band() {
    let hub = hub();
    match one(hub.create(anyone(), "FRESH".into(), "a build".into(), None, None, None, None, None, None, None).await).await {
        TicketsEvent::Error { message } => {
            assert!(message.contains("FRESH") && message.contains("scope"), "{message}");
        }
        other => panic!("expected Error, got {other:?}"),
    }
    assert!(matches!(
        one(hub.get(anyone(), "FRESH-1".into()).await).await,
        TicketsEvent::NotFound { .. }
    ));
}

/// ACCESS-29 (ACCESS-4 L5): `list` answers 1,000 rows unless `limit` says,
/// and never more than 10,000; the page event carries the cursor to the
/// rest, so a client that pages still reads everything.
#[tokio::test]
async fn list_pages_by_default_and_never_past_the_most() {
    use crate::limits::{LIST_PAGE_DEFAULT, LIST_PAGE_MAX};
    let store = Arc::new(Store::new());
    store.seed_synthetic(SYS, crate::store::SeedSpec { count: 1_200, seed: 7, into_existing: false }).unwrap();
    let hub = TicketsActivation::new(store);
    let list = |after: Option<crate::vocab::Cursor>, limit: Option<u32>| {
        hub.list(anyone(), None, None, None, after, limit, Some(true), None, None, None, None)
    };
    let (ids, page) = ids_and_page(list(None, None).await).await;
    assert_eq!(ids.len(), LIST_PAGE_DEFAULT as usize);
    let (next, remaining) = page.expect("a page");
    assert_eq!(remaining, 200);
    let (rest, page) = ids_and_page(list(next.as_deref().map(Into::into), None).await).await;
    assert_eq!((rest.len(), page.map(|p| p.0)), (200, Some(None)), "paged, everything is read");
    let (ids, _) = ids_and_page(list(None, Some(u32::MAX)).await).await;
    assert_eq!(ids.len(), 1_200, "under the most, what is asked");
    assert!(LIST_PAGE_MAX >= 10_000);
    // Full rows page the same way.
    let (ids, _) = ids_and_page(hub.list(anyone(), None, None, None, None, None, None, None, None, None, None).await).await;
    assert_eq!(ids.len(), LIST_PAGE_DEFAULT as usize);
}

/// ACCESS-29: a title past 1 KiB and a filter past 4 KiB are refused,
/// naming the size — ACCESS-8: by their types, at dispatch
/// (`invalid_params`, tests/wire.rs), before the method runs; a title at
/// the cap is kept.
#[tokio::test]
async fn a_title_and_a_filter_are_held_to_their_sizes() {
    let hub = hub();
    one(hub.create(anyone(), "SZ".into(), "root".into(), Some("scope".into()), None, None, None, None, None, None).await).await;
    let long = "t".repeat(crate::limits::TITLE_MAX + 1);
    let refused = long.parse::<crate::vocab::Title>().unwrap_err();
    assert!(refused.contains("at most 1024 bytes"), "{refused}");
    let at = "t".repeat(crate::limits::TITLE_MAX);
    assert!(matches!(one(hub.amend(anyone(), "SZ-1".into(), Some(at.as_str().into()), None).await).await, TicketsEvent::Amended { .. }));
    let filter = format!("kind build and {}", "not kind doc and ".repeat(300));
    let refused = filter.parse::<crate::vocab::FilterText>().unwrap_err();
    assert!(refused.contains("a filter is at most 4096 bytes"), "{refused}");
}

/// ACCESS-29: `comments.list` answers 500 unless `limit` says, never past
/// 1,000.
#[tokio::test]
async fn comments_list_pages_by_default_and_never_past_the_most() {
    use crate::comments::{Anchor, CommentsEvent};
    let store = Arc::new(Store::new());
    for i in 0..1_100 {
        let anchor = Anchor { route: "#/".into(), path: "body".into(), label: "x".into(), excerpt: "x".into(), ticket: None, version: None };
        store.add_comment(SYS, anchor, format!("note {i}"), None).await.unwrap();
    }
    let comments = crate::CommentsActivation::new(store);
    let count = |evs: Vec<CommentsEvent>| {
        let n = evs.iter().filter(|e| matches!(e, CommentsEvent::Comment { .. })).count();
        let next = evs.iter().find_map(|e| if let CommentsEvent::Page { next } = e { Some(*next) } else { None });
        (n, next.expect("a page"))
    };
    let list = |limit| comments.list(anyone(), None, None, None, None, None, None, None, None, None, limit);
    let (n, next) = count(Box::pin(list(None).await).collect().await);
    assert_eq!((n, next.is_some()), (500, true));
    let (n, next) = count(Box::pin(list(Some(5_000)).await).collect().await);
    assert_eq!((n, next.is_some()), (1_000, true));
}

/// APP-24: `await_reply` — `waiting` at once, then one `replied`, then the
/// end. A write after the call subscribed and before its first ask is
/// answered once (no gap, no duplicate); a live one wakes it; `since`
/// answers what already happened; `timeout_minutes` ends it with
/// `timed_out`; a ticket that is none is `not_found`.
#[tokio::test]
async fn await_reply_waits_then_answers_once() {
    use crate::queue::{Reply, Urgency};
    let store = Arc::new(Store::new());
    let hub = TicketsActivation::new(Arc::clone(&store));
    let decision = "<m-decision data-field=\"decision\" id=\"landing\">\n<m-question data-field=\"question\">A or B?</m-question>\n\
<m-option data-field=\"option\" name=\"A\" value=\"a\"></m-option>\n<m-option data-field=\"option\" name=\"B\" value=\"b\"></m-option>\n</m-decision>\n";
    store.create(SYS, crate::store::NewTicket { epic: "AW".into(), title: "root".into(), kind: Kind::Scope, severity: None, blocked_by: vec![], parent: None, path: None, body: String::new() }).await.unwrap();
    store.create(SYS, crate::store::NewTicket { epic: "AW".into(), title: "asks".into(), kind: Kind::Build, severity: None, blocked_by: vec![], parent: None, path: None, body: decision.into() }).await.unwrap();
    store.queue_by(SYS.permit("AW-2"), Urgency::Blocking, "idle", Some("landing"), Act::default()).await.unwrap();

    // Subscribed at the call; the answer lands before the first ask.
    let mut s = Box::pin(hub.await_reply(anyone(), "aw-2".into(), None, Some("landing".into()), None).await);
    store.answer(SYS.permit("AW-2"), "landing", InputValue::Choice { option: "b".into() }, None, AnswerSource::App).await.unwrap();
    match s.next().await {
        Some(TicketsEvent::Waiting { ticket, queued }) => assert_eq!((ticket.as_str(), queued.map(|e| e.urgency)), ("AW-2", Some(Urgency::Blocking))),
        other => panic!("expected waiting, got {other:?}"),
    }
    match s.next().await {
        Some(TicketsEvent::Replied { ticket, reply: Reply::Answered { input, value, .. } }) => {
            assert_eq!((ticket.as_str(), input.as_str(), value), ("AW-2", "landing", InputValue::Choice { option: "b".into() }))
        }
        other => panic!("expected replied, got {other:?}"),
    }
    assert!(s.next().await.is_none(), "once, then the end — the event still on the channel says nothing more");
    // Called again without `since`: the answer already there (this very
    // second, likely) is set aside — nothing new, so the timeout.
    let mut s = Box::pin(hub.await_reply(anyone(), "AW-2".into(), None, None, Some(0)).await);
    assert!(matches!(s.next().await, Some(TicketsEvent::Waiting { .. })));
    assert!(matches!(s.next().await, Some(TicketsEvent::TimedOut { .. })), "what came before the call is no reply");

    // Live: nothing yet, then a comment wakes it.
    let mut s = Box::pin(hub.await_reply(anyone(), "AW-1".into(), None, None, None).await);
    assert!(matches!(s.next().await, Some(TicketsEvent::Waiting { queued: None, .. })));
    let writer = Arc::clone(&store);
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        writer.add_comment(SYS, crate::store::conversation_anchor("AW-1"), "do X instead".into(), None).await.unwrap();
    });
    match tokio::time::timeout(std::time::Duration::from_secs(5), s.next()).await.expect("woken") {
        Some(TicketsEvent::Replied { reply: Reply::Commented { comment }, .. }) => assert_eq!(comment.body, "do X instead"),
        other => panic!("expected replied, got {other:?}"),
    }
    assert!(s.next().await.is_none());

    // `since`: what already happened answers at once.
    let at = store.comments(SYS, &Default::default(), None, None).unwrap().comments[0].at;
    let mut s = Box::pin(hub.await_reply(anyone(), "AW-1".into(), Some(at), None, None).await);
    assert!(matches!(s.next().await, Some(TicketsEvent::Waiting { .. })));
    assert!(matches!(s.next().await, Some(TicketsEvent::Replied { reply: Reply::Commented { .. }, .. })));

    // A timeout with nothing: timed_out, the end.
    let mut s = Box::pin(hub.await_reply(anyone(), "AW-1".into(), Some(at + 3600), None, Some(0)).await);
    assert!(matches!(s.next().await, Some(TicketsEvent::Waiting { .. })));
    assert!(matches!(s.next().await, Some(TicketsEvent::TimedOut { ref ticket }) if ticket == "AW-1"));
    assert!(s.next().await.is_none());

    // None such.
    let s = hub.await_reply(anyone(), "AW-99".into(), None, None, None).await;
    assert!(matches!(one(s).await, TicketsEvent::NotFound { ref id } if id == "AW-99"));
}
