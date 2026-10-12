//! MENGER-29: the tickets activation's tests, moved out of `activation.rs`.

use super::*;
use futures::StreamExt;

fn hub() -> Tickets {
    Tickets::new(Arc::new(Store::new()))
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
    match one(hub.health(None, Reach::everywhere()).await).await {
        TicketsEvent::Health { ticket_count, .. } => assert_eq!(ticket_count, 0),
        other => panic!("expected Health, got {other:?}"),
    }
}

#[tokio::test]
async fn create_allocates_sequential_ids() {
    let hub = hub();
    let ev = one(hub.create(None, Reach::everywhere(), "mng".into(), "first".into(), Some("scope".into()), None, None, None, None, None).await).await;
    match ev {
        TicketsEvent::Created { ticket } => {
            assert_eq!(ticket.id, "MNG-1");
            assert_eq!(ticket.status, Status::Pending);
        }
        other => panic!("expected Created, got {other:?}"),
    }
    let ev = one(hub.create(None, Reach::everywhere(), "MNG".into(), "second".into(), None, None, None, None, None, None).await).await;
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
    one(hub.create(None, Reach::everywhere(), "MNG".into(), "root".into(), Some("scope".into()), None, None, None, None, None).await).await;
    match one(hub.create(None, Reach::everywhere(), "MNG".into(), "no kind".into(), None, None, None, None, None, None).await).await {
        TicketsEvent::Created { ticket } => assert_eq!(ticket.kind, Kind::Build),
        other => panic!("expected Created, got {other:?}"),
    }
    match one(hub.create(None, Reach::everywhere(), "MNG".into(), "a gap".into(), Some("gap".into()), None, None, None, None, None).await)
        .await
    {
        TicketsEvent::Created { ticket } => assert_eq!(ticket.kind, Kind::Gap),
        other => panic!("expected Created, got {other:?}"),
    }
    let ev = one(
        hub.create(None, Reach::everywhere(), "MNG".into(), "x".into(), Some("bug".into()), None, None, None, None, None)
            .await,
    )
    .await;
    match ev {
        TicketsEvent::Error { message } => {
            assert!(message.contains("'bug'"), "{message}");
            for name in ["scope", "execution", "build", "gap", "spike", "doc"] {
                assert!(message.contains(name), "names {name}: {message}");
            }
        }
        other => panic!("expected Error, got {other:?}"),
    }
    assert!(matches!(
        one(hub.get(None, Reach::everywhere(), "MNG-4".into()).await).await,
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
    match one(hub.create(None, Reach::everywhere(), "DOCS".into(), "the manifest".into(), Some("doc".into()), None, None, None, None, None).await).await {
        TicketsEvent::Created { ticket } => {
            assert_eq!((ticket.id.as_str(), ticket.kind, ticket.status), ("DOCS-1", Kind::Doc, Status::Doc));
        }
        other => panic!("expected Created, got {other:?}"),
    }
    one(hub.create(None, Reach::everywhere(), "DOCS".into(), "work".into(), None, None, None, None, None, None).await).await; // DOCS-2
    for status in ["Ready", "Complete", "Doc", "active"] {
        match one(hub.set_status(None, Reach::everywhere(), "DOCS-1".into(), status.into(), None, None, None).await).await {
            TicketsEvent::Error { message } => assert!(message.contains("a doc has no lifecycle"), "{message}"),
            other => panic!("{status}: expected Error, got {other:?}"),
        }
    }
    match one(hub.set_status(None, Reach::everywhere(), "DOCS-2".into(), "doc".into(), None, None, None).await).await {
        TicketsEvent::Error { message } => assert!(message.contains("Doc is a doc's own status"), "{message}"),
        other => panic!("expected Error, got {other:?}"),
    }
    let blocked = Some(vec!["DOCS-2".to_string()]);
    match one(hub.create(None, Reach::everywhere(), "DOCS".into(), "x".into(), Some("doc".into()), None, blocked, None, None, None).await).await {
        TicketsEvent::Error { message } => assert!(message.contains("a doc is never blocked"), "{message}"),
        other => panic!("expected Error, got {other:?}"),
    }
    let on_doc = Some(vec!["DOCS-1".to_string()]);
    match one(hub.create(None, Reach::everywhere(), "DOCS".into(), "x".into(), None, None, on_doc, None, None, None).await).await {
        TicketsEvent::Error { message } => assert!(message.contains("'DOCS-1' is a doc"), "{message}"),
        other => panic!("expected Error, got {other:?}"),
    }
    match one(hub.list(None, Reach::everywhere(), None, Some("Doc".into()), None, None, None, Some(true), None, None, None, None).await).await {
        TicketsEvent::Summary { summary } => assert_eq!((summary.id.as_str(), summary.status), ("DOCS-1", Status::Doc)),
        other => panic!("expected Summary, got {other:?}"),
    }
    match one(hub.get(None, Reach::everywhere(), "DOCS-1".into()).await).await {
        TicketsEvent::Ticket { ticket } => assert_eq!(ticket.status, Status::Doc, "nothing moved it"),
        other => panic!("expected Ticket, got {other:?}"),
    }
}

#[tokio::test]
async fn get_distinguishes_found_from_missing() {
    let hub = hub();
    one(hub.create(None, Reach::everywhere(), "MNG".into(), "x".into(), Some("scope".into()), None, None, None, None, None).await).await;
    assert!(matches!(
        one(hub.get(None, Reach::everywhere(), "MNG-1".into()).await).await,
        TicketsEvent::Ticket { .. }
    ));
    assert!(matches!(
        one(hub.get(None, Reach::everywhere(), "MNG-99".into()).await).await,
        TicketsEvent::NotFound { .. }
    ));
}

#[tokio::test]
async fn list_filters_by_status() {
    let hub = hub();
    one(hub.create(None, Reach::everywhere(), "MNG".into(), "a".into(), Some("scope".into()), None, None, None, None, None).await).await;
    one(hub.create(None, Reach::everywhere(), "MNG".into(), "b".into(), None, None, None, None, None, None).await).await;
    one(hub.set_status(None, Reach::everywhere(), "MNG-1".into(), "Ready".into(), None, None, None).await).await;

    let (ids, page) =
        ids_and_page(hub.list(None, Reach::everywhere(), None, Some("Ready".into()), None, None, None, None, None, None, None, None).await).await;
    assert_eq!(ids, vec!["MNG-1"]);
    assert_eq!(page, Some((None, 0)), "one match, nothing beyond it");
}

#[tokio::test]
async fn set_status_reports_previous() {
    let hub = hub();
    one(hub.create(None, Reach::everywhere(), "MNG".into(), "a".into(), Some("scope".into()), None, None, None, None, None).await).await;
    match one(hub.set_status(None, Reach::everywhere(), "MNG-1".into(), "ready".into(), None, None, None).await).await {
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
    one(hub.create(None, Reach::everywhere(), "IDN".into(), "identity".into(), Some("doc".into()), None, None, None, None, None).await).await;
    one(hub.create(None, Reach::everywhere(), "IDN".into(), "a".into(), None, None, None, None, None, None).await).await;
    match one(hub.set_kind(None, Reach::everywhere(), "IDN-1".into(), "Scope".into(), None, None, None).await).await {
        TicketsEvent::KindChanged { ticket, previous } => {
            assert_eq!((ticket.kind, ticket.status, previous), (Kind::Scope, Status::Epic, Kind::Doc));
        }
        other => panic!("expected KindChanged, got {other:?}"),
    }
    match one(hub.set_kind(None, Reach::everywhere(), "IDN-2".into(), "gap".into(), Some("active".into()), None, None).await).await {
        TicketsEvent::KindChanged { ticket, previous } => {
            assert_eq!((ticket.kind, ticket.status, previous), (Kind::Gap, Status::Coding, Kind::Build));
        }
        other => panic!("expected KindChanged, got {other:?}"),
    }
    assert!(matches!(
        one(hub.set_kind(None, Reach::everywhere(), "IDN-9".into(), "gap".into(), None, None, None).await).await,
        TicketsEvent::NotFound { id } if id == "IDN-9"
    ));
    for (id, kind, status, says) in [
        ("IDN-2", "bug", None, "unknown kind 'bug'"),
        ("IDN-2", "gap", Some("Shipped"), "unknown status 'shipped'"),
        ("IDN-1", "build", None, "'IDN-1' is the root of IDN"),
        ("IDN-2", "doc", Some("Ready"), "a doc's status is Doc"),
        ("IDN-2", "spike", Some("Doc"), "Doc is a doc's own status"),
    ] {
        match one(hub.set_kind(None, Reach::everywhere(), id.into(), kind.into(), status.map(Into::into), None, None).await).await {
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
    one(hub.create(None, Reach::everywhere(), "WK".into(), "root".into(), Some("scope".into()), None, None, None, None, None).await).await;
    one(hub.create(None, Reach::everywhere(), "WK".into(), "a".into(), None, None, None, None, None, None).await).await;
    one(hub.create(None, Reach::everywhere(), "WK".into(), "b".into(), None, None, None, None, None, None).await).await;

    for (id, spelling, want, previous) in [
        ("WK-2", "Coding", Status::Coding, Status::Pending),
        ("WK-3", "active", Status::Coding, Status::Pending),
        ("WK-3", "In Review", Status::InReview, Status::Coding),
    ] {
        match one(hub.set_status(None, Reach::everywhere(), id.into(), spelling.into(), None, None, None).await).await {
            TicketsEvent::StatusChanged { ticket, previous: was } => {
                assert_eq!((ticket.id.as_str(), ticket.status, was), (id, want, previous), "{spelling}");
            }
            other => panic!("{spelling}: expected StatusChanged, got {other:?}"),
        }
    }
    let wire = serde_json::to_value(one(hub.get(None, Reach::everywhere(), "WK-3".into()).await).await).unwrap();
    assert_eq!(wire["ticket"]["status"], "In Review");

    let (ids, _) = ids_and_page(hub.list(None, Reach::everywhere(), None, Some("coding".into()), None, None, None, None, None, None, None, None).await).await;
    assert_eq!(ids, ["WK-2"]);
    let (ids, _) = ids_and_page(hub.list(None, Reach::everywhere(), None, Some("In Review".into()), None, None, None, None, None, None, None, None).await).await;
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
    let hub = Tickets::new(Arc::new(Store::new().with_body_max(10)));
    one(hub.create(None, Reach::everywhere(), "BIG".into(), "root".into(), Some("scope".into()), None, None, None, None, None).await).await;
    let ev = one(hub.create(None, Reach::everywhere(), "BIG".into(), "page".into(), Some("doc".into()), None, None, Some("x".repeat(11)), None, None).await).await;
    let wire = serde_json::to_value(&ev).unwrap();
    assert_eq!((wire["type"].as_str(), wire["bytes"].as_u64(), wire["max"].as_u64()), (Some("body_too_large"), Some(11), Some(10)), "{wire}");
    assert!(wire["id"].is_null() && wire["message"].as_str().unwrap().contains("past the 10-byte cap"), "{wire}");
    let ev = one(hub.amend(None, Reach::everywhere(), "BIG-1".into(), None, Some("y".repeat(12))).await).await;
    let wire = serde_json::to_value(&ev).unwrap();
    assert_eq!((wire["type"].as_str(), wire["id"].as_str(), wire["bytes"].as_u64()), (Some("body_too_large"), Some("BIG-1"), Some(12)), "{wire}");
    // At the cap, kept.
    let ev = one(hub.amend(None, Reach::everywhere(), "BIG-1".into(), None, Some("z".repeat(10))).await).await;
    assert!(matches!(ev, TicketsEvent::Amended { .. }), "{ev:?}");
}

#[tokio::test]
async fn amend_yields_amended_and_watch_carries_only_what_changed() {
    let hub = hub();
    one(hub.create(None, Reach::everywhere(), "AM".into(), "root".into(), Some("scope".into()), None, None, None, None, None).await).await;
    let mut watch = Box::pin(hub.watch(None, Reach::everywhere(), None, None, None, None).await);

    let ev = one(hub.amend(None, Reach::everywhere(), "am-1".into(), None, Some("the body, now".into())).await).await;
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

    match one(hub.amend(None, Reach::everywhere(), "AM-1".into(), None, Some("the body, now".into())).await).await {
        TicketsEvent::Amended { changed, .. } => assert!(changed.is_empty(), "{changed:?}"),
        other => panic!("expected Amended, got {other:?}"),
    }
    assert!(matches!(
        one(hub.amend(None, Reach::everywhere(), "AM-7".into(), Some("t".into()), None).await).await,
        TicketsEvent::NotFound { id } if id == "AM-7"
    ));
    match one(hub.amend(None, Reach::everywhere(), "AM-1".into(), Some("  ".into()), None).await).await {
        TicketsEvent::Error { message } => assert!(message.contains("empty"), "{message}"),
        other => panic!("expected Error, got {other:?}"),
    }

    one(hub.amend(None, Reach::everywhere(), "AM-1".into(), Some("a better title".into()), None).await).await;
    match watch.next().await.expect("change") {
        TicketsEvent::Amended { ticket, changed } => {
            assert_eq!(ticket.title, "a better title");
            assert_eq!(changed, ["title"], "the no-op and the refusals said nothing");
        }
        other => panic!("expected Amended, got {other:?}"),
    }
}

/// SDLC-9 over the activation (criteria 1, 2, 4): two commits linked,
/// then one of them again with a third — three in link order, the
/// second call's `added` the third alone, `get` showing them; a call
/// with one malformed entry is one in-band `error` naming it and writes
/// nothing; `not_found` for a missing id; and `watch` carries one event
/// per call that changed something — `commits_linked`,
/// `commits_unlinked` — and none for the no-ops or the refusal.
#[tokio::test]
async fn link_commits_yields_commits_linked_and_watch_carries_only_what_changed() {
    let hub = hub();
    one(hub.create(None, Reach::everywhere(), "LC".into(), "root".into(), Some("scope".into()), None, None, None, None, None).await).await;
    let mut watch = Box::pin(hub.watch(None, Reach::everywhere(), None, None, None, None).await);
    let commits = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<String>>();

    let ev = one(hub.link_commits(None, Reach::everywhere(), "lc-1".into(), commits(&["77df9a0", "plexus-macros@4F7114A"])).await).await;
    let wire = serde_json::to_value(&ev).unwrap();
    assert_eq!(wire["type"], "commits_linked");
    assert_eq!(wire["added"], serde_json::json!(["77df9a0", "plexus-macros@4f7114a"]));
    assert_eq!(wire["ticket"]["id"], "LC-1");
    assert_eq!(wire["ticket"]["commits"], wire["added"]);
    let ev = one(hub.link_commits(None, Reach::everywhere(), "LC-1".into(), commits(&["77df9a0", "0bb28b2"])).await).await;
    let wire = serde_json::to_value(&ev).unwrap();
    assert_eq!(wire["added"], serde_json::json!(["0bb28b2"]), "the third alone");
    let three = serde_json::json!(["77df9a0", "plexus-macros@4f7114a", "0bb28b2"]);
    assert_eq!(wire["ticket"]["commits"], three);
    let got = serde_json::to_value(one(hub.get(None, Reach::everywhere(), "LC-1".into()).await).await).unwrap();
    assert_eq!(got["ticket"]["commits"], three, "get shows them, in link order");
    let (rows, _) = ids_and_page(hub.list(None, Reach::everywhere(), None, None, None, None, None, Some(true), None, None, None, None).await).await;
    assert_eq!(rows, ["LC-1"]);
    let evs: Vec<TicketsEvent> = Box::pin(hub.list(None, Reach::everywhere(), None, None, None, None, None, Some(true), None, None, None, None).await).collect().await;
    assert_eq!(serde_json::to_value(&evs[0]).unwrap()["summary"]["commits"], three, "and the summary row");

    for (i, added) in [&["77df9a0"], &["0bb28b2"]].into_iter().enumerate() {
        match watch.next().await.expect("change") {
            TicketsEvent::CommitsLinked { ticket, added: got } => {
                assert_eq!((ticket.id.as_str(), got.len()), ("LC-1", if i == 0 { 2 } else { 1 }));
                assert_eq!(got[0], added[0]);
            }
            other => panic!("expected CommitsLinked, got {other:?}"),
        }
    }

    // The no-op, the refusal, the miss: nothing on `watch`.
    match one(hub.link_commits(None, Reach::everywhere(), "LC-1".into(), commits(&["0BB28B2"])).await).await {
        TicketsEvent::CommitsLinked { added, ticket } => {
            assert!(added.is_empty(), "{added:?}");
            assert_eq!(ticket.commits.len(), 3);
        }
        other => panic!("expected CommitsLinked, got {other:?}"),
    }
    match one(hub.link_commits(None, Reach::everywhere(), "LC-1".into(), commits(&["c8c5678", "77df9a0;rm", "090cdec"])).await).await {
        TicketsEvent::Error { message } => assert!(message.contains("'77df9a0;rm'"), "names the entry: {message}"),
        other => panic!("expected Error, got {other:?}"),
    }
    assert!(matches!(
        one(hub.link_commits(None, Reach::everywhere(), "LC-9".into(), commits(&["c8c5678"])).await).await,
        TicketsEvent::NotFound { id } if id == "LC-9"
    ));
    let got = serde_json::to_value(one(hub.get(None, Reach::everywhere(), "LC-1".into()).await).await).unwrap();
    assert_eq!(got["ticket"]["commits"], three, "the refused call wrote nothing");

    let ev = one(hub.unlink_commits(None, Reach::everywhere(), "LC-1".into(), commits(&["plexus-macros@4f7114a", "c8c5678"])).await).await;
    let wire = serde_json::to_value(&ev).unwrap();
    assert_eq!(wire["type"], "commits_unlinked");
    assert_eq!(wire["removed"], serde_json::json!(["plexus-macros@4f7114a"]));
    assert_eq!(wire["ticket"]["commits"], serde_json::json!(["77df9a0", "0bb28b2"]), "exactly what it named");
    match one(hub.unlink_commits(None, Reach::everywhere(), "LC-1".into(), commits(&["c8c5678"])).await).await {
        TicketsEvent::CommitsUnlinked { removed, .. } => assert!(removed.is_empty()),
        other => panic!("expected CommitsUnlinked, got {other:?}"),
    }
    match watch.next().await.expect("change") {
        TicketsEvent::CommitsUnlinked { ticket, removed } => {
            assert_eq!(removed, ["plexus-macros@4f7114a"], "the no-op, the refusal and the miss said nothing");
            assert_eq!(ticket.commits, ["77df9a0", "0bb28b2"]);
        }
        other => panic!("expected CommitsUnlinked, got {other:?}"),
    }
    assert!(futures::FutureExt::now_or_never(watch.next()).is_none(), "and nothing more");
}

#[tokio::test]
async fn watch_sees_creations_and_status_changes() {
    let hub = hub();
    let mut watch = Box::pin(hub.watch(None, Reach::everywhere(), None, None, None, None).await);

    one(hub.create(None, Reach::everywhere(), "MNG".into(), "a".into(), Some("scope".into()), None, None, None, None, None).await).await;
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

    one(hub.set_status(None, Reach::everywhere(), "MNG-1".into(), "Ready".into(), None, None, None).await).await;
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

/// MNG-10: `groups` yields one `Group` per shortcode row and nothing
/// else — no trailing page; the stream's end is the answer's end — and
/// `watch { epic }` delivers a `GroupChanged` by the group's own
/// shortcode: a watcher of PLX hears PLX's count move when a ticket of
/// its sub-shortcode completes, without hearing the ticket.
#[tokio::test]
async fn groups_yields_the_rows_and_watch_filters_group_changes_by_their_shortcode() {
    let hub = hub();
    one(hub.create(None, Reach::everywhere(), "PLX".into(), "host".into(), Some("scope".into()), None, None, None, None, None).await).await;
    one(hub.create(None, Reach::everywhere(), "SUB".into(), "hosted".into(), Some("scope".into()), None, None, None, Some("PLX-1".into()), None).await).await;
    one(hub.create(None, Reach::everywhere(), "SUB".into(), "work".into(), None, None, None, None, None, None).await).await;

    let evs: Vec<TicketsEvent> = Box::pin(hub.groups(None, Reach::everywhere()).await).collect().await;
    let groups: Vec<crate::types::Group> = evs
        .into_iter()
        .map(|ev| match ev {
            TicketsEvent::Group { group } => group,
            other => panic!("groups yields only `group` events, got {other:?}"),
        })
        .collect();
    assert_eq!(
        groups.iter().map(|g| (g.epic.as_str(), g.total, g.host.as_deref())).collect::<Vec<_>>(),
        [("PLX", 3, None), ("SUB", 2, Some("PLX-1"))]
    );
    assert_eq!(serde_json::to_value(&groups[0]).unwrap()["host"], serde_json::Value::Null, "null, not missing");

    let mut plx = Box::pin(hub.watch(None, Reach::everywhere(), Some("plx".into()), None, None, None).await);
    one(hub.set_status(None, Reach::everywhere(), "SUB-2".into(), "Complete".into(), None, None, None).await).await;
    match plx.next().await.expect("change") {
        TicketsEvent::GroupChanged { group } => {
            assert_eq!((group.epic.as_str(), group.total, group.done), ("PLX", 3, 1))
        }
        other => panic!("the SUB ticket's status_changed is filtered out; got {other:?}"),
    }
}

/// SCALE-16: `extents` yields one `Extent` per shortcode that holds a
/// ticket and nothing else — no trailing page — in shortcode order;
/// a refused create moves nothing, and the `last_seq`s add up to
/// `health`'s count.
#[tokio::test]
async fn extents_yields_each_shortcodes_last_seq_in_shortcode_order() {
    let hub = hub();
    for (epic, kind) in [("ZED", "scope"), ("AB", "scope"), ("ZED", "build"), ("B", "scope"), ("ZED", "gap")] {
        one(hub.create(None, Reach::everywhere(), epic.into(), "t".into(), Some(kind.into()), None, None, None, None, None).await).await;
    }
    // Refused: AB-2 would be its own blocker. Nothing is allocated.
    let refused = one(hub.create(None, Reach::everywhere(), "AB".into(), "t".into(), None, None, Some(vec!["AB-2".into()]), None, None, None).await).await;
    assert!(matches!(refused, TicketsEvent::Error { .. }), "{refused:?}");

    let evs: Vec<TicketsEvent> = Box::pin(hub.extents(None, Reach::everywhere()).await).collect().await;
    let extents: Vec<(String, u32)> = evs
        .into_iter()
        .map(|ev| match ev {
            TicketsEvent::Extent { epic, last_seq } => (epic, last_seq),
            other => panic!("extents yields only `extent` events, got {other:?}"),
        })
        .collect();
    assert_eq!(
        extents,
        [("AB".to_string(), 1), ("B".to_string(), 1), ("ZED".to_string(), 3)],
        "byte order on the epic: AB before B"
    );
    let total: u64 = extents.iter().map(|(_, n)| u64::from(*n)).sum();
    match one(hub.health(None, Reach::everywhere()).await).await {
        TicketsEvent::Health { ticket_count, .. } => assert_eq!(ticket_count, total),
        other => panic!("expected Health, got {other:?}"),
    }
    let wire = serde_json::to_value(TicketsEvent::Extent { epic: "AB".into(), last_seq: 1 }).unwrap();
    assert_eq!(wire, serde_json::json!({"type": "extent", "epic": "AB", "last_seq": 1}));
}

/// Ids of a `list` / `ready_set` stream, in order — asserting the
/// MNG-9 shape on the way through: rows, then exactly one closing
/// `page` event.
async fn ticket_ids(s: impl Stream<Item = TicketsEvent> + Send + 'static) -> Vec<String> {
    let (ids, page) = ids_and_page(s).await;
    assert!(page.is_some(), "every list/ready_set call ends with a page");
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
    (ids, page)
}

#[tokio::test]
async fn ready_set_gates_on_blockers() {
    let hub = hub();
    one(hub.create(None, Reach::everywhere(), "DAG".into(), "a".into(), Some("scope".into()), None, None, None, None, None).await).await;
    one(hub
        .create(None, Reach::everywhere(), "DAG".into(), "b".into(), None, None, Some(vec!["DAG-1".into()]), None, None, None)
        .await)
    .await;
    one(hub.set_status(None, Reach::everywhere(), "DAG-1".into(), "Ready".into(), None, None, None).await).await;
    one(hub.set_status(None, Reach::everywhere(), "DAG-2".into(), "Ready".into(), None, None, None).await).await;

    assert_eq!(ticket_ids(hub.ready_set(None, Reach::everywhere(), None, None, None, None, None, None).await).await, vec!["DAG-1"]);

    one(hub.set_status(None, Reach::everywhere(), "DAG-1".into(), "Complete".into(), None, None, None).await).await;
    assert_eq!(ticket_ids(hub.ready_set(None, Reach::everywhere(), None, None, None, None, None, None).await).await, vec!["DAG-2"]);
}

#[tokio::test]
async fn ready_set_accepts_superseded_blockers_and_filters_by_epic() {
    let hub = hub();
    one(hub.create(None, Reach::everywhere(), "ONE".into(), "a".into(), Some("scope".into()), None, None, None, None, None).await).await;
    one(hub
        .create(None, Reach::everywhere(), "ONE".into(), "b".into(), None, None, Some(vec!["ONE-1".into()]), None, None, None)
        .await)
    .await;
    one(hub.create(None, Reach::everywhere(), "TWO".into(), "c".into(), Some("scope".into()), None, None, None, None, None).await).await;
    one(hub.set_status(None, Reach::everywhere(), "ONE-1".into(), "Superseded".into(), None, None, None).await).await;
    one(hub.set_status(None, Reach::everywhere(), "ONE-2".into(), "Ready".into(), None, None, None).await).await;
    one(hub.set_status(None, Reach::everywhere(), "TWO-1".into(), "Ready".into(), None, None, None).await).await;

    assert_eq!(
        ticket_ids(hub.ready_set(None, Reach::everywhere(), None, None, None, None, None, None).await).await,
        vec!["ONE-2", "TWO-1"]
    );
    assert_eq!(
        ticket_ids(hub.ready_set(None, Reach::everywhere(), Some("one".into()), None, None, None, None, None).await).await,
        vec!["ONE-2"]
    );
}

#[tokio::test]
async fn dangling_blocker_excludes_from_ready_set_and_is_marked() {
    let hub = hub();
    one(hub
        .create(None, Reach::everywhere(), "DAG".into(), "c".into(), Some("scope".into()), None, Some(vec!["DAG-99".into()]), None, None, None)
        .await)
    .await;
    one(hub.set_status(None, Reach::everywhere(), "DAG-1".into(), "Ready".into(), None, None, None).await).await;

    assert!(ticket_ids(hub.ready_set(None, Reach::everywhere(), None, None, None, None, None, None).await).await.is_empty());

    let evs: Vec<_> = Box::pin(hub.blockers(None, Reach::everywhere(), "DAG-1".into()).await).collect().await;
    assert_eq!(evs.len(), 1);
    match &evs[0] {
        TicketsEvent::DanglingBlocker { id } => assert_eq!(id, "DAG-99"),
        other => panic!("expected DanglingBlocker, got {other:?}"),
    }
}

#[tokio::test]
async fn blockers_resolve_to_current_statuses_in_order() {
    let hub = hub();
    one(hub.create(None, Reach::everywhere(), "DAG".into(), "a".into(), Some("scope".into()), None, None, None, None, None).await).await;
    one(hub.create(None, Reach::everywhere(), "DAG".into(), "b".into(), None, None, None, None, None, None).await).await;
    one(hub
        .create(None, Reach::everywhere(), 
            "DAG".into(),
            "c".into(),
            None,
            None,
            Some(vec!["DAG-1".into(), "DAG-2".into(), "DAG-99".into()]),
            None, None, None,
        )
        .await)
    .await;
    one(hub.set_status(None, Reach::everywhere(), "DAG-1".into(), "Complete".into(), None, None, None).await).await;

    let evs: Vec<_> = Box::pin(hub.blockers(None, Reach::everywhere(), "DAG-3".into()).await).collect().await;
    assert_eq!(evs.len(), 3);
    match &evs[0] {
        TicketsEvent::Blocker { ticket } => {
            assert_eq!(ticket.id, "DAG-1");
            assert_eq!(ticket.status, Status::Complete);
        }
        other => panic!("expected Blocker, got {other:?}"),
    }
    match &evs[1] {
        TicketsEvent::Blocker { ticket } => {
            assert_eq!(ticket.id, "DAG-2");
            assert_eq!(ticket.status, Status::Pending);
        }
        other => panic!("expected Blocker, got {other:?}"),
    }
    match &evs[2] {
        TicketsEvent::DanglingBlocker { id } => assert_eq!(id, "DAG-99"),
        other => panic!("expected DanglingBlocker, got {other:?}"),
    }

    assert!(matches!(
        one(hub.blockers(None, Reach::everywhere(), "DAG-42".into()).await).await,
        TicketsEvent::NotFound { .. }
    ));
}

#[tokio::test]
async fn create_adds_new_id_to_blockers_unlocks_and_watch_carries_it() {
    let hub = hub();
    one(hub.create(None, Reach::everywhere(), "DAG".into(), "a".into(), Some("scope".into()), None, None, None, None, None).await).await;
    let mut watch = Box::pin(untransitioned(hub.watch(None, Reach::everywhere(), None, None, None, None).await));

    one(hub
        .create(None, Reach::everywhere(), "DAG".into(), "b".into(), None, None, Some(vec!["DAG-1".into()]), None, None, None)
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

    match one(hub.get(None, Reach::everywhere(), "DAG-1".into()).await).await {
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
    one(hub.create(None, Reach::everywhere(), "FAM".into(), "P".into(), Some("scope".into()), None, None, None, None, None).await).await;
    let mut watch = Box::pin(untransitioned(hub.watch(None, Reach::everywhere(), None, None, None, None).await));

    match one(
        hub.create(None, Reach::everywhere(), 
            "FAM".into(),
            "C".into(),
            Some("execution".into()),
            None,
            None,
            None,
            Some("fam-1".into()), None,
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

    match one(hub.get(None, Reach::everywhere(), "FAM-1".into()).await).await {
        TicketsEvent::Ticket { ticket } => {
            assert_eq!(ticket.child_count, 1);
            assert_eq!(ticket.parent, None);
        }
        other => panic!("expected Ticket, got {other:?}"),
    }
    assert_eq!(
        ticket_ids(hub.list(None, Reach::everywhere(), None, None, Some("FAM-1".into()), None, None, None, None, None, None, None).await).await,
        vec!["FAM-2"]
    );
    match one(hub.get(None, Reach::everywhere(), "FAM-2".into()).await).await {
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
        hub.create(None, Reach::everywhere(), 
            "ORPH".into(),
            "no such parent".into(),
            Some("scope".into()),
            None,
            None,
            None,
            Some("ORPH-9".into()), None,
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
        one(hub.get(None, Reach::everywhere(), "ORPH-1".into()).await).await,
        TicketsEvent::NotFound { .. }
    ));
}

/// AGS-19 acceptance 4: `list {parent: P}` yields exactly P's
/// children, in (epic, seq) order.
#[tokio::test]
async fn list_filters_by_parent() {
    let hub = hub();
    one(hub.create(None, Reach::everywhere(), "LP".into(), "P".into(), Some("scope".into()), None, None, None, None, None).await).await;
    one(hub.create(None, Reach::everywhere(), "LP".into(), "unrelated".into(), None, None, None, None, None, None).await).await;
    // ZZ's first ticket is its scope root (MNG-12), filed under LP-1.
    for (epic, title, kind) in [("LP", "child a", None), ("ZZ", "child b", Some("scope"))] {
        one(hub
            .create(None, Reach::everywhere(), 
                epic.into(),
                title.into(),
                kind.map(String::from),
                None,
                None,
                None,
                Some("LP-1".into()), None,
            )
            .await)
        .await;
    }

    // MNG-14: "unrelated" (LP-2) named no parent and hangs under the root.
    assert_eq!(
        ticket_ids(hub.list(None, Reach::everywhere(), None, None, Some("LP-1".into()), None, None, None, None, None, None, None).await).await,
        vec!["LP-2", "LP-3", "ZZ-1"]
    );
    assert!(ticket_ids(hub.list(None, Reach::everywhere(), None, None, Some("LP-2".into()), None, None, None, None, None, None, None).await)
        .await
        .is_empty());
    assert_eq!(
        ticket_ids(hub.list(None, Reach::everywhere(), None, None, None, None, None, None, None, None, None, None).await).await.len(),
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
        .create(None, Reach::everywhere(), 
            "SUM".into(),
            "with a body".into(),
            Some("scope".into()), // MNG-12: a prefix's first ticket is its scope
            Some("High".into()),
            None,
            Some("## Problem\n\nthe weight of a list".into()),
            None, None,
        )
        .await)
    .await;

    let evs: Vec<_> = Box::pin(hub.list(None, Reach::everywhere(), None, None, None, None, None, Some(true), None, None, None, None).await)
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
    let evs: Vec<_> = Box::pin(hub.list(None, Reach::everywhere(), None, None, None, None, None, None, None, None, None, None).await)
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
    one(hub.create(None, Reach::everywhere(), "CUR".into(), "a".into(), None, None, None, None, None, None).await).await;

    for evs in [
        Box::pin(hub.list(None, Reach::everywhere(), None, None, None, Some("ZZZ-9".into()), None, None, None, None, None, None).await)
            .collect::<Vec<_>>()
            .await,
        Box::pin(hub.ready_set(None, Reach::everywhere(), None, Some("ZZZ-9".into()), None, None, None, None).await)
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

/// MNG-9: `ready_set` takes the same three params, and its page event
/// counts the ready set — not the store.
#[tokio::test]
async fn ready_set_takes_the_same_page_params() {
    let hub = hub();
    for i in 0..3 {
        let kind = (i == 0).then(|| "scope".to_string()); // MNG-12: the root first
        one(hub.create(None, Reach::everywhere(), "RS".into(), "r".into(), kind, None, None, None, None, None).await).await;
    }
    one(hub.create(None, Reach::everywhere(), "RS".into(), "still pending".into(), None, None, None, None, None, None).await).await;
    for id in ["RS-1", "RS-2", "RS-3"] {
        one(hub.set_status(None, Reach::everywhere(), id.into(), "Ready".into(), None, None, None).await).await;
    }

    let (ids, page) = ids_and_page(hub.ready_set(None, Reach::everywhere(), None, None, Some(2), Some(true), None, None).await).await;
    assert_eq!(ids, vec!["RS-1", "RS-2"]);
    assert_eq!(
        page,
        Some((Some("RS-2".into()), 1)),
        "one ready ticket left, not two store rows"
    );

    let (ids, page) =
        ids_and_page(hub.ready_set(None, Reach::everywhere(), None, Some("RS-2".into()), Some(2), Some(true), None, None).await).await;
    assert_eq!(ids, vec!["RS-3"]);
    assert_eq!(page, Some((None, 0)));
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
        let kind = (i == 1).then(|| "scope".to_string()); // MNG-12: the root first
        one(hub.create(None, Reach::everywhere(), "FND".into(), format!("row {i}"), kind, None, None, Some("a body".into()), None, None).await).await;
    }
    one(hub.create(None, Reach::everywhere(), "OTH".into(), "The Ticket Page".into(), Some("scope".into()), None, None, None, None, None).await).await;

    let evs: Vec<TicketsEvent> = Box::pin(hub.find(None, Reach::everywhere(), "".into(), None, None, None, None).await).collect().await;
    assert_eq!(evs.len(), 51, "50 rows and the page");
    let row = serde_json::to_value(&evs[0]).unwrap();
    assert_eq!((row["type"].as_str(), row["summary"]["id"].as_str()), (Some("summary"), Some("FND-1")));
    assert!(row["summary"].get("body").is_none(), "a summary carries no body: {row}");
    let (ids, page) = ids_and_page(hub.find(None, Reach::everywhere(), "".into(), None, None, None, None).await).await;
    assert_eq!(ids.len(), 50, "the palette's 50 when no limit is asked");
    assert_eq!(page, Some((Some("FND-50".into()), 11)));

    let (ids, page) = ids_and_page(hub.find(None, Reach::everywhere(), "  fnd-5 ".into(), None, None, None, None).await).await;
    let expected: Vec<String> =
        std::iter::once(5).chain(50..=59).map(|n| format!("FND-{n}")).collect();
    assert_eq!(ids, expected, "trimmed, case-folded, in (epic, seq) order");
    assert_eq!(page, Some((None, 0)));

    let (ids, page) =
        ids_and_page(hub.find(None, Reach::everywhere(), "ticket page".into(), Some("fnd-3".into()), Some(1), None, None).await).await;
    assert_eq!((ids, page), (vec!["OTH-1".to_string()], Some((None, 0))));

    let evs: Vec<TicketsEvent> = Box::pin(hub.find(None, Reach::everywhere(), "row".into(), Some("ZZZ-9".into()), None, None, None).await)
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
        .create(None, Reach::everywhere(), "DAG".into(), "d1".into(), Some("scope".into()), None, Some(vec!["DAG-2".into()]), None, None, None)
        .await)
    .await;
    // The ticket that becomes DAG-2 has DAG-1 in unlocks from birth.
    match one(hub.create(None, Reach::everywhere(), "DAG".into(), "d2".into(), None, None, None, None, None, None).await).await {
        TicketsEvent::Created { ticket } => {
            assert_eq!(ticket.id, "DAG-2");
            assert_eq!(ticket.unlocks, vec!["DAG-1"]);
        }
        other => panic!("expected Created, got {other:?}"),
    }
    match one(hub.get(None, Reach::everywhere(), "DAG-2".into()).await).await {
        TicketsEvent::Ticket { ticket } => assert_eq!(ticket.unlocks, vec!["DAG-1"]),
        other => panic!("expected Ticket, got {other:?}"),
    }
}

#[tokio::test]
async fn create_rejects_self_reference() {
    let hub = hub();
    // First create in the epic allocates DAG-1; naming it is a self-cycle.
    let ev = one(hub
        .create(None, Reach::everywhere(), "DAG".into(), "me".into(), Some("scope".into()), None, Some(vec!["DAG-1".into()]), None, None, None)
        .await)
    .await;
    match ev {
        TicketsEvent::Error { message } => assert!(message.contains("cycle"), "{message}"),
        other => panic!("expected Error, got {other:?}"),
    }
    assert!(matches!(
        one(hub.get(None, Reach::everywhere(), "DAG-1".into()).await).await,
        TicketsEvent::NotFound { .. }
    ));
}

#[tokio::test]
async fn create_rejects_cycle_through_dangling_forward_reference() {
    let hub = hub();
    // DAG-1 blocked_by the not-yet-existing DAG-2.
    one(hub
        .create(None, Reach::everywhere(), "DAG".into(), "d1".into(), Some("scope".into()), None, Some(vec!["DAG-2".into()]), None, None, None)
        .await)
    .await;
    // The create that would become DAG-2, blocked_by DAG-1, closes the loop.
    let ev = one(hub
        .create(None, Reach::everywhere(), "DAG".into(), "d2".into(), None, None, Some(vec!["DAG-1".into()]), None, None, None)
        .await)
    .await;
    match ev {
        TicketsEvent::Error { message } => assert!(message.contains("cycle"), "{message}"),
        other => panic!("expected Error, got {other:?}"),
    }
    // Nothing was stored.
    assert!(matches!(
        one(hub.get(None, Reach::everywhere(), "DAG-2".into()).await).await,
        TicketsEvent::NotFound { .. }
    ));
}

#[tokio::test]
async fn create_rejects_longer_cycles() {
    let hub = hub();
    one(hub
        .create(None, Reach::everywhere(), "DAG".into(), "p1".into(), Some("scope".into()), None, Some(vec!["DAG-3".into()]), None, None, None)
        .await)
    .await;
    one(hub
        .create(None, Reach::everywhere(), "DAG".into(), "p2".into(), None, None, Some(vec!["DAG-1".into()]), None, None, None)
        .await)
    .await;
    // Would become DAG-3: DAG-3 -> DAG-2 -> DAG-1 -> DAG-3.
    let ev = one(hub
        .create(None, Reach::everywhere(), "DAG".into(), "p3".into(), None, None, Some(vec!["DAG-2".into()]), None, None, None)
        .await)
    .await;
    assert!(matches!(ev, TicketsEvent::Error { .. }), "got {ev:?}");
    assert!(matches!(
        one(hub.get(None, Reach::everywhere(), "DAG-3".into()).await).await,
        TicketsEvent::NotFound { .. }
    ));
}

#[tokio::test]
async fn watch_epic_filter_drops_other_epics() {
    let hub = hub();
    let mut watch = Box::pin(hub.watch(None, Reach::everywhere(), Some("mng".into()), None, None, None).await);

    one(hub.create(None, Reach::everywhere(), "OTHER".into(), "x".into(), Some("scope".into()), None, None, None, None, None).await).await;
    one(hub.create(None, Reach::everywhere(), "MNG".into(), "y".into(), Some("scope".into()), None, None, None, None, None).await).await;

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
    one(hub.create(None, Reach::everywhere(), "LAG".into(), "root".into(), Some("scope".into()), None, None, None, None, None).await).await;
    let mut watch = Box::pin(hub.watch(None, Reach::everywhere(), None, None, None, None).await);
    let mut other = Box::pin(hub.watch(None, Reach::everywhere(), Some("other".into()), None, None, None).await);

    let mut broadcast: Vec<String> = Vec::new();
    for k in 2..=CREATES + 1 {
        match one(hub.create(None, Reach::everywhere(), "LAG".into(), format!("t{k}"), None, None, None, None, None, None).await).await {
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

    one(hub.create(None, Reach::everywhere(), "LAG".into(), "after".into(), None, None, None, None, None, None).await).await;
    assert_eq!(label(&watch.next().await.expect("an event")), format!("created LAG-{}", CREATES + 2), "and on, as usual");

    one(hub.create(None, Reach::everywhere(), "OTHER".into(), "its own".into(), Some("scope".into()), None, None, None, None, None).await).await;
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
    one(hub.create(None, Reach::everywhere(), "SP".into(), "root".into(), Some("scope".into()), None, None, None, None, None).await).await;
    one(hub.create(None, Reach::everywhere(), "SP".into(), "a".into(), None, None, None, None, None, None).await).await; // SP-2 → SP-1
    one(hub.create(None, Reach::everywhere(), "SP".into(), "b".into(), None, None, None, None, None, None).await).await; // SP-3 → SP-1
    one(hub.create(None, Reach::everywhere(), "OT".into(), "other".into(), Some("scope".into()), None, None, None, None, None).await).await;
    let mut watch = Box::pin(hub.watch(None, Reach::everywhere(), None, None, None, None).await);

    match one(hub.set_parent(None, Reach::everywhere(), "sp-3".into(), "sp-2".into()).await).await {
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
        one(hub.set_parent(None, Reach::everywhere(), "SP-9".into(), "SP-1".into()).await).await,
        TicketsEvent::NotFound { .. }
    ));
    match one(hub.set_parent(None, Reach::everywhere(), "SP-2".into(), "SP-9".into()).await).await {
        TicketsEvent::Error { message } => {
            assert!(message.contains("SP-9") && message.contains("must exist"), "{message}")
        }
        other => panic!("expected Error, got {other:?}"),
    }
    match one(hub.set_parent(None, Reach::everywhere(), "SP-2".into(), "OT-1".into()).await).await {
        TicketsEvent::Error { message } => {
            assert!(message.contains("OT-1") && message.contains("outside"), "{message}")
        }
        other => panic!("expected Error, got {other:?}"),
    }
    match one(hub.set_parent(None, Reach::everywhere(), "SP-1".into(), "SP-3".into()).await).await {
        TicketsEvent::Error { message } => assert!(message.contains("own ancestor"), "{message}"),
        other => panic!("expected Error, got {other:?}"),
    }
    // The root may be hosted by another shortcode.
    match one(hub.set_parent(None, Reach::everywhere(), "SP-1".into(), "OT-1".into()).await).await {
        TicketsEvent::ParentChanged { ticket, .. } => assert_eq!(ticket.parent.as_deref(), Some("OT-1")),
        other => panic!("expected ParentChanged, got {other:?}"),
    }
}

/// MNG-12: the first ticket of a prefix must be a scope — over the
/// activation the refusal is the in-band error naming the rule.
#[tokio::test]
async fn first_ticket_of_a_prefix_must_be_a_scope_in_band() {
    let hub = hub();
    match one(hub.create(None, Reach::everywhere(), "FRESH".into(), "a build".into(), None, None, None, None, None, None).await).await {
        TicketsEvent::Error { message } => {
            assert!(message.contains("FRESH") && message.contains("scope"), "{message}");
        }
        other => panic!("expected Error, got {other:?}"),
    }
    assert!(matches!(
        one(hub.get(None, Reach::everywhere(), "FRESH-1".into()).await).await,
        TicketsEvent::NotFound { .. }
    ));
}

/// The `waves` stream, split the way the contract reads it: one
/// `(id, placement, waiting_on)` per `placed`, then the one closing
/// `wave_counts` — asserted last and alone.
#[allow(clippy::type_complexity)]
async fn placed_and_counts(
    s: impl Stream<Item = TicketsEvent> + Send + 'static,
) -> (Vec<(String, crate::types::Placement, Vec<String>)>, (Vec<u64>, u64, u64)) {
    let evs: Vec<TicketsEvent> = Box::pin(s).collect().await;
    let mut placed = Vec::new();
    let mut counts = None;
    for ev in evs {
        assert!(counts.is_none(), "wave_counts is last: {ev:?}");
        match ev {
            TicketsEvent::Placed { summary, placement, waiting_on } => {
                placed.push((summary.id, placement, waiting_on))
            }
            TicketsEvent::WaveCounts { waves, missing, done } => counts = Some((waves, missing, done)),
            other => panic!("expected placed or wave_counts, got {other:?}"),
        }
    }
    (placed, counts.expect("every waves answer ends with wave_counts"))
}

/// WAVE-4 acceptance 2: the yield order exactly as specified — waves
/// ascending; inside a wave Coding, In Review, Ready, Pending, Blocked,
/// Idea, Epic, then (epic, seq); then the missing lane; then the done,
/// each in (epic, seq) — with the default kinds (the scope roots are not
/// work), a blocker followed into a shortcode not asked for, and the
/// tally after the rows, exactly once.
#[tokio::test]
async fn waves_yields_placements_in_the_specified_order_then_one_wave_counts() {
    use crate::types::Placement::{Done, MissingBlocker, Wave};
    let hub = hub();
    let create = |epic: &str, kind: Option<&str>, blocked_by: &[&str]| {
        let blocked_by = (!blocked_by.is_empty()).then(|| blocked_by.iter().map(|b| b.to_string()).collect());
        let (epic, kind) = (epic.to_string(), kind.map(String::from));
        let hub = hub.clone();
        async move { one(hub.create(None, Reach::everywhere(), epic, "t".into(), kind, None, blocked_by, None, None, None).await).await }
    };
    let status = |id: &str, s: &str| {
        let (id, s, hub) = (id.to_string(), s.to_string(), hub.clone());
        async move { one(hub.set_status(None, Reach::everywhere(), id, s, None, None, None).await).await }
    };
    for epic in ["WA", "WB", "ELSE"] {
        create(epic, Some("scope"), &[]).await; // X-1, each shortcode's root
    }
    create("ELSE", None, &[]).await; // ELSE-2, wave 1 — not asked for
    create("WA", None, &["ELSE-2"]).await; // WA-2: wave 2, Pending
    create("WB", Some("spike"), &[]).await; // WB-2: wave 1, Pending
    create("WA", Some("gap"), &[]).await; // WA-3: wave 1, Ready
    status("WA-3", "Ready").await;
    create("WB", None, &[]).await; // WB-3: wave 1, Coding
    status("WB-3", "active").await;
    create("WA", None, &["WB-3"]).await; // WA-4: wave 2, In Review
    status("WA-4", "in review").await;
    create("WB", None, &["WB-9"]).await; // WB-4: missing (WB-9 is no ticket)
    create("WA", None, &["WB-4"]).await; // WA-5: missing, behind WB-4
    create("WA", None, &[]).await; // WA-6: done
    status("WA-6", "Complete").await;
    create("WA", Some("execution"), &[]).await; // WA-7: not work, not yielded
    create("WB", None, &[]).await; // WB-5: wave 1, Blocked (a marker, not an edge)
    status("WB-5", "Blocked").await;

    let (placed, counts) = placed_and_counts(hub.waves(None, Reach::everywhere(), vec!["wa".into(), "WB".into()], None).await).await;
    let want: Vec<(&str, crate::types::Placement, Vec<&str>)> = vec![
        ("WB-3", Wave { n: 1 }, vec![]),
        ("WA-3", Wave { n: 1 }, vec![]),
        ("WB-2", Wave { n: 1 }, vec![]),
        ("WB-5", Wave { n: 1 }, vec![]),
        ("WA-4", Wave { n: 2 }, vec!["WB-3"]),
        ("WA-2", Wave { n: 2 }, vec!["ELSE-2"]),
        ("WA-5", MissingBlocker, vec!["WB-4"]),
        ("WB-4", MissingBlocker, vec!["WB-9"]),
        ("WA-6", Done, vec![]),
    ];
    let got: Vec<(&str, crate::types::Placement, Vec<&str>)> = placed
        .iter()
        .map(|(id, p, w)| (id.as_str(), *p, w.iter().map(String::as_str).collect()))
        .collect();
    assert_eq!(got, want);
    assert_eq!(counts, (vec![4, 2], 2, 1));

    // `kinds` names what is yielded: the scopes and the execution too.
    let (placed, counts) = placed_and_counts(
        hub.waves(None, Reach::everywhere(), vec!["WA".into()], Some(vec!["scope".into(), "Execution".into()])).await,
    )
    .await;
    let ids: Vec<&str> = placed.iter().map(|(id, _, _)| id.as_str()).collect();
    assert_eq!(ids, ["WA-1", "WA-7"]);
    assert_eq!(counts, (vec![2], 0, 0));

    // A shortcode with no tickets: nothing placed, the tally all zero.
    let (placed, counts) = placed_and_counts(hub.waves(None, Reach::everywhere(), vec!["NONE".into()], None).await).await;
    assert!(placed.is_empty());
    assert_eq!(counts, (vec![], 0, 0));
}

/// WAVE-4 acceptance 2: the error cases are in-band — `epics` empty, and
/// a kind outside the six (the error `create` gives, naming them) — one
/// `Error` and nothing else, no tally.
#[tokio::test]
async fn waves_errors_are_in_band_and_alone() {
    let hub = hub();
    one(hub.create(None, Reach::everywhere(), "ER".into(), "root".into(), Some("scope".into()), None, None, None, None, None).await).await;

    let evs: Vec<TicketsEvent> = Box::pin(hub.waves(None, Reach::everywhere(), vec![], None).await).collect().await;
    assert_eq!(evs.len(), 1, "{evs:?}");
    match &evs[0] {
        TicketsEvent::Error { message } => assert!(message.contains("epics"), "{message}"),
        other => panic!("expected Error, got {other:?}"),
    }

    let evs: Vec<TicketsEvent> =
        Box::pin(hub.waves(None, Reach::everywhere(), vec!["ER".into()], Some(vec!["build".into(), "bug".into()])).await).collect().await;
    assert_eq!(evs.len(), 1, "{evs:?}");
    match &evs[0] {
        TicketsEvent::Error { message } => {
            assert!(message.contains("'bug'"), "{message}");
            for k in Kind::ALL {
                assert!(message.contains(k.as_str()), "names {k}: {message}");
            }
        }
        other => panic!("expected Error, got {other:?}"),
    }
}
