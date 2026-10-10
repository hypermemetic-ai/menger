//! FILTER-4: the cells — every ticket's cell, and every shortcode's bitmap
//! of each cell it holds, held to a scan of the rows after every write of
//! every kind: in the cache, and read back from the file through a
//! connection of its own. Each write's announced moves are exactly the
//! tickets whose cell a scan says it changed, and nothing else. A file in
//! form 3 gains the cells once on first open; `--verify-derived` names an
//! altered container and an altered count; the container encoding round
//! trips at Roaring's array/bitmap threshold.

use super::cells::{self, Key};
use super::stored::{self, State};
use super::tests_ready::{done, new_ticket, random_write, rows, Rng, Tally};
use super::*;
use std::collections::BTreeSet;

/// The oracle: each ticket's cell from its row and the distinct ids it
/// names that are not a `Complete` or `Superseded` ticket — written out
/// here, owing the code under test nothing. FILTER-21: a menger doc is one
/// whose path starts with `_MENGER/`.
pub(super) fn scanned(rows: &[Row]) -> BTreeMap<OrderKey, Key> {
    let by_id: HashMap<&str, &Row> = rows.iter().map(|t| (t.id.as_str(), t)).collect();
    rows.iter()
        .map(|t| {
            let named: BTreeSet<&str> = t.blocked_by.iter().map(String::as_str).collect();
            let blocked = named.iter().any(|b| !by_id.get(b).is_some_and(|bt| done(bt.status)));
            let menger = t.path.as_deref().is_some_and(|p| p.starts_with("_MENGER/"));
            (OrderKey::of(t), Key::new(t.status, t.kind, t.severity, blocked, !t.commits.is_empty(), menger))
        })
        .collect()
}

/// The cells a stored form holds, by ticket.
fn by_ticket(stored: &cells::Stored) -> BTreeMap<OrderKey, Key> {
    let mut out = BTreeMap::new();
    for ((epic, cell, hi), (card, raw)) in &stored.bits {
        let lows = cells::decode(*card, raw).unwrap();
        for low in lows {
            let key = OrderKey { epic: epic.clone(), seq: (hi << 16) | u32::from(low) };
            assert!(out.insert(key.clone(), stored.keys[cell]).is_none(), "{key:?} is stored in two cells");
        }
    }
    out
}

/// What the file holds, on a read-only connection of its own.
fn filed(path: &Path) -> (State, Vec<Row>, cells::Stored) {
    let mut conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
    let tx = conn.transaction().unwrap();
    let mut rows: Vec<Row> = db::load_rows(&tx).unwrap().into_values().collect();
    rows.sort_by_key(OrderKey::of);
    (stored::state(&tx).unwrap(), rows, stored::read(&tx).unwrap().cells)
}

/// The cells are a scan's — in the cache, and in the file.
#[track_caller]
pub(super) fn assert_cells_are_a_scan(store: &Store, path: Option<&Path>, after: &str) {
    let rows = rows(store);
    let want = scanned(&rows);
    let cache = store.tickets.read().unwrap();
    assert_eq!(cache.cells.by_ticket(), want, "the cache's cells, after {after}");
    let kept = cache.cells.stored();
    let keys: BTreeSet<Key> = kept.keys.values().copied().collect();
    assert_eq!(keys.len(), kept.keys.len(), "every cell interned once, after {after}");
    for ((epic, cell), n) in &kept.counts {
        let members = kept.bits.range((epic.clone(), *cell, 0)..=(epic.clone(), *cell, u32::MAX)).map(|(_, (c, _))| c).sum::<u64>();
        assert_eq!(*n, members, "{epic} cell {cell}'s count is its members, after {after}");
    }
    drop(cache);
    if let Some(path) = path {
        let (state, stored_rows, file) = filed(path);
        assert_eq!(state, State::Current, "after {after}");
        assert_eq!(by_ticket(&file), want, "the file's cells, after {after}");
        assert_eq!(file, kept, "the cache's cells are the file's, ids and all, after {after}");
        let scan = Cache::new(stored_rows.into_iter().map(|t| (t.id.clone(), t)).collect());
        assert_eq!(stored::cell_differences(&file, &scan.cells.stored()), Vec::<String>::new(), "after {after}");
    }
}

/// Every cell move announced since the last drain, `(ticket, from, to)`.
fn announced(rx: &mut broadcast::Receiver<Change>, store: &Store) -> BTreeSet<(OrderKey, Option<Key>, Key)> {
    let keys = store.tickets.read().unwrap().cells.keys().to_vec();
    let mut out = BTreeSet::new();
    loop {
        match rx.try_recv() {
            Ok(Change::CellsMoved { moves }) => {
                for mv in &moves.moves {
                    let key = OrderKey { epic: mv.epic.to_string(), seq: mv.seq };
                    assert!(
                        out.insert((key.clone(), mv.from.map(|c| keys[c as usize]), keys[mv.to as usize])),
                        "{key:?} moved twice in one write"
                    );
                }
            }
            Ok(_) => {}
            Err(broadcast::error::TryRecvError::Empty) => return out,
            Err(e) => panic!("the filtered channel: {e}"),
        }
    }
}

/// What a scan says moved between two states of the store.
fn diff(before: &BTreeMap<OrderKey, Key>, after: &BTreeMap<OrderKey, Key>) -> BTreeSet<(OrderKey, Option<Key>, Key)> {
    after
        .iter()
        .filter(|(k, c)| before.get(*k) != Some(*c))
        .map(|(k, c)| (k.clone(), before.get(k).copied(), *c))
        .collect()
}

/// `writes` random writes; after each, the cells held to a scan and the
/// moves the write announced held to what the scan says moved. Then a
/// reopen, which reads the cells it kept.
async fn walk(store: Store, path: &Path, seed: u64, writes: usize, epics: &[&str]) -> Store {
    let mut rng = Rng(seed);
    // FILTER-21: board notes among the docs.
    let mut tally = Tally::filing_menger_docs();
    let mut rx = store.subscribe_filtered();
    let mut moved = 0;
    for step in 0..writes {
        let before = scanned(&rows(&store));
        let what = random_write(&store, &mut rng, epics, &mut tally).await;
        let after = scanned(&rows(&store));
        let said = announced(&mut rx, &store);
        assert_eq!(said, diff(&before, &after), "step {step}: {what}: the moves announced are the scan's");
        moved += said.len();
        assert_cells_are_a_scan(&store, Some(path), &format!("step {step}: {what}"));
    }
    assert!(moved > writes / 2 && tally.kinds > 0, "{moved} cell moves over {writes} writes: {tally:?}");
    let before = store.tickets.read().unwrap().cells.stored();
    drop(rx);
    drop(store);
    let store = Store::open(path).unwrap();
    assert_eq!(store.tickets.read().unwrap().cells.stored(), before, "a reopen reads what was kept");
    assert_cells_are_a_scan(&store, Some(path), "the reopen");
    store
}

/// Acceptance 2's store built by `create` — and every other write — from
/// nothing.
#[tokio::test]
async fn the_cells_are_a_scan_on_a_store_built_by_create() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("created.db");
    let store = Store::open(&path).unwrap();
    assert_cells_are_a_scan(&store, Some(&path), "a fresh open");
    walk(store, &path, 0xF117_E404, 400, &["AA", "MAP", "SUB", "ZED"]).await;
}

/// …by a seed, and by a seed into a store already walked.
#[tokio::test]
async fn the_cells_are_a_scan_on_a_seeded_store() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("seeded.db");
    let store = Store::open(&path).unwrap();
    store.seed_synthetic(SeedSpec { count: 1_500, seed: 44, into_existing: false }).unwrap();
    assert_cells_are_a_scan(&store, Some(&path), "a seed");
    let epics: Vec<String> = store.extents().into_iter().map(|(e, _)| e).take(4).chain(["NEW".into()]).collect();
    let epics: Vec<&str> = epics.iter().map(String::as_str).collect();
    let store = walk(store, &path, 0x5EED_F404, 200, &epics).await;
    store.seed_synthetic(SeedSpec { count: 700, seed: 9, into_existing: true }).unwrap();
    assert_cells_are_a_scan(&store, Some(&path), "a seed into the walked store");
    walk(store, &path, 0x5EED_F405, 100, &epics).await;
}

/// …and from each fixture written before SCALE-28, whose first open builds
/// them with the rest.
#[tokio::test]
async fn the_cells_are_a_scan_on_each_fixture() {
    let fixtures = concat!(env!("CARGO_MANIFEST_DIR"), "/../tests/fixtures");
    for (i, name) in ["pre-ags13.db", "pre-ags19.db", "pre-scale4.db", "pre-sdlc7.db"].into_iter().enumerate() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(name);
        std::fs::copy(format!("{fixtures}/{name}"), &path).unwrap();
        let store = Store::open(&path).unwrap();
        assert_cells_are_a_scan(&store, Some(&path), &format!("the first open of {name}"));
        let mut epics: Vec<String> = store.extents().into_iter().map(|(e, _)| e).collect();
        epics.push("NEW".into());
        let epics: Vec<&str> = epics.iter().map(String::as_str).collect();
        walk(store, &path, 0xF1C5 + i as u64, 120, &epics).await;
    }
}

/// Required behavior, "the cells", row by row: which writes move a cell —
/// a birth, a status change, a crossing's fan-out, the first commit and
/// the last — and which move none: an amend, a re-parent, a second commit,
/// an unlink that leaves one, a status set to itself.
#[tokio::test]
async fn exactly_the_writes_that_change_a_cell_move_one() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("moves.db");
    let store = Store::open(&path).unwrap();
    let mut rx = store.subscribe_filtered();
    let ids = |set: &BTreeSet<(OrderKey, Option<Key>, Key)>| -> Vec<String> {
        set.iter().map(|(k, _, _)| format!("{}-{}", k.epic, k.seq)).collect()
    };
    let key = |status, blocked, commits| Some(Key::new(status, Kind::Build, None, blocked, commits, false));

    store.create(new_ticket("MV", Kind::Scope, None, vec![])).await.unwrap();
    let born = announced(&mut rx, &store);
    assert_eq!(ids(&born), ["MV-1"]);
    assert!(born.iter().all(|(_, from, _)| from.is_none()), "a birth joins its cell from none");
    // A create that names the id it would take is refused as a cycle — and
    // moves nothing.
    assert!(store.create(new_ticket("MV", Kind::Build, None, vec!["MV-2".into()])).await.is_err());
    let born = announced(&mut rx, &store);
    assert_eq!(born.len(), 0, "a refused create moves nothing: {born:?}");
    store.create(new_ticket("MV", Kind::Build, None, vec![])).await.unwrap();
    for _ in 0..3 {
        store.create(new_ticket("MV", Kind::Build, None, vec!["MV-2".into()])).await.unwrap();
    }
    let born = announced(&mut rx, &store);
    assert_eq!(ids(&born), ["MV-2", "MV-3", "MV-4", "MV-5"]);
    let blocked: Vec<_> = born.iter().map(|(_, _, to)| to.blocked()).collect();
    assert_eq!(blocked, [false, true, true, true], "born blocked by MV-2, which is not done");

    // A status change of the ticket itself.
    store.set_status("MV-3", Status::Ready).await.unwrap();
    let moved = announced(&mut rx, &store);
    assert_eq!(moved.into_iter().collect::<Vec<_>>(), [(OrderKey { epic: "MV".into(), seq: 3 }, key(Status::Pending, true, false), key(Status::Ready, true, false).unwrap())]);
    // To itself: a write, and no move.
    store.set_status("MV-3", Status::Ready).await.unwrap();
    assert!(announced(&mut rx, &store).is_empty(), "a status set to itself moves no cell");

    // A crossing fans out: MV-3, MV-4 and MV-5 leave `blocked`, with MV-2's own move.
    store.set_status("MV-2", Status::Complete).await.unwrap();
    let moved = announced(&mut rx, &store);
    assert_eq!(ids(&moved), ["MV-2", "MV-3", "MV-4", "MV-5"]);
    assert!(moved.iter().filter(|(k, _, _)| k.seq > 2).all(|(_, from, to)| from.unwrap().blocked() && !to.blocked()));
    // Complete → Superseded crosses nothing: MV-2 alone moves.
    store.set_status("MV-2", Status::Superseded).await.unwrap();
    assert_eq!(ids(&announced(&mut rx, &store)), ["MV-2"]);

    // Commits: the first moves, a second does not; an unlink that leaves one
    // does not, the last does.
    store.link_commits("MV-4", vec!["abc1234".into()]).await.unwrap();
    assert_eq!(ids(&announced(&mut rx, &store)), ["MV-4"]);
    store.link_commits("MV-4", vec!["abc1235".into()]).await.unwrap();
    assert!(announced(&mut rx, &store).is_empty(), "a second commit moves no cell");
    store.unlink_commits("MV-4", vec!["abc1234".into()]).await.unwrap();
    assert!(announced(&mut rx, &store).is_empty(), "an unlink that leaves one moves no cell");
    store.unlink_commits("MV-4", vec!["abc1235".into()]).await.unwrap();
    let last = announced(&mut rx, &store);
    assert_eq!(ids(&last), ["MV-4"]);
    assert!(last.iter().all(|(_, from, to)| from.unwrap().commits() && !to.commits()));

    // No cell moves: an amend, a re-parent, a no-op link.
    store.amend("MV-4", Some("renamed".into()), Some("a body".into())).await.unwrap();
    store.set_parent("MV-5", "MV-4").await.unwrap();
    store.link_commits("MV-5", vec![]).await.unwrap();
    assert!(announced(&mut rx, &store).is_empty(), "amend, set_parent and a no-op move no cell");

    // FILTER-21: a board note is born into a menger doc's cell, an ordinary
    // doc beside it into the cell it would share without the flag; nothing
    // after its birth moves it — its path is set then and never again.
    let note = NewTicket { path: Some("_MENGER/board".into()), ..new_ticket("MV", Kind::Doc, None, vec![]) };
    store.create(note).await.unwrap();
    store.create(new_ticket("MV", Kind::Doc, None, vec![])).await.unwrap();
    let born: Vec<_> = announced(&mut rx, &store).into_iter().collect();
    assert_eq!(born.iter().map(|(k, _, _)| k.seq).collect::<Vec<_>>(), [6, 7]);
    let doc = |menger| Key::new(Status::Doc, Kind::Doc, None, false, false, menger);
    assert_eq!((born[0].2, born[1].2), (doc(true), doc(false)), "two cells, the flag apart");
    assert!(born[0].2.menger() && born[0].2.to_string().ends_with(", menger doc)"), "{}", born[0].2);
    assert_eq!(born[1].2.to_string(), "(Doc, doc, none, not blocked, no commits)", "an ordinary doc is named as before");
    store.amend("MV-6", Some("board: MV".into()), Some("notes".into())).await.unwrap();
    store.set_parent("MV-6", "MV-4").await.unwrap();
    assert!(announced(&mut rx, &store).is_empty(), "amend and set_parent leave a menger doc in its cell");
    store.link_commits("MV-6", vec!["abc1236".into()]).await.unwrap();
    let moved: Vec<_> = announced(&mut rx, &store).into_iter().collect();
    assert_eq!(moved.len(), 1);
    assert!(moved[0].1.unwrap().menger() && moved[0].2.menger() && moved[0].2.commits(), "a commit moves it within the flag");
    assert_cells_are_a_scan(&store, Some(&path), "the writes");
}

/// A crossing that moves more than [`RECOUNT_OVER`] tickets carries the
/// counts of every shortcode it moved, as the write leaves them — what a
/// `recount` reads; one that moves fewer carries none.
#[tokio::test]
async fn a_crossing_past_the_threshold_carries_the_counts_it_leaves() {
    let store = Store::new();
    store.create(new_ticket("FAN", Kind::Scope, None, vec![])).await.unwrap();
    store.create(new_ticket("OTHER", Kind::Scope, None, vec![])).await.unwrap();
    let hub = store.create(new_ticket("FAN", Kind::Build, None, vec![])).await.unwrap().id;
    for i in 0..300 {
        let epic = if i % 3 == 0 { "OTHER" } else { "FAN" };
        store.create(new_ticket(epic, Kind::Build, None, vec![hub.clone()])).await.unwrap();
    }
    let mut rx = store.subscribe_filtered();
    store.set_status(&hub, Status::Complete).await.unwrap();
    let mut counts = None;
    while let Ok(change) = rx.try_recv() {
        if let Change::CellsMoved { moves } = change {
            assert_eq!(moves.moves.len(), 301);
            counts = moves.counts.clone();
        }
    }
    let counts = counts.expect("past the threshold, the counts ride with the moves");
    let kept = store.tickets.read().unwrap().cells.stored().counts;
    let want: BTreeMap<String, Vec<(u32, u64)>> = ["FAN", "OTHER"]
        .into_iter()
        .map(|e| (e.to_string(), kept.iter().filter(|((epic, _), _)| epic == e).map(|((_, c), n)| (*c, *n)).collect()))
        .collect();
    assert_eq!(counts, want, "every count of each shortcode moved, as the write left it");
    store.set_status(&hub, Status::Ready).await.unwrap();
    store.set_status("FAN-1", Status::Ready).await.unwrap();
    let mut seen = 0;
    while let Ok(change) = rx.try_recv() {
        if let Change::CellsMoved { moves } = change {
            seen += 1;
            assert_eq!(moves.counts.is_some(), moves.moves.len() > RECOUNT_OVER);
        }
    }
    assert_eq!(seen, 2);
}

/// Required behavior, "a file in form 3 or older": a file stored in form 3
/// — no cell tables — gains them once, on its first open, in one
/// transaction, and verifies; the next open reads them: a container changed
/// by hand between two opens is what the cache then holds.
#[tokio::test]
async fn a_file_in_form_3_gains_the_cells_once_on_first_open() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("form3.db");
    let store = Store::open(&path).unwrap();
    store.seed_synthetic(SeedSpec { count: 900, seed: 3, into_existing: false }).unwrap();
    let want = scanned(&rows(&store));
    drop(store);
    Connection::open(&path)
        .unwrap()
        .execute_batch("DROP TABLE cells; DROP TABLE cell_bits; DROP TABLE cell_counts; UPDATE derived_state SET form = 3;")
        .unwrap();
    assert!(matches!(Store::verify_derived(&path).unwrap(), Verdict::NotStored(why) if why.contains("form 3")));
    let store = Store::open(&path).unwrap();
    assert_eq!(store.tickets.read().unwrap().cells.by_ticket(), want, "built on first open");
    assert_cells_are_a_scan(&store, Some(&path), "the first open of a form-3 file");
    drop(store);
    assert!(matches!(Store::verify_derived(&path).unwrap(), Verdict::Consistent { tickets: 900, .. }));

    // The next open reads: drop a member from one container by hand, and the
    // cache holds the container as stored — a rebuild would have the scan's.
    let (epic, cell, hi, card, raw): (String, u32, u32, u64, Vec<u8>) = Connection::open(&path)
        .unwrap()
        .query_row("SELECT epic, cell, hi, card, bits FROM cell_bits WHERE card BETWEEN 2 AND 4096 LIMIT 1", [], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
        })
        .unwrap();
    let lows = cells::decode(card, &raw).unwrap();
    let gone = OrderKey { epic: epic.clone(), seq: (hi << 16) | u32::from(lows[0]) };
    Connection::open(&path)
        .unwrap()
        .execute(
            "UPDATE cell_bits SET card = ?4, bits = ?5 WHERE epic = ?1 AND cell = ?2 AND hi = ?3",
            rusqlite::params![epic, cell, hi, card - 1, cells::encode(&lows[1..])],
        )
        .unwrap();
    let store = Store::open(&path).unwrap();
    assert_eq!(store.tickets.read().unwrap().cells.cell_of(&gone), None, "read, not rebuilt: {gone:?} is in no cell");
}

/// FILTER-21, "a file in form 5 is opened": a seeded file with board notes
/// beside ordinary docs, as a form-5 build leaves it — the cell table with
/// no `menger` column, each board note in an ordinary doc's cell — gains
/// form 6 once, on its first open: the column, every cell a scan's, the
/// counts and pages of every filter as before, and `--verify-derived`
/// consistent; the next open reads it.
#[tokio::test]
async fn a_file_in_form_5_gains_the_menger_column_once_on_first_open() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("form5.db");
    let store = Store::open(&path).unwrap();
    store.seed_synthetic(SeedSpec { count: 600, seed: 21, into_existing: false }).unwrap();
    for epic in ["PLX", "MNG"] {
        let root = store.create(new_ticket(epic, Kind::Scope, None, vec![])).await.unwrap().id;
        store.create(NewTicket { path: Some("_MENGER/board".into()), ..new_ticket(epic, Kind::Doc, None, vec![]) }).await.unwrap();
        store.create(new_ticket(epic, Kind::Doc, Some(root), vec![])).await.unwrap();
    }
    let want = scanned(&rows(&store));
    assert_eq!(want.values().filter(|k| k.menger()).count(), 2);
    let filters = ["", "menger doc", "not menger doc", "kind doc", "not menger doc and kind doc", "(menger doc or ready)"];
    let answers = |store: &Store| -> Vec<(Vec<(String, u64)>, Vec<String>)> {
        filters
            .iter()
            .map(|text| {
                let lens = Lens::parse(text).unwrap();
                let q = Filtered { lens: lens.clone(), epic: None, status: None, parent: None };
                let page = store.filtered_page(&q, None, Some(3), Some(50)).unwrap();
                (store.counts(&lens), page.tickets.into_iter().map(|t| t.id).collect())
            })
            .collect()
    };
    let before = answers(&store);
    assert_eq!(before[1].0, [("MNG".to_string(), 1), ("PLX".to_string(), 1)], "`menger doc`: the board notes");
    drop(store);

    // The file as a form-5 build leaves it: the derived state built with no
    // path in sight, then the cell table as form 5 declared it, the paths
    // back, and the form 5.
    let conn = Connection::open(&path).unwrap();
    let paths: Vec<(String, String)> = conn
        .prepare("SELECT id, path FROM tickets WHERE path IS NOT NULL")
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    conn.execute_batch("UPDATE tickets SET path = NULL; UPDATE derived_state SET form = 5;").unwrap();
    drop(conn);
    drop(Store::open(&path).unwrap());
    let conn = Connection::open(&path).unwrap();
    conn.execute_batch(
        "CREATE TABLE cells_form5 (id INTEGER PRIMARY KEY, state TEXT NOT NULL, kind TEXT NOT NULL, severity TEXT,
                                   blocked INTEGER NOT NULL, commits INTEGER NOT NULL);
         INSERT INTO cells_form5 SELECT id, state, kind, severity, blocked, commits FROM cells;
         DROP TABLE cells;
         ALTER TABLE cells_form5 RENAME TO cells;
         UPDATE derived_state SET form = 5;",
    )
    .unwrap();
    for (id, p) in &paths {
        conn.execute("UPDATE tickets SET path = ?2 WHERE id = ?1", rusqlite::params![id, p]).unwrap();
    }
    assert!(!db::has_column(&conn, "cells", "menger").unwrap());
    drop(conn);
    assert!(matches!(Store::verify_derived(&path).unwrap(), Verdict::NotStored(why) if why.contains("form 5")));

    let store = Store::open(&path).unwrap();
    assert_eq!(store.tickets.read().unwrap().cells.by_ticket(), want, "built on first open, the flag apart");
    assert_cells_are_a_scan(&store, Some(&path), "the first open of a form-5 file");
    assert_eq!(answers(&store), before, "every filter answers as before");
    drop(store);
    assert!(matches!(Store::verify_derived(&path).unwrap(), Verdict::Consistent { tickets: 606, .. }));
    let conn = Connection::open(&path).unwrap();
    assert!(db::has_column(&conn, "cells", "menger").unwrap());
    assert_eq!(conn.query_row("SELECT form FROM derived_state", [], |r| r.get::<_, i64>(0)).unwrap(), 9, "AUTH-7: built to form 9");
    drop(conn);
    let store = Store::open(&path).unwrap();
    assert_eq!(answers(&store), before, "the next open reads what the first stored");
}

/// Acceptance 3: `--verify-derived` names the row when one `cell_bits`
/// container or one `cell_counts` row is altered by hand.
#[tokio::test]
async fn verify_names_an_altered_container_and_an_altered_count() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("verify.db");
    let store = Store::open(&path).unwrap();
    store.create(new_ticket("VC", Kind::Scope, None, vec![])).await.unwrap();
    for _ in 0..3 {
        store.create(new_ticket("VC", Kind::Build, None, vec![])).await.unwrap();
    }
    store.set_status("VC-3", Status::Ready).await.unwrap();
    drop(store);
    assert!(matches!(Store::verify_derived(&path).unwrap(), Verdict::Consistent { tickets: 4, cells: 3, bitmaps: 3, .. }));
    let conn = Connection::open(&path).unwrap();
    let pending: u32 = conn
        .query_row("SELECT id FROM cells WHERE state = 'Pending' AND kind = 'build'", [], |r| r.get(0))
        .unwrap();
    // VC-2 and VC-4 are Pending builds; the container says VC-2 alone.
    conn.execute(
        "UPDATE cell_bits SET card = 1, bits = ?2 WHERE epic = 'VC' AND cell = ?1",
        rusqlite::params![pending, cells::encode(&[2])],
    )
    .unwrap();
    conn.execute("UPDATE cell_counts SET held = 7 WHERE epic = 'VC' AND cell = ?1", [pending]).unwrap();
    drop(conn);
    let Verdict::Differs(found) = Store::verify_derived(&path).unwrap() else {
        panic!("an altered container differs from a scan");
    };
    assert_eq!(
        found,
        [
            format!("cell_bits VC cell {pending} (Pending, build, none, not blocked, no commits) container 0: stored 1 members [2]; a scan gives 2 members [2, 4]"),
            format!("cell_counts VC cell {pending} (Pending, build, none, not blocked, no commits): stored 7; a scan gives 2"),
        ],
        "{found:#?}"
    );
    // A container naming a cell the table does not hold, and a cell twice.
    let conn = Connection::open(&path).unwrap();
    conn.execute("INSERT INTO cell_bits (epic, cell, hi, card, bits) VALUES ('VC', 99, 0, 1, ?1)", [cells::encode(&[9])]).unwrap();
    conn.execute(
        "INSERT INTO cells (id, state, kind, severity, blocked, commits) SELECT 98, state, kind, severity, blocked, commits FROM cells WHERE id = ?1",
        [pending],
    )
    .unwrap();
    drop(conn);
    let Verdict::Differs(found) = Store::verify_derived(&path).unwrap() else { unreachable!() };
    assert!(found.contains(&format!("cells {pending} and 98: both are (Pending, build, none, not blocked, no commits)")), "{found:#?}");
    assert!(found.contains(&"cell_bits VC cell 99, which the cell table does not hold, container 0: stored 1 members".to_string()), "{found:#?}");
}

/// The container encoding: Roaring's array up to 4,096 members, its 8 KiB
/// bitmap past it; both decode to what was encoded, and a row that cannot
/// be a container is refused, saying why.
#[test]
fn a_container_round_trips_at_the_threshold() {
    for n in [1usize, 2, 4095, 4096, 4097, 30_000, 65_536] {
        let lows: Vec<u16> = (0..n).map(|i| ((i * 65_536) / n) as u16).collect();
        let raw = cells::encode(&lows);
        assert_eq!(raw.len(), if n <= 4096 { 2 * n } else { 8192 }, "{n}");
        assert_eq!(cells::decode(n as u64, &raw).unwrap(), lows, "{n}");
    }
    assert!(cells::decode(0, &[]).is_err());
    assert!(cells::decode(2, &[1, 0, 1, 0]).unwrap_err().contains("ascending"));
    assert!(cells::decode(3, &[1, 0]).unwrap_err().contains("holds 2 bytes"));
    assert!(cells::decode(5000, &[0; 8192]).unwrap_err().contains("holds 0"));
}

/// The cells' bitmaps against the scan on a store with every status, kind,
/// severity, blocked, commits and — FILTER-21 — menger doc mixed in, and
/// none of them testing a row per ticket: a count is the bitmap's length.
#[tokio::test]
async fn every_dimension_of_a_cell_is_kept() {
    let store = Store::new();
    store.create(new_ticket("DIM", Kind::Scope, None, vec![])).await.unwrap();
    let severities = [None, Some(Severity::Critical), Some(Severity::High), Some(Severity::Medium), Some(Severity::Low)];
    let mut n = 1;
    for (i, kind) in Kind::ALL.into_iter().enumerate() {
        for (j, severity) in severities.into_iter().enumerate() {
            let blocked_by = if kind != Kind::Doc && (i + j) % 2 == 0 { vec!["DIM-1".to_string()] } else { vec![] };
            let t = store
                .create(NewTicket { severity, ..new_ticket("DIM", kind, None, blocked_by) })
                .await
                .unwrap();
            n += 1;
            if kind != Kind::Doc {
                store.set_status(&t.id, Status::LIFECYCLE[n % Status::LIFECYCLE.len()]).await.unwrap();
            }
            if n % 3 == 0 {
                store.link_commits(&t.id, vec!["fedcba9".into()]).await.unwrap();
            }
        }
    }
    store.create(NewTicket { path: Some("_MENGER/board".into()), ..new_ticket("DIM", Kind::Doc, None, vec![]) }).await.unwrap();
    assert_cells_are_a_scan(&store, None, "every dimension");
    let kept = store.tickets.read().unwrap().cells.by_ticket();
    let seen = |f: fn(&Key) -> String| kept.values().map(f).collect::<BTreeSet<String>>().len();
    assert_eq!(seen(|k| k.kind().as_str().into()), 6);
    assert_eq!(seen(|k| format!("{:?}", k.severity())), 5);
    assert_eq!(seen(|k| k.blocked().to_string()), 2);
    assert_eq!(seen(|k| k.commits().to_string()), 2);
    assert_eq!(seen(|k| k.menger().to_string()), 2);
    assert!(seen(|k| k.status().as_str().into()) >= 9);
}

/// Not a test: where a k = 10⁵ crossing's time goes, phase by phase, on a
/// copy of `perf/fan-out.py`'s file (`FAN_DB`), FAN-4 blocking 10⁵.
/// `FAN_DB=… cargo test --release -p menger-tickets-api --lib fan_out_phases -- --ignored --nocapture`
#[tokio::test]
#[ignore]
async fn fan_out_phases() {
    let Ok(source) = std::env::var("FAN_DB") else {
        eprintln!("FAN_DB names perf/fan-out.py's file");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("fan.db");
    std::fs::copy(source, &path).unwrap();
    let store = Store::open(&path).unwrap();
    let hub = "FAN-4".to_string();
    for status in [Status::Complete, Status::Ready, Status::Complete, Status::Ready] {
        let cache = store.tickets.read().unwrap();
        let mut row = cache.get(&hub).unwrap().clone();
        let previous = row.status;
        row.status = status;
        let rows = cache.rows().with(&row);
        let t = std::time::Instant::now();
        let (_ready, touched) = cache.ready.status_changed_touching(rows, &row, previous);
        let t_ready = t.elapsed();
        let t = std::time::Instant::now();
        let (cells, _tree) = cache.recell(rows, &row, false, &touched);
        let t_cells = t.elapsed();
        let t = std::time::Instant::now();
        let mut stored = stored::Changes::default();
        cells.store(&mut stored);
        let t_store = t.elapsed();
        let mut copy = cache.cells.clone();
        let t = std::time::Instant::now();
        copy.take(cells);
        let t_take = t.elapsed();
        drop(cache);
        let t = std::time::Instant::now();
        store.set_status(&hub, status).await.unwrap();
        let t_write = t.elapsed();
        println!(
            "{previous} -> {status}: ready move {:.1} ms, cell move {:.1} ms, its rows {:.1} ms, cache take {:.1} ms; the whole write {:.1} ms",
            t_ready.as_secs_f64() * 1e3,
            t_cells.as_secs_f64() * 1e3,
            t_store.as_secs_f64() * 1e3,
            t_take.as_secs_f64() * 1e3,
            t_write.as_secs_f64() * 1e3
        );
    }
}
