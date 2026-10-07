//! SDLC-11: docs — the kind outside the lifecycle. A doc is born in `Doc`
//! and stays there; nothing moves it and nothing else is ever in it; it is
//! never blocked, never a blocker, never ready; a roll-up's `done/total`
//! ignores it and counts it apart, in `docs`; and everything else a ticket
//! has — amend, parenthood, commits, find, list, groups, extents, watch — a
//! doc has too. One test per row of the ticket's table, then the file: a
//! store with docs verifies and reopens as it was, and a file in the form
//! before this one gains the `docs` columns once, on its first open.

use super::stored::{self, State};
use super::tests_children::children;
use super::*;

fn new(epic: &str, kind: Kind, parent: Option<&str>, blocked_by: &[&str]) -> NewTicket {
    NewTicket {
        path: None,
        epic: epic.into(),
        title: format!("a {} in {epic}", kind.as_str()),
        kind,
        severity: None,
        blocked_by: blocked_by.iter().map(|b| b.to_string()).collect(),
        parent: parent.map(Into::into),
        body: format!("the body of a {} in {epic}", kind.as_str()),
    }
}

fn doc(epic: &str, parent: Option<&str>) -> NewTicket {
    new(epic, Kind::Doc, parent, &[])
}

fn work(epic: &str, parent: Option<&str>) -> NewTicket {
    new(epic, Kind::Build, parent, &[])
}

fn group(store: &Store, epic: &str) -> Group {
    store
        .groups()
        .into_iter()
        .find(|g| g.epic == epic)
        .unwrap_or_else(|| panic!("no group {epic}"))
}

fn ids<T: AsRef<str>>(v: impl IntoIterator<Item = T>) -> Vec<String> {
    v.into_iter().map(|s| s.as_ref().to_string()).collect()
}

/// Every `GroupChanged` waiting on `rx`, in the order sent.
fn group_changes(rx: &mut broadcast::Receiver<Change>) -> Vec<Group> {
    let mut out = Vec::new();
    while let Ok(change) = rx.try_recv() {
        if let Change::GroupChanged { group } = change {
            out.push(group);
        }
    }
    out
}

/// `create --kind doc` starts in `Doc`, not `Pending`, and a doc may be the
/// first ticket of a shortcode — a doc collection's root, which the
/// shortcode's row names as a scope root is named, which the tickets after
/// it hang under by default, and which may be hosted under another
/// shortcode (a sub-shortcode) and re-hosted, as a scope root may.
#[tokio::test]
async fn a_doc_is_born_in_doc_and_may_root_a_shortcode() {
    let store = Store::new();
    let root = store.create(doc("MAN", None)).await.unwrap();
    assert_eq!((root.id.as_str(), root.kind, root.status), ("MAN-1", Kind::Doc, Status::Doc));
    assert_eq!(root.parent, None, "a shortcode's root, top level");
    assert_eq!(
        store.groups(),
        vec![Group {
            epic: "MAN".into(),
            total: 0,
            done: 0,
            docs: 1,
            root: Some("MAN-1".into()),
            // AGS-23: the root's row, as the nav draws it — its `Doc` chip.
            root_row: Some(RootRow { title: "a doc in MAN".into(), status: Status::Doc, kind: Kind::Doc }),
            host: None,
        }],
        "a doc collection is a row: no work, one doc"
    );
    assert_eq!(store.groups(), tests::nav(&store));

    let child = store.create(doc("MAN", None)).await.unwrap();
    assert_eq!((child.status, child.parent.as_deref()), (Status::Doc, Some("MAN-1")), "under the root");
    let w = store.create(work("MAN", None)).await.unwrap();
    assert_eq!((w.status, w.parent.as_deref()), (Status::Pending, Some("MAN-1")), "work starts Pending");
    assert_eq!((group(&store, "MAN").total, group(&store, "MAN").docs), (1, 2));

    // In a scope's shortcode a doc is any child.
    store.create(new("SC", Kind::Scope, None, &[])).await.unwrap(); // SC-1
    let in_scope = store.create(doc("SC", None)).await.unwrap();
    assert_eq!((in_scope.id.as_str(), in_scope.status, in_scope.parent.as_deref()), ("SC-2", Status::Doc, Some("SC-1")));

    // A doc root hosted under another shortcode: a sub-shortcode.
    let sub = store.create(doc("SUBD", Some("SC-1"))).await.unwrap();
    assert_eq!((sub.id.as_str(), sub.status), ("SUBD-1", Status::Doc));
    store.create(work("SUBD", None)).await.unwrap(); // SUBD-2 → SUBD-1
    let g = group(&store, "SUBD");
    assert_eq!((g.total, g.docs, g.root.as_deref(), g.host.as_deref()), (1, 1, Some("SUBD-1"), Some("SC-1")));
    assert_eq!(store.groups(), tests::nav(&store));

    // The doc root may be re-hosted; a doc that is not the root may not leave.
    store.set_parent("SUBD-1", "MAN-3").await.unwrap();
    assert_eq!(group(&store, "SUBD").host.as_deref(), Some("MAN-3"));
    let g = group(&store, "MAN");
    assert_eq!((g.total, g.docs), (2, 3), "SUBD's work and doc count in MAN's row now");
    assert!(matches!(
        store.set_parent("MAN-2", "SC-1").await,
        Err(StoreError::ParentOutsideShortcode { .. })
    ));
    assert_eq!(store.groups(), tests::nav(&store));

    // Anything that is neither scope nor doc is still refused as a first ticket.
    for kind in [Kind::Execution, Kind::Build, Kind::Gap, Kind::Spike] {
        let err = store.create(new("NOPE", kind, None, &[])).await.unwrap_err();
        assert!(matches!(err, StoreError::FirstMustBeScope { .. }), "{kind}: {err}");
        assert!(err.to_string().contains("a scope or a doc"), "{err}");
    }
}

/// `--blocked-by` with a doc is refused: a doc has no lifecycle, so nothing
/// can hold it back. Nothing is allocated — the next create takes the id.
#[tokio::test]
async fn a_doc_takes_no_blocked_by_and_nothing_is_allocated() {
    let store = Store::new();
    store.create(new("B", Kind::Scope, None, &[])).await.unwrap(); // B-1
    store.create(work("B", None)).await.unwrap(); // B-2
    for blocked_by in [&["B-2"][..], &["NEVER-1"], &["B-1", "B-2"]] {
        let err = store.create(new("B", Kind::Doc, None, blocked_by)).await.unwrap_err();
        match &err {
            StoreError::DocBlocked { epic, blocked_by: named } => {
                assert_eq!((epic.as_str(), named), ("B", &ids(blocked_by)));
            }
            other => panic!("expected DocBlocked, got {other:?}"),
        }
        assert!(err.to_string().contains("a doc is never blocked"), "{err}");
    }
    assert_eq!(store.count(), 2, "nothing was created");
    assert_eq!(store.create(doc("B", None)).await.unwrap().id, "B-3", "nothing was allocated");
}

/// `set_status` on a doc is refused — every status, `Doc` included — as an
/// error that says a doc has no lifecycle; nothing is written (not even
/// `updated_unix`) and nothing is broadcast.
#[tokio::test]
async fn set_status_on_a_doc_is_refused_whatever_the_status() {
    let store = Store::new();
    store.create(doc("D", None)).await.unwrap(); // D-1
    store.create(doc("D", None)).await.unwrap(); // D-2
    let before = store.get("D-2").await.unwrap();
    let mut rx = store.subscribe();
    for status in Status::ALL {
        let err = store.set_status("D-2", status).await.unwrap_err();
        match &err {
            StoreError::DocHasNoLifecycle { id } => assert_eq!(id, "D-2"),
            other => panic!("{status}: expected DocHasNoLifecycle, got {other:?}"),
        }
        assert!(err.to_string().contains("a doc has no lifecycle"), "{err}");
    }
    assert_eq!(store.get("D-2").await.unwrap(), before, "nothing changed");
    assert!(rx.try_recv().is_err(), "nothing was broadcast");
}

/// `set_status … Doc` on any other kind is refused: `Doc` is a doc's own.
/// The ticket stays as it was, and every lifecycle status still sets.
#[tokio::test]
async fn doc_is_no_other_kinds_status() {
    let store = Store::new();
    store.create(new("W", Kind::Scope, None, &[])).await.unwrap(); // W-1
    let mut kinds = vec![("W-1".to_string(), Kind::Scope)];
    for kind in [Kind::Execution, Kind::Build, Kind::Gap, Kind::Spike] {
        kinds.push((store.create(new("W", kind, None, &[])).await.unwrap().id, kind));
    }
    let mut rx = store.subscribe();
    for (id, kind) in &kinds {
        let before = store.get(id).await.unwrap();
        let err = store.set_status(id, Status::Doc).await.unwrap_err();
        match &err {
            StoreError::DocStatusOnWork { id: named, kind: k } => assert_eq!((named, k), (id, kind)),
            other => panic!("{id}: expected DocStatusOnWork, got {other:?}"),
        }
        assert!(err.to_string().contains("Doc is a doc's own status"), "{err}");
        assert_eq!(store.get(id).await.unwrap(), before, "{id}: nothing changed");
    }
    assert!(rx.try_recv().is_err(), "nothing was broadcast");
    assert!(store.list(None, Some(Status::Doc), None).await.unwrap().is_empty());
    for status in Status::LIFECYCLE {
        store.set_status("W-3", status).await.unwrap();
    }
}

/// A `blocked_by` naming a doc is refused — a doc never completes, so it
/// can never unblock anything — whether the doc is named once, twice, or
/// beside tickets that may block; and so is a doc born as an id some
/// `blocked_by` already names (it would be waited on forever), while the
/// same id may still be born as work and resolve the reference.
#[tokio::test]
async fn nothing_may_wait_on_a_doc() {
    let store = Store::new();
    store.create(doc("N", None)).await.unwrap(); // N-1, a doc root
    store.create(work("N", None)).await.unwrap(); // N-2
    for blocked_by in [&["N-1"][..], &["N-2", "N-1", "N-1"]] {
        let err = store.create(new("N", Kind::Build, None, blocked_by)).await.unwrap_err();
        match &err {
            StoreError::BlockedByDoc { id, docs } => assert_eq!((id.as_str(), docs), ("N-3", &ids(["N-1"]))),
            other => panic!("expected BlockedByDoc, got {other:?}"),
        }
        let msg = err.to_string();
        assert!(msg.contains("'N-1' is a doc") && msg.contains("never completes"), "{msg}");
    }
    assert_eq!(store.count(), 2, "nothing was created");
    assert!(store.get("N-1").await.unwrap().unlocks.is_empty(), "the doc unlocks nothing");

    // A forward reference to N-4, then a doc that would be N-4.
    let waiting = store.create(new("N", Kind::Build, None, &["N-4"])).await.unwrap();
    assert_eq!(waiting.id, "N-3");
    let err = store.create(doc("N", None)).await.unwrap_err();
    match &err {
        StoreError::DocAwaited { id, by } => assert_eq!((id.as_str(), by), ("N-4", &ids(["N-3"]))),
        other => panic!("expected DocAwaited, got {other:?}"),
    }
    assert!(err.to_string().contains("'N-4' cannot be born a doc"), "{err}");
    assert_eq!(store.count(), 3, "nothing was created");
    let resolved = store.create(work("N", None)).await.unwrap();
    assert_eq!((resolved.id.as_str(), resolved.unlocks.as_slice()), ("N-4", &ids(["N-3"])[..]));
}

/// `ready_set` never holds a doc — not the whole set, not a page of it, not
/// one shortcode's — and the store keeps no count of unsatisfied blockers
/// for one. `list { status: Doc }` finds the docs, and pages them with
/// `remaining` from the `Doc` bucket of the status counts.
#[tokio::test]
async fn the_ready_set_never_holds_a_doc_and_list_filters_on_doc() {
    let store = Store::new();
    store.create(doc("R", None)).await.unwrap(); // R-1
    store.create(doc("R", None)).await.unwrap(); // R-2
    store.create(work("R", None)).await.unwrap(); // R-3
    store.create(work("R", Some("R-2"))).await.unwrap(); // R-4, work under a doc
    for id in ["R-3", "R-4"] {
        store.set_status(id, Status::Ready).await.unwrap();
    }
    let ready = ids(store.ready_set(None).await.unwrap().iter().map(|t| t.id.clone()));
    assert_eq!(ready, ["R-3", "R-4"]);
    assert_eq!(ids(store.ready_set(Some("R")).await.unwrap().iter().map(|t| t.id.clone())), ready);
    let page = store.ready_page(None, None, Some(1)).unwrap();
    assert_eq!((page.tickets[0].id.as_str(), page.remaining), ("R-3", 1));
    {
        let cache = store.tickets.read().unwrap();
        assert_eq!(cache.ready.size(), 2);
        assert!(cache.ready.counts().is_empty(), "no ticket waits on anything");
        assert_eq!((cache.ready.held("R", Status::Doc), cache.ready.held("R", Status::Ready)), (2, 2));
    }

    let docs = ids(store.list(None, Some(Status::Doc), None).await.unwrap().iter().map(|t| t.id.clone()));
    assert_eq!(docs, ["R-1", "R-2"]);
    let page = store
        .page(PageQuery { status: Some(Status::Doc), limit: Some(1), ..PageQuery::default() })
        .unwrap();
    assert_eq!((page.tickets[0].id.as_str(), page.next.as_deref(), page.remaining), ("R-1", Some("R-1"), 1));
}

/// Roll-ups: a doc has no completion, so a scope's `done/total` ignores doc
/// children — every piece of work complete is a row done, docs or no docs —
/// and the docs are counted apart, in `docs`, through every write that moves
/// a row: a doc's birth moves `docs` and nothing else, a completion moves
/// `done` and leaves `docs` as it was. Equal to the nav's plan() throughout;
/// and the status counts carry `Doc` as a bucket of its own.
#[tokio::test]
async fn roll_ups_ignore_docs_and_count_them_apart() {
    let scope_row = |status| RootRow { title: "a scope in P".into(), status, kind: Kind::Scope };
    let store = Store::new();
    store.create(new("P", Kind::Scope, None, &[])).await.unwrap(); // P-1
    store.create(work("P", None)).await.unwrap(); // P-2
    store.create(work("P", None)).await.unwrap(); // P-3
    store.create(doc("P", None)).await.unwrap(); // P-4, a doc child of the scope
    store.create(doc("P", Some("P-2"))).await.unwrap(); // P-5, a doc child of work
    store.create(work("P", Some("P-4"))).await.unwrap(); // P-6, work under a doc
    assert_eq!(
        group(&store, "P"),
        Group { epic: "P".into(), total: 4, done: 0, docs: 2, root: Some("P-1".into()), root_row: Some(scope_row(Status::Pending)), host: None }
    );
    assert_eq!(store.groups(), tests::nav(&store));

    let mut rx = store.subscribe();
    for id in ["P-2", "P-3", "P-6", "P-1"] {
        store.set_status(id, Status::Complete).await.unwrap();
        assert_eq!(store.groups(), tests::nav(&store), "{id}");
    }
    let moved = group_changes(&mut rx);
    assert_eq!(
        moved.iter().map(|g| (g.total, g.done, g.docs)).collect::<Vec<_>>(),
        [(4, 1, 2), (4, 2, 2), (4, 3, 2), (4, 4, 2)],
        "done reaches total with the docs still there"
    );
    let g = group(&store, "P");
    assert_eq!((g.done, g.total), (4, 4), "the scope's work is done; its docs do not hold it open");

    store.create(doc("P", Some("P-6"))).await.unwrap(); // P-7
    assert_eq!(
        group_changes(&mut rx),
        vec![Group { epic: "P".into(), total: 4, done: 4, docs: 3, root: Some("P-1".into()), root_row: Some(scope_row(Status::Complete)), host: None }],
        "a doc's birth moves docs, and nothing else"
    );
    let cache = store.tickets.read().unwrap();
    assert_eq!((cache.ready.held("P", Status::Doc), cache.ready.held("P", Status::Complete)), (3, 4));
}

/// Everything else a ticket has, a doc has: `amend` (title and body, the
/// status staying `Doc`), `link_commits` and `unlink_commits`, parenthood
/// both ways (a doc's children may be docs or work; a doc may be filed under
/// work) with `set_parent`, `list { parent }`, `find`, `extents`, `groups`,
/// and `watch` carrying each change whole.
#[tokio::test]
async fn everything_else_works_for_a_doc() {
    let store = Store::new();
    let mut rx = store.subscribe();
    store.create(doc("E", None)).await.unwrap(); // E-1
    match rx.try_recv().unwrap() {
        Change::Created { ticket } => assert_eq!((ticket.id.as_str(), ticket.status), ("E-1", Status::Doc)),
        other => panic!("expected Created, got {other:?}"),
    }
    store.create(doc("E", None)).await.unwrap(); // E-2, a doc child of a doc
    store.create(work("E", Some("E-2"))).await.unwrap(); // E-3, work under a doc
    let kinds: Vec<&str> = ["created", "children_changed"].to_vec();
    let mut seen: Vec<&str> = Vec::new();
    while let Ok(change) = rx.try_recv() {
        seen.push(match change {
            Change::Created { .. } => "created",
            Change::ChildrenChanged { .. } => "children_changed",
            Change::GroupChanged { .. } => continue,
            other => panic!("unexpected {other:?}"),
        });
    }
    assert_eq!(seen, [kinds.clone(), kinds].concat());
    assert_eq!(children(&store, "E-1"), ["E-2"]);
    assert_eq!(children(&store, "E-2"), ["E-3"]);

    match store.amend("E-2", Some("the manifest".into()), Some("## Findings\n\nnone".into())).await.unwrap() {
        Change::Amended { ticket, changed } => {
            assert_eq!(changed, ["title", "body"]);
            assert_eq!((ticket.title.as_str(), ticket.status), ("the manifest", Status::Doc));
        }
        other => panic!("expected Amended, got {other:?}"),
    }
    assert!(matches!(rx.try_recv().unwrap(), Change::Amended { .. }));
    assert_eq!(store.get("E-2").await.unwrap().body, "## Findings\n\nnone");

    match store.link_commits("E-2", vec!["77df9a0".into()]).await.unwrap() {
        Change::CommitsLinked { ticket, added } => {
            assert_eq!((ticket.commits.as_slice(), added.as_slice(), ticket.status), (&ids(["77df9a0"])[..], &ids(["77df9a0"])[..], Status::Doc));
        }
        other => panic!("expected CommitsLinked, got {other:?}"),
    }
    assert!(matches!(rx.try_recv().unwrap(), Change::CommitsLinked { .. }));
    match store.unlink_commits("E-2", vec!["77df9a0".into()]).await.unwrap() {
        Change::CommitsUnlinked { ticket, removed } => assert_eq!((ticket.commits.len(), removed.len()), (0, 1)),
        other => panic!("expected CommitsUnlinked, got {other:?}"),
    }

    // Parenthood both ways: the work moves under the root doc; the doc E-2
    // is filed under that work.
    store.set_parent("E-3", "E-1").await.unwrap();
    store.set_parent("E-2", "E-3").await.unwrap();
    assert_eq!(children(&store, "E-1"), ["E-3"]);
    assert_eq!(children(&store, "E-3"), ["E-2"]);
    assert_eq!(store.get("E-3").await.unwrap().child_count, 1);
    let g = group(&store, "E");
    assert_eq!((g.total, g.done, g.docs), (1, 0, 2));
    assert_eq!(store.groups(), tests::nav(&store));

    let found = store.find_page("manifest", None, None).unwrap();
    assert_eq!(ids(found.tickets.iter().map(|t| t.id.clone())), ["E-2"]);
    assert!(store.extents().contains(&("E".to_string(), 3)));
}

/// A store with docs, from a file: the derived state verifies (roll-ups with
/// their `docs`, the `Doc` status counts, a ready set without docs), and a
/// reopen reads back the same groups, statuses and ready set.
#[tokio::test]
async fn a_file_with_docs_verifies_and_reopens_as_it_was() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("docs.db");
    let store = Store::open(&path).unwrap();
    store.create(new("HOST", Kind::Scope, None, &[])).await.unwrap(); // HOST-1
    store.create(work("HOST", None)).await.unwrap(); // HOST-2
    store.create(doc("MAN", Some("HOST-2"))).await.unwrap(); // MAN-1, a hosted doc collection
    store.create(doc("MAN", None)).await.unwrap(); // MAN-2
    store.create(work("MAN", Some("MAN-2"))).await.unwrap(); // MAN-3
    store.create(doc("HOST", None)).await.unwrap(); // HOST-3
    store.create(doc("LONE", None)).await.unwrap(); // LONE-1, docs alone
    store.set_status("MAN-3", Status::Ready).await.unwrap();
    store.set_status("HOST-2", Status::Complete).await.unwrap();
    let groups = store.groups();
    assert_eq!(groups, tests::nav(&store));
    let ready = ids(store.ready_set(None).await.unwrap().iter().map(|t| t.id.clone()));
    assert_eq!(ready, ["MAN-3"]);
    drop(store);

    match Store::verify_derived(&path).unwrap() {
        Verdict::Consistent { tickets: 7, ready: 1, .. } => {}
        other => panic!("expected Consistent, got {other:?}"),
    }
    let form = stored::read(&Connection::open(&path).unwrap()).unwrap();
    assert_eq!(form.statuses.get(&("MAN".to_string(), "Doc")), Some(&2));
    assert_eq!(form.groups["LONE"].roll, (0, 0, 1));

    let store = Store::open(&path).unwrap();
    assert_eq!(store.groups(), groups, "the reopen reads what was stored");
    assert_eq!(ids(store.ready_set(None).await.unwrap().iter().map(|t| t.id.clone())), ready);
    let lone = groups.iter().find(|g| g.epic == "LONE").unwrap();
    assert_eq!((lone.total, lone.docs, lone.drawn()), (0, 1, true));
    assert_eq!(store.get("MAN-1").await.unwrap().status, Status::Doc);
}

/// The file a binary from before SDLC-11 leaves — derived state in form 2:
/// no `docs` columns in the roll-up tables — is reported, not changed, by
/// `--verify-derived`; built again once on its first open (every number as
/// it was, every `docs` 0); verifies after; and takes docs from then on.
#[tokio::test]
async fn a_file_in_form_2_gains_the_docs_columns_once_on_its_first_open() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("form2.db");
    let store = Store::open(&path).unwrap();
    store.create(new("OLD", Kind::Scope, None, &[])).await.unwrap(); // OLD-1
    store.create(work("OLD", None)).await.unwrap(); // OLD-2
    store.create(new("SUB", Kind::Scope, Some("OLD-2"), &[])).await.unwrap(); // SUB-1, a cross tree
    store.set_status("OLD-2", Status::Complete).await.unwrap();
    let groups = store.groups();
    drop(store);
    Connection::open(&path)
        .unwrap()
        .execute_batch(
            "ALTER TABLE rollup_groups DROP COLUMN base_docs; ALTER TABLE rollup_groups DROP COLUMN own_docs;
             ALTER TABLE rollup_groups DROP COLUMN docs; ALTER TABLE rollup_cross DROP COLUMN docs;
             UPDATE derived_state SET form = 2;",
        )
        .unwrap();
    assert!(matches!(Store::verify_derived(&path).unwrap(), Verdict::NotStored(why) if why.contains("form 2")));

    let store = Store::open(&path).unwrap();
    assert_eq!(store.groups(), groups, "built again: every number as it was");
    assert_eq!(
        stored::state(&Connection::open(&path).unwrap()).unwrap(),
        State::Current,
        "and stored in this form"
    );
    store.create(doc("OLD", None)).await.unwrap(); // OLD-3
    assert_eq!(group(&store, "OLD").docs, 1);
    drop(store);
    assert!(matches!(Store::verify_derived(&path).unwrap(), Verdict::Consistent { tickets: 4, .. }));
}
