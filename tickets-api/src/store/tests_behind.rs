//! DEPLOY-20: a ticket's deployment follows its version — DEPLOY-20's
//! table, row by row: every linked commit carried is `running` as before;
//! a version linked after the newest carried one is `behind` (its commits
//! in link order, the version that runs); none carried is `not_running`; a
//! rollback takes it back to behind on the same fold; `behind <env>` beside
//! `deployed` and `not deployed`; a merge and its branch linked together
//! are one version; a record naming the ticket carries what was linked by
//! then; an old database keeps its answers until a ticket gains a commit;
//! the kept behind is a scan, across a reopen and `--verify-derived`.

use super::history::Clock;
use super::tests::Changes;
use super::*;
use crate::address::ShortcodeId;
use crate::deployments::{DeploymentsEvent, Record};

fn ticket(epic: &str, title: &str, kind: Kind) -> NewTicket {
    NewTicket { epic: epic.into(), title: title.into(), kind, severity: None, blocked_by: Vec::new(), parent: None, path: None, body: String::new() }
}

fn rec(env: &str, state: &str, revision: &str, commits: &[&str], at: i64) -> Record {
    Record {
        project: "menger".into(),
        environment: env.into(),
        state: state.into(),
        revision: revision.into(),
        commits: commits.iter().map(|c| c.to_string()).collect(),
        at: Some(at.to_string()),
        ..Record::default()
    }
}

async fn record(store: &Store, r: Record) -> DeploymentsEvent {
    store.record_deployment(SYS, r, "ci".into()).await.unwrap()
}

async fn link(store: &Store, id: &str, commits: &[&str]) {
    store.link_commits(SYS.permit(id), commits.iter().map(|c| (*c).into()).collect()).await.unwrap();
}

/// MENGER-1 a scope, MENGER-2 a build.
async fn menger(store: &Store) {
    store.create(SYS, ticket("MENGER", "menger", Kind::Scope)).await.unwrap();
    store.create(SYS, ticket("MENGER", "the dock", Kind::Build)).await.unwrap();
}

/// `for_ticket`'s rows as words: `prod running`, `prod behind d607756:
/// 68e1271 4145520`, `prod not`.
fn where_(store: &Store, id: &str) -> Vec<String> {
    store
        .for_ticket(id, None)
        .unwrap()
        .into_iter()
        .map(|e| match e {
            DeploymentsEvent::Running { environment, behind, version, .. } if behind.is_empty() => {
                assert!(version.is_none());
                format!("{environment} running")
            }
            DeploymentsEvent::Running { environment, behind, version, .. } => {
                format!("{environment} behind {}: {}", version.as_deref().unwrap_or("-"), behind.join(" "))
            }
            DeploymentsEvent::NotRunning { environment, .. } => format!("{environment} not"),
            other => panic!("{other:?}"),
        })
        .collect()
}

fn admitted(store: &Store, text: &str) -> Vec<String> {
    let clock = Clock { fixed: Some(T0 + 1_000_000), ..Clock::default() };
    let lens = Lens::parse(text).unwrap_or_else(|e| panic!("{text}: {e}")).with_clock(clock);
    let q = Filtered { lens, epic: None, status: None, parent: None, keep: None };
    store.filtered_page(SYS, &q, None, None, None).unwrap().tickets.into_iter().map(|t| t.id.into_string()).collect()
}

/// The cells `running` answers for `env`: each ticket, its behind and version.
fn cells(store: &Store, env: &str) -> Vec<(String, Vec<String>, Option<String>)> {
    match store.running(None, Some(env), None, None).unwrap().pop() {
        Some(DeploymentsEvent::Cells { running, .. }) => running.into_iter().map(|c| (c.ticket, c.behind, c.version)).collect(),
        other => panic!("{other:?}"),
    }
}

const T0: i64 = 1_791_800_000;

/// LOOK-7's case, row by row: its first version (a branch commit and its
/// merge, linked together) deployed; then reopened, a second branch commit
/// and its merge linked, each its own call — prod runs the earlier version,
/// and says which and what is not there. The next promote makes it current;
/// a rollback takes it back to behind on the same fold.
#[tokio::test]
async fn look_7_runs_its_first_version_until_the_next_promote() {
    let store = Store::new();
    menger(&store).await;
    link(&store, "MENGER-2", &["a7badc5", "d607756"]).await;
    record(&store, rec("prod", "succeeded", "d607756", &["a7badc5", "d607756"], T0)).await;
    assert_eq!(where_(&store, "MENGER-2"), ["prod running"], "every linked commit carried: running, as before");
    assert!(admitted(&store, "behind prod").is_empty());

    link(&store, "MENGER-2", &["68e1271"]).await;
    link(&store, "MENGER-2", &["4145520"]).await;
    assert_eq!(where_(&store, "MENGER-2"), ["prod behind d607756: 68e1271 4145520"]);
    assert_eq!(cells(&store, "prod"), [("MENGER-2".to_string(), vec!["68e1271".to_string(), "4145520".to_string()], Some("d607756".to_string()))]);
    assert_eq!(admitted(&store, "behind prod"), ["MENGER-2"]);
    assert_eq!(admitted(&store, "deployed prod"), ["MENGER-2"], "deployed holds a ticket behind or not");
    assert!(admitted(&store, "not deployed prod and kind build").is_empty(), "not deployed unchanged");
    assert_eq!(admitted(&store, "deployed prod and not behind prod"), Vec::<String>::new());

    record(&store, rec("prod", "succeeded", "4145520", &["68e1271", "4145520"], T0 + 100)).await;
    assert_eq!(where_(&store, "MENGER-2"), ["prod running"], "the next promote: current");
    assert!(admitted(&store, "behind prod").is_empty());
    assert_eq!(admitted(&store, "deployed prod and not behind prod"), ["MENGER-2"]);

    let mut back = rec("prod", "rolled_back", "d607756", &[], T0 + 200);
    back.previous = Some("4145520".into());
    record(&store, back).await;
    assert_eq!(where_(&store, "MENGER-2"), ["prod behind d607756: 68e1271 4145520"], "rolled back: behind again");
    assert_eq!(admitted(&store, "behind menger/prod"), ["MENGER-2"]);
}

/// Some carried, a later one not: behind; none carried: not running; an
/// earlier commit not carried (a branch commit, its merge linked later and
/// carried alone) is history, not behind; a merge and its branch commit
/// linked together are one version, whichever order and whichever the
/// record lists.
#[tokio::test]
async fn versions_are_link_calls_in_link_order() {
    let store = Store::new();
    menger(&store).await;
    for _ in 0..3 {
        store.create(SYS, ticket("MENGER", "more", Kind::Build)).await.unwrap();
    }
    // MENGER-2: never deployed.
    link(&store, "MENGER-2", &["1111111"]).await;
    // MENGER-3: the branch commit first, the merge later; only the merge carried.
    link(&store, "MENGER-3", &["2222222"]).await;
    link(&store, "MENGER-3", &["3333333"]).await;
    // MENGER-4: the merge and its branch linked together, merge first.
    link(&store, "MENGER-4", &["4444444", "5555555"]).await;
    // MENGER-5: carried, then a later version.
    link(&store, "MENGER-5", &["6666666"]).await;
    record(&store, rec("prod", "succeeded", "3333333", &["4444444", "6666666"], T0)).await;
    link(&store, "MENGER-5", &["7777777", "8888888"]).await;
    assert_eq!(where_(&store, "MENGER-2"), ["prod not"]);
    assert_eq!(where_(&store, "MENGER-3"), ["prod running"], "an earlier commit not carried is history");
    assert_eq!(where_(&store, "MENGER-4"), ["prod running"], "linked together: one version");
    assert_eq!(where_(&store, "MENGER-5"), ["prod behind 6666666: 7777777 8888888"]);
    assert_eq!(admitted(&store, "behind prod"), ["MENGER-5"]);
    // Unlinking the later version makes it current again.
    store.unlink_commits(SYS.permit("MENGER-5"), vec!["7777777".into(), "8888888".into()]).await.unwrap();
    assert_eq!(where_(&store, "MENGER-5"), ["prod running"]);
    // Relinked, it is a new version again — behind.
    link(&store, "MENGER-5", &["8888888"]).await;
    assert_eq!(where_(&store, "MENGER-5"), ["prod behind 6666666: 8888888"]);
    assert!(Lens::parse("behind prod within 2d").is_err(), "no window");
    assert!(Lens::parse("behind prod by me").is_err(), "no by");
    assert!(Lens::parse("behind").is_err());
    assert_eq!(Lens::parse("behind  Prod").unwrap().text(), "behind prod");
}

/// A record that names the ticket carries what was linked by its moment:
/// a commit linked after it is behind, the version that runs unnamed.
#[tokio::test]
async fn a_record_naming_the_ticket_carries_what_was_linked_by_then() {
    let store = Store::new();
    menger(&store).await;
    let mut r = rec("prod", "succeeded", "9999999", &[], chrono::Utc::now().timestamp() + 3600);
    r.tickets = vec!["MENGER-2".into()];
    link(&store, "MENGER-2", &["1111111"]).await;
    record(&store, r).await;
    assert_eq!(where_(&store, "MENGER-2"), ["prod running"], "linked before the record's moment");
    let mut early = rec("test", "succeeded", "9999999", &[], T0);
    early.tickets = vec!["MENGER-2".into()];
    record(&store, early).await;
    assert_eq!(where_(&store, "MENGER-2"), ["test behind -: 1111111", "prod running"], "linked after the record's moment");
}

/// A watch on `behind prod` hears a link enter it and a promote leave it.
#[tokio::test]
async fn a_behind_watch_hears_a_link_and_a_promote() {
    let store = Store::new();
    menger(&store).await;
    link(&store, "MENGER-2", &["aaaaaaa"]).await;
    record(&store, rec("prod", "succeeded", "aaaaaaa", &["aaaaaaa"], T0)).await;
    let watcher = store.watcher(Lens::parse("behind prod").unwrap());
    let mut rx = store.subscribe_filtered();
    link(&store, "MENGER-2", &["bbbbbbb"]).await;
    record(&store, rec("prod", "succeeded", "bbbbbbb", &["bbbbbbb"], T0 + 10)).await;
    let mut heard = Vec::new();
    while let Ok(c) = rx.try_change() {
        if let Change::UnderMoved { moves } = c {
            heard.extend(watcher.under_membership(&moves));
        }
    }
    assert_eq!(
        heard,
        [
            Membership::Entered { id: "MENGER-2".into(), shortcode: ShortcodeId::from("MENGER"), seq: 2 },
            Membership::Left { id: "MENGER-2".into(), shortcode: ShortcodeId::from("MENGER"), seq: 2 }
        ]
    );
}

/// An old database — commits linked before DEPLOY-20, no record of their
/// calls — keeps its answers: they are one version. Once the ticket gains a
/// commit, that one is the next version. Kept across a reopen;
/// `--verify-derived` holds the kept behind to a scan.
#[tokio::test]
async fn an_old_database_keeps_its_answers_until_a_ticket_gains_a_commit() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("behind.db");
    {
        let store = Store::open(&path).unwrap();
        menger(&store).await;
        link(&store, "MENGER-2", &["a7badc5", "d607756"]).await;
        link(&store, "MENGER-2", &["68e1271"]).await;
        record(&store, rec("prod", "succeeded", "d607756", &["a7badc5", "d607756"], T0)).await;
        assert_eq!(where_(&store, "MENGER-2"), ["prod behind d607756: 68e1271"]);
    }
    // As a file from before DEPLOY-20: no record of the calls at all.
    let conn = rusqlite::Connection::open(&path).unwrap();
    conn.execute("DROP TABLE commit_links", []).unwrap();
    drop(conn);
    {
        let store = Store::open(&path).unwrap();
        assert_eq!(where_(&store, "MENGER-2"), ["prod running"], "one version: today's answer");
        link(&store, "MENGER-2", &["4145520"]).await;
        assert_eq!(where_(&store, "MENGER-2"), ["prod behind d607756: 4145520"], "a commit gained: the next version");
    }
    let store = Store::open(&path).unwrap();
    assert_eq!(where_(&store, "MENGER-2"), ["prod behind d607756: 4145520"], "kept across a reopen");
    drop(store);
    match Store::verify_derived(&path).unwrap() {
        Verdict::Consistent { deployments, .. } => assert_eq!(deployments, Some(1)),
        other => panic!("{other:?}"),
    }
}

/// The kept behind, after many records, links and unlinks in any order, is
/// what a scan gives.
#[tokio::test]
async fn the_kept_behind_is_a_scan() {
    let store = Store::new();
    menger(&store).await;
    for _ in 0..3 {
        store.create(SYS, ticket("MENGER", "more", Kind::Build)).await.unwrap();
    }
    let commits = ["aaaaaaa", "bbbbbbb", "ccccccc", "ddddddd", "eeeeeee"];
    let mut seed = 11u64;
    let mut next = |n: usize| {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (seed >> 33) as usize % n
    };
    let mut saw_behind = false;
    for i in 0..80 {
        match next(4) {
            0 | 1 => {
                let env = ["test", "prod"][next(2)];
                let state = ["succeeded", "succeeded", "rolled_back", "failed"][next(4)];
                let picked: Vec<&str> = (0..next(3)).map(|_| commits[next(5)]).collect();
                let mut r = rec(env, state, commits[next(5)], &picked, T0 + i * 7);
                r.previous = Some(commits[next(5)].into());
                record(&store, r).await;
            }
            2 => {
                let picked: Vec<CommitRef> = (0..1 + next(2)).map(|_| commits[next(5)].into()).collect();
                store.link_commits(SYS.permit(&format!("MENGER-{}", 2 + next(4))), picked).await.unwrap();
            }
            _ => {
                let _ = store.unlink_commits(SYS.permit(&format!("MENGER-{}", 2 + next(4))), vec![commits[next(5)].into()]).await;
            }
        }
        let cache = store.tickets.read().unwrap();
        saw_behind |= cache.deploys.envs.values().any(|k| !k.behind.is_empty());
    }
    assert!(saw_behind, "the walk was behind somewhere");
    let cache = store.tickets.read().unwrap();
    let rows: Vec<&Row> = cache.order.values().collect();
    let scanned = super::deployments::scanned_cells(&cache.deploys, &rows);
    let behind = super::deployments::scanned_behind(&cache.deploys, &scanned);
    for (env, k) in &cache.deploys.envs {
        assert_eq!(&k.behind, behind.get(env).unwrap(), "{env:?}");
    }
}
