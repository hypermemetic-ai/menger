//! DEPLOY-37: a request's steps — reported by its executor, kept by name,
//! said on watch; refused once the request is canceled or ended; a cancel
//! of a request under way asks its executor to stop; the deployer's grant;
//! a reopen.

use super::*;
use crate::access::{Access, Held, VerifiedUser};
use crate::address::OrgId;
use crate::deployments::{DeployStep, Deployment, DeploymentsEvent, Record, RequestState, StepState, STEPS_MAX, STEP_DETAIL_MAX};
use crate::members::Role;

const WRITER: &str = "idp:0b0e1d2c-60aa-4534-b2e3-83634591fee2";

async fn menger(store: &Store) {
    let scope = NewTicket { epic: "MENGER".into(), title: "menger".into(), kind: Kind::Scope, severity: None, blocked_by: Vec::new(), parent: None, path: None, body: String::new() };
    store.create(SYS, scope).await.unwrap();
    let ran = Record { project: "menger".into(), environment: "test".into(), state: "succeeded".into(), revision: "aaaaaaa".into(), ..Record::default() };
    store.record_deployment(SYS, ran, "ci".into()).await.unwrap();
}

fn ask() -> RequestAsk {
    RequestAsk { project: "menger".into(), environment: "test".into(), revision: Some("bbbbbbb".into()), ..RequestAsk::default() }
}

fn report(request: u64, name: &str, state: StepState) -> StepReport {
    StepReport { request, name: name.into(), state, detail: None, url: None }
}

fn request_of(ev: DeploymentsEvent) -> Deployment {
    match ev {
        DeploymentsEvent::Request { deployment, .. } => deployment,
        other => panic!("{other:?}"),
    }
}

fn steps(d: &Deployment) -> Vec<(String, StepState, bool)> {
    d.requested.as_ref().unwrap().steps.iter().map(|s: &DeployStep| (s.name.clone(), s.state, s.ended_at.is_some())).collect()
}

/// The executor's `started` record, naming the request.
async fn pick_up(store: &Store, id: u64) {
    let started = Record { project: "menger".into(), environment: "test".into(), state: "started".into(), revision: "bbbbbbb".into(), request: Some(id), ..Record::default() };
    store.record_deployment(SYS, started, "runner".into()).await.unwrap();
}

/// Steps are kept by name in the order first reported — a step reported
/// again keeps its place and start — each said on watch as the request; a
/// detail keeps its end; names and urls are checked.
#[tokio::test]
async fn steps_are_kept_by_name_and_said_on_watch() {
    let store = Store::new();
    menger(&store).await;
    let id = request_of(store.request_deployment(SYS, ask(), "owner".into()).await.unwrap()).id;
    let mut events = store.subscribe_deployments();
    let first = request_of(store.request_step(SYS, report(id, "Build", StepState::Started), "runner".into()).await.unwrap());
    let q = first.requested.as_ref().unwrap();
    assert!(q.state == RequestState::Started && !q.webhook.due(), "a step says its executor has it: started, the webhook done");
    pick_up(&store, id).await;
    store.request_step(SYS, report(id, "build", StepState::Succeeded), "runner".into()).await.unwrap();
    store.request_step(SYS, report(id, "gate", StepState::Started), "runner".into()).await.unwrap();
    let long = format!("{}\nthe last line", "x".repeat(STEP_DETAIL_MAX));
    let failed = StepReport { detail: Some(long), url: Some("http://127.0.0.1:4471/runs/9".into()), ..report(id, "gate", StepState::Failed) };
    let d = request_of(store.request_step(SYS, failed, "runner".into()).await.unwrap());
    assert_eq!(steps(&d), [("build".to_string(), StepState::Succeeded, true), ("gate".to_string(), StepState::Failed, true)]);
    let gate = &d.requested.as_ref().unwrap().steps[1];
    let detail = gate.detail.as_deref().unwrap();
    assert!(detail.len() <= STEP_DETAIL_MAX && detail.ends_with("the last line"), "the end of the log is kept");
    assert_eq!((gate.url.as_deref(), gate.by.as_str()), (Some("http://127.0.0.1:4471/runs/9"), "runner"));
    assert_eq!(d.requested.as_ref().unwrap().state, RequestState::Started, "a step ends nothing: the records do");
    let heard: Vec<usize> = std::iter::from_fn(|| events.try_recv().ok())
        .filter_map(|e| match e {
            DeploymentsEvent::Request { deployment, .. } => Some(deployment.requested.unwrap().steps.len()),
            _ => None,
        })
        .collect();
    assert_eq!(heard, [1, 1, 1, 2, 2], "each step (and the record) said on watch");

    for (name, url, why) in [("", None, "not a step name"), ("a b", None, "not a step name"), ("ok", Some("http://example.com/x"), "")] {
        let r = StepReport { url: url.map(String::from), ..report(id, name, StepState::Started) };
        let e = store.request_step(SYS, r, "runner".into()).await.unwrap_err().to_string();
        assert!(e.contains(why) && e.contains("nothing was recorded"), "{e}");
    }
    assert!(store.request_step(SYS, report(9999, "build", StepState::Started), "runner".into()).await.unwrap_err().to_string().contains("no request 9999"));
}

/// A canceled request takes no step's start (its end, yes), one a record
/// has ended no step at all; one with
/// the most steps takes no new name (an old one still moves).
#[tokio::test]
async fn a_canceled_or_ended_request_takes_no_step() {
    let store = Store::new();
    menger(&store).await;
    let a = request_of(store.request_deployment(SYS, ask(), "owner".into()).await.unwrap()).id;
    store.cancel_request(SYS, a, "owner".into()).await.unwrap();
    let e = store.request_step(SYS, report(a, "build", StepState::Started), "runner".into()).await.unwrap_err().to_string();
    assert!(e.contains("is canceled: stop"), "{e}");
    let d = request_of(store.request_step(SYS, report(a, "build", StepState::Canceled), "runner".into()).await.unwrap());
    assert_eq!(steps(&d), [("build".to_string(), StepState::Canceled, true)], "how the cancel left a step is still said");

    let b = request_of(store.request_deployment(SYS, ask(), "owner".into()).await.unwrap()).id;
    for i in 0..STEPS_MAX {
        store.request_step(SYS, report(b, &format!("s{i}"), StepState::Started), "runner".into()).await.unwrap();
    }
    let e = store.request_step(SYS, report(b, "one-more", StepState::Started), "runner".into()).await.unwrap_err().to_string();
    assert!(e.contains(&format!("has {STEPS_MAX} steps")), "{e}");
    store.request_step(SYS, report(b, "s0", StepState::Succeeded), "runner".into()).await.unwrap();
    let done = Record { project: "menger".into(), environment: "test".into(), state: "succeeded".into(), revision: "bbbbbbb".into(), request: Some(b), ..Record::default() };
    store.record_deployment(SYS, done, "runner".into()).await.unwrap();
    let e = store.request_step(SYS, report(b, "s1", StepState::Succeeded), "runner".into()).await.unwrap_err().to_string();
    assert!(e.contains("has ended — succeeded"), "{e}");
}

/// A cancel of a request under way asks its executor to stop: the request
/// runs on, the next step's answer says so, a second ask is refused, and
/// the executor's `failed` ends it.
#[tokio::test]
async fn a_cancel_under_way_asks_the_executor_to_stop() {
    let store = Store::new();
    menger(&store).await;
    let id = request_of(store.request_deployment(SYS, ask(), "owner".into()).await.unwrap()).id;
    pick_up(&store, id).await;
    let d = request_of(store.cancel_request(SYS, id, "owner".into()).await.unwrap());
    let q = d.requested.unwrap();
    assert_eq!((q.state, q.cancel_asked_by.as_deref()), (RequestState::Started, Some("owner")));
    assert!(q.cancel_asked_at.is_some() && q.canceled_at.is_none());
    let e = store.cancel_request(SYS, id, "owner".into()).await.unwrap_err().to_string();
    assert!(e.contains("asked to stop already"), "{e}");
    let d = request_of(store.request_step(SYS, report(id, "build", StepState::Canceled), "runner".into()).await.unwrap());
    assert!(d.requested.unwrap().cancel_asked_at.is_some(), "the step's answer tells the executor");
    let failed = Record { project: "menger".into(), environment: "test".into(), state: "failed".into(), revision: "bbbbbbb".into(), request: Some(id), ..Record::default() };
    store.record_deployment(SYS, failed, "runner".into()).await.unwrap();
    let e = store.cancel_request(SYS, id, "owner".into()).await.unwrap_err().to_string();
    assert!(e.contains("picked up"), "{e}");
}

/// A step is the deployer's on the request's project (or admin's): an
/// editor's proof, or a deployer's elsewhere, records none.
#[tokio::test]
async fn a_step_is_the_deployers() {
    let store = Store::new();
    menger(&store).await;
    store.create(SYS, NewTicket { epic: "OTHER".into(), title: "other".into(), kind: Kind::Scope, severity: None, blocked_by: Vec::new(), parent: None, path: None, body: String::new() }).await.unwrap();
    let id = request_of(store.request_deployment(SYS, ask(), "owner".into()).await.unwrap()).id;
    let holding = |role: Role, s: &str| {
        let held = vec![(Held::Shortcode { org: OrgId::new(OrgId::HYPERMEMETIC).unwrap(), id: ShortcodeId::new(s) }, role)];
        VerifiedUser::for_tests(&Access::for_tests(), WRITER.parse().unwrap(), held)
    };
    let (menger, other) = (store.scope_of_shortcode(&ShortcodeId::new("MENGER")), store.scope_of_shortcode(&ShortcodeId::new("OTHER")));
    let editor = holding(Role::Editor, "MENGER").on::<crate::access::Editor>(&menger, &store).unwrap();
    assert!(store.request_step(editor, report(id, "build", StepState::Started), "x".into()).await.is_err());
    let elsewhere = holding(Role::Deployer, "OTHER").on::<crate::access::Deployer>(&other, &store).unwrap();
    assert!(store.request_step(elsewhere, report(id, "build", StepState::Started), "x".into()).await.is_err());
    let on = holding(Role::Deployer, "MENGER").on::<crate::access::Deployer>(&menger, &store).unwrap();
    let d = request_of(store.request_step(on, report(id, "build", StepState::Started), "ci-key".into()).await.unwrap());
    assert_eq!(d.requested.unwrap().steps[0].by, "ci-key");
}

/// Steps and a cancel ask are kept across a reopen.
#[tokio::test]
async fn kept_across_a_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("steps.db");
    let (id, tip);
    {
        let store = Store::open(&path).unwrap();
        menger(&store).await;
        let main = RequestAsk { project: "menger".into(), environment: "test".into(), branch: Some("main".into()), ..RequestAsk::default() };
        tip = request_of(store.request_deployment(SYS, main, "owner".into()).await.unwrap()).id;
        id = request_of(store.request_deployment(SYS, ask(), "owner".into()).await.unwrap()).id;
        pick_up(&store, id).await;
        store.request_step(SYS, report(id, "build", StepState::Succeeded), "runner".into()).await.unwrap();
        store.request_step(SYS, StepReport { detail: Some("ok".into()), ..report(id, "deploy", StepState::Started) }, "runner".into()).await.unwrap();
        store.cancel_request(SYS, id, "owner".into()).await.unwrap();
    }
    let store = Store::open(&path).unwrap();
    let q = DeployQuery { state: Some(crate::deployments::DeployState::Requested), ..DeployQuery::default() };
    let d = store.deployments(&q, None, None, None).unwrap().rows.into_iter().find_map(|e| match e {
        DeploymentsEvent::Deployment { deployment, .. } if deployment.id == id => Some(deployment),
        _ => None,
    }).unwrap();
    assert_eq!(steps(&d), [("build".to_string(), StepState::Succeeded, true), ("deploy".to_string(), StepState::Started, false)]);
    let r = d.requested.unwrap();
    assert_eq!((r.state, r.steps[1].detail.as_deref(), r.cancel_asked_by.as_deref()), (RequestState::Started, Some("ok"), Some("owner")));
    let branch = store.deployments(&q, None, None, None).unwrap().rows.into_iter().find_map(|e| match e {
        DeploymentsEvent::Deployment { deployment, .. } if deployment.id == tip => deployment.requested.and_then(|r| r.branch),
        _ => None,
    });
    assert_eq!(branch.as_deref(), Some("main"));
}

/// A request for a branch's tip names the branch and no revision — the
/// executor resolves it, and its record says what it deployed; a branch
/// with a revision, or a bad name, is refused.
#[tokio::test]
async fn a_request_for_a_branchs_tip() {
    let store = Store::new();
    menger(&store).await;
    let tip = RequestAsk { project: "menger".into(), environment: "test".into(), branch: Some("main".into()), ..RequestAsk::default() };
    let d = request_of(store.request_deployment(SYS, tip.clone(), "owner".into()).await.unwrap());
    assert_eq!((d.revision.as_str(), d.requested.as_ref().unwrap().branch.as_deref()), ("", Some("main")));
    let both = RequestAsk { revision: Some("bbbbbbb".into()), ..tip.clone() };
    assert!(store.request_deployment(SYS, both, "owner".into()).await.unwrap_err().to_string().contains("a revision, a release or a branch"));
    let bad = RequestAsk { branch: Some("../x".into()), ..tip };
    assert!(store.request_deployment(SYS, bad, "owner".into()).await.unwrap_err().to_string().contains("not a branch name"));
    let started = Record { project: "menger".into(), environment: "test".into(), state: "started".into(), revision: "ccccccc".into(), request: Some(d.id), ..Record::default() };
    store.record_deployment(SYS, started, "runner".into()).await.unwrap();
    let q = DeployQuery { state: Some(crate::deployments::DeployState::Requested), ..DeployQuery::default() };
    let rows = store.deployments(&q, None, None, None).unwrap().rows;
    assert!(rows.iter().any(|e| matches!(e, DeploymentsEvent::Deployment { deployment, .. } if deployment.id == d.id && deployment.requested.as_ref().unwrap().state == RequestState::Started)));
}
