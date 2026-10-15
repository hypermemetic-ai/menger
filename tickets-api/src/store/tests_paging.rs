//! SCALE-19: `page.remaining` by arithmetic — the proof that it is the
//! count.
//!
//! The oracle here is MNG-9's rule applied to the store's whole order as a
//! plain list: the tickets under the epic and past the cursor, the first
//! `limit` of them the page, and everything left *counted*. Every page the
//! store answers — `rows`, `next` and `remaining` — must be that, for
//! stores built every way tickets reach the cache: `create`, a seed (fresh
//! and into an existing store), a load, both in-place migrations, and a
//! database written by hand with holes in its sequences, a sequence 0 and
//! a lowercase shortcode — the shapes only a hand edit makes, which the API
//! cannot.

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

fn scope(epic: &str) -> NewTicket {
    NewTicket {
        path: None,
        epic: epic.into(),
        title: "t".into(),
        kind: Kind::Scope,
        severity: None,
        blocked_by: Vec::new(),
        parent: None,
        body: String::new(),
    }
}

fn build(epic: &str) -> NewTicket {
    NewTicket {
        path: None,
        kind: Kind::Build,
        ..scope(epic)
    }
}

/// A create in `epic` — its scope root first if the shortcode is new.
async fn create_in(store: &Store, epic: &str) -> Ticket {
    let new = if store.extents_text().iter().any(|(e, _)| e == epic) {
        build(epic)
    } else {
        scope(epic)
    };
    store.create(new).await.unwrap()
}

/// Writes tickets straight through the schema, `(epic, seq)` each, id
/// `EPIC-seq` — any shape, including the ones the server never makes.
pub(super) fn write_runs(path: &Path, rows: &[(&str, u32)]) {
    let conn = Connection::open(path).unwrap();
    conn.execute_batch(db::SCHEMA).unwrap();
    for (epic, seq) in rows {
        db::upsert(
            &conn,
            &Ticket {
                hidden_blocked_by: 0,
                hidden_unlocks: 0,
                parent_hidden: false,
                labels: Vec::new(),
                id: format!("{epic}-{seq}").into(),
                epic: (*epic).into(),
                shortcode: (*epic).into(),
                address: String::new(),
                seq: *seq,
                title: format!("{epic}-{seq}"),
                status: Status::Pending,
                kind: Kind::Scope,
                severity: None,
                blocked_by: Vec::new(),
                unlocks: Vec::new(),
                parent: None,
                child_count: 0,
                commits: Vec::new(),
                path: None,
                placement: None,
                body: String::new(),
                created_unix: 0,
                updated_unix: 0,
                moved_from: Vec::new(),
            },
        )
        .unwrap();
    }
}

/// What a page is, `(rows, next, remaining)`.
type Answer = (Vec<String>, Option<String>, u64);

/// The store's order as a plain list of `(id, key)`, read once per state
/// of the store: the oracle's whole input.
pub(super) struct Oracle {
    rows: Vec<(String, OrderKey)>,
}

impl Oracle {
    pub(super) fn of(store: &Store) -> Self {
        let cache = store.tickets.read().unwrap();
        Self {
            rows: cache.order.iter().map(|(k, t)| (t.id.to_string(), k.clone())).collect(),
        }
    }

    /// MNG-9's page, by filtering a list and counting what is left over.
    fn page(&self, q: &PageQuery<'_>) -> Result<Answer, String> {
        let epic = q.epic.map(str::to_ascii_uppercase);
        let after = q.after.map(str::to_ascii_uppercase);
        let cursor = match &after {
            None => None,
            Some(id) => match self.rows.iter().find(|(i, _)| i == id) {
                Some((_, key)) => Some(key.clone()),
                None => return Err(format!("after: no ticket {id}")),
            },
        };
        let matching: Vec<&String> = self
            .rows
            .iter()
            .filter(|(_, k)| epic.as_deref().is_none_or(|e| k.shortcode.as_str() == e))
            .filter(|(_, k)| cursor.as_ref().is_none_or(|c| k > c))
            .map(|(id, _)| id)
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

    fn ids(&self) -> Vec<&str> {
        self.rows.iter().map(|(id, _)| id.as_str()).collect()
    }

    /// Each shortcode as a scan finds it: `(lowest, highest, how many,
    /// every sequence 1..=highest)` — what the cache keeps, worked out.
    pub(super) fn runs(&self) -> BTreeMap<ShortcodeId, (u32, u32, u64, bool)> {
        let mut seqs: BTreeMap<ShortcodeId, BTreeSet<u32>> = BTreeMap::new();
        for (_, k) in &self.rows {
            seqs.entry(k.shortcode.clone()).or_default().insert(k.seq);
        }
        seqs.into_iter()
            .map(|(epic, s)| {
                let (first, last) = (*s.first().unwrap(), *s.last().unwrap());
                let dense = s.iter().copied().eq(1..=last);
                (epic, (first, last, s.len() as u64, dense))
            })
            .collect()
    }
}

fn answered(store: &Store, q: &PageQuery<'_>) -> Result<Answer, String> {
    store
        .page(SYS, *q)
        .map(|p| (p.tickets.iter().map(|t| t.id.to_string()).collect(), p.next, p.remaining))
        .map_err(|e| e.to_string())
}

/// What the cache keeps about each shortcode is what a scan finds.
#[track_caller]
fn assert_runs_are_a_scan(store: &Store, after: &str) {
    let kept = store.tickets.read().unwrap().ids.runs();
    assert_eq!(kept, Oracle::of(store).runs(), "after {after}");
}

/// The page the store answers is the oracle's, for `q`.
#[track_caller]
fn assert_page(store: &Store, oracle: &Oracle, q: PageQuery<'_>, what: &str) {
    assert_eq!(answered(store, &q), oracle.page(&q), "{what}: {q:?}");
}

/// Walks the whole of `epic` (or the store) in pages of `limit`, as a
/// client does, checking every page against the oracle on the way; returns
/// what it visited.
///
/// A walk that reaches a ticket whose id is not uppercase — only a hand
/// edit writes one — ends there: `after` is uppercased (MNG-9), so the id
/// handed back as `next` names no ticket when it is passed in. That is
/// older than SCALE-19 and not its to change; the oracle agrees with it,
/// and the walk stops on the error both give.
#[track_caller]
fn walk(store: &Store, oracle: &Oracle, epic: Option<&str>, limit: u32, what: &str) -> Vec<String> {
    let mut seen = Vec::new();
    let mut after: Option<String> = None;
    loop {
        let q = PageQuery {
            epic,
            after: after.as_deref(),
            limit: Some(limit),
            ..PageQuery::default()
        };
        assert_page(store, oracle, q, what);
        let Ok(page) = store.page(SYS, q) else {
            return seen;
        };
        seen.extend(page.tickets.iter().map(|t| t.id.to_string()));
        match page.next {
            Some(next) => after = Some(next),
            None => return seen,
        }
    }
}

/// Every cursor worth trying against `oracle`'s store: none, every
/// shortcode's first and last ticket and one inside it, and a few at
/// random. Plus one that names no ticket.
fn cursors(oracle: &Oracle, rng: &mut Rng) -> Vec<Option<String>> {
    let mut out: Vec<Option<String>> = vec![None, Some("NO-SUCH-1".into())];
    let ids = oracle.ids();
    let mut by_epic: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for (id, k) in &oracle.rows {
        by_epic.entry(k.shortcode.as_str()).or_default().push(id);
    }
    for run in by_epic.values() {
        out.push(Some(run[0].to_string()));
        out.push(Some(run[run.len() / 2].to_string()));
        out.push(Some(run[run.len() - 1].to_string()));
    }
    for _ in 0..8 {
        if !ids.is_empty() {
            out.push(Some(ids[rng.below(ids.len())].to_lowercase()));
        }
    }
    out
}

/// Every epic worth filtering on: none, each one held, a lowercase spelling
/// (filters are case-insensitive), and one that holds nothing.
fn epics(oracle: &Oracle) -> Vec<Option<String>> {
    let held: BTreeSet<&str> = oracle.rows.iter().map(|(_, k)| k.shortcode.as_str()).collect();
    let mut out: Vec<Option<String>> = vec![None, Some("NOTHING".into())];
    for e in held {
        out.push(Some(e.to_string()));
        out.push(Some(e.to_lowercase()));
    }
    out
}

/// Every epic × cursor × limit, then whole walks of the store and of each
/// epic at a few page sizes: each page equal to the oracle's.
#[track_caller]
fn assert_every_page_is_counted(store: &Store, rng: &mut Rng, what: &str) {
    let oracle = Oracle::of(store);
    assert_runs_are_a_scan(store, what);
    let limits = [None, Some(0), Some(1), Some(2), Some(3), Some(7), Some(200)];
    for epic in epics(&oracle) {
        for after in cursors(&oracle, rng) {
            for limit in limits {
                let q = PageQuery {
                    epic: epic.as_deref(),
                    after: after.as_deref(),
                    limit,
                    ..PageQuery::default()
                };
                assert_page(store, &oracle, q, what);
            }
        }
    }
    let passable = oracle.ids().iter().all(|id| *id == id.to_ascii_uppercase());
    for limit in [1, 2 + rng.below(5) as u32, 200] {
        let seen = walk(store, &oracle, None, limit, what);
        if passable {
            assert_eq!(seen, oracle.ids(), "{what}: a walk visits all");
        }
        for epic in epics(&oracle).into_iter().flatten() {
            walk(store, &oracle, Some(&epic), limit, what);
        }
    }
}

/// Acceptance 2: a store with a hole in one shortcode's sequences —
/// `GAP-1, GAP-2, GAP-5, GAP-6`, between two dense shortcodes — answers
/// every page with the count, for cursors before, inside (on each side of
/// the hole) and after that shortcode, unfiltered and filtered to it; and
/// the creates that follow keep it exact: one past the hole (still a
/// hole), and the store's own shortcodes beside it.
#[tokio::test]
async fn a_hole_in_a_shortcode_still_pages_by_the_count() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("hole.db");
    write_runs(
        &path,
        &[
            ("AAA", 1),
            ("AAA", 2),
            ("AAA", 3),
            ("GAP", 1),
            ("GAP", 2),
            ("GAP", 5),
            ("GAP", 6),
            ("ZED", 1),
            ("ZED", 2),
        ],
    );
    let store = Store::open(&path).unwrap();
    let runs = store.tickets.read().unwrap().ids.runs();
    assert_eq!(runs[&ShortcodeId::new("GAP")], (1, 6, 4, false), "GAP holds 4 of 6: not dense");
    assert_eq!(runs[&ShortcodeId::new("AAA")], (1, 3, 3, true));

    let oracle = Oracle::of(&store);
    for after in [None, Some("AAA-2"), Some("AAA-3"), Some("GAP-1"), Some("GAP-2"), Some("GAP-5"), Some("GAP-6"), Some("ZED-1"), Some("ZED-2")] {
        for epic in [None, Some("GAP"), Some("AAA"), Some("ZED")] {
            for limit in [None, Some(0), Some(1), Some(2), Some(3)] {
                let q = PageQuery {
                    epic,
                    after,
                    limit,
                    ..PageQuery::default()
                };
                assert_page(&store, &oracle, q, "the hole");
            }
        }
    }
    // The numbers themselves, for the cursors the ticket names.
    let page = |after, epic| {
        store
            .page(SYS, PageQuery {
                epic,
                after,
                limit: Some(1),
                ..PageQuery::default()
            })
            .unwrap()
    };
    assert_eq!(page(Some("AAA-3"), None).remaining, 5, "before GAP: GAP-2, 5, 6 and ZED's two");
    assert_eq!(page(Some("GAP-1"), None).remaining, 4, "inside, before the hole: 5, 6, ZED-1, ZED-2");
    assert_eq!(page(Some("GAP-2"), None).remaining, 3, "inside, across the hole — 6 − 2 would say 4");
    assert_eq!(page(Some("GAP-5"), Some("GAP")).remaining, 0, "inside, after the hole");
    assert_eq!(page(Some("ZED-1"), None).remaining, 0, "after GAP");
    assert!(matches!(
        store.page(SYS, PageQuery {
            after: Some("GAP-3"),
            ..PageQuery::default()
        }),
        Err(StoreError::CursorNotFound(_))
    ), "the hole itself is no cursor");

    // Births keep it: GAP-7 past the hole (GAP stays holed), AAA-4 and a
    // new shortcode (dense).
    for epic in ["GAP", "AAA", "NEW", "GAP", "NEW"] {
        create_in(&store, epic).await;
        assert_every_page_is_counted(&store, &mut Rng(7), &format!("a create in {epic}"));
    }
    let runs = store.tickets.read().unwrap().ids.runs();
    assert_eq!(runs[&ShortcodeId::new("GAP")], (1, 8, 6, false));
    assert_eq!(runs[&ShortcodeId::new("NEW")], (1, 2, 2, true));
}

/// A sequence 0 is a hole of another shape: `ODD-0..ODD-2` holds three
/// tickets with highest sequence 2, and `ZRO-0, ZRO-1, ZRO-3` holds as
/// many tickets as its highest sequence — the count alone would call it
/// dense, and `3 − 0` would be one too many after `ZRO-0`. Neither is, and
/// both page by the count.
#[tokio::test]
async fn a_sequence_zero_is_not_dense() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("zero.db");
    write_runs(&path, &[("ODD", 0), ("ODD", 1), ("ODD", 2), ("ZRO", 0), ("ZRO", 1), ("ZRO", 3)]);
    let store = Store::open(&path).unwrap();
    let runs = store.tickets.read().unwrap().ids.runs();
    assert_eq!(runs[&ShortcodeId::new("ODD")], (0, 2, 3, false));
    assert_eq!(runs[&ShortcodeId::new("ZRO")], (0, 3, 3, false), "held == last, and still a hole");
    let page = store
        .page(SYS, PageQuery {
            after: Some("ZRO-0"),
            limit: Some(0),
            ..PageQuery::default()
        })
        .unwrap();
    assert_eq!(page.remaining, 2, "ZRO-1 and ZRO-3");
    assert_every_page_is_counted(&store, &mut Rng(0), "sequence 0");
}

/// Acceptance 3: for random stores, cursors, epics and limits, `remaining`
/// (and the rows, and `next`) equal the count. Stores built through
/// `create` — shortcodes interleaved, so birth order is not the store's
/// order — then a seed into it; a fresh seed; and random databases written
/// by hand with holes, sequence 0s and a lowercase shortcode, opened and
/// then grown by creates, each of which may land in a holed shortcode.
#[tokio::test]
async fn remaining_is_the_count_for_random_stores_cursors_epics_and_limits() {
    let mut rng = Rng(0x0053_4341_4C45_3139);
    const EPICS: [&str; 7] = ["A", "AB", "B", "BA", "MID", "Z", "ZZ"];

    // Through create, checked as it grows.
    let store = Store::new();
    assert_every_page_is_counted(&store, &mut rng, "an empty store");
    for step in 0..120 {
        create_in(&store, EPICS[rng.below(EPICS.len())]).await;
        if step % 15 == 14 {
            assert_every_page_is_counted(&store, &mut rng, &format!("create {step}"));
        }
    }
    // A seed into it: some of its shortcodes may be the seed's own.
    store
        .seed_synthetic(SeedSpec {
            count: 400,
            seed: 3,
            into_existing: true,
        })
        .unwrap();
    assert_every_page_is_counted(&store, &mut rng, "a seed into a created store");

    for seed in [1, 2] {
        let store = Store::new();
        store
            .seed_synthetic(SeedSpec {
                count: 600,
                seed,
                into_existing: false,
            })
            .unwrap();
        assert_every_page_is_counted(&store, &mut rng, &format!("a fresh seed {seed}"));
        create_in(&store, "ZZZ").await;
        assert_every_page_is_counted(&store, &mut rng, "a create after a seed");
    }

    // By hand: each shortcode a random subset of 0..=12 — dense, holed,
    // or starting at 0.
    let dir = tempfile::tempdir().unwrap();
    for round in 0..12 {
        let path = dir.path().join(format!("random-{round}.db"));
        let mut rows: Vec<(&str, u32)> = Vec::new();
        for epic in ["A", "AB", "B", "MID", "Zz", "low"] {
            match rng.below(4) {
                0 => {} // absent
                1 => rows.extend((1..=1 + rng.below(9) as u32).map(|s| (epic, s))),
                _ => rows.extend((0..=12).filter(|_| rng.below(3) > 0).map(|s| (epic, s))),
            }
        }
        write_runs(&path, &rows);
        let store = Store::open(&path).unwrap();
        assert_every_page_is_counted(&store, &mut rng, &format!("random database {round}: {rows:?}"));
        for _ in 0..3 {
            let epic = ["A", "AB", "B", "MID", "NEW"][rng.below(5)];
            create_in(&store, epic).await;
        }
        assert_every_page_is_counted(&store, &mut rng, &format!("creates into random database {round}"));
    }
}

/// The paths the kept runs are built or moved by, each against a scan: a
/// load, a reopen, both in-place migrations (pre-AGS-13 `ticket_type` and
/// pre-AGS-19 no `parent`), each holding a hole), every create, and a seed
/// fresh and into an existing store. `set_status` and `set_parent` change
/// no shortcode's tickets, and move nothing.
#[tokio::test]
async fn the_kept_runs_are_a_scan_after_every_path_that_brings_tickets_in() {
    let dir = tempfile::tempdir().unwrap();

    let path = dir.path().join("pre-ags13.db");
    {
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch(
            "CREATE TABLE tickets (id TEXT PRIMARY KEY, epic TEXT NOT NULL, seq INTEGER NOT NULL,
                 title TEXT NOT NULL, status TEXT NOT NULL, ticket_type TEXT NOT NULL, severity TEXT,
                 blocked_by TEXT NOT NULL, unlocks TEXT NOT NULL, body TEXT NOT NULL,
                 created_unix INTEGER NOT NULL, updated_unix INTEGER NOT NULL, UNIQUE (epic, seq));",
        )
        .unwrap();
        for seq in [1, 2, 4] {
            conn.execute(
                "INSERT INTO tickets VALUES (?1, 'LEG', ?2, 'legacy', 'Pending', 'epic', NULL, '[]', '[]', 'body', 1, 1)",
                rusqlite::params![format!("LEG-{seq}"), seq],
            )
            .unwrap();
        }
    }
    let store = Store::open(&path).unwrap();
    assert_runs_are_a_scan(&store, "the AGS-13 migration");
    assert_eq!(store.tickets.read().unwrap().ids.runs()[&ShortcodeId::new("LEG")], (1, 4, 3, false));
    assert_every_page_is_counted(&store, &mut Rng(13), "the AGS-13 migration");

    let path = dir.path().join("pre-ags19.db");
    {
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch(
            "CREATE TABLE tickets (id TEXT PRIMARY KEY, epic TEXT NOT NULL, seq INTEGER NOT NULL,
                 title TEXT NOT NULL, status TEXT NOT NULL, kind TEXT NOT NULL, severity TEXT,
                 blocked_by TEXT NOT NULL, unlocks TEXT NOT NULL, body TEXT NOT NULL,
                 created_unix INTEGER NOT NULL, updated_unix INTEGER NOT NULL, UNIQUE (epic, seq));",
        )
        .unwrap();
        for (seq, body) in [(1, "prose"), (2, "---\nparent: OLD-1\n---\n"), (3, "prose"), (5, "prose")] {
            conn.execute(
                "INSERT INTO tickets VALUES (?1, 'OLD', ?2, 'old', 'Pending', 'scope', NULL, '[]', '[]', ?3, 1, 1)",
                rusqlite::params![format!("OLD-{seq}"), seq, body],
            )
            .unwrap();
        }
    }
    let store = Store::open(&path).unwrap();
    assert_eq!(store.get(SYS, "OLD-2").await.unwrap().parent.as_deref(), Some("OLD-1"), "migrated");
    assert_runs_are_a_scan(&store, "the AGS-19 migration");
    assert_every_page_is_counted(&store, &mut Rng(19), "the AGS-19 migration");

    // Creates — a new shortcode, one past a hole — then the ones that
    // change no shortcode's tickets, then a reopen and a seed into it.
    let t = store.create(build("OLD")).await.unwrap();
    assert_eq!(t.id, "OLD-6", "past the hole, which stays");
    assert_runs_are_a_scan(&store, "a create past a hole");
    store.create(scope("NEW")).await.unwrap();
    store.create(build("NEW")).await.unwrap();
    assert_runs_are_a_scan(&store, "a new shortcode");
    let before = store.tickets.read().unwrap().ids.runs();
    store.set_status("NEW-2", Status::Complete).await.unwrap();
    store.set_parent("OLD-3", "OLD-1").await.unwrap();
    assert!(store.create(build("FRESH")).await.is_err(), "refused: no scope first");
    assert_eq!(store.tickets.read().unwrap().ids.runs(), before, "nothing moved");
    drop(store);
    let store = Store::open(&path).unwrap();
    assert_eq!(store.tickets.read().unwrap().ids.runs(), before, "a reopen rebuilds what was kept");
    store
        .seed_synthetic(SeedSpec {
            count: 300,
            seed: 4,
            into_existing: true,
        })
        .unwrap();
    assert_runs_are_a_scan(&store, "a seed into an existing store");

    let fresh = Store::new();
    fresh
        .seed_synthetic(SeedSpec {
            count: 2_000,
            seed: 9,
            into_existing: false,
        })
        .unwrap();
    assert_runs_are_a_scan(&fresh, "a fresh seed");
    assert!(
        fresh.tickets.read().unwrap().ids.runs().values().all(|r| r.3),
        "every shortcode the server makes is dense"
    );
}

/// Concurrent creates in two shortcodes — the race the write lock
/// serializes — leave the kept runs a scan's, and pages exact.
#[tokio::test]
async fn racing_creates_keep_the_runs_and_the_pages_exact() {
    let store = Arc::new(Store::new());
    store.create(scope("RACE")).await.unwrap();
    store.create(scope("PACE")).await.unwrap();
    let mut handles = Vec::new();
    for i in 0..40 {
        let store = store.clone();
        let epic = if i % 3 == 0 { "PACE" } else { "RACE" };
        handles.push(tokio::spawn(async move { store.create(build(epic)).await.unwrap() }));
    }
    for h in handles {
        h.await.unwrap();
    }
    assert_every_page_is_counted(&store, &mut Rng(40), "the race");
}
