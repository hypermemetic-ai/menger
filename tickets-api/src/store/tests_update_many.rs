//! ACT-2: `update_many` — every row of the ticket's table: `update`'s change
//! to each by `update`'s rules, in one write; any refusal writing nothing
//! and naming each refused id with its reasons; a filter's tickets at the
//! write, capped; a parent cycle among the moved refused for the tickets
//! involved; the message on every transition with one shared cause; the
//! events (each ticket's own, then one `updated_many`); the caller's `may`
//! refusing the whole write; a ticket nothing changes for reported
//! unchanged; `label` and `link_commits` over many ids, all or none; and
//! `--verify-derived` consistent after a mixed write.

use super::tests_update::assert_kept;
use super::*;

fn new(epic: &str, title: &str, kind: Kind, parent: Option<&str>) -> NewTicket {
    NewTicket {
        path: None,
        epic: epic.into(),
        title: title.into(),
        kind,
        severity: None,
        blocked_by: Vec::new(),
        parent: parent.map(Into::into),
        body: format!("the body of {title}"),
    }
}

/// `M`: M-1 its scope root, M-2 … M-6 builds under it, M-7 a doc, M-8 a
/// scope under the root with M-9 a build under it; `N`: N-1 its root, N-2 a
/// build.
async fn fixture(store: &Store) {
    store.create(new("M", "the root", Kind::Scope, None)).await.unwrap();
    for i in 2..=6 {
        store.create(new("M", &format!("build {i}"), Kind::Build, None)).await.unwrap();
    }
    store.create(new("M", "a doc", Kind::Doc, None)).await.unwrap();
    store.create(new("M", "an area", Kind::Scope, Some("M-1"))).await.unwrap();
    store.create(new("M", "nine", Kind::Build, Some("M-8"))).await.unwrap();
    store.create(new("N", "n root", Kind::Scope, None)).await.unwrap();
    store.create(new("N", "n two", Kind::Build, None)).await.unwrap();
}

async fn store() -> Store {
    let store = Store::new();
    fixture(&store).await;
    store
}

fn ids(ids: &[&str]) -> Selection {
    Selection::Ids(ids.iter().map(|s| s.to_string()).collect())
}

fn drained(rx: &mut broadcast::Receiver<Change>) -> Vec<Change> {
    let mut out = Vec::new();
    while let Ok(change) = rx.try_recv() {
        out.push(change);
    }
    out
}

fn written(store: &Store) -> u64 {
    store.tickets.read().unwrap().written
}

async fn status_of(store: &Store, id: &str) -> Status {
    store.get(id).await.unwrap().status
}

fn status(s: Status) -> Update {
    Update { status: Some(s), ..Update::default() }
}

#[tokio::test]
async fn each_ticket_gets_updates_change_in_one_write() {
    let store = store().await;
    let mut rx = store.subscribe_filtered();
    let before = written(&store);
    let u = Update { status: Some(Status::Ready), severity: Some(Some(Severity::High)), ..Update::default() };
    let done = store.update_many(ids(&["m-2", "M-3", "M-4", "M-3"]), u).await.unwrap();
    assert_eq!(written(&store), before + 1, "one write");
    assert_eq!(done.ids, ["M-2", "M-3", "M-4"], "each once, in the order named");
    assert_eq!(done.changed, ["status", "severity"]);
    assert!(done.unchanged.is_empty());
    assert_eq!(done.tickets.iter().map(|t| (t.id.as_str(), t.status, t.severity)).collect::<Vec<_>>(), [
        ("M-2", Status::Ready, Some(Severity::High)),
        ("M-3", Status::Ready, Some(Severity::High)),
        ("M-4", Status::Ready, Some(Severity::High)),
    ]);
    let sent = drained(&mut rx);
    // Each ticket's own events, then one updated_many, then the cell moves.
    let last_own = sent.iter().rposition(|c| matches!(c, Change::SeverityChanged { .. })).unwrap();
    let many = sent.iter().position(|c| matches!(c, Change::UpdatedMany { .. })).unwrap();
    assert_eq!(many, last_own + 1, "{sent:?}");
    assert_eq!(sent.iter().filter(|c| matches!(c, Change::UpdatedMany { .. })).count(), 1);
    assert!(matches!(&sent[many], Change::UpdatedMany { ids, changed } if ids.len() == 3 && changed == &["status", "severity"]));
    assert!(sent[many + 1..].iter().all(|c| matches!(c, Change::CellsMoved { .. } | Change::UnderMoved { .. })));
    assert_eq!(sent.iter().filter(|c| matches!(c, Change::StatusChanged { .. })).count(), 3);
    assert_kept(&store, "a status and a severity to three");
}

#[tokio::test]
async fn any_refusal_writes_nothing_and_names_each_refused_id() {
    let store = store().await;
    let before = written(&store);
    // M-7 is a doc (no status), M-40 names nothing; M-2 alone would land.
    let err = store.update_many(ids(&["M-2", "M-7", "M-40"]), status(Status::Ready)).await.unwrap_err();
    match &err {
        StoreError::RefusedMany { refusals } => {
            let refused: Vec<&str> = refusals.iter().map(|(id, _)| id.as_str()).collect();
            assert_eq!(refused, ["M-7", "M-40"]);
            assert!(matches!(refusals[0].1[0], StoreError::DocHasNoLifecycle { .. }), "{err}");
            assert!(matches!(refusals[1].1[0], StoreError::NotFound(_)), "{err}");
        }
        other => panic!("{other:?}"),
    }
    let said = err.to_string();
    assert!(said.starts_with("2 tickets refused, nothing written") && said.contains("M-7: ") && said.contains("M-40: "), "{said}");
    assert_eq!(written(&store), before, "nothing written");
    assert_eq!(status_of(&store, "M-2").await, Status::Pending, "not even the one that would land");
    // Several reasons for one ticket are each named.
    let u = Update { status: Some(Status::Doc), block: vec!["M-7".into()], ..Update::default() };
    match store.update_many(ids(&["M-2"]), u).await.unwrap_err() {
        StoreError::RefusedMany { refusals } => assert_eq!(refusals[0].1.len(), 2, "{refusals:?}"),
        other => panic!("{other:?}"),
    }
    // A title or a body is each ticket's own.
    let titled = Update { title: Some("one for all".into()), ..Update::default() };
    assert!(matches!(store.update_many(ids(&["M-2", "M-3"]), titled).await, Err(StoreError::Invalid(_))));
    assert_eq!(written(&store), before);
}

#[tokio::test]
async fn a_filter_takes_the_tickets_it_admits_at_the_write_capped() {
    let store = store().await;
    store.update_many(ids(&["M-2", "M-3"]), Update { label: vec!["stale".into()], ..Update::default() }).await.unwrap();
    let lens = Lens::parse("label stale").unwrap();
    let done = store.update_many(Selection::Filter { lens: lens.clone(), max: 500 }, status(Status::Ready)).await.unwrap();
    assert_eq!(done.ids, ["M-2", "M-3"]);
    // Over the cap: refused, naming the count; nothing written.
    let before = written(&store);
    let err = store.update_many(Selection::Filter { lens: Lens::parse("in M").unwrap(), max: 3 }, status(Status::Ready)).await;
    match err {
        Err(StoreError::Invalid(why)) => assert!(why.contains("admits 9 tickets") && why.contains("3"), "{why}"),
        other => panic!("{other:?}"),
    }
    assert_eq!(written(&store), before);
    // Admitting nothing is no write.
    let none = store.update_many(Selection::Filter { lens: Lens::parse("label gone").unwrap(), max: 5 }, status(Status::Ready)).await.unwrap();
    assert!(none.ids.is_empty() && none.tickets.is_empty());
    assert_eq!(written(&store), before);
}

#[tokio::test]
async fn a_parent_cycle_among_the_moved_is_refused_for_the_tickets_involved() {
    let store = store().await;
    let before = written(&store);
    // M-9 under M-9 is itself; M-8 under M-9 is under its own child.
    let err = store.update_many(ids(&["M-2", "M-8", "M-9"]), Update { parent: Some("M-9".into()), ..Update::default() }).await.unwrap_err();
    match err {
        StoreError::RefusedMany { refusals } => {
            assert_eq!(refusals.iter().map(|(id, _)| id.as_str()).collect::<Vec<_>>(), ["M-8", "M-9"]);
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(written(&store), before);
    // Moved together, without the cycle, they land.
    let done = store.update_many(ids(&["M-2", "M-3", "N-2"]), Update { parent: Some("M-8".into()), ..Update::default() }).await;
    // N-2 is another shortcode's ticket: set_parent's rule refuses it.
    assert!(matches!(done, Err(StoreError::RefusedMany { ref refusals }) if refusals.len() == 1 && refusals[0].0 == "N-2"), "{done:?}");
    let done = store.update_many(ids(&["M-2", "M-3"]), Update { parent: Some("M-8".into()), ..Update::default() }).await.unwrap();
    assert_eq!(done.changed, ["parent"]);
    assert_eq!(store.get("M-8").await.unwrap().child_count, 3);
    // A blocker that waits on one of them is a cycle for that one.
    store.update("M-5", Update { block: vec!["M-4".into()], ..Update::default() }).await.unwrap();
    match store.update_many(ids(&["M-3", "M-4"]), Update { block: vec!["M-5".into()], ..Update::default() }).await.unwrap_err() {
        StoreError::RefusedMany { refusals } => assert_eq!(refusals.iter().map(|(id, _)| id.as_str()).collect::<Vec<_>>(), ["M-4"]),
        other => panic!("{other:?}"),
    }
    assert_kept(&store, "the moves");
}

#[tokio::test]
async fn the_message_is_on_every_transition_with_one_shared_cause() {
    let store = store().await;
    let act = Act { message: Some("swept".into()), ..Act::default() };
    store.update_many_by(ids(&["M-2", "M-3", "M-4"]), status(Status::Ready), act, |_| Ok(())).await.unwrap();
    let first = store.history("M-2").unwrap().own.pop().unwrap();
    assert_eq!((first.message.as_deref(), first.cause), (Some("swept"), None));
    for id in ["M-3", "M-4"] {
        let t = store.history(id).unwrap().own.pop().unwrap();
        assert_eq!(t.message.as_deref(), Some("swept"), "{id}");
        assert_eq!((t.cause, t.cause_ticket.as_deref()), (Some(first.id), Some("M-2")), "{id}");
    }
    // The first's history names what it caused: the write.
    let caused: Vec<String> = store.history("M-2").unwrap().caused.into_iter().map(|t| t.ticket).collect();
    assert_eq!(caused, ["M-3", "M-4"]);
}

#[tokio::test]
async fn a_cancel_cascading_into_a_selected_ticket_moves_it_once() {
    let store = store().await;
    let done = store.update_many(ids(&["M-8", "M-9"]), status(Status::Canceled)).await.unwrap();
    assert_eq!(done.ids, ["M-8", "M-9"]);
    assert_eq!(status_of(&store, "M-9").await, Status::Canceled);
    // M-9 was canceled by M-8's cascade, once.
    let own = store.history("M-9").unwrap().own;
    assert_eq!(own.iter().filter(|t| t.to == "Canceled").count(), 1, "{own:?}");
    assert_kept(&store, "a cancel over a parent and its child");
}

#[tokio::test]
async fn may_is_asked_of_the_whole_selection_and_refuses_it() {
    let store = store().await;
    let before = written(&store);
    let asked = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let seen = asked.clone();
    let may = move |ids: &[String]| {
        seen.lock().unwrap().extend(ids.iter().cloned());
        if ids.iter().any(|i| i.starts_with("N-")) { Err("needs tickets.update_many on N for N-2".to_string()) } else { Ok(()) }
    };
    let err = store.update_many_by(ids(&["M-2", "N-2"]), status(Status::Ready), Act::default(), may).await.unwrap_err();
    assert!(err.to_string().contains("N-2"), "{err}");
    assert_eq!(*asked.lock().unwrap(), ["M-2", "N-2"]);
    assert_eq!(written(&store), before);
    assert_eq!(status_of(&store, "M-2").await, Status::Pending);
}

#[tokio::test]
async fn a_ticket_nothing_changes_for_is_reported_unchanged() {
    let store = store().await;
    store.set_status("M-3", Status::Ready).await.unwrap();
    let mut rx = store.subscribe_filtered();
    let done = store.update_many(ids(&["M-2", "M-3"]), status(Status::Ready)).await.unwrap();
    assert_eq!((done.ids.as_slice(), done.unchanged.as_slice()), (&["M-2".to_string()][..], &["M-3".to_string()][..]));
    assert_eq!(done.tickets.len(), 2, "every ticket selected, as the write left it");
    let sent = drained(&mut rx);
    assert!(sent.iter().all(|c| !matches!(c, Change::StatusChanged { ticket, .. } if ticket.id == "M-3")));
    // Nothing changes for any: no write at all.
    let before = written(&store);
    let none = store.update_many(ids(&["M-2", "M-3"]), status(Status::Ready)).await.unwrap();
    assert!(none.ids.is_empty() && none.unchanged.len() == 2);
    assert_eq!(written(&store), before);
    assert!(drained(&mut rx).is_empty());
}

#[tokio::test]
async fn labels_move_with_update_many_and_with_label_over_many_ids() {
    let store = store().await;
    let done = store.update_many(ids(&["M-2", "M-3"]), Update { label: vec!["Needs-You".into()], ..Update::default() }).await.unwrap();
    assert_eq!(done.changed, ["labels"]);
    assert!(done.tickets.iter().all(|t| t.labels == ["needs-you"]));
    let before = written(&store);
    let changes = store.label_many(&["M-3".into(), "M-4".into()], vec!["stale".into()], vec!["needs-you".into()]).await.unwrap();
    assert_eq!(written(&store), before + 1, "one write");
    let said: Vec<(String, Vec<String>, Vec<String>)> = changes
        .into_iter()
        .map(|c| match c {
            Change::LabelsChanged { ticket, added, removed } => (ticket.id, added, removed),
            other => panic!("{other:?}"),
        })
        .collect();
    assert_eq!(said, [
        ("M-3".to_string(), vec!["stale".to_string()], vec!["needs-you".to_string()]),
        ("M-4".to_string(), vec!["stale".to_string()], vec![]),
    ]);
    // All or none: a bad label, or an id that names nothing, refuses all.
    let before = written(&store);
    assert!(matches!(store.label_many(&["M-2".into(), "M-40".into()], vec!["x".into()], vec![]).await, Err(StoreError::RefusedMany { .. })));
    assert!(store.label_many(&["M-2".into()], vec!["no spaces".into()], vec![]).await.is_err());
    assert_eq!(written(&store), before);
    assert!(store.get("M-2").await.unwrap().labels == ["needs-you"]);
    assert_kept(&store, "labels");
}

#[tokio::test]
async fn link_commits_over_many_ids_is_one_write_all_or_none() {
    let store = store().await;
    store.link_commits("M-3", vec!["77df9a0".into()]).await.unwrap();
    let before = written(&store);
    let changes = store.link_commits_many(&["M-2".into(), "M-3".into()], vec!["77df9a0".into()]).await.unwrap();
    assert_eq!(written(&store), before + 1);
    let added: Vec<usize> = changes.iter().map(|c| match c { Change::CommitsLinked { added, .. } => added.len(), _ => 9 }).collect();
    assert_eq!(added, [1, 0], "M-3 had it");
    let before = written(&store);
    assert!(store.link_commits_many(&["M-2".into(), "M-40".into()], vec!["c8c5678".into()]).await.is_err());
    assert!(store.link_commits_many(&["M-2".into()], vec!["not;a;hash".into()]).await.is_err());
    assert_eq!(written(&store), before);
    assert_eq!(store.get("M-2").await.unwrap().commits, ["77df9a0"]);
}

#[test]
fn updated_many_in_an_epic_is_its_ids_there() {
    let c = update_many::in_epic(vec!["M-2".into(), "N-2".into(), "MM-1".into()], vec!["status".into()], &ShortcodeId::from("M"));
    assert!(matches!(c, Some(Change::UpdatedMany { ref ids, .. }) if ids == &["M-2"]), "{c:?}");
    assert!(update_many::in_epic(vec!["N-2".into()], vec![], &ShortcodeId::from("M")).is_none());
}

/// A mixed write — a cancel that cascades, a parent, a blocker, a severity
/// and labels over tickets of two shortcodes' rows — leaves the file's
/// derived state what a scan gives.
#[tokio::test]
async fn verify_derived_is_consistent_after_a_mixed_write() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("many.db");
    let store = Store::open(&path).unwrap();
    fixture(&store).await;
    for i in 0..20 {
        store.create(new("M", &format!("more {i}"), Kind::Build, Some("M-8"))).await.unwrap();
    }
    let all: Vec<String> = (10..30).map(|i| format!("M-{i}")).collect();
    let u = Update {
        status: Some(Status::Ready),
        parent: Some("M-1".into()),
        severity: Some(Some(Severity::Low)),
        block: vec!["M-2".into()],
        label: vec!["swept".into()],
        ..Update::default()
    };
    let done = store.update_many(Selection::Ids(all.clone()), u).await.unwrap();
    assert_eq!(done.ids.len(), 20);
    assert_eq!(done.changed, ["status", "parent", "severity", "blocked_by", "labels"]);
    store.update_many(ids(&["M-8", "M-9", "M-3"]), status(Status::Canceled)).await.unwrap();
    store.update_many(Selection::Filter { lens: Lens::parse("label swept").unwrap(), max: 500 }, Update { unblock: vec!["M-2".into()], kind: Some(Kind::Gap), ..Update::default() }).await.unwrap();
    assert_kept(&store, "the mixed writes");
    let rows: Vec<Row> = store.tickets.read().unwrap().order.values().cloned().collect();
    drop(store);
    match Store::verify_derived(&path).unwrap() {
        Verdict::Consistent { .. } => {}
        other => panic!("{other:?}"),
    }
    let again = Store::open(&path).unwrap();
    let reread: Vec<Row> = again.tickets.read().unwrap().order.values().cloned().collect();
    assert_eq!(reread, rows, "the file holds what the cache held");
}
