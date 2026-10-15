//! SCALE-14: the body leaves the cache. The cache holds rows; everything
//! that answers a whole ticket — `get`, `blockers`, a full-ticket walk,
//! every event that carries a ticket — reads the body from the database: a
//! read on a connection of its own, a write in its own transaction. These
//! hold it to the ticket's table:
//! byte-exact as before, a page of bodies at a time, no database read for
//! the cache-only answers, no wait behind a writer, and a read that sees
//! every acknowledged write as one version of each ticket.

use super::*;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

/// How many reads met a commit between their rows and their snapshot, and
/// so read the stored version ([`Store::full`]) — process-wide, for the
/// hammer's report; no test asserts on it.
pub(super) static STORED_VERSION_READS: AtomicU64 = AtomicU64::new(0);

fn new(epic: &str, kind: Kind, parent: Option<&str>, blocked_by: &[&str], body: &str) -> NewTicket {
    NewTicket {
        path: None,
        epic: epic.into(),
        title: format!("a {} in {epic}", kind.as_str()),
        kind,
        severity: None,
        blocked_by: blocked_by.iter().map(|b| b.to_string()).collect(),
        parent: parent.map(Into::into),
        body: body.into(),
    }
}

/// A body nothing normalizes — CRLF, a tab, trailing spaces, astral
/// characters, no final newline — with `tag` in it, so two are different.
fn awkward(tag: &str) -> String {
    format!("## {tag}\r\n\nünïcode ✓ 𝔘 🧪\t(tab)   \r\n```mermaid\ngraph LR\n  A --> B\n```\ntrailing   ")
}

/// A store on a real file — so its reads take the read connections, not
/// the writer's turn, as `:memory:` must.
fn on_disk(dir: &tempfile::TempDir) -> Store {
    let store = Store::open(&dir.path().join("bodies.db")).unwrap();
    assert!(matches!(&*store.readers, Readers::File { .. }), "a file store reads on its own connections");
    store
}

/// The body as the file holds it, read past the store on a connection of
/// the test's own.
fn stored_body(dir: &tempfile::TempDir, id: &str) -> String {
    let conn = Connection::open(dir.path().join("bodies.db")).unwrap();
    db::body(&conn, id).unwrap().expect("a stored row")
}

/// Every path that answers a whole ticket answers the body byte-exact —
/// `get`, `blockers`, `list`, `ready_set` — and each is the body the file
/// holds, beside the row the cache holds: a ticket whole is its summary
/// row plus its body, and serializes as it did when the body was cached.
#[tokio::test]
async fn every_whole_ticket_is_its_row_and_the_stored_body_byte_exact() {
    let dir = tempfile::tempdir().unwrap();
    let store = on_disk(&dir);
    let root = store.create(new("BD", Kind::Scope, None, &[], &awkward("root"))).await.unwrap();
    let a = store.create(new("BD", Kind::Build, None, &[], &awkward("a"))).await.unwrap();
    let b = store.create(new("BD", Kind::Gap, None, &["BD-2", "BD-9"], "")).await.unwrap();
    let c = store.create(new("BD", Kind::Spike, None, &["BD-2"], &awkward("c"))).await.unwrap();
    store.set_status("BD-2", Status::Complete).await.unwrap();
    store.set_status("BD-3", Status::Ready).await.unwrap();
    store.set_status("BD-4", Status::Ready).await.unwrap();

    for (id, born) in [("BD-1", &root), ("BD-2", &a), ("BD-3", &b), ("BD-4", &c)] {
        let got = store.get(SYS, id).await.unwrap();
        assert_eq!(got.body.as_bytes(), born.body.as_bytes(), "{id} through get");
        assert_eq!(got.body, stored_body(&dir, id), "{id}: the file's body");
        let row = store.row(id).unwrap().0;
        assert_eq!(TicketSummary::from(got.clone()), row, "{id}: the cache's row, all of it");
        assert_eq!(whole(row, got.body.clone()), got);
        let json = serde_json::to_value(&got).unwrap();
        assert_eq!(json["body"].as_str().unwrap().as_bytes(), born.body.as_bytes(), "{id} on the wire");
    }
    assert_eq!(store.get(SYS, "BD-3").await.unwrap().body, "", "an empty body is a body");

    let refs = store.blockers(SYS, "BD-3").await.unwrap();
    match refs.as_slice() {
        [BlockerRef::Resolved(t), BlockerRef::Dangling(d)] => {
            assert_eq!((t.id.as_str(), d.as_str()), ("BD-2", "BD-9"), "in blocked_by order");
            assert_eq!(t, &store.get(SYS, "BD-2").await.unwrap(), "the blocker whole, at its status");
        }
        other => panic!("one resolved, one dangling: {other:?}"),
    }
    assert!(matches!(store.blockers(SYS, "BD-7").await, Err(StoreError::NotFound(id)) if id == "BD-7"));
    assert!(matches!(store.get(SYS, "BD-7").await, Err(StoreError::NotFound(id)) if id == "BD-7"));

    let all = store.list(SYS, None, None, None).await.unwrap();
    assert_eq!(all.iter().map(|t| t.id.as_str()).collect::<Vec<_>>(), ["BD-1", "BD-2", "BD-3", "BD-4"]);
    for t in &all {
        assert_eq!(t, &store.get(SYS, &t.id).await.unwrap(), "list answers what get does");
    }
    // BD-3 waits on a dangling BD-9; BD-4's one blocker is done.
    assert_eq!(store.ready_set(SYS, None).await.unwrap(), [store.get(SYS, "BD-4").await.unwrap()]);

    // A reopen loads rows, and the bodies are still the file's.
    drop(store);
    let reopened = Store::open(&dir.path().join("bodies.db")).unwrap();
    assert_eq!(reopened.list(SYS, None, None, None).await.unwrap(), all);
}

/// Every event that carries a ticket carries it whole — `Created`,
/// `UnlocksChanged`, `StatusChanged`, `ParentChanged`, `Amended` — with
/// the body the file holds, though a write after birth no longer holds a
/// body to send: it reads it back in its own transaction. And it never
/// writes one it did not change.
#[tokio::test]
async fn every_event_that_carries_a_ticket_carries_its_stored_body() {
    let dir = tempfile::tempdir().unwrap();
    let store = on_disk(&dir);
    store.create(new("EV", Kind::Scope, None, &[], &awkward("EV-1"))).await.unwrap();
    store.create(new("EV", Kind::Build, None, &[], &awkward("EV-2"))).await.unwrap();
    let mut rx = store.subscribe();
    let whole_tickets = |rx: &mut broadcast::Receiver<Change>| {
        let mut out = Vec::new();
        while let Ok(change) = rx.try_recv() {
            match change {
                Change::Created { ticket }
                | Change::StatusChanged { ticket, .. }
                | Change::Amended { ticket, .. }
                | Change::UnlocksChanged { ticket, .. }
                | Change::ParentChanged { ticket, .. } => out.push(ticket),
                _ => {}
            }
        }
        out
    };

    // Born under EV-1, blocked by EV-1 and EV-2: Created, then each blocker
    // whole in its UnlocksChanged — EV-1 also counting its new child.
    store.create(new("EV", Kind::Gap, None, &["EV-1", "EV-2"], &awkward("EV-3"))).await.unwrap();
    let seen = whole_tickets(&mut rx);
    assert_eq!(seen.iter().map(|t| t.id.as_str()).collect::<Vec<_>>(), ["EV-3", "EV-1", "EV-2"]);
    assert_eq!(seen[1].child_count, 2, "EV-1 rides out with the count it now has");
    store.set_status("EV-2", Status::Ready).await.unwrap();
    store.set_parent("EV-3", "EV-2").await.unwrap();
    store.amend("EV-3", Some("retitled".into()), None).await.unwrap();
    store.amend("EV-2", None, Some(awkward("EV-2, amended"))).await.unwrap();
    let seen: Vec<(String, String)> =
        seen.into_iter().chain(whole_tickets(&mut rx)).map(|t| (t.id.into_string(), t.body)).collect();
    let expect = [
        ("EV-3", "EV-3"), // created
        ("EV-1", "EV-1"), // unlocks_changed
        ("EV-2", "EV-2"), // unlocks_changed
        ("EV-2", "EV-2"), // status_changed
        ("EV-3", "EV-3"), // parent_changed
        ("EV-3", "EV-3"), // amended: the title only
        ("EV-2", "EV-2, amended"),
    ];
    assert_eq!(seen, expect.map(|(id, tag)| (id.to_string(), awkward(tag))), "each whole, byte-exact");
    for id in ["EV-1", "EV-2", "EV-3"] {
        assert_eq!(store.get(SYS, id).await.unwrap().body, stored_body(&dir, id), "{id}");
    }

    // The no-ops answer the ticket whole too, and write and say nothing.
    match store.set_parent("EV-3", "EV-2").await.unwrap() {
        Change::ParentChanged { ticket, previous } => {
            assert_eq!(previous.as_deref(), Some("EV-2"));
            assert_eq!(ticket, store.get(SYS, "EV-3").await.unwrap());
        }
        other => panic!("{other:?}"),
    }
    match store.amend("EV-1", Some("a scope in EV".into()), None).await.unwrap() {
        Change::Amended { ticket, changed } => {
            assert!(changed.is_empty());
            assert_eq!(ticket.body, awkward("EV-1"));
        }
        other => panic!("{other:?}"),
    }
    assert!(whole_tickets(&mut rx).is_empty(), "the no-ops said nothing");
}

/// `list {}` and `ready_set {}` without `summary` walk a page of bodies at
/// a time and answer exactly what one page of the same `limit` answers:
/// the same tickets in the same order, the same `next` and `remaining` —
/// under every filter, from every kind of cursor, at limits around the
/// walk's own page.
#[tokio::test]
async fn a_walk_holds_a_page_of_bodies_and_answers_what_one_page_would() {
    let store = Store::new();
    store.seed_synthetic(SeedSpec { count: 1_000, seed: 14, into_existing: false }).unwrap();
    let rows = store.page(SYS, PageQuery::default()).unwrap().tickets;
    let (first, mid) = (rows[0].id.clone(), rows[480].id.clone());
    let root = rows[0].id.clone();
    let epic = rows[0].epic.clone();

    let selects = [
        Select::List { epic: None, status: None, parent: None },
        Select::List { epic: Some(epic.to_ascii_lowercase()), status: None, parent: None },
        Select::List { epic: None, status: Some(Status::Ready), parent: None },
        Select::List { epic: None, status: None, parent: Some(root.to_string()) },
        Select::Ready { epic: None },
        // FILTER-4: a filtered walk — every ticket, then a status by a lens
        // with the old parameters beside it.
        Select::Filtered(Filtered { lens: Lens::all(), epic: None, status: None, parent: None, keep: None }),
        Select::Filtered(Filtered {
            lens: Lens::new("ready-or-blocked".into(), vec![], |_, k| matches!(k.status(), Status::Ready | Status::Blocked)),
            epic: Some(epic.to_ascii_lowercase()),
            status: None,
            parent: Some(root.to_string()),
            keep: None,
        }),
    ];
    for select in &selects {
        for after in [None, Some(first.as_str()), Some(mid.as_str())] {
            for limit in [None, Some(0), Some(1), Some(99), Some(100), Some(101), Some(199), Some(200), Some(201), Some(450), Some(5_000)] {
                let what = format!("{select:?} after {after:?} limit {limit:?}");
                let page = match select {
                    Select::List { epic, status, parent } => store.page(SYS, PageQuery {
                        epic: epic.as_deref(),
                        status: *status,
                        parent: parent.as_deref(),
                        after,
                        limit,
                    }),
                    Select::Ready { epic } => store.ready_page(SYS, epic.as_deref(), after, limit),
                    Select::Filtered(q) => store.filtered_page(SYS, q, after, None, limit),
                }
                .unwrap();
                let mut walk = store.walk(SYS, select.clone(), after, limit);
                let mut got: Vec<Ticket> = Vec::new();
                let end = loop {
                    match walk.next(&store).await.unwrap() {
                        Step::Tickets(tickets) => {
                            assert!(!tickets.is_empty() && tickets.len() <= BODIES_PER_STEP as usize, "{what}: {}", tickets.len());
                            got.extend(tickets);
                        }
                        Step::End { next, remaining, first } => break (next, remaining, first),
                    }
                };
                assert!(matches!(walk.next(&store).await.unwrap(), Step::End { .. }), "{what}: over is over");
                assert_eq!(got.iter().map(TicketSummary::from).collect::<Vec<_>>(), page.tickets, "{what}");
                assert_eq!(end, (page.next, page.remaining, page.first), "{what}");
                assert!(got.iter().all(|t| t.body.contains(&format!("_{}: synthetic", t.id))), "{what}: each its own body");
            }
        }
    }
    let mut walk = store.walk(SYS, Select::Ready { epic: None }, Some("no-such-1"), None);
    assert!(matches!(walk.next(&store).await, Err(StoreError::CursorNotFound(id)) if id == "NO-SUCH-1"));
}

/// The answers the cache gives alone — a summary page, the ready set's
/// summary page, `find`, `groups`, `extents`, the count — read no database:
/// with the read side pointed at a file that does not exist, every one of
/// them still answers, while `get` cannot.
#[tokio::test]
async fn the_cache_only_answers_read_no_database() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = on_disk(&dir);
    store.create(new("CO", Kind::Scope, None, &[], "a body")).await.unwrap();
    store.create(new("CO", Kind::Build, None, &[], "a body")).await.unwrap();
    store.set_status("CO-2", Status::Ready).await.unwrap();
    store.readers = Arc::new(Readers::File {
        path: dir.path().join("no-such-dir").join("absent.db"),
        idle: StdMutex::new(Vec::new()),
        permits: Arc::new(Semaphore::new(READERS)),
    });

    assert_eq!(store.page(SYS, PageQuery::default()).unwrap().tickets.len(), 2);
    assert_eq!(store.page(SYS, PageQuery { status: Some(Status::Ready), ..PageQuery::default() }).unwrap().tickets[0].id, "CO-2");
    assert_eq!(store.ready_page(SYS, None, None, Some(5)).unwrap().tickets[0].id, "CO-2");
    assert_eq!(store.find_page(SYS, "co-2", None, None).unwrap().tickets.len(), 1);
    assert_eq!(store.groups(SYS).len(), 1);
    assert_eq!(store.extents_text(), [("CO".to_string(), 2)]);
    assert_eq!(store.count(SYS), 2);
    assert!(matches!(store.get(SYS, "CO-2").await, Err(StoreError::Db(_))), "and the trap is real");
    assert!(matches!(store.get(SYS, "CO-9").await, Err(StoreError::NotFound(_))), "a miss is the cache's answer");
}

/// A read does not wait for a write: with the writer's connection taken
/// and a write transaction open on the file — the state a write is in for
/// its whole commit, fsync included — `get` answers at once, from its own
/// connection, and answers the last committed body, not the open one.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_read_answers_beside_an_open_write() {
    let dir = tempfile::tempdir().unwrap();
    let store = on_disk(&dir);
    store.create(new("RW", Kind::Scope, None, &[], "committed")).await.unwrap();
    let writer = Arc::clone(&store.db);
    let (open, release) = (Arc::new(AtomicBool::new(false)), Arc::new(AtomicBool::new(false)));
    let (o, r) = (Arc::clone(&open), Arc::clone(&release));
    let held = std::thread::spawn(move || {
        let mut conn = writer.lock().unwrap();
        let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate).unwrap();
        tx.execute("UPDATE tickets SET body = 'not yet' WHERE id = 'RW-1'", []).unwrap();
        o.store(true, Ordering::SeqCst);
        while !r.load(Ordering::SeqCst) {
            std::thread::sleep(Duration::from_millis(5));
        }
        tx.rollback().unwrap();
    });
    while !open.load(Ordering::SeqCst) {
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    let got = tokio::time::timeout(Duration::from_secs(5), store.get(SYS, "RW-1")).await;
    release.store(true, Ordering::SeqCst);
    held.join().unwrap();
    assert_eq!(got.expect("the read did not wait for the writer").unwrap().body, "committed");
}

/// Read-your-writes, deterministically, at the one moment it could fail:
/// a write committed to the file and not yet in the cache. Its body is in
/// the snapshot a read takes; its row is not in the rows the read took
/// first. The read must answer one version — the stored one, row and body
/// together — never the old title over the new body.
#[tokio::test]
async fn a_read_that_meets_a_commit_answers_the_stored_version_whole() {
    let dir = tempfile::tempdir().unwrap();
    let store = on_disk(&dir);
    store.create(new("MC", Kind::Scope, None, &[], "old body")).await.unwrap();
    store.create(new("MC", Kind::Build, None, &[], "child")).await.unwrap();
    let before = store.get(SYS, "MC-1").await.unwrap();

    // What `commit` does, by hand, up to the moment `apply` would run.
    store.committing.fetch_add(1, Ordering::SeqCst);
    store
        .db
        .lock()
        .unwrap()
        .execute("UPDATE tickets SET title = 'new title', body = 'new body', updated_unix = 42 WHERE id = 'MC-1'", [])
        .unwrap();
    assert_eq!(store.row("MC-1").unwrap().0.title, before.title, "the cache has not seen it");

    let got = store.get(SYS, "MC-1").await.unwrap();
    assert_eq!((got.title.as_str(), got.body.as_str(), got.updated_unix), ("new title", "new body", 42));
    assert_eq!(got.child_count, 1, "the count is the cache's: the file keeps none");
    let walked = store.list(SYS, Some("MC"), None, None).await.unwrap();
    assert_eq!(walked[0], got, "a walk's page reads the same way");

    // Without the count raised, the same read would have trusted its rows:
    // the old title over the new body. That is the anomaly the count is for.
    store.committing.fetch_sub(1, Ordering::SeqCst);
    let torn = store.get(SYS, "MC-1").await.unwrap();
    assert_eq!((torn.title.as_str(), torn.body.as_str()), (before.title.as_str(), "new body"));
}

/// Read-your-writes under a hammer: one writer amends a few tickets'
/// title and body together, version after version, while readers `get`
/// them and walk them whole. Every answer is one version — its title's and
/// its body's numbers agree — and never older than the last amend
/// acknowledged before the read began.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn read_your_writes_under_a_hammer_of_amends() {
    const TICKETS: usize = 3;
    const VERSIONS: u64 = 150;
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(on_disk(&dir));
    store.create(new("HM", Kind::Scope, None, &[], "title 0 / body 0")).await.unwrap();
    for _ in 1..TICKETS {
        store.create(new("HM", Kind::Build, None, &[], "body 0")).await.unwrap();
    }
    for i in 1..=TICKETS {
        store.amend(&format!("HM-{i}"), Some("title 0".into()), Some(awkward("body 0"))).await.unwrap();
    }
    let acked: Arc<Vec<AtomicU64>> = Arc::new((0..TICKETS).map(|_| AtomicU64::new(0)).collect());
    let done = Arc::new(AtomicBool::new(false));
    let version = |t: &Ticket| -> (u64, u64) {
        let title: u64 = t.title.strip_prefix("title ").unwrap().parse().unwrap();
        let body: u64 = t.body.split_once("## body ").unwrap().1.split('\r').next().unwrap().parse().unwrap();
        (title, body)
    };
    let stored_before = STORED_VERSION_READS.load(Ordering::Relaxed);

    let writer = {
        let (store, acked, done) = (Arc::clone(&store), Arc::clone(&acked), Arc::clone(&done));
        tokio::spawn(async move {
            for v in 1..=VERSIONS {
                for i in 0..TICKETS {
                    let body = awkward(&format!("body {v}"));
                    store.amend(&format!("HM-{}", i + 1), Some(format!("title {v}")), Some(body)).await.unwrap();
                    acked[i].store(v, Ordering::SeqCst);
                }
            }
            done.store(true, Ordering::SeqCst);
        })
    };
    let mut readers = Vec::new();
    for r in 0..4 {
        let (store, acked, done) = (Arc::clone(&store), Arc::clone(&acked), Arc::clone(&done));
        readers.push(tokio::spawn(async move {
            let mut reads = 0u64;
            while !done.load(Ordering::SeqCst) {
                if r == 0 {
                    let floor: Vec<u64> = acked.iter().map(|a| a.load(Ordering::SeqCst)).collect();
                    for (i, t) in store.list(SYS, Some("HM"), None, None).await.unwrap().iter().enumerate() {
                        let (title, body) = version(t);
                        assert_eq!(title, body, "{}: one version, walked", t.id);
                        assert!(title >= floor[i], "{}: {title} older than the acknowledged {}", t.id, floor[i]);
                    }
                } else {
                    let i = (reads as usize + r) % TICKETS;
                    let floor = acked[i].load(Ordering::SeqCst);
                    let t = store.get(SYS, &format!("HM-{}", i + 1)).await.unwrap();
                    let (title, body) = version(&t);
                    assert_eq!(title, body, "{}: one version", t.id);
                    assert!(title >= floor, "{}: {title} older than the acknowledged {floor}", t.id);
                }
                reads += 1;
            }
            reads
        }));
    }
    writer.await.unwrap();
    let mut reads = 0;
    for r in readers {
        reads += r.await.unwrap();
    }
    for i in 1..=TICKETS {
        let t = store.get(SYS, &format!("HM-{i}")).await.unwrap();
        assert_eq!(version(&t), (VERSIONS, VERSIONS), "the last acknowledged, once the writer is done");
    }
    eprintln!(
        "{reads} reads beside {} amends; {} met a commit and read the stored version",
        VERSIONS * TICKETS as u64,
        STORED_VERSION_READS.load(Ordering::Relaxed) - stored_before
    );
}

/// MENGER-15: a body past the cap is refused, typed and naming the cap, on
/// `create` (no id spent) and on `amend` (nothing written, the title
/// either); a body at the cap is kept byte-exact. The default is 8 MiB.
#[tokio::test]
async fn a_body_past_the_cap_is_refused_naming_the_cap() {
    assert_eq!(BODY_MAX_DEFAULT, 8 * 1024 * 1024);
    assert_eq!(Store::new().body_max(), BODY_MAX_DEFAULT);
    let dir = tempfile::tempdir().unwrap();
    let store = on_disk(&dir).with_body_max(1000);
    assert_eq!(store.body_max(), 1000);
    store.create(new("CAP", Kind::Scope, None, &[], "")).await.unwrap();
    let over = "x".repeat(1001);
    match store.create(new("CAP", Kind::Doc, None, &[], &over)).await {
        Err(e @ StoreError::BodyTooLarge { .. }) => {
            assert!(matches!(e, StoreError::BodyTooLarge { id: None, bytes: 1001, max: 1000 }), "{e:?}");
            assert!(e.to_string().contains("1001 bytes, past the 1000-byte cap"), "{e}");
        }
        other => panic!("refused, typed: {other:?}"),
    }
    let at = "y".repeat(1000);
    let kept = store.create(new("CAP", Kind::Doc, None, &[], &at)).await.unwrap();
    assert_eq!(kept.id, "CAP-2", "the refused create spent no id");
    assert_eq!(stored_body(&dir, "CAP-2"), at);
    match store.amend("cap-2", Some("a new title".into()), Some(over.clone())).await {
        Err(StoreError::BodyTooLarge { id, bytes, max }) => assert_eq!((id.as_deref(), bytes, max), (Some("CAP-2"), 1001, 1000)),
        other => panic!("refused, typed: {other:?}"),
    }
    let after = store.get(SYS, "CAP-2").await.unwrap();
    assert_eq!((after.title.as_str(), after.body.as_str()), (kept.title.as_str(), at.as_str()), "nothing written");
}
