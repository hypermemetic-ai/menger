//! DEPLOY-25: a request — a deploy asked for from menger. Kept, then
//! canceled; a CI's record naming it closes it (the request says the
//! record's state, and cannot be canceled after); `requested` runs and
//! completes nothing; the `deployer` grant is enforced; `requested <env>`
//! as a filter; what a request deploys when it names nothing; the
//! deliveries the store keeps; a reopen and `--verify-derived`.

use super::history::Clock;
use super::*;
use crate::access::{Access, Held, VerifiedUser};
use crate::address::OrgId;
use crate::deployments::{DeliveryAttempt, DeployState, Deployment, DeploymentsEvent, Record, RequestState, WebhookState};
use crate::members::Role;

const WRITER: &str = "idp:0b0e1d2c-60aa-4534-b2e3-83634591fee2";
const SECRET: &str = "store-test-secret-0123456789";
const T0: i64 = 1_791_800_000;

fn ticket(epic: &str, title: &str, kind: Kind) -> NewTicket {
    NewTicket { epic: epic.into(), title: title.into(), kind, severity: None, blocked_by: Vec::new(), parent: None, path: None, body: String::new() }
}

fn rec(env: &str, state: &str, revision: &str, at: i64) -> Record {
    Record {
        project: "menger".into(),
        environment: env.into(),
        state: state.into(),
        revision: revision.into(),
        commits: vec![revision.into()],
        release: Some(format!("r-{revision}")),
        at: Some(at.to_string()),
        ..Record::default()
    }
}

/// MENGER-1 a scope, MENGER-2 linked to `aaaaaaa` (Complete), MENGER-3 to
/// `bbbbbbb`; OTHER-1 a scope. Test runs `aaaaaaa` then `bbbbbbb`; prod
/// runs `aaaaaaa` and promotes from test.
async fn menger(store: &Store) {
    store.create(SYS, ticket("MENGER", "menger", Kind::Scope)).await.unwrap();
    store.create(SYS, ticket("MENGER", "older work", Kind::Build)).await.unwrap();
    store.create(SYS, ticket("MENGER", "newer work", Kind::Build)).await.unwrap();
    store.create(SYS, ticket("OTHER", "other", Kind::Scope)).await.unwrap();
    store.link_commits(SYS.permit("MENGER-2"), vec!["aaaaaaa".into()]).await.unwrap();
    store.link_commits(SYS.permit("MENGER-3"), vec!["bbbbbbb".into()]).await.unwrap();
    store.set_status(SYS.permit("MENGER-2"), Status::Complete).await.unwrap();
    for (env, r, at) in [("test", "aaaaaaa", T0), ("prod", "aaaaaaa", T0 + 10), ("test", "bbbbbbb", T0 + 20)] {
        store.record_deployment(SYS, rec(env, "succeeded", r, at), "ci".into()).await.unwrap();
    }
    store.set_promotes(SYS, "menger", "prod", Some("test"), "owner".into()).await.unwrap();
}

async fn tracker() -> Store {
    let store = Store::new();
    menger(&store).await;
    store
}

fn ask(env: &str) -> RequestAsk {
    RequestAsk { project: "menger".into(), environment: env.into(), note: Some("the week's work".into()), ..RequestAsk::default() }
}

/// The request answered: its deployment and what it would carry.
fn request_of(ev: DeploymentsEvent) -> (Deployment, Vec<String>) {
    match ev {
        DeploymentsEvent::Request { deployment, carried } => (deployment, carried),
        other => panic!("{other:?}"),
    }
}

/// The request `id` as `list` says it.
fn listed(store: &Store, id: u64) -> Deployment {
    let q = DeployQuery { state: Some(DeployState::Requested), ..DeployQuery::default() };
    store
        .deployments(&q, None, None, None)
        .unwrap()
        .rows
        .into_iter()
        .find_map(|e| match e {
            DeploymentsEvent::Deployment { deployment, .. } if deployment.id == id => Some(deployment),
            _ => None,
        })
        .expect("listed")
}

fn admitted(store: &Store, text: &str) -> Vec<String> {
    let lens = Lens::parse(text).unwrap_or_else(|e| panic!("{text}: {e}")).with_clock(Clock { fixed: Some(T0 + 100), ..Clock::default() });
    let q = Filtered { lens, epic: None, status: None, parent: None, keep: None };
    store.filtered_page(SYS, &q, None, None, None).unwrap().tickets.into_iter().map(|t| t.id.into_string()).collect()
}

/// A user holding `role` on each of `shortcodes`.
fn holding(role: Role, shortcodes: &[&str]) -> VerifiedUser {
    let held = shortcodes.iter().map(|s| (Held::Shortcode { org: OrgId::new(OrgId::HYPERMEMETIC).unwrap(), id: ShortcodeId::new(s) }, role)).collect();
    VerifiedUser::for_tests(&Access::for_tests(), WRITER.parse().unwrap(), held)
}

/// Kept as a `requested` deployment — what test runs, from test, pending,
/// by who asked, with its note and no webhook — then canceled: it says so,
/// and a second cancel is refused.
#[tokio::test]
async fn a_request_is_kept_then_canceled() {
    let store = tracker().await;
    let (d, carried) = request_of(store.request_deployment(SYS, ask("prod"), "owner".into()).await.unwrap());
    assert_eq!((d.state, d.project.as_str(), d.environment.as_str()), (DeployState::Requested, "MENGER", "prod"));
    assert_eq!((d.revision.as_str(), d.release.as_deref(), d.previous.as_deref()), ("bbbbbbb", Some("r-bbbbbbb"), Some("aaaaaaa")));
    assert_eq!(d.recorded_by, "owner");
    let q = d.requested.clone().unwrap();
    assert_eq!((q.state, q.from.as_deref(), q.note.as_deref(), q.webhook, q.delivery), (RequestState::Pending, Some("test"), Some("the week's work"), WebhookState::None, None));
    assert_eq!(carried, ["MENGER-3"], "what runs on test and not on prod");
    assert_eq!(listed(&store, d.id), d);

    let (c, carried) = request_of(store.cancel_request(SYS, d.id, "owner".into()).await.unwrap());
    let q = c.requested.unwrap();
    assert_eq!((q.state, q.canceled_by.as_deref()), (RequestState::Canceled, Some("owner")));
    assert!(q.canceled_at.is_some() && carried.is_empty());
    let again = store.cancel_request(SYS, d.id, "owner".into()).await.unwrap_err();
    assert!(again.to_string().contains("canceled already"), "{again}");
    assert!(store.cancel_request(SYS, 1, "owner".into()).await.unwrap_err().to_string().contains("no request 1"));
}

/// A CI's records naming the request close it: it says each one's state,
/// the latest is its outcome, and it cannot be canceled now. A record
/// naming what is no request, or a request to another environment, is
/// refused; `requested` is no state a CI records.
#[tokio::test]
async fn a_ci_record_naming_the_request_closes_it() {
    let store = tracker().await;
    let (d, _) = request_of(store.request_deployment(SYS, ask("prod"), "owner".into()).await.unwrap());
    let mut events = store.subscribe_deployments();
    let mut started = rec("prod", "started", "bbbbbbb", T0 + 30);
    started.request = Some(d.id);
    let s = store.record_deployment(SYS, started, "ci".into()).await.unwrap();
    let DeploymentsEvent::Deployed { deployment: sd, .. } = &s else { panic!() };
    assert_eq!(sd.request, Some(d.id));
    assert_eq!(listed(&store, d.id).requested.unwrap().state, RequestState::Started);
    let mut done = rec("prod", "succeeded", "bbbbbbb", T0 + 40);
    done.request = Some(d.id);
    let DeploymentsEvent::Deployed { deployment: ok, .. } = store.record_deployment(SYS, done, "ci".into()).await.unwrap() else { panic!() };
    let q = listed(&store, d.id).requested.unwrap();
    assert_eq!((q.state, q.outcome), (RequestState::Succeeded, Some(ok.id)));
    let heard: Vec<RequestState> = std::iter::from_fn(|| events.try_recv().ok())
        .filter_map(|e| match e {
            DeploymentsEvent::Request { deployment, .. } => deployment.requested.map(|q| q.state),
            _ => None,
        })
        .collect();
    assert_eq!(heard, [RequestState::Started, RequestState::Succeeded], "watch follows the request");
    let e = store.cancel_request(SYS, d.id, "owner".into()).await.unwrap_err().to_string();
    assert!(e.contains("picked up") && e.contains(&ok.id.to_string()), "{e}");

    let mut wrong = rec("test", "succeeded", "bbbbbbb", T0 + 50);
    wrong.request = Some(d.id);
    assert!(store.record_deployment(SYS, wrong, "ci".into()).await.unwrap_err().to_string().contains("is to MENGER/prod, not MENGER/test"));
    let mut none = rec("prod", "succeeded", "bbbbbbb", T0 + 60);
    none.request = Some(ok.id);
    assert!(store.record_deployment(SYS, none, "ci".into()).await.unwrap_err().to_string().contains("no request"));
    let e = store.record_deployment(SYS, rec("prod", "requested", "bbbbbbb", T0 + 70), "ci".into()).await.unwrap_err().to_string();
    assert!(e.contains("deployments request"), "{e}");
}

/// A request runs nothing and completes nothing: no ticket runs on prod
/// because of it, prod's environment is as it was, no status moves — and
/// it carries what it would, for `requested prod` and `for_ticket`.
#[tokio::test]
async fn requested_never_runs_or_completes_a_ticket() {
    let store = tracker().await;
    let before = store.environments(Some("menger"), None).unwrap();
    let (d, _) = request_of(store.request_deployment(SYS, ask("prod"), "owner".into()).await.unwrap());
    assert_eq!(store.environments(Some("menger"), None).unwrap(), before, "no environment moves");
    let prod: Vec<DeploymentsEvent> = store.for_ticket("MENGER-3", None).unwrap();
    assert!(prod.iter().any(|e| matches!(e, DeploymentsEvent::NotRunning { environment, .. } if environment == "prod")), "{prod:?}");
    assert!(prod.iter().any(|e| matches!(e, DeploymentsEvent::Requested { environment, request, .. } if environment == "prod" && *request == d.id)), "{prod:?}");
    assert_eq!(store.row("MENGER-3").unwrap().0.status, Status::Pending);
    assert_eq!(store.row("MENGER-2").unwrap().0.status, Status::Complete);
    assert!(admitted(&store, "deployed prod").contains(&"MENGER-2".to_string()) && !admitted(&store, "deployed prod").contains(&"MENGER-3".to_string()));
    assert_eq!(admitted(&store, "requested prod"), ["MENGER-3"]);
    assert_eq!(admitted(&store, "requested menger/prod"), ["MENGER-3"]);
    assert!(admitted(&store, "requested test").is_empty());
    // A deploy stage is not moved by it either.
    assert_eq!(admitted(&store, "ready for prod"), ["MENGER-3"]);
    assert!(admitted(&store, "deploying prod").is_empty());
    // Canceled, it carries nothing.
    store.cancel_request(SYS, d.id, "owner".into()).await.unwrap();
    assert!(admitted(&store, "requested prod").is_empty());
    assert!(!store.for_ticket("MENGER-3", None).unwrap().iter().any(|e| matches!(e, DeploymentsEvent::Requested { .. })));
}

/// `request` and `cancel_request` need deployer (or admin) on exactly the
/// project: a deployer on MENGER may, an editor may not mint the proof, and
/// a proof for another project is refused by the store.
#[tokio::test]
async fn the_deployer_grant_is_enforced() {
    let store = tracker().await;
    let menger = store.scope_of_shortcode(&ShortcodeId::new("MENGER"));
    let other = store.scope_of_shortcode(&ShortcodeId::new("OTHER"));
    assert!(holding(Role::Editor, &["MENGER"]).on::<crate::access::Deployer>(&menger, &store).is_err(), "an editor holds no deployer's proof");
    assert!(holding(Role::Deployer, &["OTHER"]).on::<crate::access::Deployer>(&menger, &store).is_err());
    let elsewhere = holding(Role::Deployer, &["OTHER"]).on::<crate::access::Deployer>(&other, &store).unwrap();
    let e = store.request_deployment(elsewhere, ask("prod"), "x".into()).await.unwrap_err();
    assert!(e.to_string().contains("the write was not given the proof it needs"), "{e}");
    let editor = holding(Role::Editor, &["MENGER"]).on::<crate::access::Editor>(&menger, &store).unwrap();
    assert!(store.request_deployment(editor, ask("prod"), "x".into()).await.is_err(), "an editor's proof is not a deployer's");
    let deployer = holding(Role::Deployer, &["MENGER"]);
    let on = deployer.on::<crate::access::Deployer>(&menger, &store).unwrap();
    let (d, _) = request_of(store.request_deployment(on, ask("prod"), "ci-key".into()).await.unwrap());
    let elsewhere = holding(Role::Deployer, &["OTHER"]).on::<crate::access::Deployer>(&other, &store).unwrap();
    assert!(store.cancel_request(elsewhere, d.id, "x".into()).await.is_err());
    let admin = holding(Role::Admin, &["MENGER"]).admin_of(&menger, &store).unwrap();
    store.cancel_request(admin, d.id, "owner".into()).await.unwrap();
    // The webhook is admin's: a deployer's proof sets none.
    let on = deployer.on::<crate::access::Deployer>(&menger, &store).unwrap();
    assert!(store.set_webhook(on, "menger", "prod", "https://ci.example.com/h", SECRET.into(), "x".into()).await.is_err());
}

/// What a request deploys when it names nothing is what its source runs;
/// a release alone is its latest recorded deploy's revision; with no
/// source, or a release never recorded, it is refused and nothing is kept.
#[tokio::test]
async fn what_a_request_deploys() {
    let store = tracker().await;
    let named = RequestAsk { revision: Some("ccccccc".into()), ..ask("prod") };
    let (d, carried) = request_of(store.request_deployment(SYS, named, "owner".into()).await.unwrap());
    assert_eq!((d.revision.as_str(), d.release.as_deref(), d.requested.unwrap().from), ("ccccccc", None, None));
    assert!(carried.is_empty(), "nothing menger knows of runs ccccccc");
    let rel = RequestAsk { release: Some("r-aaaaaaa".into()), ..ask("prod") };
    let (d, _) = request_of(store.request_deployment(SYS, rel, "owner".into()).await.unwrap());
    assert_eq!(d.revision, "aaaaaaa");
    let n = store.deployments(&DeployQuery::default(), None, None, None).unwrap().rows.len();
    for (a, why) in [
        (ask("test"), "promotes from no environment"),
        (RequestAsk { release: Some("r-zzz".into()), ..ask("prod") }, "no deployment of release r-zzz"),
        (RequestAsk { revision: Some("not-a-sha".into()), ..ask("prod") }, "not a commit reference"),
        (RequestAsk { project: "nope".into(), ..ask("prod") }, "no shortcode NOPE"),
        (RequestAsk { note: Some("x".repeat(deployments::NOTE_MAX + 1)), ..ask("prod") }, "a note is at most"),
    ] {
        let e = store.request_deployment(SYS, a, "owner".into()).await.unwrap_err().to_string();
        assert!(e.contains(why) && e.contains("nothing was requested"), "{e}");
    }
    assert_eq!(store.deployments(&DeployQuery::default(), None, None, None).unwrap().rows.len(), n, "nothing kept");
}

/// With a webhook set, a request's delivery is due: the store hands it out
/// (URL, secret, the body), keeps each attempt and the state it leaves —
/// retrying, then delivered — and, once the webhook is cleared, says why it
/// cannot be made.
#[tokio::test]
async fn the_deliveries_the_store_keeps() {
    let store = tracker().await;
    let set = store.set_webhook(SYS, "menger", "prod", "https://ci.example.com/hook", SECRET.into(), "owner".into()).await.unwrap();
    assert!(!format!("{set:?}").contains(SECRET));
    let (d, _) = request_of(store.request_deployment(SYS, ask("prod"), "owner".into()).await.unwrap());
    let q = d.requested.unwrap();
    assert_eq!(q.webhook, WebhookState::Pending);
    assert_eq!(store.deliveries_due(), [d.id]);
    let due = store.delivery(d.id).unwrap().unwrap();
    assert_eq!((due.attempt, due.url.as_str(), due.secret(), due.delivery.clone()), (1, "https://ci.example.com/hook", SECRET, q.delivery.unwrap()));
    assert_eq!((due.payload.request, due.payload.revision.as_str(), due.payload.from.as_deref(), due.payload.note.as_deref()), (d.id, "bbbbbbb", Some("test"), Some("the week's work")));
    assert!(!format!("{due:?}").contains(SECRET));
    let failed = DeliveryAttempt { attempt: 1, at: T0, status: Some(502), error: Some("x".repeat(500)) };
    let (r, _) = request_of(store.delivery_attempted(SYS, d.id, failed, Some(T0 + 5)).await.unwrap());
    let q = r.requested.unwrap();
    assert_eq!((q.webhook, q.next_attempt_at, q.attempts[0].error.as_ref().map(String::len)), (WebhookState::Retrying, Some(T0 + 5), Some(ATTEMPT_ERROR)));
    assert_eq!(store.delivery(d.id).unwrap().unwrap().attempt, 2);
    let ok = DeliveryAttempt { attempt: 2, at: T0 + 5, status: Some(204), error: None };
    let (r, _) = request_of(store.delivery_attempted(SYS, d.id, ok, None).await.unwrap());
    assert_eq!((r.requested.as_ref().unwrap().webhook, r.requested.unwrap().attempts.len()), (WebhookState::Delivered, 2));
    assert!(store.delivery(d.id).unwrap().is_none() && store.deliveries_due().is_empty());

    // Another, then the webhook cleared: due, and it cannot be made.
    let (d2, _) = request_of(store.request_deployment(SYS, ask("prod"), "owner".into()).await.unwrap());
    store.clear_webhook(SYS, "menger", "prod").await.unwrap();
    let (n, why) = store.delivery(d2.id).unwrap_err();
    assert!(n == 1 && why.contains("no webhook"), "{why}");
    // A third, with none: no CI configured.
    let (d3, _) = request_of(store.request_deployment(SYS, ask("prod"), "owner".into()).await.unwrap());
    assert_eq!(d3.requested.unwrap().webhook, WebhookState::None);
}

/// The length an attempt's error is cut to.
const ATTEMPT_ERROR: usize = 200;

/// Kept in the file: a reopen answers each request as it was — canceled,
/// answered, its attempts and the webhook (secret and all, never said) —
/// and `--verify-derived` finds the store consistent.
#[tokio::test]
async fn kept_across_a_reopen_and_verified() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("requests.db");
    let (a, b, c);
    {
        let store = Store::open(&path).unwrap();
        menger(&store).await;
        store.set_webhook(SYS, "menger", "prod", "https://ci.example.com/hook", SECRET.into(), "owner".into()).await.unwrap();
        a = request_of(store.request_deployment(SYS, ask("prod"), "owner".into()).await.unwrap()).0;
        b = request_of(store.request_deployment(SYS, ask("prod"), "owner".into()).await.unwrap()).0;
        c = request_of(store.request_deployment(SYS, ask("prod"), "owner".into()).await.unwrap()).0;
        store.cancel_request(SYS, a.id, "owner".into()).await.unwrap();
        let mut done = rec("prod", "succeeded", "bbbbbbb", T0 + 40);
        done.request = Some(b.id);
        store.record_deployment(SYS, done, "ci".into()).await.unwrap();
        let tried = DeliveryAttempt { attempt: 1, at: T0, status: None, error: Some("connection refused".into()) };
        store.delivery_attempted(SYS, c.id, tried, Some(T0 + 1)).await.unwrap();
    }
    let store = Store::open(&path).unwrap();
    let (qa, qb, qc) = (listed(&store, a.id).requested.unwrap(), listed(&store, b.id).requested.unwrap(), listed(&store, c.id).requested.unwrap());
    assert_eq!((qa.state, qa.webhook), (RequestState::Canceled, WebhookState::Stopped));
    assert_eq!((qb.state, qb.webhook), (RequestState::Succeeded, WebhookState::Stopped));
    assert_eq!((qc.state, qc.webhook, qc.attempts.len(), qc.next_attempt_at), (RequestState::Pending, WebhookState::Retrying, 1, Some(T0 + 1)));
    assert_eq!(store.deliveries_due(), [c.id]);
    assert_eq!(store.delivery(c.id).unwrap().unwrap().secret(), SECRET);
    assert!(admitted(&store, "requested prod").is_empty(), "prod runs bbbbbbb now: nothing more to carry");
    drop(store);
    match Store::verify_derived(&path).unwrap() {
        Verdict::Consistent { deployments, .. } => assert_eq!(deployments, Some(3 + 3 + 1)),
        other => panic!("{other:?}"),
    }
}
