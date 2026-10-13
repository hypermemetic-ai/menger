//! DEPLOY-11: deployments — every row of the ticket's table held: a
//! succeeded record runs its tickets from `at`; a commit linked after it was
//! deployed counts; a rollback, or a later deploy to an older revision,
//! takes out what only the undone deploys carried; `failed` and `started`
//! count for nothing; a retried record is one deployment; an unlinked commit
//! is kept and said. And the `deployed` cells as filters (windows, `not`,
//! a project's environment), a `watch { filter }` hearing a deploy, the
//! refusals, a reopen, and `--verify-derived`.

use super::history::Clock;
use super::tests::Changes;
use super::*;
use crate::deployments::{DeployState, DeploymentsEvent, Record};
use std::collections::HashSet;

fn ticket(epic: &str, title: &str, kind: Kind) -> NewTicket {
    NewTicket {
        epic: epic.into(),
        title: title.into(),
        kind,
        severity: None,
        blocked_by: Vec::new(),
        parent: None,
        path: None,
        body: String::new(),
    }
}

/// A record to `env` of MENGER, at `at` (unix seconds).
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

/// MENGER-1 a scope; MENGER-2..4 builds, linked to `aaaaaaa`, `bbbbbbb`,
/// `ccccccc`; MENGER-5 linked to nothing.
async fn menger() -> Store {
    let store = Store::new();
    store.create(ticket("MENGER", "menger", Kind::Scope)).await.unwrap();
    for (i, c) in ["aaaaaaa", "bbbbbbb", "ccccccc"].into_iter().enumerate() {
        store.create(ticket("MENGER", &format!("work {i}"), Kind::Build)).await.unwrap();
        store.link_commits(&format!("MENGER-{}", i + 2), vec![c.into()]).await.unwrap();
    }
    store.create(ticket("MENGER", "unlinked", Kind::Build)).await.unwrap();
    store
}

async fn record(store: &Store, r: Record) -> DeploymentsEvent {
    store.record_deployment(r, "ci".into()).await.unwrap()
}

/// The environments `id` runs in, each with since.
fn running(store: &Store, id: &str) -> Vec<(String, i64)> {
    store
        .for_ticket(id, None)
        .unwrap()
        .into_iter()
        .filter_map(|e| match e {
            DeploymentsEvent::Running { environment, since, .. } => Some((environment, since)),
            _ => None,
        })
        .collect()
}

fn admitted(store: &Store, text: &str, now: i64) -> Vec<String> {
    let clock = Clock { fixed: Some(now), ..Clock::default() };
    let lens = Lens::parse(text).unwrap_or_else(|e| panic!("{text}: {e}")).with_clock(clock);
    let q = Filtered { lens, epic: None, status: None, parent: None, keep: None };
    store.filtered_page(&q, None, None, None).unwrap().tickets.into_iter().map(|t| t.id).collect()
}

fn entered_left(ev: &DeploymentsEvent) -> (Vec<String>, Vec<String>) {
    match ev {
        DeploymentsEvent::Deployed { entered, left, .. } => (entered.clone(), left.clone()),
        other => panic!("{other:?}"),
    }
}

const T0: i64 = 1_791_800_000;

/// A succeeded record: the environment runs its revision, and the tickets
/// whose linked commits are among its commits — a CI's whole SHA matching a
/// link's seven — plus the ids it names run there from `at`. The status
/// moves not at all.
#[tokio::test]
async fn a_succeeded_record_runs_its_tickets_from_at() {
    let store = menger().await;
    let mut r = rec("prod", "succeeded", "bbbbbbb", &["aaaaaaa0123456789abcdef0123456789abcdef0", "bbbbbbb"], T0);
    r.tickets = vec!["menger-5".into()];
    let ev = record(&store, r).await;
    let DeploymentsEvent::Deployed { deployment, carried, unlinked, live, entered, duplicate, .. } = ev else { panic!() };
    assert_eq!(deployment.project, "MENGER");
    assert_eq!(carried, ["MENGER-2", "MENGER-3", "MENGER-5"]);
    assert_eq!(entered, carried);
    assert!(unlinked.is_empty() && live && !duplicate);
    assert_eq!(running(&store, "MENGER-2"), [("prod".to_string(), T0)]);
    assert!(running(&store, "MENGER-4").is_empty());
    let envs = store.environments(Some("menger"), None).unwrap();
    assert_eq!(envs.len(), 1);
    assert_eq!((envs[0].revision.as_deref(), envs[0].since, envs[0].tickets), (Some("bbbbbbb"), Some(T0), 3));
    assert_eq!(store.row("MENGER-2").unwrap().0.status, Status::Pending, "a deployment is a fact beside the status");
}

/// A commit linked after it was deployed counts wherever it runs; one
/// unlinked takes it back out.
#[tokio::test]
async fn the_join_is_kept_not_computed_once() {
    let store = menger().await;
    record(&store, rec("test", "succeeded", "ddddddd", &["ddddddd"], T0)).await;
    assert!(running(&store, "MENGER-5").is_empty());
    store.link_commits("MENGER-5", vec!["ddddddd".into()]).await.unwrap();
    assert_eq!(running(&store, "MENGER-5"), [("test".to_string(), T0)]);
    assert_eq!(admitted(&store, "deployed test", T0 + 10), ["MENGER-5"]);
    store.unlink_commits("MENGER-5", vec!["ddddddd".into()]).await.unwrap();
    assert!(running(&store, "MENGER-5").is_empty());
}

/// A rollback cuts the environment back to its revision: what only the
/// undone deploys carried leaves. A later deploy to an older revision does
/// the same.
#[tokio::test]
async fn rollbacks_take_work_back_out() {
    let store = menger().await;
    record(&store, rec("prod", "succeeded", "aaaaaaa", &["aaaaaaa"], T0)).await;
    record(&store, rec("prod", "succeeded", "bbbbbbb", &["bbbbbbb"], T0 + 100)).await;
    let ev = record(&store, rec("prod", "succeeded", "ccccccc", &["ccccccc"], T0 + 200)).await;
    assert_eq!(entered_left(&ev).0, ["MENGER-4"]);
    let mut back = rec("prod", "rolled_back", "aaaaaaa", &[], T0 + 300);
    back.previous = Some("ccccccc".into());
    let ev = record(&store, back).await;
    assert_eq!(entered_left(&ev), (vec![], vec!["MENGER-3".to_string(), "MENGER-4".to_string()]));
    assert_eq!(running(&store, "MENGER-2"), [("prod".to_string(), T0)], "since its own deploy, not the rollback");
    assert!(running(&store, "MENGER-3").is_empty());
    let env = &store.environments(None, None).unwrap()[0];
    assert_eq!((env.revision.as_deref(), env.since, env.last_state), (Some("aaaaaaa"), Some(T0 + 300), Some(DeployState::RolledBack)));
    // Forward again, then a deploy back to an older revision.
    record(&store, rec("prod", "succeeded", "ccccccc", &["bbbbbbb", "ccccccc"], T0 + 400)).await;
    assert_eq!(admitted(&store, "deployed prod", T0 + 500), ["MENGER-2", "MENGER-3", "MENGER-4"]);
    let ev = record(&store, rec("prod", "succeeded", "aaaaaaa", &[], T0 + 500)).await;
    assert_eq!(entered_left(&ev).1, ["MENGER-3", "MENGER-4"]);
    assert_eq!(admitted(&store, "deployed prod", T0 + 600), ["MENGER-2"]);
    // A rollback naming only what it took out: below it.
    record(&store, rec("prod", "succeeded", "bbbbbbb", &["bbbbbbb"], T0 + 600)).await;
    let mut out = rec("prod", "rolled_back", "9999999", &[], T0 + 700);
    out.previous = Some("bbbbbbb".into());
    let ev = record(&store, out).await;
    assert_eq!(entered_left(&ev).1, ["MENGER-3"]);
    let list = store.deployments(&DeployQuery::default(), None, None, None).unwrap();
    let live: Vec<bool> = list.rows.iter().map(|r| matches!(r, DeploymentsEvent::Deployment { live: true, .. })).collect();
    assert_eq!(live.iter().filter(|l| **l).count(), 2, "the first aaaaaaa deploy and its redeploy still count: {live:?}");
}

/// `failed` and `started` are recorded and shown; nothing runs because of
/// them, and the environment's revision stays.
#[tokio::test]
async fn failed_and_started_count_for_nothing() {
    let store = menger().await;
    record(&store, rec("prod", "succeeded", "aaaaaaa", &["aaaaaaa"], T0)).await;
    for state in ["started", "failed"] {
        let ev = record(&store, rec("prod", state, "bbbbbbb", &["bbbbbbb"], T0 + 10)).await;
        let DeploymentsEvent::Deployed { live, entered, left, deployment, .. } = ev else { panic!() };
        assert!(!live && entered.is_empty() && left.is_empty(), "{state}");
        assert_eq!(deployment.state.as_str(), state);
    }
    let env = &store.environments(None, None).unwrap()[0];
    assert_eq!((env.revision.as_deref(), env.last_state), (Some("aaaaaaa"), Some(DeployState::Failed)));
    assert!(running(&store, "MENGER-3").is_empty());
    assert_eq!(store.deployments(&DeployQuery::default(), None, None, None).unwrap().rows.len(), 3);
}

/// The same record twice — a CI's retry — is one deployment: the second
/// answers the first, `duplicate`, and writes nothing. The run singles it
/// out when given, else `at`.
#[tokio::test]
async fn a_retried_record_is_one_deployment() {
    let store = menger().await;
    let mut r = rec("prod", "succeeded", "aaaaaaa", &["aaaaaaa"], T0);
    r.source = Some(crate::deployments::DeploySource { system: Some("gha".into()), run: Some("41".into()), url: None });
    let first = record(&store, r.clone()).await;
    r.at = Some((T0 + 60).to_string());
    let again = record(&store, r).await;
    let id = |e: &DeploymentsEvent| match e {
        DeploymentsEvent::Deployed { deployment, duplicate, .. } => (deployment.id, *duplicate),
        _ => panic!(),
    };
    assert_eq!((id(&first), id(&again)), ((1, false), (1, true)));
    assert_eq!(store.deployments(&DeployQuery::default(), None, None, None).unwrap().rows.len(), 1);
}

/// A commit no ticket links is kept on the deployment, and listed as
/// unlinked.
#[tokio::test]
async fn an_unlinked_commit_is_kept_and_said() {
    let store = menger().await;
    let ev = record(&store, rec("prod", "succeeded", "eeeeeee", &["aaaaaaa", "plexus-core@1234567"], T0)).await;
    let DeploymentsEvent::Deployed { deployment, unlinked, carried, .. } = ev else { panic!() };
    assert_eq!(deployment.commits, ["aaaaaaa", "plexus-core@1234567"]);
    assert_eq!(unlinked, ["plexus-core@1234567", "eeeeeee"]);
    assert_eq!(carried, ["MENGER-2"]);
}

/// `deployed <env>`, `not deployed <env>`, a window over since, a project's
/// environment; refused with `by`.
#[tokio::test]
async fn the_deployed_cells_filter() {
    let store = menger().await;
    store.create(ticket("OTHER", "other", Kind::Scope)).await.unwrap();
    record(&store, rec("test", "succeeded", "aaaaaaa", &["aaaaaaa", "bbbbbbb"], T0)).await;
    record(&store, rec("prod", "succeeded", "aaaaaaa", &["aaaaaaa"], T0 + 86_400 * 2)).await;
    let now = T0 + 86_400 * 2 + 3600;
    assert_eq!(admitted(&store, "deployed prod", now), ["MENGER-2"]);
    assert_eq!(admitted(&store, "deployed menger/test", now), ["MENGER-2", "MENGER-3"]);
    assert!(admitted(&store, "deployed other/test", now).is_empty());
    assert_eq!(admitted(&store, "deployed test within 24h", now), Vec::<String>::new());
    assert_eq!(admitted(&store, "deployed prod within 24h", now), ["MENGER-2"]);
    assert_eq!(admitted(&store, "in MENGER and kind build and not deployed prod", now), ["MENGER-3", "MENGER-4", "MENGER-5"]);
    assert_eq!(admitted(&store, "deployed test and not deployed prod", now), ["MENGER-3"]);
    assert!(Lens::parse("deployed prod by me").is_err());
    assert!(Lens::parse("deployed").is_err());
    assert_eq!(Lens::parse("deployed  Prod within 24h").unwrap().text(), "deployed prod within 24h");
    // A relative window moves with the clock, at the moment it will.
    let lens = Lens::parse("deployed prod within 24h").unwrap().with_clock(Clock { fixed: Some(now), ..Clock::default() });
    assert!(lens.relative());
    assert_eq!(store.next_boundary(&lens), Some(T0 + 86_400 * 3 + 1));
}

/// A recorded deployment moves a `watch { filter }` on `deployed`: entered,
/// and — rolled back — left.
#[tokio::test]
async fn a_deployed_watch_hears_a_deploy_and_its_rollback() {
    let store = menger().await;
    let watcher = store.watcher(Lens::parse("deployed prod").unwrap());
    let mut rx = store.subscribe_filtered();
    let mut deployed = store.subscribe_deployments();
    record(&store, rec("prod", "succeeded", "aaaaaaa", &["aaaaaaa"], T0)).await;
    let mut back = rec("prod", "rolled_back", "0000000", &[], T0 + 10);
    back.previous = Some("aaaaaaa".into());
    record(&store, back).await;
    let mut heard = Vec::new();
    while let Ok(c) = rx.try_change() {
        if let Change::UnderMoved { moves } = c {
            heard.extend(watcher.under_membership(&moves));
        }
    }
    assert_eq!(
        heard,
        [
            Membership::Entered { id: "MENGER-2".into(), epic: "MENGER".into(), seq: 2 },
            Membership::Left { id: "MENGER-2".into(), epic: "MENGER".into(), seq: 2 }
        ]
    );
    let mut types = Vec::new();
    while let Ok(e) = deployed.try_recv() {
        types.push(serde_json::to_value(&e).unwrap()["type"].as_str().unwrap().to_string());
    }
    assert_eq!(types, ["deployed", "environment_changed", "deployed", "environment_changed"]);
}

/// Each refusal names its problem and keeps nothing.
#[tokio::test]
async fn refusals_keep_nothing() {
    let store = menger().await;
    let cases = [
        (Record { project: "nope".into(), ..rec("prod", "succeeded", "aaaaaaa", &[], T0) }, "no shortcode NOPE"),
        (rec("prod", "deployed", "aaaaaaa", &[], T0), "unknown deployment state"),
        (rec("prod", "succeeded", "xyz", &[], T0), "revision `xyz` is not a commit reference"),
        (rec("prod", "succeeded", "aaaaaaa", &["zz"], T0), "not commit references: `zz`"),
        (rec("a b", "succeeded", "aaaaaaa", &[], T0), "not an environment name"),
        (Record { at: Some("yesterday".into()), ..rec("prod", "succeeded", "aaaaaaa", &[], T0) }, "not RFC 3339"),
        (Record { tickets: vec!["nope".into()], ..rec("prod", "succeeded", "aaaaaaa", &[], T0) }, "not a ticket id"),
    ];
    for (r, why) in cases {
        let e = store.record_deployment(r, "ci".into()).await.unwrap_err().to_string();
        assert!(e.contains(why), "{e} — wanted {why}");
    }
    assert!(store.deployments(&DeployQuery::default(), None, None, None).unwrap().rows.is_empty());
    assert!(store.environments(None, None).unwrap().is_empty());
}

/// `list` newest first, paged; by environment, state and carried ticket;
/// within the projects allowed. `for_ticket` says where it does not run too.
#[tokio::test]
async fn list_pages_and_for_ticket_says_where_not() {
    let store = menger().await;
    record(&store, rec("test", "succeeded", "aaaaaaa", &["aaaaaaa"], T0)).await;
    record(&store, rec("prod", "succeeded", "aaaaaaa", &["aaaaaaa"], T0 + 10)).await;
    record(&store, rec("staging", "failed", "bbbbbbb", &["bbbbbbb"], T0 + 20)).await;
    let ids = |p: &DeployPage| -> Vec<u64> {
        p.rows
            .iter()
            .map(|r| match r {
                DeploymentsEvent::Deployment { deployment, .. } => deployment.id,
                _ => panic!(),
            })
            .collect()
    };
    let first = store.deployments(&DeployQuery::default(), None, None, Some(2)).unwrap();
    assert_eq!((ids(&first), first.next), (vec![3, 2], Some(2)));
    let rest = store.deployments(&DeployQuery::default(), None, first.next, Some(2)).unwrap();
    assert_eq!((ids(&rest), rest.next), (vec![1], None));
    let q = DeployQuery { ticket: Some("menger-2".into()), ..DeployQuery::default() };
    assert_eq!(ids(&store.deployments(&q, None, None, None).unwrap()), [2, 1]);
    let q = DeployQuery { state: Some(DeployState::Failed), environment: Some("STAGING".into()), ..DeployQuery::default() };
    assert_eq!(ids(&store.deployments(&q, None, None, None).unwrap()), [3]);
    let none: HashSet<String> = HashSet::new();
    assert!(store.deployments(&DeployQuery::default(), Some(&none), None, None).unwrap().rows.is_empty());
    let names: Vec<String> = store.environments(None, None).unwrap().into_iter().map(|e| e.name).collect();
    assert_eq!(names, ["test", "staging", "prod"], "test → staging → prod");
    let rows: Vec<String> = store
        .for_ticket("MENGER-2", None)
        .unwrap()
        .into_iter()
        .map(|e| match e {
            DeploymentsEvent::Running { environment, .. } => format!("{environment} running"),
            DeploymentsEvent::NotRunning { environment, .. } => format!("{environment} not"),
            _ => panic!(),
        })
        .collect();
    assert_eq!(rows, ["test running", "staging not", "prod running"]);
    assert!(matches!(store.for_ticket("MENGER-99", None), Err(StoreError::NotFound(_))));
}

/// Kept in the file: a reopen answers the same; `--verify-derived` finds it
/// consistent, and names a stored environment edited by hand.
#[tokio::test]
async fn kept_across_a_reopen_and_verified() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("deploys.db");
    {
        let store = Store::open(&path).unwrap();
        store.create(ticket("MENGER", "menger", Kind::Scope)).await.unwrap();
        store.create(ticket("MENGER", "work", Kind::Build)).await.unwrap();
        store.link_commits("MENGER-2", vec!["aaaaaaa".into()]).await.unwrap();
        record(&store, rec("prod", "succeeded", "aaaaaaa", &["aaaaaaa"], T0)).await;
        record(&store, rec("prod", "succeeded", "bbbbbbb", &["bbbbbbb"], T0 + 5)).await;
        // Linked after it was deployed.
        store.create(ticket("MENGER", "late", Kind::Build)).await.unwrap();
        store.link_commits("MENGER-3", vec!["bbbbbbb".into()]).await.unwrap();
    }
    let store = Store::open(&path).unwrap();
    assert_eq!(running(&store, "MENGER-2"), [("prod".to_string(), T0)]);
    assert_eq!(running(&store, "MENGER-3"), [("prod".to_string(), T0 + 5)]);
    drop(store);
    match Store::verify_derived(&path).unwrap() {
        Verdict::Consistent { deployments, .. } => assert_eq!(deployments, Some(2)),
        other => panic!("{other:?}"),
    }
    let conn = rusqlite::Connection::open(&path).unwrap();
    conn.execute("UPDATE deploy_environments SET revision = 'ccccccc'", []).unwrap();
    drop(conn);
    match Store::verify_derived(&path).unwrap() {
        Verdict::Differs(d) => assert!(d.join("\n").contains("environment MENGER/prod: stored as"), "{d:?}"),
        other => panic!("{other:?}"),
    }
}

/// The kept cells, after many records, links and unlinks in any order, are
/// what a scan of every ticket against every deployment gives.
#[tokio::test]
async fn the_kept_cells_are_a_scan() {
    let store = menger().await;
    let commits = ["aaaaaaa", "bbbbbbb", "ccccccc", "ddddddd", "eeeeeee"];
    let mut seed = 7u64;
    let mut next = |n: usize| {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (seed >> 33) as usize % n
    };
    for i in 0..60 {
        match next(4) {
            0 | 1 => {
                let env = ["test", "prod"][next(2)];
                let state = ["succeeded", "succeeded", "rolled_back", "failed"][next(4)];
                let picked: Vec<&str> = (0..next(3)).map(|_| commits[next(5)]).collect();
                let mut r = rec(env, state, commits[next(5)], &picked, T0 + i * 7 - next(3) as i64 * 50);
                r.previous = Some(commits[next(5)].into());
                record(&store, r).await;
            }
            2 => {
                store.link_commits(&format!("MENGER-{}", 2 + next(4)), vec![commits[next(5)].into()]).await.unwrap();
            }
            _ => {
                let _ = store.unlink_commits(&format!("MENGER-{}", 2 + next(4)), vec![commits[next(5)].into()]).await;
            }
        }
    }
    let cache = store.tickets.read().unwrap();
    let rows: Vec<&Row> = cache.order.values().collect();
    assert!(cache.deploys.envs.values().any(|k| !k.running.is_empty()), "the walk ran something somewhere");
    let scanned = super::deployments::scanned_cells(&cache.deploys, &rows);
    for (env, k) in &cache.deploys.envs {
        assert_eq!(&k.running, scanned.get(env).unwrap(), "{env:?}");
    }
}

/// A fixture DEPLOY-10's `deploy/record` is held to: what it prints.
fn fixture(name: &str) -> String {
    let path = format!("{}/../deploy/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"))
}

/// DEPLOY-10's records, as `deploy/record` makes them from each CI's form —
/// the pipeline's log (deploys, a failed one, its rollback, a sync, a
/// promote rolled back), a GitHub `deployment_status`, a GitHub Actions job,
/// a GitLab review app, any JSON — taken whole as `record`'s parameters.
#[tokio::test]
async fn deploy_10s_records_are_taken_whole() {
    let store = Store::new();
    store.create(ticket("MENGER", "menger", Kind::Scope)).await.unwrap();
    store.create(ticket("MENGER", "first", Kind::Build)).await.unwrap();
    store.create(ticket("MENGER", "third", Kind::Build)).await.unwrap();
    store.link_commits("MENGER-2", vec!["1111111".into()]).await.unwrap();
    store.link_commits("MENGER-3", vec!["3333333".into()]).await.unwrap();
    for line in fixture("pipeline.expected.ndjson").lines().filter(|l| !l.trim().is_empty()) {
        let r: Record = serde_json::from_str(line).unwrap();
        let ev = record(&store, r).await;
        assert!(matches!(ev, DeploymentsEvent::Deployed { duplicate: false, .. }), "{line}: {ev:?}");
    }
    let envs: Vec<(String, Option<String>)> =
        store.environments(Some("MENGER"), None).unwrap().into_iter().map(|e| (e.name, e.revision)).collect();
    assert_eq!(
        envs,
        [("test".into(), Some("3333333".into())), ("staging".into(), Some("3333333".into())), ("prod".into(), Some("1111111".into()))]
    );
    let at = |s: &str| chrono::DateTime::parse_from_rfc3339(s).unwrap().timestamp();
    assert_eq!(
        running(&store, "MENGER-2"),
        [("test".to_string(), at("2026-10-01T10:01:00Z")), ("prod".to_string(), at("2026-10-01T10:02:00Z"))]
    );
    assert_eq!(
        running(&store, "MENGER-3"),
        [("test".to_string(), at("2026-10-03T08:01:00Z")), ("staging".to_string(), at("2026-10-05T09:00:00Z"))]
    );

    store.create(ticket("SHOP", "shop", Kind::Scope)).await.unwrap();
    for i in 2..=9 {
        store.create(ticket("SHOP", &format!("shop {i}"), Kind::Build)).await.unwrap();
    }
    store.link_commits("SHOP-2", vec!["4f1c2a9".into()]).await.unwrap();
    for name in ["deployment_status", "github-actions", "gitlab", "ci-output"] {
        let r: Record = serde_json::from_str(&fixture(&format!("{name}.expected.json"))).unwrap();
        let ev = record(&store, r).await;
        assert!(matches!(ev, DeploymentsEvent::Deployed { duplicate: false, .. }), "{name}: {ev:?}");
    }
    let rows = |id: &str| -> Vec<String> {
        store
            .for_ticket(id, None)
            .unwrap()
            .into_iter()
            .map(|e| match e {
                DeploymentsEvent::Running { environment, .. } => format!("{environment} running"),
                DeploymentsEvent::NotRunning { environment, .. } => format!("{environment} not"),
                _ => panic!(),
            })
            .collect()
    };
    assert_eq!(rows("SHOP-3"), ["review/pr-412 not", "staging running", "prod not", "production running"]);
    assert_eq!(rows("SHOP-9"), ["review/pr-412 not", "staging running", "prod not", "production not"]);
    assert_eq!(rows("SHOP-2"), ["review/pr-412 not", "staging not", "prod running", "production not"], "4f1c2a9 is 4f1c2a9be0d1");
    assert_eq!(admitted(&store, "deployed shop/review/pr-412", T0), Vec::<String>::new(), "a failed review app runs nothing");
    assert_eq!(admitted(&store, "deployed staging", T0 * 2), ["MENGER-3", "SHOP-3", "SHOP-9"]);
}

/// DEPLOY-13: `running` — every environment's kept cells in one read, in
/// display order (test before prod), each ticket in the store's order with
/// since and the deploy that brought it; narrowed to a project, an
/// environment, or ids (an environment with none of them answers empty, so
/// a copy can replace it); a rollback's answer is the cells it left.
#[tokio::test]
async fn running_answers_every_environments_cells() {
    let store = menger().await;
    record(&store, rec("prod", "succeeded", "aaaaaaa", &["aaaaaaa"], T0)).await;
    record(&store, rec("test", "succeeded", "bbbbbbb", &["aaaaaaa", "bbbbbbb"], T0 + 10)).await;
    let cells = |r: Vec<DeploymentsEvent>| -> Vec<(String, Vec<(String, i64, u64)>)> {
        r.into_iter()
            .map(|e| match e {
                DeploymentsEvent::Cells { environment, running, .. } => {
                    (environment, running.into_iter().map(|c| (c.ticket, c.since, c.deployment)).collect())
                }
                other => panic!("{other:?}"),
            })
            .collect()
    };
    let all = cells(store.running(None, None, None, None).unwrap());
    assert_eq!(
        all,
        [
            ("test".to_string(), vec![("MENGER-2".to_string(), T0 + 10, 2), ("MENGER-3".to_string(), T0 + 10, 2)]),
            ("prod".to_string(), vec![("MENGER-2".to_string(), T0, 1)]),
        ]
    );
    assert_eq!(cells(store.running(Some("MENGER"), Some("Prod"), None, None).unwrap()), [all[1].clone()]);
    let ids = vec!["menger-3".to_string()];
    assert_eq!(
        cells(store.running(None, None, Some(&ids), None).unwrap()),
        [("test".to_string(), vec![("MENGER-3".to_string(), T0 + 10, 2)]), ("prod".to_string(), vec![])]
    );
    let mut back = rec("test", "rolled_back", "aaaaaaa", &[], T0 + 20);
    back.previous = Some("bbbbbbb".into());
    record(&store, back).await;
    assert_eq!(cells(store.running(None, Some("test"), None, None).unwrap()), [("test".to_string(), vec![])], "nothing ran on test before");
    let none: HashSet<String> = HashSet::new();
    assert!(store.running(None, None, None, Some(&none)).unwrap().is_empty(), "no project held: nothing");
    assert!(store.running(Some("no such"), None, None, None).is_err());
    let many: Vec<String> = (0..=super::deployments::RUNNING_IDS_MAX).map(|i| format!("MENGER-{i}")).collect();
    assert!(store.running(None, None, Some(&many), None).is_err());
}
