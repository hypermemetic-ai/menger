//! MENGER-30: `update` — any part of a ticket in one write. Each field
//! alone, all of them at once, the no-op; every refusal writing nothing (a
//! valid title beside an invalid status among them), several named at once;
//! blockers added and removed moving `unlocks`, the forward references, the
//! ready set, the `blocked` cells and the waves; a cycle refused, named; the
//! events in order; and a random walk of updates leaving the cache a scan of
//! its rows and `--verify-derived` consistent.

use super::*;

fn new(epic: &str, title: &str, kind: Kind, parent: Option<&str>, blocked_by: &[&str]) -> NewTicket {
    NewTicket {
        path: None,
        epic: epic.into(),
        title: title.into(),
        kind,
        severity: None,
        blocked_by: blocked_by.iter().map(|b| b.to_string()).collect(),
        parent: parent.map(Into::into),
        body: format!("the body of {title}"),
    }
}

/// A shortcode `U`: U-1 its scope root, U-2 and U-3 builds, U-4 a doc, U-5 a
/// scope under the root with U-6 a build under it; and `V`, V-1 its root
/// and V-2 a build.
async fn fixture(store: &Store) {
    store.create(new("U", "the root", Kind::Scope, None, &[])).await.unwrap();
    store.create(new("U", "two", Kind::Build, None, &[])).await.unwrap();
    store.create(new("U", "three", Kind::Build, None, &[])).await.unwrap();
    store.create(new("U", "a doc", Kind::Doc, None, &[])).await.unwrap();
    store.create(new("U", "an area", Kind::Scope, Some("U-1"), &[])).await.unwrap();
    store.create(new("U", "six", Kind::Build, Some("U-5"), &[])).await.unwrap();
    store.create(new("V", "v root", Kind::Scope, None, &[])).await.unwrap();
    store.create(new("V", "v two", Kind::Build, None, &[])).await.unwrap();
}

async fn store() -> Store {
    let store = Store::new();
    fixture(&store).await;
    store
}

/// Every change waiting on `rx`, transitions included.
fn drained(rx: &mut broadcast::Receiver<Change>) -> Vec<Change> {
    let mut out = Vec::new();
    while let Ok(change) = rx.try_recv() {
        out.push(change);
    }
    out
}

/// The `type` of each change.
fn types(changes: &[Change]) -> Vec<&'static str> {
    changes
        .iter()
        .map(|c| match c {
            Change::Amended { .. } => "amended",
            Change::StatusChanged { .. } => "status_changed",
            Change::KindChanged { .. } => "kind_changed",
            Change::Transition { .. } => "transition",
            Change::BlockersChanged { .. } => "blockers_changed",
            Change::UnlocksChanged { .. } => "unlocks_changed",
            Change::UnlocksRemoved { .. } => "unlocks_removed",
            Change::ParentChanged { .. } => "parent_changed",
            Change::ChildrenChanged { .. } => "children_changed",
            Change::ChildRemoved { .. } => "child_removed",
            Change::SeverityChanged { .. } => "severity_changed",
            Change::GroupChanged { .. } => "group_changed",
            Change::PlacementsChanged { .. } => "placements_changed",
            Change::ComponentChanged { .. } => "component_changed",
            Change::InputChanged { .. } => "input_changed",
            Change::CellsMoved { .. } => "cells_moved",
            Change::UnderMoved { .. } => "under_moved",
            _ => "other",
        })
        .collect()
}

/// Everything kept, against the rows: the roll-ups the nav draws, the
/// placements WAVE-4's rule gives, every derived structure a build by scan
/// makes, and the forward references a scan finds.
#[track_caller]
pub(super) fn assert_kept(store: &Store, after: &str) {
    assert_eq!(store.groups(), tests::nav(store), "the roll-ups the nav draws, after {after}");
    tests_placements::assert_kept_placements_are_the_rule(store, after);
    let cache = store.tickets.read().unwrap();
    let scanned = Cache::new(cache.order.values().map(|t| (t.id.clone(), t.clone())).collect(), cache.inputs.clone());
    assert_eq!(stored::compare(&cache.form(), &scanned.form()), Vec::<String>::new(), "after {after}");
    // Every blocker that exists mirrors who names it.
    for t in cache.order.values() {
        for b in &t.blocked_by {
            if let Some(bt) = cache.get(b) {
                assert!(bt.unlocks.contains(&t.id), "{b}'s unlocks name {}, after {after}", t.id);
            }
        }
        for u in &t.unlocks {
            let ut = cache.get(u).unwrap_or_else(|| panic!("{u} in {}'s unlocks is a ticket, after {after}", t.id));
            assert!(ut.blocked_by.contains(&t.id), "{u} names {} it unlocks, after {after}", t.id);
        }
    }
    drop(cache);
    assert_eq!(store.tickets.read().unwrap().ids.unborn(), tests::scanned(store).1, "forward references, after {after}");
}

fn blocked(ids: &[&str]) -> Update {
    Update { block: ids.iter().map(|s| s.to_string()).collect(), ..Update::default() }
}

fn unblocked(ids: &[&str]) -> Update {
    Update { unblock: ids.iter().map(|s| s.to_string()).collect(), ..Update::default() }
}

/// How many tickets of `epic` a filter admits.
fn count(store: &Store, filter: &str, epic: &str) -> u64 {
    store.counts_text(&Lens::parse(filter).unwrap()).into_iter().find(|(e, _)| e == epic).map_or(0, |(_, n)| n)
}

fn ready(store: &Store) -> Vec<String> {
    let cache = store.tickets.read().unwrap();
    cache.ready.members().iter().flat_map(|(e, seqs)| seqs.iter().map(move |s| format!("{e}-{s}"))).collect()
}

#[tokio::test]
async fn each_field_alone_moves_it_and_sends_its_own_event() {
    let store = store().await;
    let mut rx = store.subscribe_filtered();

    let u = store.update("U-2", Update { title: Some("two, renamed".into()), ..Update::default() }).await.unwrap();
    assert_eq!((u.changed.as_slice(), u.ticket.title.as_str()), (&["title".to_string()][..], "two, renamed"));
    assert_eq!(types(&drained(&mut rx)), ["amended"]);
    assert_kept(&store, "a title");

    let u = store.update("U-2", Update { body: Some("a new body".into()), ..Update::default() }).await.unwrap();
    assert_eq!((u.changed, u.ticket.body.as_str()), (vec!["body".to_string()], "a new body"));
    assert_eq!(store.get("U-2").await.unwrap().body, "a new body", "the body is stored");
    assert_eq!(types(&drained(&mut rx)), ["amended"]);

    let u = store.update("U-2", Update { kind: Some(Kind::Gap), ..Update::default() }).await.unwrap();
    assert_eq!((u.changed, u.ticket.kind, u.ticket.status), (vec!["kind".to_string()], Kind::Gap, Status::Pending));
    assert_eq!(types(&drained(&mut rx)), ["kind_changed", "transition", "cells_moved"]);
    assert_kept(&store, "a kind");

    let u = store.update("U-2", Update { status: Some(Status::Ready), ..Update::default() }).await.unwrap();
    assert_eq!((u.changed, u.ticket.status), (vec!["status".to_string()], Status::Ready));
    assert_eq!(types(&drained(&mut rx)), ["status_changed", "transition", "cells_moved"]);
    assert!(ready(&store).contains(&"U-2".to_string()));
    assert_kept(&store, "a status");

    let u = store.update("U-2", Update { parent: Some("u-5".into()), ..Update::default() }).await.unwrap();
    assert_eq!((u.changed, u.ticket.parent.as_deref()), (vec!["parent".to_string()], Some("U-5")));
    assert_eq!(types(&drained(&mut rx)), ["parent_changed", "children_changed", "child_removed"]);
    assert_eq!(store.get("U-5").await.unwrap().child_count, 2);
    assert_kept(&store, "a parent");

    let u = store.update("U-2", Update { severity: Some(Some(Severity::High)), ..Update::default() }).await.unwrap();
    assert_eq!((u.changed, u.ticket.severity), (vec!["severity".to_string()], Some(Severity::High)));
    let sent = drained(&mut rx);
    assert_eq!(types(&sent), ["severity_changed", "cells_moved"]);
    assert!(matches!(&sent[0], Change::SeverityChanged { previous: None, .. }));
    assert_eq!(count(&store, "severity High", "U"), 1);
    let u = store.update("U-2", Update { severity: Some(None), ..Update::default() }).await.unwrap();
    assert_eq!((u.changed, u.ticket.severity), (vec!["severity".to_string()], None), "severity clears");
    assert!(matches!(&drained(&mut rx)[0], Change::SeverityChanged { previous: Some(Severity::High), .. }));
    assert_eq!(count(&store, "severity High", "U"), 0);
    assert_kept(&store, "a severity");

    let u = store.update("U-3", blocked(&["u-2"])).await.unwrap();
    assert_eq!((u.changed, u.ticket.blocked_by.clone()), (vec!["blocked_by".to_string()], vec!["U-2".to_string()]));
    assert_eq!(types(&drained(&mut rx)), ["blockers_changed", "unlocks_changed", "placements_changed", "cells_moved"]);
    assert_eq!(store.get("U-2").await.unwrap().unlocks, ["U-3"]);
    assert_kept(&store, "a block");

    let u = store.update("U-3", unblocked(&["U-2"])).await.unwrap();
    assert_eq!((u.changed, u.ticket.blocked_by.len()), (vec!["blocked_by".to_string()], 0));
    let sent = drained(&mut rx);
    assert_eq!(types(&sent), ["blockers_changed", "unlocks_removed", "placements_changed", "cells_moved"]);
    assert!(matches!(&sent[1], Change::UnlocksRemoved { ticket, removed } if ticket.id == "U-2" && ticket.unlocks.is_empty() && removed == "U-3"));
    assert_kept(&store, "an unblock");
}

#[tokio::test]
async fn all_at_once_is_one_write_its_events_in_order() {
    let store = store().await;
    let mut rx = store.subscribe_filtered();
    let written = store.tickets.read().unwrap().written;
    let u = store
        .update(
            "U-3",
            Update {
                title: Some("three, all at once".into()),
                body: Some("everything moves".into()),
                kind: Some(Kind::Gap),
                status: Some(Status::Ready),
                parent: Some("U-5".into()),
                severity: Some(Some(Severity::Low)),
                block: vec!["U-2".into(), "V-2".into()],
                unblock: vec![],
                ..Update::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(u.changed, ["title", "body", "kind", "status", "parent", "severity", "blocked_by"]);
    let t = &u.ticket;
    assert_eq!(
        (t.title.as_str(), t.kind, t.status, t.parent.as_deref(), t.severity, t.blocked_by.clone()),
        ("three, all at once", Kind::Gap, Status::Ready, Some("U-5"), Some(Severity::Low), vec!["U-2".to_string(), "V-2".to_string()])
    );
    assert_eq!(store.get("U-3").await.unwrap(), *t, "the answer is the ticket as stored");
    assert_eq!(store.tickets.read().unwrap().written, written + 1, "one write");
    let sent = drained(&mut rx);
    assert_eq!(
        types(&sent),
        [
            "amended",
            "kind_changed",
            "transition",
            "transition",
            "blockers_changed",
            "unlocks_changed",
            "unlocks_changed",
            "parent_changed",
            "children_changed",
            "child_removed",
            "severity_changed",
            "placements_changed",
            "cells_moved",
            "cells_moved",
            "cells_moved",
        ]
    );
    // Blocked by two open tickets: not ready, `blocked`, in a later wave.
    assert!(!ready(&store).contains(&"U-3".to_string()));
    assert_eq!(count(&store, "blocked", "U"), 1);
    assert_eq!(t.placement, Some(crate::types::Placement::Wave { n: 2 }));
    assert_kept(&store, "all at once");
}

#[tokio::test]
async fn nothing_to_change_writes_and_sends_nothing() {
    let store = store().await;
    let before = store.get("U-2").await.unwrap();
    let written = store.tickets.read().unwrap().written;
    let mut rx = store.subscribe_filtered();
    for asked in [
        Update::default(),
        Update {
            title: Some(before.title.clone()),
            body: Some(before.body.clone()),
            kind: Some(before.kind),
            status: Some(before.status),
            parent: before.parent.clone(),
            severity: Some(before.severity),
            unblock: vec!["U-3".into()], // not a blocker of U-2: nothing to take off
            ..Update::default()
        },
    ] {
        let u = store.update("U-2", asked).await.unwrap();
        assert!(u.changed.is_empty(), "{:?}", u.changed);
        assert_eq!(u.ticket, before);
    }
    assert_eq!(store.get("U-2").await.unwrap(), before);
    assert_eq!(store.tickets.read().unwrap().written, written, "no write");
    assert!(drained(&mut rx).is_empty(), "nothing sent");
}

/// Each refusal: nothing written, nothing sent, the ticket and its
/// neighbours as they were.
#[tokio::test]
async fn every_refusal_writes_nothing() {
    let store = Store::new().with_body_max(64);
    fixture(&store).await;
    store.update("U-3", blocked(&["U-2"])).await.unwrap();
    let ids = ["U-1", "U-2", "U-3", "U-4", "U-5", "U-6", "V-1", "V-2"];
    let mut before = Vec::new();
    for id in ids {
        before.push(store.get(id).await.unwrap());
    }
    let written = store.tickets.read().unwrap().written;
    let mut rx = store.subscribe_filtered();
    let title = |t: &str| Some(t.to_string());
    type Case = (&'static str, Update, fn(&StoreError) -> bool);
    let cases: Vec<Case> = vec![
        ("U-2", Update { title: title("  "), ..Update::default() }, |e| matches!(e, StoreError::EmptyTitle { .. })),
        ("U-2", Update { body: Some("x".repeat(65)), ..Update::default() }, |e| matches!(e, StoreError::BodyTooLarge { .. })),
        ("U-2", Update { body: Some("```component nope_\n```".into()), ..Update::default() }, |e| matches!(e, StoreError::ComponentName { .. })),
        ("U-2", Update { kind: Some(Kind::Doc), ..Update::default() }, |e| matches!(e, StoreError::DocInGraph { .. })),
        ("U-1", Update { kind: Some(Kind::Build), ..Update::default() }, |e| matches!(e, StoreError::RootKind { .. })),
        ("U-2", Update { kind: Some(Kind::Gap), status: Some(Status::Doc), ..Update::default() }, |e| {
            matches!(e, StoreError::DocStatusForWork { .. })
        }),
        ("U-2", Update { status: Some(Status::Doc), ..Update::default() }, |e| matches!(e, StoreError::DocStatusOnWork { .. })),
        ("U-4", Update { status: Some(Status::Ready), ..Update::default() }, |e| matches!(e, StoreError::DocHasNoLifecycle { .. })),
        ("U-2", Update { parent: Some("U-99".into()), ..Update::default() }, |e| matches!(e, StoreError::ParentNotFound { .. })),
        ("U-2", Update { parent: Some("V-1".into()), ..Update::default() }, |e| matches!(e, StoreError::ParentOutsideShortcode { .. })),
        ("U-5", Update { parent: Some("U-6".into()), ..Update::default() }, |e| matches!(e, StoreError::ParentCycle { .. })),
        ("U-2", blocked(&["U-2"]), |e| matches!(e, StoreError::BlockCycle { .. })),
        ("U-2", blocked(&["U-3"]), |e| matches!(e, StoreError::BlockCycle { .. })),
        ("U-2", blocked(&["U-4"]), |e| matches!(e, StoreError::BlockedByDoc { .. })),
        ("U-4", blocked(&["U-2"]), |e| matches!(e, StoreError::DocInGraph { .. })),
        ("U-2", Update { block: vec!["V-2".into()], unblock: vec!["v-2".into()], ..Update::default() }, |e| {
            matches!(e, StoreError::Blockers { .. })
        }),
        ("U-2", blocked(&[" "]), |e| matches!(e, StoreError::Blockers { .. })),
        ("U-2", Update { refused: vec!["unknown kind 'bug'".into()], ..Update::default() }, |e| matches!(e, StoreError::Invalid(_))),
        // A valid title beside an invalid status: the whole update refused.
        ("U-2", Update { title: title("a fine title"), status: Some(Status::Doc), ..Update::default() }, |e| {
            matches!(e, StoreError::DocStatusOnWork { .. })
        }),
    ];
    for (id, asked, expected) in cases {
        let what = format!("{id} {asked:?}");
        let e = store.update(id, asked).await.unwrap_err();
        assert!(expected(&e), "{what}: {e:?}");
    }
    assert!(matches!(store.update("U-77", Update::default()).await, Err(StoreError::NotFound(_))));
    for (id, was) in ids.iter().zip(&before) {
        assert_eq!(&store.get(id).await.unwrap(), was, "{id} untouched");
    }
    assert_eq!(store.tickets.read().unwrap().written, written, "no write");
    assert!(drained(&mut rx).is_empty(), "nothing sent");
    assert_kept(&store, "every refusal");
}

#[tokio::test]
async fn several_refusals_are_named_together() {
    let store = store().await;
    let e = store
        .update(
            "U-2",
            Update {
                title: Some("".into()),
                status: Some(Status::Doc),
                parent: Some("V-1".into()),
                block: vec!["U-4".into()],
                refused: vec!["unknown severity 'Huge'".into()],
                ..Update::default()
            },
        )
        .await
        .unwrap_err();
    let StoreError::Refused { id, refusals } = &e else { panic!("{e:?}") };
    assert_eq!(id, "U-2");
    assert_eq!(refusals.len(), 5, "{refusals:?}");
    let said = e.to_string();
    for part in ["unknown severity 'Huge'", "Doc is a doc's own status", "'U-4' is a doc", "nothing changed for 'U-2'"] {
        assert!(said.contains(part), "{part} in {said}");
    }
}

#[tokio::test]
async fn a_cycle_is_refused_and_named() {
    let store = store().await;
    store.update("U-3", blocked(&["U-2"])).await.unwrap();
    store.update("U-6", blocked(&["U-3"])).await.unwrap();
    let e = store.update("U-2", blocked(&["U-6"])).await.unwrap_err();
    assert!(matches!(&e, StoreError::BlockCycle { id, path } if id == "U-2" && path == &["U-6", "U-3", "U-2"]), "{e:?}");
    assert!(e.to_string().contains("U-2 -> U-6 -> U-3 -> U-2"), "{e}");
}

/// Blockers added and removed move every structure that hangs on them: the
/// blockers' `unlocks`, the forward references, the counts and the ready
/// set, the `blocked` cells, the waves; a completed blocker frees as it
/// always has.
#[tokio::test]
async fn blockers_move_unlocks_ready_cells_and_waves() {
    let store = store().await;
    for id in ["U-2", "U-3", "U-6"] {
        store.set_status(id, Status::Ready).await.unwrap();
    }
    assert_eq!(ready(&store), ["U-2", "U-3", "U-6"]);
    assert_eq!(count(&store, "blocked", "U"), 0);

    // U-6 waits on U-3, which waits on U-2: waves 1, 2, 3.
    store.update("U-3", blocked(&["U-2"])).await.unwrap();
    store.update("U-6", blocked(&["U-3"])).await.unwrap();
    assert_eq!(ready(&store), ["U-2"]);
    assert_eq!(count(&store, "blocked", "U"), 2);
    let wave = |id: &str| store.tickets.read().unwrap().get(id).unwrap().placement;
    use crate::types::Placement::Wave;
    assert_eq!((wave("U-2"), wave("U-3"), wave("U-6")), (Some(Wave { n: 1 }), Some(Wave { n: 2 }), Some(Wave { n: 3 })));
    assert_kept(&store, "a chain");

    // Unblocking the middle moves it and what waits on it a wave left.
    let mut rx = store.subscribe_filtered();
    store.update("U-3", unblocked(&["U-2"])).await.unwrap();
    let sent = drained(&mut rx);
    let Some(Change::PlacementsChanged { changes }) = sent.iter().find(|c| matches!(c, Change::PlacementsChanged { .. })) else {
        panic!("{:?}", types(&sent))
    };
    let moved: Vec<(&str, crate::types::Placement)> = changes.iter().map(|(_, c)| (c.id.as_str(), c.placement)).collect();
    assert_eq!(moved, [("U-3", Wave { n: 1 }), ("U-6", Wave { n: 2 })]);
    assert_eq!(ready(&store), ["U-2", "U-3"]);
    assert_eq!(count(&store, "blocked", "U"), 1);
    assert!(store.get("U-2").await.unwrap().unlocks.is_empty());
    assert_kept(&store, "the middle unblocked");

    // A completed blocker is satisfied: blocking on it blocks nothing.
    store.set_status("U-2", Status::Complete).await.unwrap();
    store.update("U-3", blocked(&["U-2"])).await.unwrap();
    assert_eq!(ready(&store), ["U-3"]);
    assert_eq!(count(&store, "blocked", "U"), 1, "only U-6, on U-3");
    assert_kept(&store, "blocked on a done ticket");

    // A forward reference: kept by the id it names, which, born, unlocks.
    store.update("U-3", blocked(&["U-8"])).await.unwrap();
    assert_eq!(store.tickets.read().unwrap().ids.unborn().get("U-8").cloned(), Some(vec!["U-3".to_string()]));
    assert!(!ready(&store).contains(&"U-3".to_string()), "a missing blocker is not done");
    assert_eq!(wave("U-3"), Some(crate::types::Placement::MissingBlocker));
    assert_kept(&store, "a forward reference");
    store.update("U-3", unblocked(&["U-8"])).await.unwrap();
    assert!(store.tickets.read().unwrap().ids.unborn().is_empty());
    assert_kept(&store, "a forward reference dropped");
    store.update("U-3", blocked(&["U-8"])).await.unwrap();
    store.create(new("U", "seven", Kind::Build, None, &[])).await.unwrap();
    let eight = store.create(new("U", "eight", Kind::Build, None, &[])).await.unwrap();
    assert_eq!((eight.id.as_str(), eight.unlocks.clone()), ("U-8", vec!["U-3".to_string()]));
    assert_kept(&store, "the forward reference born");
}

/// The file: an update's derived state in the same transaction as its
/// rows — `--verify-derived` is consistent after a random walk of updates
/// (refused ones among them), and a reopen reads what the cache held.
#[tokio::test]
async fn random_updates_leave_verify_derived_consistent() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("update.db");
    let store = Store::open(&path).unwrap();
    fixture(&store).await;
    for i in 0..8 {
        store.create(new(if i % 2 == 0 { "U" } else { "V" }, &format!("more {i}"), Kind::Build, None, &[])).await.unwrap();
    }
    let mut seed: u64 = 0x5eed_0030;
    let mut below = move |n: usize| {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        (seed % n as u64) as usize
    };
    let (mut done, mut refused) = (0, 0);
    for step in 0..300 {
        let ids: Vec<String> = store.tickets.read().unwrap().order.values().map(|t| t.id.clone()).collect();
        let pick = |below: &mut dyn FnMut(usize) -> usize| ids[below(ids.len())].clone();
        let mut u = Update::default();
        if below(5) == 0 {
            u.title = Some(format!("title {step}"));
        }
        if below(8) == 0 {
            u.body = Some(format!("body {step}"));
        }
        if below(6) == 0 {
            u.kind = Some(Kind::ALL[below(Kind::ALL.len())]);
        }
        if below(3) == 0 {
            u.status = Some(Status::ALL[below(Status::ALL.len())]);
        }
        if below(6) == 0 {
            u.parent = Some(pick(&mut below));
        }
        if below(5) == 0 {
            u.severity = Some([None, Some(Severity::High), Some(Severity::Low)][below(3)]);
        }
        for _ in 0..below(3) {
            u.block.push(if below(10) == 0 { format!("U-{}", 40 + below(5)) } else { pick(&mut below) });
        }
        for _ in 0..below(3) {
            u.unblock.push(pick(&mut below));
        }
        let id = pick(&mut below);
        let what = format!("step {step}: update {id} {u:?}");
        match store.update(&id, u).await {
            Ok(_) => done += 1,
            Err(StoreError::NotFound(_)) => panic!("{what}"),
            Err(_) => refused += 1,
        }
        if step % 25 == 0 {
            assert_kept(&store, &what);
        }
    }
    assert!(done > 100 && refused > 10, "{done} done, {refused} refused");
    assert_kept(&store, "the walk");
    let rows: Vec<Row> = store.tickets.read().unwrap().order.values().cloned().collect();
    drop(store);
    match Store::verify_derived(&path).unwrap() {
        Verdict::Consistent { .. } => {}
        other => panic!("{other:?}"),
    }
    let again = Store::open(&path).unwrap();
    let reread: Vec<Row> = again.tickets.read().unwrap().order.values().cloned().collect();
    assert_eq!(reread, rows, "the file holds what the cache held");
    assert_kept(&again, "a reopen");
}
