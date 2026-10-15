//! MNG-20: a write whose caller goes away mid-commit. Found by reading the
//! code during SCALE-14 (SCALE-27, "found, not fixed") and reproduced here
//! first: each test starts a write with the writer's connection held, lets
//! it run until its commit has begun — `committing` has moved, so it is
//! parked on that connection inside its commit — drops it there, as a
//! dropped socket or a cancelled request drops a call's future, and lets
//! the connection go. The commit then lands; what is asked is whether the
//! rest of the write does — the cache, `watch` — and whether the store
//! still mirrors its database: the same row in both, every write it began
//! counted in, the next id the next one.

#![allow(clippy::await_holding_lock)] // the held connection is the point

use super::*;
use super::tests::Changes;
use std::future::Future;
use std::time::{Duration, Instant};

fn new(epic: &str, kind: Kind, title: &str) -> NewTicket {
    NewTicket {
        path: None,
        epic: epic.into(),
        title: title.into(),
        kind,
        severity: None,
        blocked_by: Vec::new(),
        parent: None,
        body: format!("the body of '{title}'"),
    }
}

/// A store on a file, so a read takes a read connection and only a write
/// needs the writer's — the one the tests hold.
fn on_disk(dir: &tempfile::TempDir) -> Store {
    Store::open(&dir.path().join("detached.db")).unwrap()
}

/// Starts `write` with the writer's connection held and polls it until its
/// commit has begun, then drops it — its caller gone — and lets the
/// connection go. Polled, never awaited: the write cannot finish while the
/// connection is held, so it is pending at every poll, and it is dropped
/// parked inside its commit.
async fn drop_mid_commit<T>(store: &Store, write: impl Future<Output = T>) {
    let held = store.db.lock().unwrap();
    let begun = store.committing.load(Ordering::SeqCst);
    let mut write = Box::pin(write);
    let deadline = Instant::now() + Duration::from_secs(5);
    while store.committing.load(Ordering::SeqCst) == begun {
        assert!(futures::poll!(write.as_mut()).is_pending(), "it cannot finish: its connection is held");
        assert!(Instant::now() < deadline, "the write never began its commit");
        tokio::time::sleep(Duration::from_millis(1)).await;
    }
    assert!(futures::poll!(write.as_mut()).is_pending(), "parked in its commit");
    drop(write);
    drop(held);
}

/// The ticket as the file holds it, once `landed` says the dropped write's
/// commit is in it.
async fn stored_when(store: &Store, id: &str, landed: impl Fn(&Ticket) -> bool) -> Ticket {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let stored = db::ticket(&store.db.lock().unwrap(), id).unwrap();
        match stored {
            Some(t) if landed(&t) => return t,
            _ => assert!(Instant::now() < deadline, "the dropped write's commit never landed"),
        }
        tokio::time::sleep(Duration::from_millis(1)).await;
    }
}

/// Every change `watch` was told of, once no write is in flight: a write
/// holds the write-serial lock until it has broadcast, so having it means
/// whatever the dropped write was going to say has been said.
async fn heard(store: &Store, rx: &mut broadcast::Receiver<Change>) -> Vec<Change> {
    drop(store.write_serial.lock().await);
    let mut out = Vec::new();
    while let Ok(change) = rx.try_change() {
        out.push(change);
    }
    out
}

/// The row a ticket's whole stored version makes — what the cache must
/// hold for it. `child_count` is the cache's alone; the file keeps none.
/// WAVE-6: a cache row as the file holds it — every field but the
/// placement, which is the cache's alone.
fn unplaced(row: Row) -> Row {
    // ORG-3: as the file holds it — the address is the registry's.
    Row { placement: None, ..super::tests::undressed(&row) }
}

fn row_of(stored: &Ticket, child_count: u64) -> Row {
    let mut row = Row::from(stored.clone());
    row.child_count = child_count;
    row
}

/// The store mirrors its database again: every write begun is a write
/// applied, so a reader trusts its rows (SCALE-14's two counts agree).
fn counts_agree(store: &Store) -> (u64, u64) {
    let written = store.tickets.read().unwrap().written;
    (store.committing.load(Ordering::SeqCst), written)
}

fn kinds(changes: &[Change]) -> Vec<String> {
    changes
        .iter()
        .map(|c| serde_json::to_value(c).unwrap()["type"].as_str().unwrap().to_string())
        .collect()
}

/// Criterion 1: a `create` dropped mid-commit. Before MNG-20 its commit
/// landed and nothing else did — the database held AB-2, the cache did not,
/// `watch` never heard of it, and the next `create` was handed AB-2 again
/// and overwrote the stored row. Now the write runs to its end without its
/// caller: AB-2 is in the cache, `watch` carried `created`, and the next
/// `create` gets AB-3, leaving AB-2 as the dropped write stored it.
#[tokio::test]
async fn a_create_dropped_mid_commit_still_reaches_the_cache_and_watch_and_the_next_id_is_the_next() {
    let dir = tempfile::tempdir().unwrap();
    let store = on_disk(&dir);
    store.create(new("AB", Kind::Scope, "the root")).await.unwrap(); // AB-1
    let mut rx = store.subscribe();

    drop_mid_commit(&store, store.create(new("AB", Kind::Build, "the dropped one"))).await;
    let stored = stored_when(&store, "AB-2", |_| true).await;
    let said = heard(&store, &mut rx).await;
    let cached = store.row("AB-2").ok().map(|(row, _)| unplaced(row));
    let counts = counts_agree(&store);
    let next = store.create(new("AB", Kind::Build, "the next one")).await.unwrap();
    let ab2_after = db::ticket(&store.db.lock().unwrap(), "AB-2").unwrap().unwrap();

    assert_eq!(stored.title, "the dropped one", "the commit landed");
    assert_eq!(
        cached,
        Some(row_of(&stored, 0)),
        "the database holds AB-2 ('{}') and the cache holds {cached:?}; watch heard {:?}; the \
         next create was handed {} and AB-2 is now stored as '{}'",
        stored.title,
        kinds(&said),
        next.id,
        ab2_after.title
    );
    assert!(
        matches!(said.first(), Some(Change::Created { ticket }) if ticket.id == "AB-2" && ticket.placement.is_some() && Ticket { placement: None, address: String::new(), ..ticket.clone() } == stored),
        "watch carried created for AB-2: {:?}",
        kinds(&said)
    );
    assert_eq!(kinds(&said), ["created", "children_changed", "group_changed"], "and the rest of the write");
    assert_eq!(counts.0, counts.1, "every write begun is applied: committing {} vs written {}", counts.0, counts.1);
    assert_eq!(next.id, "AB-3", "the next create gets the next id");
    assert_eq!(ab2_after, stored, "and AB-2 is as the dropped write stored it");
    assert_eq!(store.get("AB-1").await.unwrap().child_count, 2);
}

/// Criterion 2: `set_status`, dropped mid-commit — the cache's row is the
/// stored one, status and all, and `watch` carried `status_changed`.
#[tokio::test]
async fn a_set_status_dropped_mid_commit_still_reaches_the_cache_and_watch() {
    let dir = tempfile::tempdir().unwrap();
    let store = on_disk(&dir);
    store.create(new("SS", Kind::Scope, "the root")).await.unwrap();
    store.create(new("SS", Kind::Build, "a ticket")).await.unwrap();
    let mut rx = store.subscribe();

    drop_mid_commit(&store, store.set_status("SS-2", Status::Complete)).await;
    let stored = stored_when(&store, "SS-2", |t| t.status == Status::Complete).await;
    let said = heard(&store, &mut rx).await;
    let cached = unplaced(store.row("SS-2").unwrap().0);
    let counts = counts_agree(&store);

    assert_eq!(
        cached,
        row_of(&stored, 0),
        "the database holds SS-2 {:?} and the cache {:?}; watch heard {:?}",
        stored.status,
        cached.status,
        kinds(&said)
    );
    assert_eq!(
        kinds(&said),
        ["status_changed", "group_changed", "placements_changed"],
        "watch carried it, the roll-up it moved and (WAVE-6) the placement"
    );
    assert_eq!(counts.0, counts.1, "committing {} vs written {}", counts.0, counts.1);
    assert_eq!(store.groups()[0].done, 1, "the roll-up counts it");
}

/// Criterion 2: `set_parent`, dropped mid-commit — the child's row and both
/// parents' counts in the cache, and `watch` carried all three changes.
#[tokio::test]
async fn a_set_parent_dropped_mid_commit_still_reaches_the_cache_and_watch() {
    let dir = tempfile::tempdir().unwrap();
    let store = on_disk(&dir);
    store.create(new("SP", Kind::Scope, "the root")).await.unwrap(); // SP-1
    store.create(new("SP", Kind::Build, "a")).await.unwrap(); // SP-2 -> SP-1
    store.create(new("SP", Kind::Build, "b")).await.unwrap(); // SP-3 -> SP-1
    let mut rx = store.subscribe();

    drop_mid_commit(&store, store.set_parent("SP-3", "SP-2")).await;
    let stored = stored_when(&store, "SP-3", |t| t.parent.as_deref() == Some("SP-2")).await;
    let said = heard(&store, &mut rx).await;
    let cached = unplaced(store.row("SP-3").unwrap().0);
    let counts = counts_agree(&store);

    assert_eq!(
        cached,
        row_of(&stored, 0),
        "the database files SP-3 under {:?} and the cache under {:?}; watch heard {:?}",
        stored.parent,
        cached.parent,
        kinds(&said)
    );
    assert_eq!(kinds(&said), ["parent_changed", "children_changed", "child_removed"]);
    assert_eq!(counts.0, counts.1, "committing {} vs written {}", counts.0, counts.1);
    let page = store.page(PageQuery { parent: Some("SP-2"), ..PageQuery::default() }).unwrap();
    assert_eq!(page.tickets.iter().map(|t| t.id.as_str()).collect::<Vec<_>>(), ["SP-3"], "the index moved");
    assert_eq!((store.row("SP-1").unwrap().0.child_count, store.row("SP-2").unwrap().0.child_count), (1, 1));
}

/// Criterion 2: `amend`, dropped mid-commit — the new title in the cache,
/// the new body in the file, one `amended` on `watch`. An amend reads the
/// stored body before it commits, so it is dropped after that read too.
/// AGS-23: AM-1 roots its shortcode, so its new title is its group's too —
/// one `group_changed` behind the `amended`, carrying it.
#[tokio::test]
async fn an_amend_dropped_mid_commit_still_reaches_the_cache_and_watch() {
    let dir = tempfile::tempdir().unwrap();
    let store = on_disk(&dir);
    store.create(new("AM", Kind::Scope, "the root")).await.unwrap();
    let mut rx = store.subscribe();

    drop_mid_commit(&store, store.amend("AM-1", Some("amended".into()), Some("a new body".into()))).await;
    let stored = stored_when(&store, "AM-1", |t| t.title == "amended").await;
    let said = heard(&store, &mut rx).await;
    let cached = unplaced(store.row("AM-1").unwrap().0);
    let counts = counts_agree(&store);

    assert_eq!(
        cached,
        row_of(&stored, 0),
        "the database titles AM-1 '{}' and the cache '{}'; watch heard {:?}",
        stored.title,
        cached.title,
        kinds(&said)
    );
    assert_eq!(stored.body, "a new body");
    assert!(
        matches!(
            said.as_slice(),
            [Change::Amended { ticket, changed }, Change::GroupChanged { group }]
                if Ticket { placement: None, address: String::new(), ..ticket.clone() } == stored
                    && changed == &["title", "body"]
                    && group.root_row.as_ref().is_some_and(|r| r.title == "amended")
        ),
        "{:?}",
        kinds(&said)
    );
    assert_eq!(counts.0, counts.1, "committing {} vs written {}", counts.0, counts.1);
}

/// The guarantee for the writes added since the ticket was filed:
/// `link_commits` and `unlink_commits` (SDLC-9), each dropped mid-commit.
#[tokio::test]
async fn a_link_or_unlink_dropped_mid_commit_still_reaches_the_cache_and_watch() {
    let dir = tempfile::tempdir().unwrap();
    let store = on_disk(&dir);
    store.create(new("LK", Kind::Scope, "the root")).await.unwrap();
    let mut rx = store.subscribe();

    drop_mid_commit(&store, store.link_commits("LK-1", vec!["77df9a0".into(), "0bb28b2".into()])).await;
    let stored = stored_when(&store, "LK-1", |t| t.commits.len() == 2).await;
    let said = heard(&store, &mut rx).await;
    let cached = unplaced(store.row("LK-1").unwrap().0);
    assert_eq!(cached, row_of(&stored, 0), "link: the database holds {:?} and the cache {:?}", stored.commits, cached.commits);
    assert_eq!(kinds(&said), ["commits_linked"]);

    drop_mid_commit(&store, store.unlink_commits("LK-1", vec!["77df9a0".into()])).await;
    let stored = stored_when(&store, "LK-1", |t| t.commits.len() == 1).await;
    let said = heard(&store, &mut rx).await;
    let cached = unplaced(store.row("LK-1").unwrap().0);
    assert_eq!(cached, row_of(&stored, 0), "unlink: the database holds {:?} and the cache {:?}", stored.commits, cached.commits);
    assert_eq!(kinds(&said), ["commits_unlinked"]);
    let counts = counts_agree(&store);
    assert_eq!(counts.0, counts.1, "committing {} vs written {}", counts.0, counts.1);
}

/// What does not change for a caller that stays: a write is acknowledged
/// only once it is durable, in the cache and broadcast — by the time
/// `create` returns, its ticket is in the file and the cache, and `watch`
/// has it — and a write that fails to commit changes nothing and says
/// nothing, its count given back.
#[tokio::test]
async fn a_caller_that_stays_is_answered_after_all_three_and_a_failed_commit_changes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let store = on_disk(&dir);
    let mut rx = store.subscribe();
    let t = store.create(new("ST", Kind::Scope, "stays")).await.unwrap();
    // WAVE-6: the file holds everything but the placement, the cache's alone.
    let unplaced = Ticket { placement: None, address: String::new(), ..t.clone() };
    assert_eq!(db::ticket(&store.db.lock().unwrap(), "ST-1").unwrap().as_ref(), Some(&unplaced), "durable when answered");
    assert!(store.row("ST-1").is_ok(), "in the cache when answered");
    assert!(matches!(rx.try_change(), Ok(Change::Created { ticket }) if ticket == t), "broadcast when answered");
    assert!(matches!(rx.try_change(), Ok(Change::GroupChanged { .. })), "all of it: the new shortcode's row");

    // A commit that fails: the table is gone from under the writer.
    store.db.lock().unwrap().execute_batch("ALTER TABLE tickets RENAME TO gone").unwrap();
    let before = counts_agree(&store);
    assert!(matches!(store.set_status("ST-1", Status::Ready).await, Err(StoreError::Db(_))));
    assert_eq!(store.row("ST-1").unwrap().0.status, Status::Pending, "the cache is untouched");
    assert_eq!(counts_agree(&store), before, "and the count given back");
    while let Ok(change) = rx.try_change() {
        panic!("a failed write said {change:?}");
    }
}
