//! SDLC-13: `set_kind` — a ticket's kind changes, and every structure kept
//! by kind moves with it in the same write. One test per row of the
//! ticket's table: the status across the doc line (to a doc, from one,
//! between two kinds of work), each refusal and what it names, a root that
//! may change only between the kinds that root, a lower-seq ticket that
//! becomes the root, the no-op, the events and their order; then the file:
//! `--verify-derived` after every kind of change, and a reopen.
//!
//! The random walks of FILTER-4 (`tests_cells`), FILTER-13 (`tests_tree`),
//! SCALE-32 (`tests_ready`), FILTER-4's answers (`tests_filtered`) and
//! SCALE-28 (`tests_stored`) take `set_kind` into their mix through
//! [`random_set_kind`], which holds each call to the rules written out here.

use super::*;
use crate::address::ShortcodeId;
use super::tests::Changes;
use std::collections::BTreeSet;

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

fn group(store: &Store, epic: &str) -> Group {
    store
        .groups(SYS)
        .into_iter()
        .find(|g| g.epic == epic)
        .unwrap_or_else(|| panic!("no group {epic}"))
}

/// Every change waiting on `rx`, in the order sent.
fn drained(rx: &mut broadcast::Receiver<Change>) -> Vec<Change> {
    let mut out = Vec::new();
    while let Ok(change) = rx.try_change() {
        out.push(change);
    }
    out
}

/// The `type` of each change — what their order is checked by.
fn types(changes: &[Change]) -> Vec<&'static str> {
    changes
        .iter()
        .map(|c| match c {
            Change::Created { .. } => "created",
            Change::StatusChanged { .. } => "status_changed",
            Change::KindChanged { .. } => "kind_changed",
            Change::GroupChanged { .. } => "group_changed",
            Change::PlacementsChanged { .. } => "placements_changed",
            Change::CellsMoved { .. } => "cells_moved",
            Change::UnderMoved { .. } => "under_moved",
            _ => "other",
        })
        .collect()
}

#[track_caller]
fn kind_changed(change: Change) -> (Ticket, Kind) {
    match change {
        Change::KindChanged { ticket, previous } => (ticket, previous),
        other => panic!("expected KindChanged, got {other:?}"),
    }
}

/// The rows' own word on where everything kept by kind stands — the roll-ups
/// against the nav's `plan()`, the placements against WAVE-4's rule — and
/// the cache against a build by scan of its rows.
#[track_caller]
fn assert_kept(store: &Store, after: &str) {
    assert_eq!(store.groups(SYS), tests::nav(store), "the roll-ups the nav draws, after {after}");
    tests_placements::assert_kept_placements_are_the_rule(store, after);
    let cache = store.tickets.read().unwrap();
    let scanned = Cache::new(cache.order.values().map(|t| (t.id.to_string(), t.clone())).collect(), cache.inputs.clone());
    assert_eq!(stored::compare(&cache.form(), &scanned.form()), Vec::<String>::new(), "after {after}");
}

/// A shortcode `K`: K-1 its scope root, K-2 a build, K-3 a doc, K-4 a build
/// blocked by K-2 (so K-2 blocks a ticket), K-5 a gap.
async fn shortcode() -> Store {
    let store = Store::new();
    store.create(new("K", Kind::Scope, None, &[])).await.unwrap();
    store.create(new("K", Kind::Build, None, &[])).await.unwrap();
    store.create(new("K", Kind::Doc, None, &[])).await.unwrap();
    store.create(new("K", Kind::Build, None, &["K-2"])).await.unwrap();
    store.create(new("K", Kind::Gap, None, &[])).await.unwrap();
    store
}

/// To a doc: its status becomes `Doc` — given as `Doc` or not given — its
/// count moves from the work to the docs, its placement goes (a doc is
/// placed nowhere), and it leaves the cells of work. `kind_changed` first,
/// then the group; no placements event (no placement *moved* — the ticket's
/// own went, which its row says).
#[tokio::test]
async fn to_a_doc_its_status_becomes_doc() {
    let store = shortcode().await;
    assert_eq!(group(&store, "K").docs, 1);
    let mut rx = store.subscribe_filtered();
    let (t, previous) = kind_changed(store.set_kind("K-5", Kind::Doc, None).await.unwrap());
    assert_eq!((t.kind, t.status, previous, t.placement), (Kind::Doc, Status::Doc, Kind::Gap, None));
    let sent = drained(&mut rx);
    assert_eq!(types(&sent), ["kind_changed", "group_changed", "cells_moved"]);
    let g = group(&store, "K");
    assert_eq!((g.total, g.done, g.docs), (3, 0, 2), "a work ticket fewer, a doc more");
    assert_eq!(store.get(SYS, "K-5").await.unwrap(), t, "what the event carried is the ticket");
    assert_kept(&store, "a gap made a doc");

    // `Doc` given is the same; anything else is refused.
    let (t, _) = kind_changed(store.set_kind("K-1", Kind::Doc, Some(Status::Doc)).await.unwrap());
    assert_eq!((t.kind, t.status), (Kind::Doc, Status::Doc), "a scope root becomes a doc root");
    let refused = store.set_kind("K-3", Kind::Doc, Some(Status::Ready)).await.unwrap_err();
    assert!(matches!(refused, StoreError::DocStatusForDoc { .. }), "{refused:?}");
    let msg = refused.to_string();
    assert!(msg.contains("'K-3'") && msg.contains("Ready") && msg.contains("nothing changed"), "{msg}");
    assert_kept(&store, "a scope root made a doc");
}

/// From a doc: the status given (anything but `Doc`), else `Epic` for a
/// scope and `Pending` for any other kind; placed by the rule, and that
/// placement said in `placements_changed`, after the group.
#[tokio::test]
async fn from_a_doc_its_status_is_given_or_epic_for_a_scope_or_pending() {
    let store = shortcode().await;
    store.create(new("K", Kind::Doc, None, &[])).await.unwrap(); // K-6
    store.create(new("K", Kind::Doc, None, &[])).await.unwrap(); // K-7
    let mut rx = store.subscribe();

    let (t, previous) = kind_changed(store.set_kind("K-3", Kind::Build, None).await.unwrap());
    assert_eq!((t.kind, t.status, previous), (Kind::Build, Status::Pending, Kind::Doc));
    assert_eq!(t.placement, Some(Placement::Wave { n: 1 }), "placed by the rule: nothing to wait on");
    let sent = drained(&mut rx);
    assert_eq!(types(&sent), ["kind_changed", "group_changed", "placements_changed"]);
    match &sent[2] {
        Change::PlacementsChanged { changes } => {
            assert_eq!(changes.iter().map(|(_, c)| (c.id.as_str(), c.placement)).collect::<Vec<_>>(), [("K-3", Placement::Wave { n: 1 })])
        }
        other => panic!("{other:?}"),
    }

    // ORG-5: a doc below the root does not become a scope there…
    match store.set_kind("K-6", Kind::Scope, None).await {
        Err(StoreError::ScopeNotRoot { id, root, .. }) => assert_eq!((id.as_str(), root.as_str()), ("K-6", "K-1")),
        other => panic!("{other:?}"),
    }
    // …a doc that roots its shortcode does, as `Epic`.
    store.create(new("KD", Kind::Doc, None, &[])).await.unwrap();
    let (t, _) = kind_changed(store.set_kind("KD-1", Kind::Scope, None).await.unwrap());
    assert_eq!((t.kind, t.status), (Kind::Scope, Status::Epic), "a scope's default");
    let (t, _) = kind_changed(store.set_kind("K-7", Kind::Spike, Some(Status::Complete)).await.unwrap());
    assert_eq!((t.kind, t.status, t.placement), (Kind::Spike, Status::Complete, Some(Placement::Done)));
    let g = group(&store, "K");
    assert_eq!((g.total, g.done, g.docs), (6, 1, 1), "two docs became work, one of it done; K-6 stayed a doc (ORG-5)");

    let refused = store.set_kind("K-6", Kind::Doc, None).await.map(|_| ());
    assert!(refused.is_ok(), "back again: {refused:?}");
    let refused = store.set_kind("K-6", Kind::Build, Some(Status::Doc)).await.unwrap_err();
    assert!(matches!(refused, StoreError::DocStatusForWork { kind: Kind::Build, .. }), "{refused:?}");
    let msg = refused.to_string();
    assert!(msg.contains("'K-6'") && msg.contains("would be a build") && msg.contains("Doc"), "{msg}");
    assert_eq!(store.get(SYS, "K-6").await.unwrap().kind, Kind::Doc, "nothing changed");
    assert_kept(&store, "docs made work");
}

/// Between two kinds of work the status stays, unless one is given — and a
/// status given moves what `set_status` moves: across `Complete`, `done` and
/// the tickets it unlocks.
#[tokio::test]
async fn between_two_kinds_of_work_the_status_stays_unless_given() {
    let store = shortcode().await;
    store.set_status("K-2", Status::Coding).await.unwrap();
    let (t, previous) = kind_changed(store.set_kind("K-2", Kind::Spike, None).await.unwrap());
    assert_eq!((t.kind, t.status, previous), (Kind::Spike, Status::Coding, Kind::Build));

    // K-4 waits on K-2: K-2 made a complete execution frees it.
    store.set_status("K-4", Status::Ready).await.unwrap();
    let mut rx = store.subscribe();
    let (t, _) = kind_changed(store.set_kind("K-2", Kind::Execution, Some(Status::Complete)).await.unwrap());
    assert_eq!((t.kind, t.status, t.placement), (Kind::Execution, Status::Complete, Some(Placement::Done)));
    let sent = drained(&mut rx);
    assert_eq!(types(&sent), ["kind_changed", "group_changed", "placements_changed"]);
    assert_eq!(store.ready_set(SYS, Some("K")).await.unwrap().into_iter().map(|t| t.id).collect::<Vec<_>>(), ["K-4"]);
    assert_eq!(group(&store, "K").done, 1);
    assert_kept(&store, "a build made a complete execution");
}

/// To a doc is refused while the ticket waits on a blocker or a ticket waits
/// on it — a doc is never either — and the refusal names them.
#[tokio::test]
async fn to_a_doc_is_refused_with_a_blocker_or_an_unlock_naming_them() {
    let store = shortcode().await;
    let blocked = store.set_kind("K-4", Kind::Doc, None).await.unwrap_err();
    match &blocked {
        StoreError::DocInGraph { id, blocked_by, unlocks } => {
            assert_eq!((id.as_str(), blocked_by.as_slice(), unlocks.is_empty()), ("K-4", &["K-2".to_string()][..], true))
        }
        other => panic!("expected DocInGraph, got {other:?}"),
    }
    let msg = blocked.to_string();
    assert!(msg.contains("'K-4' cannot become a doc") && msg.contains("blocked by 'K-2'") && msg.contains("nothing changed"), "{msg}");
    let unlocks = store.set_kind("K-2", Kind::Doc, None).await.unwrap_err();
    match &unlocks {
        StoreError::DocInGraph { id, blocked_by, unlocks } => {
            assert_eq!((id.as_str(), blocked_by.is_empty(), unlocks.as_slice()), ("K-2", true, &["K-4".to_string()][..]))
        }
        other => panic!("expected DocInGraph, got {other:?}"),
    }
    assert!(unlocks.to_string().contains("'K-4' waits on it"), "{unlocks}");
    // A dangling blocker is a blocker too.
    store.create(new("K", Kind::Build, None, &["NEVER-1", "NEVER-1"])).await.unwrap(); // K-6
    let dangling = store.set_kind("K-6", Kind::Doc, Some(Status::Doc)).await.unwrap_err();
    assert!(dangling.to_string().contains("blocked by 'NEVER-1' (nothing changed)"), "{dangling}");
    for id in ["K-2", "K-4", "K-6"] {
        assert_eq!(store.get(SYS, id).await.unwrap().kind, Kind::Build, "{id}: nothing changed");
    }
    assert_kept(&store, "the refusals");
}

/// A shortcode's root becomes a kind that roots nothing: refused, naming the
/// shortcode. Between a doc and a scope it may — and the group carries its
/// new kind (AGS-23's `root_row`), the root's own shortcode's, once.
#[tokio::test]
async fn a_root_may_change_only_between_the_kinds_that_root() {
    let store = shortcode().await;
    for kind in [Kind::Build, Kind::Execution, Kind::Gap, Kind::Spike] {
        let refused = store.set_kind("K-1", kind, None).await.unwrap_err();
        assert!(matches!(&refused, StoreError::RootKind { epic, .. } if epic == "K"), "{refused:?}");
        let msg = refused.to_string();
        assert!(msg.contains("'K-1' is the root of K") && msg.contains(kind.as_str()), "{msg}");
    }
    let mut rx = store.subscribe();
    let (t, _) = kind_changed(store.set_kind("K-1", Kind::Doc, None).await.unwrap());
    assert_eq!((t.kind, t.status), (Kind::Doc, Status::Doc));
    let sent = drained(&mut rx);
    assert_eq!(types(&sent), ["kind_changed", "group_changed"]);
    let Change::GroupChanged { group: g } = &sent[1] else { unreachable!() };
    assert_eq!(g.root.as_deref(), Some("K-1"));
    assert_eq!(g.root_row.as_ref().map(|r| (r.kind, r.status)), Some((Kind::Doc, Status::Doc)));
    assert_eq!((g.total, g.docs), (3, 2));

    // A doc root made a scope again: Epic.
    let (t, _) = kind_changed(store.set_kind("K-1", Kind::Scope, None).await.unwrap());
    assert_eq!((t.kind, t.status), (Kind::Scope, Status::Epic));
    assert_eq!(group(&store, "K").root_row.map(|r| r.kind), Some(Kind::Scope));
    let _ = drained(&mut rx);

    // The kind it has and a new status, which moves no count: the root's
    // row moved, so its group is said all the same.
    kind_changed(store.set_kind("K-1", Kind::Scope, Some(Status::Ready)).await.unwrap());
    let sent = drained(&mut rx);
    assert_eq!(types(&sent), ["kind_changed", "group_changed"]);
    let Change::GroupChanged { group: g } = &sent[1] else { unreachable!() };
    assert_eq!(g.root_row.as_ref().map(|r| (r.kind, r.status)), Some((Kind::Scope, Status::Ready)));
    assert_kept(&store, "a root, doc and back");
}

/// A lower-seq ticket that becomes a scope or a doc becomes its shortcode's
/// root — a legacy shape, where the root is not seq 1 (or there is none) —
/// and `group_changed` carries the new `root` and `root_row`; a re-root that
/// hosts the shortcode elsewhere moves every row it moves. Then the old
/// root may become work: it roots nothing any more.
#[tokio::test]
async fn a_lower_seq_ticket_made_a_scope_or_a_doc_becomes_the_root() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("legacy.db");
    tests::write_db(&path, &tests::legacy());
    let store = Store::open(&path).unwrap();
    assert_eq!(group(&store, "DEMO").root.as_deref(), Some("DEMO-4"));
    let mut rx = store.subscribe();

    let (t, _) = kind_changed(store.set_kind("DEMO-2", Kind::Scope, None).await.unwrap());
    assert_eq!((t.kind, t.status), (Kind::Scope, Status::Pending), "work to work: the status stays");
    let sent = drained(&mut rx);
    assert_eq!(types(&sent), ["kind_changed", "group_changed"]);
    let Change::GroupChanged { group: g } = &sent[1] else { unreachable!() };
    assert_eq!(g.root.as_deref(), Some("DEMO-2"), "the new root");
    assert_eq!(g.root_row.as_ref().map(|r| (r.title.as_str(), r.kind)), Some(("DEMO-2", Kind::Scope)));
    assert_kept(&store, "DEMO-2 made the root");

    // The old root roots nothing now, so it may become work; the new one may
    // not.
    kind_changed(store.set_kind("DEMO-4", Kind::Build, None).await.unwrap());
    assert!(matches!(store.set_kind("DEMO-2", Kind::Gap, None).await, Err(StoreError::RootKind { .. })));
    // Lower still, as a doc.
    let (t, _) = kind_changed(store.set_kind("DEMO-1", Kind::Doc, None).await.unwrap());
    assert_eq!((t.status, group(&store, "DEMO").root.as_deref()), (Status::Doc, Some("DEMO-1")));
    assert_kept(&store, "DEMO-1 made a doc root");

    // A shortcode that never had a root gains one.
    assert_eq!(group(&store, "NOSC").root, None);
    kind_changed(store.set_kind("NOSC-2", Kind::Scope, None).await.unwrap());
    assert_eq!(group(&store, "NOSC").root.as_deref(), Some("NOSC-2"));
    assert_kept(&store, "NOSC rooted");

    // XPC's tickets are drawn under AGS-3 — XPC has no row. XPC-1 made its
    // scope root makes XPC a sub-shortcode hosted at AGS-3, a row of its own.
    assert!(store.groups(SYS).iter().all(|g| g.epic != "XPC"));
    kind_changed(store.set_kind("XPC-1", Kind::Scope, Some(Status::Ready)).await.unwrap());
    let sent = drained(&mut rx);
    let moved: Vec<String> = sent
        .iter()
        .filter_map(|c| match c {
            Change::GroupChanged { group } => Some(group.epic.to_string()),
            _ => None,
        })
        .collect();
    assert!(moved.contains(&"XPC".to_string()), "{moved:?}");
    let x = group(&store, "XPC");
    assert_eq!((x.root.as_deref(), x.host.as_deref()), (Some("XPC-1"), Some("AGS-3")));
    assert_kept(&store, "XPC rooted and hosted");

    drop(store);
    assert!(matches!(Store::verify_derived(&path).unwrap(), Verdict::Consistent { .. }));
    let store = Store::open(&path).unwrap();
    assert_eq!(group(&store, "DEMO").root.as_deref(), Some("DEMO-1"), "as stored");
    assert_kept(&store, "the reopen");
}

/// The same kind, and no status: nothing changes and nothing is sent — the
/// answer's `previous` is the ticket's kind. So for the status it already
/// has; another status is a change.
#[tokio::test]
async fn the_same_kind_and_no_status_changes_nothing() {
    let store = shortcode().await;
    let before = store.get(SYS, "K-2").await.unwrap();
    let mut rx = store.subscribe_filtered();
    for status in [None, Some(Status::Pending)] {
        let (t, previous) = kind_changed(store.set_kind("K-2", Kind::Build, status).await.unwrap());
        assert_eq!((t, previous), (before.clone(), Kind::Build));
    }
    let (t, previous) = kind_changed(store.set_kind("K-3", Kind::Doc, None).await.unwrap());
    assert_eq!((t.kind, previous), (Kind::Doc, Kind::Doc));
    assert!(drained(&mut rx).is_empty(), "nothing sent");
    assert_eq!(store.tickets.read().unwrap().written, 5, "nothing written: the five creates alone");

    let (t, previous) = kind_changed(store.set_kind("K-2", Kind::Build, Some(Status::Ready)).await.unwrap());
    assert_eq!((t.status, previous), (Status::Ready, Kind::Build), "a status given is a change");
    assert_eq!(types(&drained(&mut rx)), ["kind_changed", "cells_moved"]);
}

/// An unknown id is `NotFound`; a path holder (WAVE-10: only a doc carries
/// one) does not become work.
#[tokio::test]
async fn an_unknown_id_and_a_path_holder() {
    let store = shortcode().await;
    assert!(matches!(store.set_kind("K-99", Kind::Build, None).await, Err(StoreError::NotFound(id)) if id == "K-99"));
    let board = store
        .create(NewTicket { path: Some("_MENGER/board".into()), ..new("K", Kind::Doc, None, &[]) })
        .await
        .unwrap();
    let refused = store.set_kind(&board.id, Kind::Build, None).await.unwrap_err();
    assert!(matches!(refused, StoreError::PathKind { .. }), "{refused:?}");
    assert!(refused.to_string().contains("only a doc carries a path"), "{refused}");
    kind_changed(store.set_kind(&board.id, Kind::Doc, None).await.unwrap());
}

/// The file: every kind of change — to and from a doc, a root between doc
/// and scope, a re-root, a crossing of `Complete` with a fan-out — commits
/// with its derived state, and `--verify-derived` is consistent after each.
#[tokio::test]
async fn verify_derived_is_consistent_after_each_kind_of_change() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("kinds.db");
    {
        let store = Store::open(&path).unwrap();
        store.create(new("K", Kind::Scope, None, &[])).await.unwrap();
        store.create(new("K", Kind::Build, None, &[])).await.unwrap();
        store.create(new("K", Kind::Doc, None, &[])).await.unwrap();
        store.create(new("K", Kind::Build, None, &["K-2"])).await.unwrap();
        store.create(new("SUB", Kind::Doc, Some("K-2"), &[])).await.unwrap();
        store.create(new("SUB", Kind::Doc, None, &[])).await.unwrap();
    }
    let steps: [(&str, Kind, Option<Status>); 8] = [
        ("K-3", Kind::Execution, None),
        ("K-2", Kind::Gap, Some(Status::Complete)),
        ("K-4", Kind::Spike, Some(Status::Ready)),
        ("K-1", Kind::Doc, None),
        ("SUB-1", Kind::Scope, Some(Status::Coding)),
        ("SUB-2", Kind::Build, None),
        ("K-2", Kind::Execution, Some(Status::Pending)),
        ("K-1", Kind::Scope, Some(Status::Ready)),
    ];
    for (id, kind, status) in steps {
        let store = Store::open(&path).unwrap();
        kind_changed(store.set_kind(id, kind, status).await.unwrap());
        assert_kept(&store, &format!("{id} -> {kind}"));
        drop(store);
        assert!(
            matches!(Store::verify_derived(&path).unwrap(), Verdict::Consistent { .. }),
            "{id} -> {kind}: {:?}",
            Store::verify_derived(&path).unwrap()
        );
    }
}

/// Every pair of kinds, with and without a status: a ticket of each kind
/// (a child of the root, and a blocker of a ticket that waits on it unless
/// it is a doc) made each kind, with no status, `Ready` and `Complete` —
/// each change answered by the rules, and after each, every kept structure
/// a scan of the rows (in the cache, and in the file by `--verify-derived`),
/// the cells it announced exactly the ones a scan says moved.
#[tokio::test]
async fn every_pair_of_kinds_with_and_without_a_status() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("pairs.db");
    let store = Store::open(&path).unwrap();
    store.create(new("P", Kind::Scope, None, &[])).await.unwrap();
    let mut rx = store.subscribe_filtered();
    let mut changed = 0;
    // ORG-5: no scope is born below P-1, so none is a `from` here.
    for from in Kind::ALL.into_iter().filter(|k| *k != Kind::Scope) {
        for to in Kind::ALL {
            for status in [None, Some(Status::Ready), Some(Status::Complete)] {
                let t = store.create(new("P", from, None, &[])).await.unwrap();
                if from.has_lifecycle() {
                    store.create(new("P", Kind::Build, None, &[t.id.as_str()])).await.unwrap();
                }
                let _ = drained(&mut rx);
                let before = tests_cells::scanned(&tests_ready::rows(&store), &tests_cells::asking(&store));
                let what = format!("{} {from} -> {to} {status:?}", t.id);
                let got = store.set_kind(&t.id, to, status).await;
                let after = tests_cells::scanned(&tests_ready::rows(&store), &tests_cells::asking(&store));
                let moved: BTreeSet<OrderKey> = drained(&mut rx)
                    .into_iter()
                    .flat_map(|c| match c {
                        Change::CellsMoved { moves } => moves.moves.iter().map(|m| OrderKey { shortcode: ShortcodeId::from(m.shortcode.clone()), seq: m.seq }).collect(),
                        _ => Vec::new(),
                    })
                    .collect();
                let differ: BTreeSet<OrderKey> = after.iter().filter(|(k, c)| before.get(*k) != Some(*c)).map(|(k, _)| k.clone()).collect();
                assert_eq!(moved, differ, "{what}: the cells announced are the scan's");
                let refused = match (to.has_lifecycle(), status) {
                    (false, Some(_)) => true,
                    (false, None) => from.has_lifecycle(), // it blocks a ticket
                    // ORG-5: a scope below P-1, its root.
                    (true, _) => to == Kind::Scope,
                };
                match got {
                    Err(e) => assert!(refused, "{what}: {e}"),
                    Ok(change) => {
                        assert!(!refused, "{what}: {change:?}");
                        let (ticket, previous) = kind_changed(change);
                        let want = match status {
                            Some(s) => s,
                            None if !to.has_lifecycle() => Status::Doc,
                            None if from.has_lifecycle() => Status::Pending,
                            None if to == Kind::Scope => Status::Epic,
                            None => Status::Pending,
                        };
                        assert_eq!((previous, ticket.kind, ticket.status), (from, to, want), "{what}");
                        changed += 1;
                    }
                }
                assert_kept(&store, &what);
                assert!(matches!(Store::verify_derived(&path).unwrap(), Verdict::Consistent { .. }), "{what}");
            }
        }
    }
    // Five kinds from, six to, three statuses; less a doc's refusals (a
    // status given, or a ticket that blocks) and a scope below P-1 (ORG-5).
    assert_eq!(changed, 5 * 6 * 3 - (5 * 2 + 4) - 5 * 3, "every pair but those the rules refuse");
}

// ---- the random walks' share ---------------------------------------------

/// What one random `set_kind` did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum KindWrite {
    /// The kind or the status changed; `reroot`: its shortcode's root moved.
    Changed { reroot: bool },
    /// The kind and the status it had: nothing written.
    Unchanged,
    /// Refused, as the rules say it must be.
    Refused,
}

thread_local! {
    /// How many random `set_kind`s on this thread re-rooted a shortcode —
    /// what a walk on the legacy shapes holds above zero.
    static REROOTS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// [`REROOTS`], as it stands.
pub(super) fn reroots() -> usize {
    REROOTS.with(std::cell::Cell::get)
}

/// Each shortcode's root by its definition — its lowest-seq scope or doc —
/// from the rows, in the store's order.
fn roots(all: &[Row]) -> BTreeMap<&str, &Row> {
    let mut out: BTreeMap<&str, &Row> = BTreeMap::new();
    for t in all {
        if t.kind.roots_a_shortcode() {
            out.entry(t.epic.as_str()).or_insert(t);
        }
    }
    out
}

/// One random `set_kind` against the store as it stands — every pair of
/// kinds, with and without a status (`Doc` among them) — held to the rules
/// written out here, owing the code under test nothing: what it must refuse
/// it refuses with the refusal that says so, what it must not change it
/// does not, and what it changes it changes to what the rules say. Half the
/// time a ticket that would root its shortcode as a scope or a doc, a
/// quarter of the rest a root. `below` is the walk's own draw.
pub(super) async fn random_set_kind(store: &Store, below: &mut dyn FnMut(usize) -> usize) -> (String, KindWrite) {
    let all: Vec<Row> = store
        .tickets
        .read()
        .unwrap()
        .order
        .values()
        .filter(|t| t.id == t.id.to_ascii_uppercase())
        .cloned()
        .collect();
    let first = roots(&all);
    let rerooting: Vec<&Row> = all.iter().filter(|t| first.get(t.epic.as_str()).is_none_or(|r| t.seq < r.seq)).collect();
    let rooting: Vec<&Row> = first.values().copied().collect();
    let t = match below(8) {
        0..=3 if !rerooting.is_empty() => rerooting[below(rerooting.len())],
        4 | 5 if !rooting.is_empty() => rooting[below(rooting.len())],
        _ => &all[below(all.len())],
    };
    let kind = Kind::ALL[below(Kind::ALL.len())];
    let status = match below(4) {
        0 | 1 => None,
        2 => Some(Status::LIFECYCLE[below(Status::LIFECYCLE.len())]),
        _ => Some(Status::ALL[below(Status::ALL.len())]),
    };
    let what = format!("set_kind {} {} {} -> {kind} {status:?}", t.id, t.kind, t.status);

    // The rules.
    enum Want {
        Refused(&'static str),
        Unchanged,
        Changed(Status),
    }
    let to = match (kind.has_lifecycle(), status) {
        (false, None | Some(Status::Doc)) => Ok(Status::Doc),
        (false, Some(_)) => Err("DocStatusForDoc"),
        (true, Some(Status::Doc)) => Err("DocStatusForWork"),
        (true, Some(s)) => Ok(s),
        (true, None) if t.kind.has_lifecycle() => Ok(t.status),
        (true, None) if kind == Kind::Scope => Ok(Status::Epic),
        (true, None) => Ok(Status::Pending),
    };
    let roots_it = first.get(t.epic.as_str()).is_some_and(|r| r.id == t.id);
    let want = match to {
        Err(e) => Want::Refused(e),
        Ok(s) if kind == t.kind && s == t.status => Want::Unchanged,
        Ok(_) if !kind.has_lifecycle() && (!t.blocked_by.is_empty() || !t.unlocks.is_empty()) => Want::Refused("DocInGraph"),
        Ok(_) if roots_it && !kind.roots_a_shortcode() => Want::Refused("RootKind"),
        // ORG-5: a scope roots its shortcode — no ticket below its root becomes one.
        Ok(_) if kind == Kind::Scope && t.kind != Kind::Scope && first.get(t.epic.as_str()).is_some_and(|r| r.id != t.id && r.seq < t.seq) => {
            Want::Refused("ScopeNotRoot")
        }
        Ok(_) if t.path.is_some() && kind.has_lifecycle() => Want::Refused("PathKind"),
        Ok(s) => Want::Changed(s),
    };
    let got = store.set_kind(&t.id, kind, status).await;
    let outcome = match (want, got) {
        (Want::Refused(name), Err(e)) => {
            let is = match &e {
                StoreError::DocStatusForDoc { .. } => "DocStatusForDoc",
                StoreError::DocStatusForWork { .. } => "DocStatusForWork",
                StoreError::DocInGraph { .. } => "DocInGraph",
                StoreError::RootKind { .. } => "RootKind",
                StoreError::PathKind { .. } => "PathKind",
                StoreError::ScopeNotRoot { .. } => "ScopeNotRoot",
                _ => "something else",
            };
            assert_eq!(is, name, "{what}: {e}");
            assert!(e.to_string().contains(&format!("'{}'", t.id)), "{what}: the refusal names the ticket: {e}");
            assert_eq!(store.row(&t.id).unwrap().0, *t, "{what}: refused, nothing changed");
            KindWrite::Refused
        }
        (Want::Unchanged, Ok(Change::KindChanged { ticket, previous })) => {
            assert_eq!((previous, ticket.kind, ticket.status), (t.kind, t.kind, t.status), "{what}");
            assert_eq!(store.row(&t.id).unwrap().0, *t, "{what}: nothing changed");
            KindWrite::Unchanged
        }
        (Want::Changed(s), Ok(Change::KindChanged { ticket, previous })) => {
            assert_eq!((previous, ticket.kind, ticket.status), (t.kind, kind, s), "{what}");
            assert_eq!(store.row(&t.id).unwrap().0, Row::from(&ticket), "{what}: the event carries the row");
            let after: Vec<Row> = store.tickets.read().unwrap().order.values().cloned().collect();
            let now = roots(&after).get(t.epic.as_str()).map(|r| r.id.clone());
            let was = first.get(t.epic.as_str());
            let expected = if kind.roots_a_shortcode() && was.is_none_or(|r| t.seq <= r.seq) {
                Some(t.id.clone())
            } else {
                was.map(|r| r.id.clone())
            };
            assert_eq!(now, expected, "{what}: the shortcode's root by its definition");
            assert_eq!(store.tickets.read().unwrap().rollups.root(&t.shortcode).map(str::to_string), now.clone().map(String::from), "{what}: the root kept");
            let reroot = now.as_deref() != was.map(|r| r.id.as_str());
            REROOTS.with(|r| r.set(r.get() + usize::from(reroot)));
            KindWrite::Changed { reroot }
        }
        (_, got) => panic!("{what}: {:?}", got.map(|c| format!("{c:?}"))),
    };
    (what, outcome)
}
