//! SCALE-4: a ticket's children are an index the cache keeps, not a list
//! on its row — the proof that the index is what a scan of every ticket's
//! `parent` finds, after every path that sets one; that every row's
//! `child_count` is its size; that a page of `list { parent }` is the page
//! the old filter gave, `remaining` included; that a parent's stored row
//! does not change when it gains or loses a child; and that a database
//! written with AGS-19's `children` column opens with the same children —
//! the same set, in the store's order — and without the column.

use super::*;
use std::collections::BTreeSet;

/// SplitMix64, as the store's other randomized tests use: reproducible,
/// dependency-free.
struct Rng(u64);

impl Rng {
    fn below(&mut self, n: usize) -> usize {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        ((z ^ (z >> 31)) % n as u64) as usize
    }
}

fn new_ticket(epic: &str, kind: Kind, parent: Option<&str>) -> NewTicket {
    NewTicket {
        path: None,
        epic: epic.into(),
        title: format!("a {} in {epic}", kind.as_str()),
        kind,
        severity: None,
        blocked_by: Vec::new(),
        parent: parent.map(Into::into),
        body: String::new(),
    }
}

/// `list { parent: id }`'s ids — how a ticket's children are read now.
pub(super) fn children(store: &Store, id: &str) -> Vec<String> {
    store
        .page(PageQuery { parent: Some(id), ..PageQuery::default() })
        .unwrap()
        .tickets
        .into_iter()
        .map(|t| t.id)
        .collect()
}

/// Every parent's children as a scan of the store finds them: each
/// ticket's `(epic, seq)` under the id its `parent` names, in the store's
/// order.
pub(super) fn scanned(store: &Store) -> BTreeMap<String, Vec<(String, u32)>> {
    let cache = store.tickets.read().unwrap();
    let mut out: BTreeMap<String, Vec<(String, u32)>> = BTreeMap::new();
    for t in cache.order.values() {
        if let Some(p) = &t.parent {
            out.entry(p.clone()).or_default().push((t.epic.clone(), t.seq));
        }
    }
    out
}

/// The index is the scan, and every row's `child_count` is the number of
/// tickets naming it.
#[track_caller]
fn assert_children_are_a_scan(store: &Store, after: &str) {
    let scan = scanned(store);
    let cache = store.tickets.read().unwrap();
    assert_eq!(cache.children.all(), scan, "the index, after {after}");
    for t in cache.order.values() {
        let n = scan.get(&t.id).map_or(0, |v| v.len() as u64);
        assert_eq!(t.child_count, n, "{}.child_count, after {after}", t.id);
        assert_eq!(cache.children.count(&t.id), n, "the index's count of {}, after {after}", t.id);
    }
}

/// What a page is, `(rows, next, remaining)`.
type Answer = (Vec<String>, Option<String>, u64);

/// MNG-9's page under AGS-19's filter, the way `list { parent }` answered
/// before SCALE-4: the store's order as a plain list, every filter a test
/// on every ticket, what is left over counted.
fn old_page(store: &Store, q: &PageQuery<'_>) -> Result<Answer, String> {
    let all: Vec<(String, OrderKey, Option<String>, Status)> = {
        let cache = store.tickets.read().unwrap();
        cache
            .order
            .iter()
            .map(|(k, t)| (t.id.clone(), k.clone(), t.parent.clone(), t.status))
            .collect()
    };
    let up = |s: Option<&str>| s.map(str::to_ascii_uppercase);
    let (epic, parent, after) = (up(q.epic), up(q.parent), up(q.after));
    let cursor = match &after {
        None => None,
        Some(id) => match all.iter().find(|(i, ..)| i == id) {
            Some((_, key, ..)) => Some(key.clone()),
            None => return Err(format!("after: no ticket {id}")),
        },
    };
    let matching: Vec<&String> = all
        .iter()
        .filter(|(_, k, ..)| epic.as_deref().is_none_or(|e| k.epic == e))
        .filter(|(_, k, ..)| cursor.as_ref().is_none_or(|c| k > c))
        .filter(|(_, _, p, _)| parent.is_none() || p.as_deref() == parent.as_deref())
        .filter(|(.., s)| q.status.is_none_or(|want| *s == want))
        .map(|(id, ..)| id)
        .collect();
    let take = q.limit.map_or(matching.len(), |n| (n as usize).min(matching.len()));
    let page: Vec<String> = matching[..take].iter().map(|id| id.to_string()).collect();
    let remaining = (matching.len() - take) as u64;
    let next = if remaining == 0 {
        None
    } else {
        page.last().cloned().or(after)
    };
    Ok((page, next, remaining))
}

fn answered(store: &Store, q: &PageQuery<'_>) -> Result<Answer, String> {
    store
        .page(*q)
        .map(|p| (p.tickets.iter().map(|t| t.id.clone()).collect(), p.next, p.remaining))
        .map_err(|e| e.to_string())
}

/// Every page of `parent`'s children the store can be asked for — every
/// cursor (none, and every ticket in the store, child or not), every
/// limit, under each epic and each status or none — is the old filter's.
#[track_caller]
fn assert_every_page_is_the_old_filters(store: &Store, parent: &str) {
    let ids: Vec<String> = store.page(PageQuery::default()).unwrap().tickets.into_iter().map(|t| t.id).collect();
    let mut epics: Vec<Option<String>> = vec![None];
    epics.extend(store.extents().into_iter().map(|(e, _)| Some(e)));
    let cursors = std::iter::once(None).chain(ids.iter().map(|id| Some(id.as_str())));
    for after in cursors {
        for limit in [Some(0), Some(1), Some(2), Some(3), Some(5), None] {
            for epic in &epics {
                for status in [None, Some(Status::Pending), Some(Status::Ready)] {
                    let q = PageQuery {
                        epic: epic.as_deref(),
                        status,
                        parent: Some(parent),
                        after,
                        limit,
                    };
                    assert_eq!(answered(store, &q), old_page(store, &q), "{q:?}");
                }
            }
        }
    }
}

/// The shape every test here starts from: a root with its own children,
/// two hosted sub-shortcode roots (one sorting before it, one after), a
/// child moved in from a sibling, a grandchild, and statuses to filter on.
async fn family(store: &Store) {
    let t = |epic, kind, parent| new_ticket(epic, kind, parent);
    store.create(t("PAR", Kind::Scope, None)).await.unwrap(); // PAR-1
    store.create(t("PAR", Kind::Build, None)).await.unwrap(); // PAR-2 → PAR-1
    store.create(t("PAR", Kind::Build, None)).await.unwrap(); // PAR-3 → PAR-1
    store.create(t("PAR", Kind::Build, None)).await.unwrap(); // PAR-4 → PAR-1
    store.create(t("SUB", Kind::Scope, Some("PAR-2"))).await.unwrap(); // SUB-1 → PAR-2
    store.create(t("AAA", Kind::Scope, Some("PAR-2"))).await.unwrap(); // AAA-1 → PAR-2
    store.create(t("PAR", Kind::Gap, Some("PAR-2"))).await.unwrap(); // PAR-5 → PAR-2
    store.set_parent("PAR-3", "PAR-2").await.unwrap();
    store.set_parent("PAR-4", "PAR-3").await.unwrap();
    store.create(t("SUB", Kind::Build, None)).await.unwrap(); // SUB-2 → SUB-1
    store.create(t("PAR", Kind::Spike, None)).await.unwrap(); // PAR-6 → PAR-1
    store.set_status("PAR-3", Status::Ready).await.unwrap();
    store.set_status("AAA-1", Status::Ready).await.unwrap();
}

/// Required behavior, row 2 and the planner's amendment: a page of
/// children — and its `remaining` — is the page the old filter gave, in
/// the store's order (not the order the children arrived in: PAR-2 got
/// SUB-1, AAA-1, PAR-5, then PAR-3), for every cursor, limit, epic and
/// status; a ticket with none, and an id that is no ticket, page empty.
#[tokio::test]
async fn a_page_of_children_is_the_old_filters_page() {
    let store = Store::new();
    family(&store).await;
    assert_children_are_a_scan(&store, "the family");
    assert_eq!(children(&store, "PAR-2"), ["AAA-1", "PAR-3", "PAR-5", "SUB-1"]);
    assert_eq!(children(&store, "par-1"), ["PAR-2", "PAR-6"], "case-folded as before");
    assert_eq!(store.get("PAR-2").await.unwrap().child_count, 4);
    for parent in ["PAR-1", "PAR-2", "PAR-3", "SUB-1", "PAR-6", "NOPE-1"] {
        assert_every_page_is_the_old_filters(&store, parent);
    }

    // A walk a page at a time visits each child once, in order, with
    // `remaining` counting down — the walk SCALE-7 expands a node with.
    let mut seen = Vec::new();
    let mut after: Option<String> = None;
    loop {
        let page = store
            .page(PageQuery {
                parent: Some("PAR-2"),
                after: after.as_deref(),
                limit: Some(1),
                ..PageQuery::default()
            })
            .unwrap();
        seen.extend(page.tickets.iter().map(|t| t.id.clone()));
        assert_eq!(page.remaining, 4 - seen.len() as u64);
        match page.next {
            Some(next) => after = Some(next),
            None => break,
        }
    }
    assert_eq!(seen, ["AAA-1", "PAR-3", "PAR-5", "SUB-1"]);
}

/// The planner's amendment 2: a page of a large parent's children costs
/// the page. Counted, not timed: the index walks no more children than the
/// page holds, wherever the cursor sits, and `remaining` is arithmetic —
/// so the work is the same for the first page and one deep in the list.
#[tokio::test]
async fn a_page_deep_in_a_large_parents_children_is_cut_by_position() {
    let store = Store::new();
    store.seed_synthetic(SeedSpec { count: 20_000, seed: 1, into_existing: false }).unwrap();
    assert_children_are_a_scan(&store, "a seed");
    let root = store.groups().into_iter().max_by_key(|g| g.total).unwrap().root.unwrap();
    let n = store.get(&root).await.unwrap().child_count;
    assert!(n > 1_000, "{root} has {n} children");
    let cache = store.tickets.read().unwrap();
    let all = scanned(&store);
    let kids = &all[&root];
    for at in [0, 1, kids.len() / 2, kids.len() - 201, kids.len() - 1] {
        let (epic, seq) = &kids[at];
        let key = OrderKey { epic: epic.clone(), seq: *seq };
        use std::ops::Bound::{Excluded, Unbounded};
        let walked: Vec<OrderKey> =
            cache.children.range(&root, Excluded(&key), Unbounded).take(200).collect();
        let want: Vec<OrderKey> = kids[at + 1..]
            .iter()
            .take(200)
            .map(|(e, s)| OrderKey { epic: e.clone(), seq: *s })
            .collect();
        assert_eq!(walked, want, "the page after child {at}");
        assert_eq!(
            cache.children.len(&root, Excluded(&key), Unbounded),
            (kids.len() - at - 1) as u64,
            "what is left after child {at}, without walking it"
        );
    }
}

/// Every path that sets a parent keeps the index a scan and every count
/// exact: a load (of tickets naming a parent that is no ticket, and a
/// parent cycle — shapes only a hand edit makes), each kind of create,
/// each kind of `set_parent`, a refused create or move, a reopen, a seed
/// and a seed into an existing store — then 600 random writes.
#[tokio::test]
async fn every_path_that_sets_a_parent_keeps_the_index_a_scan() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("shapes.db");
    {
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch(db::SCHEMA).unwrap();
        for (id, kind, parent) in [
            ("HAND-1", Kind::Scope, None),
            ("HAND-2", Kind::Build, Some("HAND-1")),
            ("HAND-3", Kind::Build, Some("HAND-9")), // names no ticket — yet
            ("HAND-4", Kind::Build, Some("HAND-9")),
            ("LOOP-1", Kind::Scope, Some("LOOP-2")), // a parent cycle
            ("LOOP-2", Kind::Build, Some("LOOP-1")),
        ] {
            let (epic, seq) = id.rsplit_once('-').unwrap();
            db::upsert(
                &conn,
                &Ticket {
                    labels: Vec::new(),
                    id: id.into(),
                    epic: epic.into(),
                    seq: seq.parse().unwrap(),
                    title: id.into(),
                    status: Status::Pending,
                    kind,
                    severity: None,
                    blocked_by: Vec::new(),
                    unlocks: Vec::new(),
                    parent: parent.map(Into::into),
                    child_count: 0,
                    commits: Vec::new(),
                    path: None,
                    placement: None,
                    body: String::new(),
                    created_unix: 0,
                    updated_unix: 0,
                },
            )
            .unwrap();
        }
    }
    let store = Store::open(&path).unwrap();
    assert_children_are_a_scan(&store, "the load");
    assert_eq!(children(&store, "HAND-9"), ["HAND-3", "HAND-4"], "kept under the id they name");
    assert_eq!(store.get("LOOP-1").await.unwrap().child_count, 1);

    family(&store).await;
    assert_children_are_a_scan(&store, "the family");
    // Both edges on one ticket: the parent is also the blocker.
    store
        .create(NewTicket {
            path: None,
            blocked_by: vec!["PAR-6".into()],
            ..new_ticket("PAR", Kind::Build, Some("PAR-6"))
        })
        .await
        .unwrap();
    assert_children_are_a_scan(&store, "a create under its own blocker");
    // Refusals move nothing.
    assert!(store.create(new_ticket("PAR", Kind::Build, Some("SUB-1"))).await.is_err());
    assert!(store.set_parent("PAR-2", "PAR-4").await.is_err(), "PAR-4 is below PAR-2");
    assert!(store.set_parent("SUB-2", "PAR-1").await.is_err(), "outside the shortcode");
    assert_children_are_a_scan(&store, "the refusals");
    // Re-hosting a sub-shortcode root; a no-op move; moving off a cycle.
    store.set_parent("SUB-1", "PAR-6").await.unwrap();
    store.set_parent("SUB-1", "PAR-6").await.unwrap();
    store.set_parent("LOOP-1", "LOOP-1").await.unwrap_err();
    store.create(new_ticket("LOOP", Kind::Build, Some("LOOP-1"))).await.unwrap();
    store.set_parent("LOOP-1", "HAND-1").await.unwrap(); // off the cycle
    assert_eq!(children(&store, "LOOP-2"), Vec::<String>::new());
    assert_children_are_a_scan(&store, "the moves");
    // The id two tickets already named is born: its count is theirs from birth.
    for _ in 5..9 {
        store.create(new_ticket("HAND", Kind::Build, None)).await.unwrap();
    }
    let born = store.create(new_ticket("HAND", Kind::Build, None)).await.unwrap();
    assert_eq!((born.id.as_str(), born.child_count), ("HAND-9", 2));
    assert_children_are_a_scan(&store, "HAND-9's birth");

    let before = scanned(&store);
    drop(store);
    let store = Store::open(&path).unwrap();
    assert_eq!(scanned(&store), before, "the parents are durable");
    assert_children_are_a_scan(&store, "the reopen");

    store
        .seed_synthetic(SeedSpec { count: 2_000, seed: 3, into_existing: true })
        .unwrap();
    assert_children_are_a_scan(&store, "a seed into an existing store");
    let fresh = Store::new();
    fresh.seed_synthetic(SeedSpec { count: 3_000, seed: 9, into_existing: false }).unwrap();
    assert_children_are_a_scan(&fresh, "a seed into an empty store");

    // 600 random writes: creates under a random parent in the shortcode
    // (or the default), new sub-shortcodes hosted anywhere, moves — legal
    // or refused — and status changes.
    let mut rng = Rng(4);
    let epics = ["RA", "RB", "RC", "RD"];
    for i in 0..600 {
        let ids: Vec<String> = store.list(None, None, None).await.unwrap().into_iter().map(|t| t.id).collect();
        let pick = |rng: &mut Rng| ids[rng.below(ids.len())].clone();
        let epic = epics[rng.below(epics.len())];
        match rng.below(10) {
            0..=4 => {
                let mine: Vec<&String> = ids.iter().filter(|id| id.starts_with(&format!("{epic}-"))).collect();
                let new = if mine.is_empty() {
                    new_ticket(epic, Kind::Scope, Some(&pick(&mut rng)))
                } else if rng.below(3) == 0 {
                    new_ticket(epic, Kind::Build, None)
                } else {
                    new_ticket(epic, Kind::Build, Some(mine[rng.below(mine.len())]))
                };
                store.create(new).await.unwrap();
            }
            5..=7 => {
                let _ = store.set_parent(&pick(&mut rng), &pick(&mut rng)).await;
            }
            _ => {
                store.set_status(&pick(&mut rng), Status::Ready).await.unwrap();
            }
        }
        if i % 50 == 49 {
            assert_children_are_a_scan(&store, &format!("{} random writes", i + 1));
        }
    }
    assert_children_are_a_scan(&store, "600 random writes");
    for parent in ["RA-1", "RB-1", "PAR-2"] {
        if store.get(parent).await.is_ok() {
            let q = PageQuery { parent: Some(parent), limit: Some(7), ..PageQuery::default() };
            assert_eq!(answered(&store, &q), old_page(&store, &q), "{parent}");
        }
    }
}

/// Required behavior, row 3: a birth and a move tell `watch` the parent's
/// id, the child and the parent's new count — not the parent. A parent
/// that is also the newborn's blocker rides out whole once more, in
/// `UnlocksChanged`, carrying the same count, so applying the events in
/// order never undoes one. `watch { epic }` files the parent's side under
/// the parent's shortcode.
#[tokio::test]
async fn a_birth_and_a_move_broadcast_the_parents_id_and_count() {
    let store = Store::new();
    store.create(new_ticket("EV", Kind::Scope, None)).await.unwrap(); // EV-1
    let mut rx = store.subscribe();
    store
        .create(NewTicket {
            path: None,
            blocked_by: vec!["EV-1".into()],
            ..new_ticket("EV", Kind::Build, None)
        })
        .await
        .unwrap(); // EV-2 → EV-1, blocked by it
    assert!(matches!(rx.try_recv().unwrap(), Change::Created { .. }));
    match rx.try_recv().unwrap() {
        Change::ChildrenChanged { id, epic, added, child_count } => {
            assert_eq!((id.as_str(), epic.as_str(), added.as_str(), child_count), ("EV-1", "EV", "EV-2", 1));
        }
        other => panic!("expected ChildrenChanged, got {other:?}"),
    }
    match rx.try_recv().unwrap() {
        Change::UnlocksChanged { ticket, .. } => {
            assert_eq!(ticket.child_count, 1, "the blocker carries the count it now has");
            assert_eq!(ticket, store.get("EV-1").await.unwrap());
        }
        other => panic!("expected UnlocksChanged, got {other:?}"),
    }
    while rx.try_recv().is_ok() {}

    store.create(new_ticket("EV", Kind::Build, None)).await.unwrap(); // EV-3 → EV-1
    store.create(new_ticket("HOSTED", Kind::Scope, Some("EV-3"))).await.unwrap();
    let parents: Vec<(String, String, u64)> = std::iter::from_fn(|| rx.try_recv().ok())
        .filter_map(|c| match c {
            Change::ChildrenChanged { id, epic, child_count, .. } => Some((id, epic, child_count)),
            _ => None,
        })
        .collect();
    assert_eq!(
        parents,
        [("EV-1".into(), "EV".into(), 2), ("EV-3".into(), "EV".into(), 1)],
        "a hosted root's birth is filed under its host's shortcode"
    );

    store.set_parent("EV-3", "EV-2").await.unwrap();
    let changes: Vec<Change> = std::iter::from_fn(|| rx.try_recv().ok()).collect();
    match &changes[..3] {
        [Change::ParentChanged { ticket, previous }, Change::ChildrenChanged { id, added, child_count, .. }, Change::ChildRemoved { id: old, removed, child_count: left, .. }] =>
        {
            assert_eq!((ticket.id.as_str(), previous.as_deref()), ("EV-3", Some("EV-1")));
            assert_eq!(ticket.child_count, 1, "the moved ticket keeps its own children");
            assert_eq!((id.as_str(), added.as_str(), *child_count), ("EV-2", "EV-3", 1));
            assert_eq!((old.as_str(), removed.as_str(), *left), ("EV-1", "EV-3", 1));
        }
        other => panic!("expected parent_changed, children_changed, child_removed: {other:?}"),
    }
    assert_eq!(store.get("EV-1").await.unwrap().child_count, 1);
    assert_eq!(store.get("EV-2").await.unwrap().child_count, 1);
}

/// Every column of one stored row, as text — what the database holds for it.
/// SCALE-28: but `child_count`, which the file keeps on the parent's row now
/// and the birth or move under it writes (asserted on its own below).
fn stored_row(path: &Path, id: &str) -> Vec<Option<String>> {
    let conn = Connection::open(path).unwrap();
    let cols: Vec<String> = columns(&conn).into_iter().filter(|c| c != "child_count").collect();
    let sql = format!(
        "SELECT {} FROM tickets WHERE id = ?1",
        cols.iter().map(|c| format!("CAST({c} AS TEXT)")).collect::<Vec<_>>().join(", ")
    );
    conn.query_row(&sql, [id], |r| (0..cols.len()).map(|i| r.get(i)).collect())
        .unwrap()
}

fn columns(conn: &Connection) -> Vec<String> {
    conn.prepare("PRAGMA table_info(tickets)")
        .unwrap()
        .query_map([], |r| r.get(1))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}

/// Required behavior, row 4: a parent's stored row does not grow with its
/// children — it is not written at all when one is born under it or moves
/// in or out, and the table has no column a list could live in.
#[tokio::test]
async fn a_parents_stored_row_does_not_change_when_it_gains_or_loses_children() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("grow.db");
    let store = Store::open(&path).unwrap();
    store.create(new_ticket("GROW", Kind::Scope, None)).await.unwrap();
    store.create(new_ticket("GROW", Kind::Build, None)).await.unwrap(); // GROW-2
    let (root, other) = (stored_row(&path, "GROW-1"), stored_row(&path, "GROW-2"));
    assert!(!columns(&Connection::open(&path).unwrap()).contains(&"children".to_string()));

    for _ in 0..100 {
        store.create(new_ticket("GROW", Kind::Build, None)).await.unwrap();
    }
    for i in 3..13 {
        store.set_parent(&format!("GROW-{i}"), "GROW-2").await.unwrap();
    }
    store.set_parent("GROW-3", "GROW-1").await.unwrap();
    assert_eq!(store.get("GROW-1").await.unwrap().child_count, 92);
    assert_eq!(store.get("GROW-2").await.unwrap().child_count, 9);
    assert_eq!(stored_row(&path, "GROW-1"), root, "the root's row, byte for byte");
    assert_eq!(stored_row(&path, "GROW-2"), other, "a parent moved into and out of");
    let stored_count = |id: &str| -> u64 {
        Connection::open(&path)
            .unwrap()
            .query_row("SELECT child_count FROM tickets WHERE id = ?1", [id], |r| r.get(0))
            .unwrap()
    };
    assert_eq!((stored_count("GROW-1"), stored_count("GROW-2")), (92, 9), "SCALE-28: the count, and only it");
    drop(store);
    let store = Store::open(&path).unwrap();
    assert_eq!(store.get("GROW-1").await.unwrap().child_count, 92, "counted from the parents at load");
}

/// The store's schema as it was from AGS-19 to SCALE-4: `children` on the
/// row, a JSON list kept by the server — so a test can write "a database
/// created before this ticket" by hand, in shapes the server never wrote.
/// The black-box counterpart is `tests/fixtures/pre-scale4.db`, written by
/// the binary at 9087ca7.
const PRE_SCALE4_SCHEMA: &str = "
        CREATE TABLE tickets (
            id           TEXT PRIMARY KEY,
            epic         TEXT NOT NULL,
            seq          INTEGER NOT NULL,
            title        TEXT NOT NULL,
            status       TEXT NOT NULL,
            kind         TEXT NOT NULL,
            severity     TEXT,
            blocked_by   TEXT NOT NULL,
            unlocks      TEXT NOT NULL,
            parent       TEXT,
            children     TEXT NOT NULL DEFAULT '[]',
            body         TEXT NOT NULL,
            created_unix INTEGER NOT NULL,
            updated_unix INTEGER NOT NULL,
            UNIQUE (epic, seq)
        );
    ";

/// `(id, kind, parent, children list)` rows into a pre-SCALE-4 database.
fn write_pre_scale4_db(path: &Path, rows: &[(&str, &str, Option<&str>, &[&str])]) {
    let conn = Connection::open(path).unwrap();
    conn.execute_batch(PRE_SCALE4_SCHEMA).unwrap();
    for (id, kind, parent, kids) in rows {
        let (epic, seq) = id.rsplit_once('-').unwrap();
        conn.execute(
            "INSERT INTO tickets (id, epic, seq, title, status, kind, severity, blocked_by, unlocks, \
             parent, children, body, created_unix, updated_unix) \
             VALUES (?1, ?2, ?3, ?1, 'Pending', ?4, NULL, '[]', '[]', ?5, ?6, 'body', 1, 1)",
            rusqlite::params![id, epic, seq.parse::<u32>().unwrap(), kind, parent, serde_json::to_string(kids).unwrap()],
        )
        .unwrap();
    }
}

/// Acceptance 3 and the planner's amendment 3, at the store: a database
/// written with `children` lists opens with every parent's children the
/// same set as its list, in the store's order, and `child_count` its
/// length; the column is gone from the file, durably, and a second open
/// migrates nothing. A list that disagrees with `parent` (only a hand
/// edit makes one) is logged and `parent` stands.
#[tokio::test]
async fn a_database_with_children_lists_opens_with_the_same_children_and_no_column() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("pre-scale4.db");
    // The lists in the order the server appended them: PAR-2's children
    // arrived SUB-1, AAA-1, PAR-5 and then PAR-3 (moved in).
    let rows: &[(&str, &str, Option<&str>, &[&str])] = &[
        ("AAA-1", "scope", Some("PAR-2"), &[]),
        ("PAR-1", "scope", None, &["PAR-2", "PAR-6"]),
        ("PAR-2", "build", Some("PAR-1"), &["SUB-1", "AAA-1", "PAR-5", "PAR-3"]),
        ("PAR-3", "build", Some("PAR-2"), &["PAR-4"]),
        ("PAR-4", "build", Some("PAR-3"), &[]),
        ("PAR-5", "build", Some("PAR-2"), &[]),
        ("PAR-6", "build", Some("PAR-1"), &[]),
        ("SUB-1", "scope", Some("PAR-2"), &["SUB-2"]),
        ("SUB-2", "build", Some("SUB-1"), &[]),
    ];
    write_pre_scale4_db(&path, rows);
    {
        let mut conn = Connection::open(&path).unwrap();
        let found = db::migrate_drop_children(&mut conn).unwrap();
        assert_eq!(found, db::DroppedChildren { lists: 4, entries: 8, disagreements: 0 });
        assert!(!columns(&conn).contains(&"children".to_string()));
        assert_eq!(db::migrate_drop_children(&mut conn).unwrap(), db::DroppedChildren::default(), "once");
    }
    // The same database, opened by the store without a hand in between.
    let path = dir.path().join("pre-scale4-open.db");
    write_pre_scale4_db(&path, rows);
    let store = Store::open(&path).unwrap();
    for (id, _, _, list) in rows {
        let mut got = children(&store, id);
        let order = got.clone();
        let mut want: Vec<String> = list.iter().map(|s| s.to_string()).collect();
        got.sort();
        want.sort();
        assert_eq!(got, want, "{id}: the same set");
        let mut by_key = order.clone();
        by_key.sort_by_key(|c| store.tickets.read().unwrap().at[c].clone());
        assert_eq!(order, by_key, "{id}: in the store's order");
        assert_eq!(store.get(id).await.unwrap().child_count, list.len() as u64, "{id}");
    }
    assert_eq!(children(&store, "PAR-2"), ["AAA-1", "PAR-3", "PAR-5", "SUB-1"]);
    assert_children_are_a_scan(&store, "the migration");
    drop(store);
    assert!(!columns(&Connection::open(&path).unwrap()).contains(&"children".to_string()));
    let store = Store::open(&path).unwrap();
    assert_eq!(children(&store, "PAR-2"), ["AAA-1", "PAR-3", "PAR-5", "SUB-1"], "durable");

    // Lists that disagree with `parent`: one names a child whose parent is
    // another ticket, one leaves out a ticket that names it, one is empty
    // while a ticket names it. `parent` stands each time.
    let path = dir.path().join("disagree.db");
    write_pre_scale4_db(
        &path,
        &[
            ("BAD-1", "scope", None, &["BAD-2", "BAD-3"]),
            ("BAD-2", "build", Some("BAD-1"), &[]),
            ("BAD-3", "build", Some("BAD-4"), &[]), // listed under BAD-1
            ("BAD-4", "build", Some("BAD-1"), &[]), // left out of BAD-1's list; names nothing
            ("BAD-5", "build", Some("BAD-2"), &[]), // BAD-2's list is empty
        ],
    );
    {
        let mut conn = Connection::open(&path).unwrap();
        let found = db::migrate_drop_children(&mut conn).unwrap();
        assert_eq!((found.lists, found.entries, found.disagreements), (1, 2, 4));
    }
    let store = Store::open(&path).unwrap();
    assert_eq!(children(&store, "BAD-1"), ["BAD-2", "BAD-4"]);
    assert_eq!(children(&store, "BAD-4"), ["BAD-3"]);
    assert_eq!(children(&store, "BAD-2"), ["BAD-5"]);
    assert_children_are_a_scan(&store, "a disagreeing database");
}

/// The fixture the unchanged binary wrote (9087ca7, through its own API: a
/// root, hosted sub-shortcode roots sorting before and after it,
/// children moved in by `set_parent`, a grandchild moved twice): every
/// parent's children are its old list's, as a set, in the store's order.
#[tokio::test]
async fn the_fixture_written_before_scale4_opens_with_its_children() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("fixture.db");
    std::fs::copy(
        concat!(env!("CARGO_MANIFEST_DIR"), "/../tests/fixtures/pre-scale4.db"),
        &path,
    )
    .unwrap();
    let lists: Vec<(String, Vec<String>)> = {
        let conn = Connection::open(&path).unwrap();
        let mut stmt = conn.prepare("SELECT id, children FROM tickets ORDER BY epic, seq").unwrap();
        stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
            .unwrap()
            .map(|r| {
                let (id, raw) = r.unwrap();
                (id, serde_json::from_str(&raw).unwrap())
            })
            .collect()
    };
    assert_eq!(lists.len(), 9);
    let store = Store::open(&path).unwrap();
    let order: Vec<String> = store.list(None, None, None).await.unwrap().into_iter().map(|t| t.id).collect();
    for (id, list) in &lists {
        let got = children(&store, id);
        let want: BTreeSet<&String> = list.iter().collect();
        assert_eq!(got.iter().collect::<BTreeSet<_>>(), want, "{id}: the same set");
        let positions: Vec<usize> =
            got.iter().map(|c| order.iter().position(|o| o == c).unwrap()).collect();
        assert!(positions.windows(2).all(|w| w[0] < w[1]), "{id}: store order, {got:?}");
        assert_eq!(store.get(id).await.unwrap().child_count, list.len() as u64, "{id}");
    }
    assert_eq!(lists[2], ("PAR-2".to_string(), vec!["SUB-1".into(), "AAA-1".into(), "PAR-5".into(), "PAR-3".into()]));
    assert_eq!(children(&store, "PAR-2"), ["AAA-1", "PAR-3", "PAR-5", "SUB-1"]);
    assert_children_are_a_scan(&store, "the fixture");
}
