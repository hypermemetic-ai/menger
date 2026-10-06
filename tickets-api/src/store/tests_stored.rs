//! SCALE-28: the derived state, stored — the proof that what the file holds
//! beside the tickets is what a scan of them gives, read back through a
//! connection of its own (never the store's, never the cache), after every
//! write of every kind: on stores built by `create`, by a seed, from each
//! fixture written before SCALE-28 and from hand-edited shapes (MNG-17's
//! parent cycles and dangling parents, SCALE-19's holes). The oracles are
//! the scans each structure was proved against before it was stored:
//! SCALE-19's and SCALE-3's for the runs and forward references, SCALE-4's
//! for the children and `child_count`, MNG-10's — the nav's own `plan()` —
//! for the roll-ups. And: a first open builds once and every later open
//! reads; the seed's marker; a write whose commit fails stores nothing;
//! `--verify-derived` names what differs and writes nothing.

use super::stored::{self, Form, RunRow, State};
use super::*;

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

fn new_ticket(epic: &str, kind: Kind, parent: Option<String>, blocked_by: Vec<String>) -> NewTicket {
    NewTicket {
        path: None,
        epic: epic.into(),
        title: format!("a {} in {epic}", kind.as_str()),
        kind,
        severity: None,
        blocked_by,
        parent,
        body: format!("the body of a {} in {epic}", kind.as_str()),
    }
}

/// What the file holds, read on a read-only connection of its own in one
/// read transaction: where its derived state stands, every ticket's row
/// (`child_count` included), the stored derived state, and the children as
/// the file's index gives them.
struct File {
    state: State,
    rows: BTreeMap<String, Row>,
    form: Form,
    children: children::Children,
}

fn read_back(path: &Path) -> File {
    let mut conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
    let tx = conn.transaction().unwrap();
    File {
        state: stored::state(&tx).unwrap(),
        rows: db::load_rows(&tx).unwrap().into_iter().collect(),
        form: stored::read(&tx).unwrap(),
        children: stored::read_children(&tx).unwrap(),
    }
}

/// Acceptance 1: the stored derived state, read back from the file, is what
/// each scan oracle computes from every ticket — and what the cache holds.
#[track_caller]
fn assert_stored_is_a_scan(store: &Store, path: &Path, after: &str) {
    let file = read_back(path);
    assert_eq!(file.state, State::Current, "after {after}");
    // The file's tickets are the cache's — so an oracle run over the store's
    // rows is a scan of the stored tickets.
    let cached: BTreeMap<String, Row> = {
        let cache = store.tickets.read().unwrap();
        cache.order.values().map(|t| (t.id.clone(), t.clone())).collect()
    };
    assert_eq!(file.rows, cached, "the file's rows are the cache's, after {after}");

    // SCALE-19's oracle: each shortcode's lowest and highest sequence and
    // how many it holds.
    let runs: BTreeMap<String, RunRow> = tests_paging::Oracle::of(store)
        .runs()
        .into_iter()
        .map(|(epic, (first, last, held, _))| (epic, (first, last, held)))
        .collect();
    assert_eq!(file.form.runs, runs, "the shortcode runs, after {after}");
    // SCALE-3's: each shortcode's highest sequence, and every id some
    // `blocked_by` names that is no ticket, with who names it in the store's
    // order.
    let (last, unborn) = tests::scanned(store);
    let stored_last: BTreeMap<String, u32> = file.form.runs.iter().map(|(e, r)| (e.clone(), r.1)).collect();
    assert_eq!(stored_last, last, "each shortcode's highest sequence, after {after}");
    let mut refs: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for ((id, _), referrer) in &file.form.refs {
        refs.entry(id.clone()).or_default().push(referrer.clone());
    }
    assert_eq!(refs, unborn, "the dangling forward references, after {after}");

    // SCALE-4's: every parent's children in the store's order, from the
    // file's index; and each ticket's `child_count` their number.
    let kids = tests_children::scanned(store);
    assert_eq!(file.children.all(), kids, "the children index, after {after}");
    let counts: BTreeMap<String, u64> = kids
        .iter()
        .filter(|(p, _)| file.rows.contains_key(*p))
        .map(|(p, v)| (p.clone(), v.len() as u64))
        .collect();
    assert_eq!(file.form.counts, counts, "every child_count, after {after}");

    // MNG-10's: the rows the nav's `plan()` draws are the stored groups'.
    let drawn: Vec<Group> = file
        .form
        .groups
        .iter()
        .filter(|(_, g)| g.own.0 > 0 || g.own.2 > 0)
        .map(|(epic, g)| Group {
            epic: epic.clone(),
            total: g.roll.0,
            done: g.roll.1,
            docs: g.roll.2,
            root: g.root.clone(),
            // AGS-23: the root's row is the file's row of the root.
            root_row: g.root.as_ref().and_then(|r| file.rows.get(r)).map(|t| RootRow {
                title: t.title.clone(),
                status: t.status,
                kind: t.kind,
            }),
            host: g.host.clone(),
        })
        .collect();
    assert_eq!(drawn, tests::nav(store), "the roll-ups the nav draws, after {after}");

    // Every field the roll-ups keep — the pools, the cross trees, the marks a
    // hand edit leaves — is what a build by scan of the file's tickets stores:
    // `--verify-derived`'s own comparison, and its own scan.
    let scanned = Cache::new(file.rows.clone().into_iter().collect());
    assert_eq!(stored::compare(&file.form, &scanned.form()), Vec::<String>::new(), "after {after}");
    assert!(scanned.children.differences(&file.children).is_empty(), "after {after}");

    // And the cache's copies, which still answer, are the stored ones.
    let cache = store.tickets.read().unwrap();
    assert_eq!(cache.form(), file.form, "the cache's derived state is the file's, after {after}");
    assert!(cache.children.differences(&file.children).is_empty(), "the cache's index, after {after}");
}

/// How many writes of each kind changed something, for the walk's own check
/// that it exercised every one.
#[derive(Debug, Default)]
struct Tally {
    created: usize,
    refused: usize,
    statuses: usize,
    crossings: usize,
    moves: usize,
    amends: usize,
    links: usize,
    unlinks: usize,
    forward: usize,
    /// SDLC-11: docs born, and creates tried (what picks a doc).
    docs: usize,
    attempts: usize,
}

/// One random write — any of the six, legal or refused — against the store
/// as it stands.
async fn random_write(store: &Store, rng: &mut Rng, epics: &[&str], tally: &mut Tally) -> String {
    // Ids the API can name: a lowercase one (only a hand edit writes one)
    // is folded to another by `amend` and its kin (SCALE-19, "found").
    let all: Vec<Row> = store
        .page(PageQuery::default())
        .unwrap()
        .tickets
        .into_iter()
        .filter(|t| t.id == t.id.to_ascii_uppercase())
        .collect();
    let extents: BTreeMap<String, u32> = store.extents().into_iter().collect();
    let pick = |rng: &mut Rng| all[rng.below(all.len())].id.clone();
    let kind = if all.is_empty() { 0 } else { rng.below(20) };
    match kind {
        0..=7 => {
            let epic = epics[rng.below(epics.len())];
            let exists = extents.contains_key(epic);
            let kind = if !exists || rng.below(5) == 0 { Kind::Scope } else { Kind::Build };
            // SDLC-11: every ninth create a doc — a new shortcode's root, or a
            // ticket inside one — with its roll-ups, counts and refusals. By a
            // count, not a draw: the walk's draws stay the ones it had.
            tally.attempts += 1;
            let kind = if tally.attempts % 9 == 4 { Kind::Doc } else { kind };
            let mine: Vec<&Row> = all.iter().filter(|t| t.epic == epic).collect();
            let parent = match (exists, rng.below(4)) {
                // A new shortcode's root: top level, or a sub-shortcode
                // hosted anywhere.
                (false, 0 | 1) | (true, 0) => None,
                (false, _) => (!all.is_empty()).then(|| pick(rng)),
                // Inside the shortcode, or (refused) outside it.
                (true, 1 | 2) => Some(mine[rng.below(mine.len())].id.clone()),
                (true, _) => Some(pick(rng)),
            };
            // Blockers: a ticket that exists, an id named before birth (the
            // next few of some shortcode — born later in the walk, or not),
            // or one that can never be.
            let mut blocked_by = Vec::new();
            for _ in 0..rng.below(3) {
                blocked_by.push(match rng.below(4) {
                    0 if !all.is_empty() => pick(rng),
                    0..=2 => {
                        let e = epics[rng.below(epics.len())];
                        let next = extents.get(e).copied().unwrap_or(0) + 1;
                        format!("{e}-{}", next + rng.below(3) as u32)
                    }
                    _ => "NEVER-0".to_string(),
                });
            }
            // A doc takes no blocked_by: most of the time it is born without
            // one, and otherwise refused.
            if kind == Kind::Doc && !tally.attempts.is_multiple_of(4) {
                blocked_by.clear();
            }
            let forward = blocked_by.iter().any(|b| !all.iter().any(|t| &t.id == b));
            let what = format!("create {epic} {kind} under {parent:?} blocked by {blocked_by:?}");
            match store.create(new_ticket(epic, kind, parent, blocked_by)).await {
                Ok(_) => {
                    tally.created += 1;
                    tally.forward += usize::from(forward);
                    tally.docs += usize::from(kind == Kind::Doc);
                }
                Err(_) => tally.refused += 1,
            }
            what
        }
        8..=10 => {
            let id = pick(rng);
            let status = if rng.below(2) == 0 {
                Status::Complete
            } else {
                Status::ALL[rng.below(Status::ALL.len())]
            };
            let (before, kind) = {
                let row = store.row(&id).unwrap().0;
                (row.status, row.kind)
            };
            // SDLC-11: a doc's status never moves, and Doc is a doc's alone.
            match store.set_status(&id, status).await {
                Ok(_) => {
                    tally.statuses += 1;
                    tally.crossings += usize::from((before == Status::Complete) != (status == Status::Complete));
                }
                Err(StoreError::DocHasNoLifecycle { .. }) if kind == Kind::Doc => tally.refused += 1,
                Err(StoreError::DocStatusOnWork { .. }) if status == Status::Doc => tally.refused += 1,
                Err(e) => panic!("set_status {id} {status}: {e}"),
            }
            format!("set_status {id} {status}")
        }
        11..=13 => {
            let child = pick(rng);
            let epic = store.row(&child).unwrap().0.epic;
            let mine: Vec<&Row> = all.iter().filter(|t| t.epic == epic).collect();
            let parent = if rng.below(3) > 0 {
                mine[rng.below(mine.len())].id.clone()
            } else {
                pick(rng)
            };
            let was = store.row(&child).unwrap().0.parent;
            match store.set_parent(&child, &parent).await {
                Ok(_) if was.as_deref() != Some(parent.as_str()) => tally.moves += 1,
                Ok(_) => {}
                Err(_) => tally.refused += 1,
            }
            format!("set_parent {child} under {parent}")
        }
        14 | 15 => {
            let id = pick(rng);
            let n = rng.below(1000);
            store
                .amend(&id, Some(format!("amended {n}")), (rng.below(2) == 0).then(|| format!("body {n}")))
                .await
                .unwrap();
            tally.amends += 1;
            format!("amend {id}")
        }
        16 | 17 => {
            let id = pick(rng);
            let commit = format!("{:07x}", 0x1000000 + rng.below(8));
            store.link_commits(&id, vec![commit.clone()]).await.unwrap();
            tally.links += 1;
            format!("link_commits {id} {commit}")
        }
        _ => {
            let id = pick(rng);
            let linked = store.row(&id).unwrap().0.commits;
            let commit = match linked.first() {
                Some(c) if rng.below(3) > 0 => c.clone(),
                _ => "abcdef0".to_string(),
            };
            store.unlink_commits(&id, vec![commit.clone()]).await.unwrap();
            tally.unlinks += 1;
            format!("unlink_commits {id} {commit}")
        }
    }
}

/// `writes` random writes, the stored state held to the scans after each;
/// then a reopen, whose cache is read from the file and is the one before.
async fn walk(store: Store, path: &Path, seed: u64, writes: usize, epics: &[&str]) -> Store {
    let mut rng = Rng(seed);
    let mut tally = Tally::default();
    for step in 0..writes {
        let what = random_write(&store, &mut rng, epics, &mut tally).await;
        assert_stored_is_a_scan(&store, path, &format!("step {step}: {what}"));
    }
    assert!(
        tally.created > writes / 5
            && tally.refused > 0
            && tally.crossings > writes / 30
            && tally.moves > writes / 40
            && tally.amends > 0
            && tally.links > 0
            && tally.unlinks > 0
            && tally.forward > 0
            && tally.docs > 0,
        "every kind of write, and enough of them: {tally:?}"
    );
    let form = store.tickets.read().unwrap().form();
    let (groups, extents) = (store.groups(), store.extents());
    drop(store);
    let store = Store::open(path).unwrap();
    assert_eq!(store.tickets.read().unwrap().form(), form, "a reopen reads what was kept");
    assert_eq!((store.groups(), store.extents()), (groups, extents));
    assert_stored_is_a_scan(&store, path, "the reopen");
    store
}

/// Acceptance 1, on a store built by `create` — and by every other write,
/// from nothing.
#[tokio::test]
async fn the_stored_state_is_a_scan_after_every_write_on_a_store_built_by_create() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("created.db");
    let store = Store::open(&path).unwrap();
    assert_stored_is_a_scan(&store, &path, "a fresh open");
    walk(store, &path, 0x5CA1_E028, 400, &["AA", "MAP", "SUB", "ZED", "QQ"]).await;
}

/// Acceptance 1, on a seeded store: a seed's state is stored whole, a write
/// moves it after, and a seed into the store it is in stores it whole again.
#[tokio::test]
async fn the_stored_state_is_a_scan_after_every_write_on_a_seeded_store() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("seeded.db");
    let store = Store::open(&path).unwrap();
    store.seed_synthetic(SeedSpec { count: 1_500, seed: 28, into_existing: false }).unwrap();
    assert_stored_is_a_scan(&store, &path, "a seed");
    let epics: Vec<String> = store.extents().into_iter().map(|(e, _)| e).take(4).chain(["NEW".into()]).collect();
    let epics: Vec<&str> = epics.iter().map(String::as_str).collect();
    let store = walk(store, &path, 0x5EED_0028, 250, &epics).await;
    store.seed_synthetic(SeedSpec { count: 700, seed: 3, into_existing: true }).unwrap();
    assert_stored_is_a_scan(&store, &path, "a seed into the walked store");
    walk(store, &path, 0x5EED_0029, 100, &epics).await;
}

/// Acceptance 1 and 5, on every fixture a binary from before SCALE-28 wrote
/// (each meeting the migrations of its age first): the first open stores the
/// derived state a scan gives, and every write after keeps it so.
#[tokio::test]
async fn the_stored_state_is_a_scan_after_every_write_on_each_fixture_from_before_it() {
    let fixtures = concat!(env!("CARGO_MANIFEST_DIR"), "/../tests/fixtures");
    for (i, name) in ["pre-ags13.db", "pre-ags19.db", "pre-scale4.db", "pre-sdlc7.db"].into_iter().enumerate() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(name);
        std::fs::copy(format!("{fixtures}/{name}"), &path).unwrap();
        assert_eq!(read_state(&path), State::Absent, "{name} predates the stored state");
        let store = Store::open(&path).unwrap();
        assert_stored_is_a_scan(&store, &path, &format!("the first open of {name}"));
        let mut epics: Vec<String> = store.extents().into_iter().map(|(e, _)| e).collect();
        epics.push("NEW".into());
        let epics: Vec<&str> = epics.iter().map(String::as_str).collect();
        walk(store, &path, 0xF128 + i as u64, 150, &epics).await;
    }
}

/// Acceptance 1 on what only a hand edit makes — parent cycles, a parent
/// that names no ticket, a legacy prefix's several trees, a hole and a
/// sequence 0 in a run: what is stored is what `rollup.rs` builds (MNG-17's
/// rule), and the two writes that repair such a shape — a create of the id a
/// dangling parent names, a move off a cycle — store what they rebuild.
#[tokio::test]
async fn the_stored_state_is_a_scan_on_hand_edited_shapes_and_through_their_repairs() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("legacy.db");
    tests::write_db(&path, &tests::legacy());
    let store = Store::open(&path).unwrap();
    assert_stored_is_a_scan(&store, &path, "the first open");
    let marks = |store: &Store| read_back(&path).form.marks.len() + store.tickets.read().unwrap().form().marks.len();
    assert!(marks(&store) > 0, "the cycles and the dangling parent are marked");

    store.set_parent("CYC-2", "CYC-4").await.unwrap();
    assert_stored_is_a_scan(&store, &path, "a move off a cycle (a rebuild)");
    store.set_parent("CXA-1", "PLEX-2").await.unwrap();
    assert_stored_is_a_scan(&store, &path, "a move off the cycle across three shortcodes");
    store.create(new_ticket("GHOST", Kind::Scope, None, Vec::new())).await.unwrap();
    assert_stored_is_a_scan(&store, &path, "the birth a dangling parent named (a rebuild)");
    let file = read_back(&path);
    assert!(!file.form.marks.iter().any(|(m, id)| *m == stored::Mark::Dangling && id == "GHOST-1"), "repaired");

    let store = walk(
        store,
        &path,
        0x4A4D_0028,
        300,
        &["AAA", "AGS", "BBB", "CYC", "DEMO", "NOSC", "PLEX", "TRAN", "XPC", "DUA", "NEW"],
    )
    .await;
    drop(store);

    // SCALE-19's shapes: holes, a sequence 0, a lowercase shortcode.
    let path = dir.path().join("holes.db");
    tests_paging::write_runs(&path, &[("HOLE", 1), ("HOLE", 2), ("HOLE", 5), ("ZERO", 0), ("ZERO", 1), ("low", 1)]);
    let store = Store::open(&path).unwrap();
    assert_stored_is_a_scan(&store, &path, "a file with holes");
    walk(store, &path, 0x401E_0028, 120, &["HOLE", "ZERO", "NEW"]).await;
}

/// Where a file's stored derived state stands, read without opening a store.
fn read_state(path: &Path) -> State {
    let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
    stored::state(&conn).unwrap()
}

/// Required behavior, rows 3 and 4: a file from before SCALE-28 has its
/// derived state built on first open, durably; every later open *reads* it.
/// Proved by changing one stored number of each structure by hand between
/// two opens: the reopened store answers with the changed numbers — a
/// rebuild would have answered with the scan's — and `--verify-derived`
/// names each one.
#[tokio::test]
async fn the_first_open_builds_once_and_every_later_open_reads_what_is_stored() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("fixture.db");
    std::fs::copy(concat!(env!("CARGO_MANIFEST_DIR"), "/../tests/fixtures/pre-scale4.db"), &path).unwrap();
    assert_eq!(read_state(&path), State::Absent);
    let store = Store::open(&path).unwrap();
    assert_eq!(read_state(&path), State::Current, "built, and committed, by the first open");
    let (groups, extents) = (store.groups(), store.extents());
    let par1 = store.get("PAR-1").await.unwrap().child_count;
    drop(store);
    assert!(matches!(Store::verify_derived(&path).unwrap(), Verdict::Consistent { tickets: 9, .. }));

    // One number of each structure, changed under the file's own rules.
    let conn = Connection::open(&path).unwrap();
    conn.execute_batch(
        "UPDATE shortcode_runs SET last = last + 4, held = held + 4 WHERE epic = 'PAR';
         UPDATE rollup_groups SET total = total + 100 WHERE epic = 'PAR';
         UPDATE tickets SET child_count = child_count + 50 WHERE id = 'PAR-1';
         INSERT INTO forward_refs (id, epic, seq, referrer) VALUES ('PAR-99', 'PAR', 1, 'PAR-1');",
    )
    .unwrap();
    drop(conn);

    let store = Store::open(&path).unwrap();
    assert_eq!(read_state(&path), State::Current);
    let last_par = |e: &[(String, u32)]| e.iter().find(|(epic, _)| epic == "PAR").unwrap().1;
    assert_eq!(last_par(&store.extents()), last_par(&extents) + 4, "the run as stored");
    let total = |g: &[Group]| g.iter().find(|g| g.epic == "PAR").unwrap().total;
    assert_eq!(total(&store.groups()), total(&groups) + 100, "the roll-up as stored");
    assert_eq!(store.get("PAR-1").await.unwrap().child_count, par1 + 50, "the count as stored");
    assert_eq!(store.tickets.read().unwrap().ids.waiting_on("PAR-99"), ["PAR-1"], "the reference as stored");
    drop(store);

    let Verdict::Differs(found) = Store::verify_derived(&path).unwrap() else {
        panic!("the changed numbers differ from a scan");
    };
    for named in ["shortcode run PAR:", "roll-up group PAR:", "child_count of PAR-1:", "dangling forward reference PAR-99 named by PAR-1:"] {
        assert!(found.iter().any(|d| d.starts_with(named)), "{named} in {found:#?}");
    }
    assert_eq!(found.len(), 4, "{found:#?}");
}

/// Acceptance 3 at the store: one stored number altered by hand is named,
/// with what is stored and what a scan gives — and verifying writes nothing:
/// the file and its log are byte for byte what they were.
#[tokio::test]
async fn verify_names_an_altered_number_and_writes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("verify.db");
    let store = Store::open(&path).unwrap();
    for (epic, kind, parent) in [("VER", Kind::Scope, None), ("VER", Kind::Build, None), ("SUBV", Kind::Scope, Some("VER-2"))] {
        store.create(new_ticket(epic, kind, parent.map(Into::into), Vec::new())).await.unwrap();
    }
    store.set_status("VER-2", Status::Complete).await.unwrap();
    let bytes = || -> Vec<Vec<u8>> {
        ["", "-wal"]
            .iter()
            .map(|suffix| std::fs::read(format!("{}{suffix}", path.display())).unwrap_or_default())
            .collect()
    };
    let before = bytes();
    let verdict = Store::verify_derived(&path).unwrap();
    assert!(
        matches!(verdict, Verdict::Consistent { tickets: 3, runs: 2, refs: 0, groups: 2, parents: 2, .. }),
        "{verdict:?}"
    );
    assert_eq!(bytes(), before, "verifying a consistent file wrote nothing");

    Connection::open(&path)
        .unwrap()
        .execute("UPDATE rollup_groups SET done = done + 1 WHERE epic = 'SUBV'", [])
        .unwrap();
    let altered = bytes();
    let Verdict::Differs(found) = Store::verify_derived(&path).unwrap() else {
        panic!("an altered number is a difference");
    };
    assert_eq!(found.len(), 1, "{found:#?}");
    assert!(found[0].starts_with("roll-up group SUBV: stored "), "{}", found[0]);
    assert!(found[0].contains("roll: (1, 1, 0)") && found[0].contains("roll: (1, 0, 0)"), "{}", found[0]);
    assert_eq!(bytes(), altered, "verifying an inconsistent file wrote nothing either");
    drop(store);
}

/// A file with no stored state, and one that says it is behind its tickets,
/// are reported, not built: verifying never migrates.
#[tokio::test]
async fn verify_reports_a_file_without_a_stored_state_and_does_not_build_one() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("old.db");
    std::fs::copy(concat!(env!("CARGO_MANIFEST_DIR"), "/../tests/fixtures/pre-sdlc7.db"), &path).unwrap();
    let before = std::fs::read(&path).unwrap();
    assert!(matches!(Store::verify_derived(&path).unwrap(), Verdict::NotStored(why) if why.contains("before SCALE-28")));
    assert_eq!(std::fs::read(&path).unwrap(), before, "not migrated, not touched");
    assert_eq!(read_state(&path), State::Absent);
    assert!(Store::verify_derived(&dir.path().join("none.db")).is_err(), "no file is an error, not a new file");
    assert!(!dir.path().join("none.db").exists());
}

/// The seed's marker: a seed stores its derived state whole and says it is
/// the tickets' again; a seed that stops after its tickets and before that
/// (here: the final transaction fails) leaves the file saying it is behind
/// them — `--verify-derived` reports it — and the next open builds it.
#[tokio::test]
async fn a_seed_that_stops_before_storing_its_derived_state_leaves_it_marked_and_the_next_open_builds_it() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("seed.db");
    let store = Store::open(&path).unwrap();
    store.seed_synthetic(SeedSpec { count: 400, seed: 7, into_existing: false }).unwrap();
    assert_eq!(read_state(&path), State::Current);
    assert_stored_is_a_scan(&store, &path, "a seed");

    // The tickets commit; the derived state cannot. (A seed the size of
    // the store drops the children index with its first chunk, too.)
    Connection::open(&path).unwrap().execute_batch("ALTER TABLE rollup_cross RENAME TO elsewhere").unwrap();
    assert!(store.seed_synthetic(SeedSpec { count: 500, seed: 8, into_existing: true }).is_err());
    drop(store);
    assert_eq!(read_state(&path), State::Stale, "the file says its derived state is behind");
    assert!(matches!(Store::verify_derived(&path).unwrap(), Verdict::NotStored(why) if why.contains("behind")));
    Connection::open(&path).unwrap().execute_batch("DROP TABLE elsewhere").unwrap();

    let store = Store::open(&path).unwrap();
    assert_eq!(store.count(), 900, "the seed's tickets are in the file");
    assert_eq!(read_state(&path), State::Current, "built again on open");
    assert_stored_is_a_scan(&store, &path, "the open after a stopped seed");
    assert!(matches!(Store::verify_derived(&path).unwrap(), Verdict::Consistent { tickets: 900, .. }));

    // A small seed into a large store keeps the index as it goes.
    store.seed_synthetic(SeedSpec { count: 50, seed: 9, into_existing: true }).unwrap();
    assert_stored_is_a_scan(&store, &path, "a small seed into the store");
}

/// Required behavior, row 1: a write whose transaction fails stores none of
/// what it would have — not its tickets, not their derived state — and the
/// cache takes none of it: a create, a crossing of `Complete` and a move,
/// each failing on the derived rows it writes last.
#[tokio::test]
async fn a_write_whose_commit_fails_stores_no_derived_state_and_changes_no_cache() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("fails.db");
    let store = Store::open(&path).unwrap();
    store.create(new_ticket("FX", Kind::Scope, None, Vec::new())).await.unwrap();
    store.create(new_ticket("FX", Kind::Build, None, Vec::new())).await.unwrap();
    store.create(new_ticket("FX", Kind::Build, None, Vec::new())).await.unwrap();
    assert_stored_is_a_scan(&store, &path, "the setup");
    let rename = |from: &str, to: &str| {
        Connection::open(&path).unwrap().execute_batch(&format!("ALTER TABLE {from} RENAME TO {to}")).unwrap();
    };
    let snapshot = |store: &Store| (read_back(&path).rows, read_back(&path).form, store.tickets.read().unwrap().form());

    let before = snapshot(&store);
    rename("rollup_groups", "away");
    assert!(store.create(new_ticket("FX", Kind::Build, None, Vec::new())).await.is_err());
    assert!(store.set_status("FX-2", Status::Complete).await.is_err());
    rename("away", "rollup_groups");
    rename("shortcode_runs", "away");
    assert!(store.create(new_ticket("FX", Kind::Build, Some("FX-2".into()), Vec::new())).await.is_err());
    rename("away", "shortcode_runs");
    Connection::open(&path)
        .unwrap()
        .execute_batch("CREATE TRIGGER no_counts BEFORE UPDATE OF child_count ON tickets BEGIN SELECT RAISE(ABORT, 'no'); END;")
        .unwrap();
    assert!(store.set_parent("FX-3", "FX-2").await.is_err());
    Connection::open(&path).unwrap().execute_batch("DROP TRIGGER no_counts").unwrap();
    assert_eq!(snapshot(&store), before, "nothing stored, nothing cached");
    assert_eq!(store.count(), 3);

    let next = store.create(new_ticket("FX", Kind::Build, Some("FX-2".into()), Vec::new())).await.unwrap();
    assert_eq!(next.id, "FX-4", "the id no failed create took");
    store.set_parent("FX-3", "FX-2").await.unwrap();
    store.set_status("FX-2", Status::Complete).await.unwrap();
    assert_stored_is_a_scan(&store, &path, "the same writes, succeeding");
}

/// What a start reads is the file's index of children, in the store's order,
/// and the stored counts — and they are what the cache answers `list
/// { parent }` and `child_count` with, a page at a time.
#[tokio::test]
async fn children_by_page_come_from_the_files_index_in_store_order() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("pages.db");
    let store = Store::open(&path).unwrap();
    store.seed_synthetic(SeedSpec { count: 3_000, seed: 11, into_existing: false }).unwrap();
    let root = store.groups().into_iter().max_by_key(|g| g.total).unwrap().root.unwrap();
    drop(store);
    let store = Store::open(&path).unwrap();
    let conn = Connection::open_with_flags(&path, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
    let mut after: Option<String> = None;
    loop {
        let page = store
            .page(PageQuery { parent: Some(&root), after: after.as_deref(), limit: Some(200), ..PageQuery::default() })
            .unwrap();
        // The same page, as the file's index answers it — what B4 will ask.
        let (from_epic, from_seq) = match &after {
            Some(id) => {
                let key = store.tickets.read().unwrap().at[id].clone();
                (key.epic, i64::from(key.seq))
            }
            None => (String::new(), -1),
        };
        let indexed: Vec<String> = conn
            .prepare(
                "SELECT id FROM tickets INDEXED BY tickets_by_parent WHERE parent = ?1 AND (epic, seq) > (?2, ?3)
                 ORDER BY epic, seq LIMIT 200",
            )
            .unwrap()
            .query_map(rusqlite::params![root, from_epic, from_seq], |r| r.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(page.tickets.iter().map(|t| t.id.clone()).collect::<Vec<_>>(), indexed, "after {after:?}");
        match page.next {
            Some(next) => after = Some(next),
            None => break,
        }
    }
    let stored: u64 = conn
        .query_row("SELECT child_count FROM tickets WHERE id = ?1", [&root], |r| r.get(0))
        .unwrap();
    assert_eq!(store.get(&root).await.unwrap().child_count, stored);
    assert!(stored > 500, "{root} has {stored} children");
}
