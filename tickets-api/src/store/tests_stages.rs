//! DEPLOY-21: a ticket's delivery stage per environment — the acceptance's
//! transitions, each: a deploy to test (waiting → ready for prod), a
//! promote (→ deployed prod), a started record (deploying), failed and
//! rolled back, a new commit linked (back to waiting), an environment with
//! no source (no `ready for` it); where an environment promotes from, set by
//! a record's `from` and by `promotes` (refusing itself and a cycle); the
//! stages in `running`'s cells; a filtered watch that hears a promote; the
//! kept stages across a reopen and `--verify-derived`.

use super::history::Clock;
use super::tests::Changes;
use super::*;
use crate::address::ShortcodeId;
use crate::deployments::{DeliveryStage, DeploymentsEvent, Record};

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
        from: (env == "prod").then(|| "test".to_string()),
        ..Record::default()
    }
}

async fn record(store: &Store, r: Record) -> DeploymentsEvent {
    store.record_deployment(SYS, r, "ci".into()).await.unwrap()
}

async fn link(store: &Store, id: &str, commits: &[&str]) {
    store.link_commits(SYS.permit(id), commits.iter().map(|c| (*c).into()).collect()).await.unwrap();
}

async fn complete(store: &Store, id: &str) {
    store.set_status(SYS.permit(id), Status::Complete).await.unwrap();
}

/// MENGER-1 a scope; MENGER-2..=MENGER-`1 + n` builds, each Complete with
/// its own commit (`2222222`, `3333333`, …).
async fn menger(store: &Store, n: u32) {
    store.create(SYS, ticket("MENGER", "menger", Kind::Scope)).await.unwrap();
    for i in 0..n {
        let id = format!("MENGER-{}", i + 2);
        store.create(SYS, ticket("MENGER", "a build", Kind::Build)).await.unwrap();
        link(store, &id, &[&sha(i + 2)]).await;
        complete(store, &id).await;
    }
}

/// A seven-digit commit for ticket number `n`: `2222222` for 2.
fn sha(n: u32) -> String {
    format!("{n}").repeat(7)[..7].to_string()
}

fn admitted(store: &Store, text: &str) -> Vec<String> {
    let clock = Clock { fixed: Some(T0 + 1_000_000), ..Clock::default() };
    let lens = Lens::parse(text).unwrap_or_else(|e| panic!("{text}: {e}")).with_clock(clock);
    let q = Filtered { lens, epic: None, status: None, parent: None, keep: None };
    store.filtered_page(SYS, &q, None, None, None).unwrap().tickets.into_iter().map(|t| t.id.into_string()).collect()
}

/// Each stage `env` holds, as `stage: ids`.
fn stages(store: &Store, env: &str) -> Vec<String> {
    DeliveryStage::ALL
        .into_iter()
        .map(|s| format!("{s}: {}", admitted(store, &format!("{s} {env}")).join(" ")))
        .chain(std::iter::once(format!("deployed: {}", admitted(store, &format!("deployed {env} and not behind {env}")).join(" "))))
        .collect()
}

fn of(lines: &[&str]) -> Vec<String> {
    lines.iter().map(|l| l.to_string()).collect()
}

const T0: i64 = 1_791_800_000;

/// The acceptance's walk, for one ticket: waiting everywhere; a deploy to
/// test starts (deploying test), succeeds (ready for prod); a promote
/// starts (deploying prod), succeeds (deployed prod); a new commit is a new
/// version (back to waiting); a failed deploy of it to test (failed test);
/// the next succeeds and its promote fails and is rolled back (failed prod).
#[tokio::test]
async fn one_ticket_walks_the_stages() {
    let store = Store::new();
    menger(&store, 1).await;
    store.set_promotes(SYS, "menger", "prod", Some("test"), "owner".into()).await.unwrap();
    let w = |t: &str, p: &str| vec![format!("waiting: {}", t), "deploying: ".into(), format!("ready for: {}", p), "failed: ".into(), "deployed: ".into()];
    assert_eq!(stages(&store, "test"), w("MENGER-2", ""), "done, deployed nowhere: waiting");
    assert_eq!(stages(&store, "prod"), w("MENGER-2", ""));

    record(&store, rec("test", "started", "2222222", &["2222222"], T0)).await;
    assert_eq!(stages(&store, "test"), of(&["waiting: ", "deploying: MENGER-2", "ready for: ", "failed: ", "deployed: "]));
    assert_eq!(stages(&store, "prod"), w("MENGER-2", ""), "still waiting for prod");

    record(&store, rec("test", "succeeded", "2222222", &["2222222"], T0 + 10)).await;
    assert_eq!(stages(&store, "test"), of(&["waiting: ", "deploying: ", "ready for: ", "failed: ", "deployed: MENGER-2"]));
    assert_eq!(stages(&store, "prod"), w("", "MENGER-2"), "on test: ready for prod");
    assert_eq!(admitted(&store, "ready for menger/prod"), ["MENGER-2"]);

    record(&store, rec("prod", "started", "2222222", &["2222222"], T0 + 20)).await;
    assert_eq!(stages(&store, "prod"), of(&["waiting: ", "deploying: MENGER-2", "ready for: ", "failed: ", "deployed: "]), "deploying, not ready for");
    record(&store, rec("prod", "succeeded", "2222222", &["2222222"], T0 + 30)).await;
    assert_eq!(stages(&store, "prod"), of(&["waiting: ", "deploying: ", "ready for: ", "failed: ", "deployed: MENGER-2"]));

    // A new commit: a new version, running nowhere — back to waiting.
    link(&store, "MENGER-2", &["2a2a2a2"]).await;
    assert_eq!(stages(&store, "test"), w("MENGER-2", ""));
    assert_eq!(stages(&store, "prod"), w("MENGER-2", ""));
    assert_eq!(admitted(&store, "deployed prod and behind prod"), ["MENGER-2"], "still on prod, behind");

    // Its deploy to test fails: failed test, waiting for prod.
    record(&store, rec("test", "started", "2a2a2a2", &["2a2a2a2"], T0 + 40)).await;
    record(&store, rec("test", "failed", "2a2a2a2", &["2a2a2a2"], T0 + 50)).await;
    assert_eq!(stages(&store, "test"), of(&["waiting: ", "deploying: ", "ready for: ", "failed: MENGER-2", "deployed: "]));
    assert_eq!(stages(&store, "prod"), w("MENGER-2", ""));

    // The next succeeds; its promote fails, and prod is rolled back.
    record(&store, rec("test", "succeeded", "2a2a2a2", &["2a2a2a2"], T0 + 60)).await;
    assert_eq!(stages(&store, "prod"), w("", "MENGER-2"));
    record(&store, rec("prod", "started", "2a2a2a2", &["2a2a2a2"], T0 + 70)).await;
    record(&store, rec("prod", "failed", "2a2a2a2", &["2a2a2a2"], T0 + 80)).await;
    let mut back = rec("prod", "rolled_back", "2222222", &[], T0 + 90);
    back.previous = Some("2a2a2a2".into());
    record(&store, back).await;
    assert_eq!(stages(&store, "prod"), of(&["waiting: ", "deploying: ", "ready for: ", "failed: MENGER-2", "deployed: "]), "failed, not ready for");
    assert_eq!(admitted(&store, "ready for prod or deploying prod"), Vec::<String>::new(), "Moving in: nothing");

    // A second promote succeeds: deployed.
    record(&store, rec("prod", "succeeded", "2a2a2a2", &["2a2a2a2"], T0 + 100)).await;
    assert_eq!(stages(&store, "prod"), of(&["waiting: ", "deploying: ", "ready for: ", "failed: ", "deployed: MENGER-2"]));
}

/// A rollback that undoes a deploy that ran: what only it carried is failed
/// there (and ready for again where it still runs upstream — failed comes
/// first); a later deploy to an older revision is no failure.
#[tokio::test]
async fn a_rollback_fails_what_it_undid() {
    let store = Store::new();
    menger(&store, 2).await;
    record(&store, rec("test", "succeeded", "3333333", &["2222222", "3333333"], T0)).await;
    record(&store, rec("prod", "succeeded", "2222222", &["2222222"], T0 + 10)).await;
    record(&store, rec("prod", "succeeded", "3333333", &["3333333"], T0 + 20)).await;
    assert_eq!(admitted(&store, "deployed prod"), ["MENGER-2", "MENGER-3"]);
    let mut back = rec("prod", "rolled_back", "2222222", &[], T0 + 30);
    back.previous = Some("3333333".into());
    record(&store, back).await;
    assert_eq!(stages(&store, "prod"), of(&["waiting: ", "deploying: ", "ready for: ", "failed: MENGER-3", "deployed: MENGER-2"]));
    // A deploy forward again ends it.
    record(&store, rec("prod", "succeeded", "3333333", &["3333333"], T0 + 40)).await;
    assert_eq!(stages(&store, "prod"), of(&["waiting: ", "deploying: ", "ready for: ", "failed: ", "deployed: MENGER-2 MENGER-3"]));
}

/// An environment with no source has no `ready for`: `edge` promotes from
/// none, and is a track of its own — waiting there is about edge alone.
/// Only a Complete ticket with commits, of the project, waits.
#[tokio::test]
async fn an_environment_with_no_source_has_nothing_ready_for_it() {
    let store = Store::new();
    menger(&store, 2).await;
    store.create(SYS, ticket("MENGER", "not done", Kind::Build)).await.unwrap();
    link(&store, "MENGER-4", &["4444444"]).await;
    store.create(SYS, ticket("OTHER", "elsewhere", Kind::Scope)).await.unwrap();
    store.create(SYS, ticket("OTHER", "done elsewhere", Kind::Build)).await.unwrap();
    link(&store, "OTHER-2", &["5555555"]).await;
    complete(&store, "OTHER-2").await;
    record(&store, rec("test", "succeeded", "2222222", &["2222222"], T0)).await;
    let mut edge = rec("edge", "succeeded", "3333333", &["3333333"], T0 + 10);
    edge.from = None;
    record(&store, edge).await;
    assert!(admitted(&store, "ready for edge").is_empty(), "edge promotes from none");
    assert!(admitted(&store, "ready for test").is_empty(), "test promotes from none");
    assert_eq!(admitted(&store, "waiting edge"), ["MENGER-2"], "on test, not edge: edge is its own track");
    assert_eq!(admitted(&store, "waiting test"), ["MENGER-3"], "Complete with commits, in MENGER; not MENGER-4 (not done), not OTHER-2");
    assert!(admitted(&store, "ready for prod").is_empty(), "prod has no source until it is said");
    store.set_promotes(SYS, "menger", "prod", Some("test"), "owner".into()).await.unwrap();
    assert_eq!(admitted(&store, "ready for prod"), ["MENGER-2"], "prod promotes from test: ready for it with no deploy of its own yet");
    assert_eq!(admitted(&store, "waiting prod"), ["MENGER-3"]);
    store.set_promotes(SYS, "menger", "prod", None, "owner".into()).await.unwrap();
    assert!(admitted(&store, "ready for prod").is_empty(), "from none again");
}

/// Where an environment promotes from: a record's `from` sets it (and a
/// later one moves it), `promotes` sets and clears it, `environments`
/// answers it; itself and a cycle are refused, nothing written.
#[tokio::test]
async fn sources_are_set_by_a_record_and_by_promotes() {
    let store = Store::new();
    menger(&store, 1).await;
    record(&store, rec("test", "succeeded", "2222222", &["2222222"], T0)).await;
    record(&store, rec("prod", "succeeded", "2222222", &["2222222"], T0 + 10)).await;
    let from = |store: &Store| -> Vec<(String, Option<String>)> {
        store.environments(Some("menger"), None).unwrap().into_iter().map(|e| (e.name, e.from)).collect()
    };
    assert_eq!(from(&store), [("test".to_string(), None), ("prod".to_string(), Some("test".to_string()))]);
    let refused = store.set_promotes(SYS, "menger", "test", Some("prod"), "owner".into()).await.unwrap_err().to_string();
    assert!(refused.contains("cycle"), "{refused}");
    let refused = store.set_promotes(SYS, "menger", "test", Some("test"), "owner".into()).await.unwrap_err().to_string();
    assert!(refused.contains("itself"), "{refused}");
    let mut r = rec("test", "succeeded", "2222222", &[], T0 + 20);
    r.from = Some("prod".into());
    assert!(store.record_deployment(SYS, r, "ci".into()).await.unwrap_err().to_string().contains("cycle"));
    assert_eq!(from(&store), [("test".to_string(), None), ("prod".to_string(), Some("test".to_string()))], "nothing written");
    match store.set_promotes(SYS, "menger", "prod", Some("staging"), "owner".into()).await.unwrap() {
        DeploymentsEvent::Promotes { from, .. } => assert_eq!(from.as_deref(), Some("staging")),
        other => panic!("{other:?}"),
    }
    assert!(store.set_promotes(SYS, "nope", "prod", None, "owner".into()).await.is_err());
}

/// `running` names each ticket's stage beside the running cells.
#[tokio::test]
async fn running_says_the_stages() {
    let store = Store::new();
    menger(&store, 3).await;
    record(&store, rec("test", "succeeded", "3333333", &["2222222", "3333333"], T0)).await;
    record(&store, rec("prod", "started", "2222222", &["2222222"], T0 + 10)).await;
    let staged = match store.running(None, Some("prod"), None, None).unwrap().pop() {
        Some(DeploymentsEvent::Cells { staged, .. }) => staged.into_iter().map(|c| format!("{} {}", c.ticket, c.stage)).collect::<Vec<_>>(),
        other => panic!("{other:?}"),
    };
    assert_eq!(staged, ["MENGER-2 deploying", "MENGER-3 ready for"]);
}

/// A filtered watch on `ready for prod` hears a deploy to test (entered) and
/// the promote (left); one on `waiting test` hears a ticket completed.
#[tokio::test]
async fn a_stage_watch_hears_a_deploy_and_a_promote() {
    let store = Store::new();
    menger(&store, 1).await;
    store.create(SYS, ticket("MENGER", "later", Kind::Build)).await.unwrap();
    link(&store, "MENGER-3", &["3333333"]).await;
    store.set_promotes(SYS, "menger", "prod", Some("test"), "owner".into()).await.unwrap();
    let ready = store.watcher(Lens::parse("ready for prod").unwrap());
    let waiting = store.watcher(Lens::parse("waiting test").unwrap());
    let mut rx = store.subscribe_filtered();
    record(&store, rec("test", "succeeded", "2222222", &["2222222"], T0)).await;
    record(&store, rec("prod", "succeeded", "2222222", &["2222222"], T0 + 10)).await;
    complete(&store, "MENGER-3").await;
    let (mut r, mut w) = (Vec::new(), Vec::new());
    while let Ok(c) = rx.try_change() {
        if let Change::UnderMoved { moves } = c {
            r.extend(ready.under_membership(&moves));
            w.extend(waiting.under_membership(&moves));
        }
    }
    let m = ShortcodeId::from("MENGER");
    assert_eq!(
        r,
        [Membership::Entered { id: "MENGER-2".into(), shortcode: m.clone(), seq: 2 }, Membership::Left { id: "MENGER-2".into(), shortcode: m.clone(), seq: 2 }]
    );
    assert_eq!(
        w,
        [Membership::Left { id: "MENGER-2".into(), shortcode: m.clone(), seq: 2 }, Membership::Entered { id: "MENGER-3".into(), shortcode: m, seq: 3 }]
    );
}

/// The language: each stage reads and says itself; no window, no `by`.
#[test]
fn the_stages_read_and_say_themselves() {
    for (text, said) in [("ready for prod", "ready for prod"), ("Ready For menger/prod", "ready for menger/prod"), ("deploying test", "deploying test"), ("waiting prod", "waiting prod"), ("failed prod", "failed prod")] {
        let lens = Lens::parse(text).unwrap_or_else(|e| panic!("{text}: {e}"));
        assert_eq!(lens.history()[0].format(), said);
    }
    for bad in ["ready for", "waiting prod today", "failed prod by me", "deploying"] {
        assert!(Lens::parse(bad).is_err(), "{bad}");
    }
    assert!(Lens::parse("ready and kind build").is_ok(), "`ready` alone is the ready set still");
}

/// The kept stages survive a reopen; `--verify-derived` holds them to a
/// fresh join.
#[tokio::test]
async fn the_stages_are_kept_across_a_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("stages.db");
    {
        let store = Store::open(&path).unwrap();
        menger(&store, 2).await;
        record(&store, rec("test", "succeeded", "3333333", &["2222222", "3333333"], T0)).await;
        record(&store, rec("prod", "started", "2222222", &["2222222"], T0 + 10)).await;
        record(&store, rec("prod", "failed", "2222222", &["2222222"], T0 + 20)).await;
        link(&store, "MENGER-3", &["3a3a3a3"]).await;
    }
    let store = Store::open(&path).unwrap();
    assert_eq!(stages(&store, "prod"), of(&["waiting: MENGER-3", "deploying: ", "ready for: ", "failed: MENGER-2", "deployed: "]));
    assert_eq!(admitted(&store, "waiting test"), ["MENGER-3"]);
    drop(store);
    match Store::verify_derived(&path).unwrap() {
        Verdict::Consistent { deployments, .. } => assert_eq!(deployments, Some(3)),
        other => panic!("{other:?}"),
    }
}
