//! DEPLOY-11: the `deployments` activation — **the one declaration**, as
//! `tickets` and `notes` are (MFE-11).
//!
//! | build | what the macro emits from it |
//! |---|---|
//! | `server` (default; the menger binary) | the `Activation` impl, dispatch, the schema, these bodies |
//! | `default-features = false` (the browser app, wasm32) | `DeploymentsClient<T>` — one typed method per `#[method]`, over [`crate::deployments`]' types |
//!
//! Deployments are kept by the store, behind the service-wide cache
//! ([`crate::store`]'s `deployments`): this activation is its methods — each
//! its attribute, the auth check and a match on what the store answers.
//!
//! **Under login** (AUTH-7): `record` is its own grant, held by the
//! `deployer` role — what a CI's key carries on the project's shortcode
//! (`deployer@MENGER` in its session's scope); `admin` holds it too, an
//! editor does not. The reads are a viewer's, and answer only of the
//! projects where the caller holds them; `for_ticket` where it holds it on
//! the ticket.

use crate::deployments::{DeploySource, DeployState, DeploymentsEvent};
// ACCESS-8: every parameter a type (MACRO-12's `typed_params` holds it).
use crate::vocab::{CommitRef, DeploymentId, EnvironmentName, Project, Release, TicketRef, Timestamp};
#[cfg(feature = "server")]
use crate::access::{Access, VerifiedUser};
#[cfg(feature = "server")]
use crate::deployments::Record;
#[cfg(feature = "server")]
use crate::store::{DeployQuery, Store, StoreError};
#[cfg(feature = "server")]
use async_stream::stream;
#[cfg(feature = "server")]
use futures::Stream;
#[cfg(feature = "server")]
use plexus_core::plexus::{AuthContext, Reach};
#[cfg(feature = "server")]
use std::collections::HashSet;
#[cfg(feature = "server")]
use std::sync::Arc;

/// The activation. Server-only: it holds the store.
#[cfg(feature = "server")]
#[derive(Clone)]
pub struct Deployments {
    store: Arc<Store>,
    /// The rule every method asks first ([`crate::access`]).
    access: Access,
}

#[cfg(feature = "server")]
impl Deployments {
    /// No login: every call admitted, as always.
    pub fn new(store: Arc<Store>) -> Self {
        Self::with_access(store, Access::open())
    }

    /// Behind [`Access`].
    pub fn with_access(store: Arc<Store>, access: Access) -> Self {
        Self { store, access }
    }
}

/// The projects `caller` holds its call's grant on, of those deployments
/// were recorded for — `None`: every one (login off, or held on `*`).
#[cfg(feature = "server")]
fn allowed(caller: &VerifiedUser, store: &Store) -> Result<Option<HashSet<String>>, String> {
    let Some(grant) = caller.grant().cloned() else { return Ok(None) };
    if caller.reach_filter()?.is_none() {
        return Ok(None);
    }
    Ok(Some(store.deploy_projects().into_iter().filter(|p| caller.check(&grant, p).is_ok()).collect()))
}

/// The project an event of `watch` is about.
#[cfg(feature = "server")]
fn project_of(ev: &DeploymentsEvent) -> Option<&str> {
    match ev {
        DeploymentsEvent::Deployed { deployment, .. } | DeploymentsEvent::Deployment { deployment, .. } => Some(&deployment.project),
        DeploymentsEvent::EnvironmentChanged { environment } | DeploymentsEvent::Environment { environment } => Some(&environment.project),
        DeploymentsEvent::Promotes { project, .. } => Some(project),
        _ => None,
    }
}

/// A refusal, in band.
#[cfg(feature = "server")]
fn refused(e: StoreError) -> DeploymentsEvent {
    DeploymentsEvent::Error { message: e.to_string() }
}

// Not `#[cfg]`-gated: the client-only build needs it too (see activation.rs).
#[plexus_macros::activation(
    namespace = "deployments",
    description = "Deployments: every deploy to any environment — record, promotes, list, environments, for_ticket, running, watch",
    client,
    typed_params,
    server_cfg = "feature = \"server\"",
    crate_path = "plexus_core",
    // AUTH-7: reads are a viewer's; recording is a deployer's alone (a
    // CI's key on the project's shortcode) — and admin's, who holds all.
    // DEPLOY-21: `promotes` is listed nowhere: admin's alone.
    roles(
        viewer = ["list", "environments", "for_ticket", "running", "watch"],
        editor = [viewer],
        deployer = [viewer, "record"],
    )
)]
impl Deployments {
    /// Records one deployment — DEPLOY-10's record, its fields as the
    /// parameters (`deploy/record --send` passes the record whole as the
    /// params object). `project` is a shortcode the store holds, any case;
    /// `environment` a name (kept lower case); `state` one of `started`,
    /// `succeeded`, `failed`, `rolled_back`; `revision` the commit now
    /// running (for `failed`, the one tried; for `rolled_back`, the one
    /// returned to — `previous` the one taken out); `commits` the range it
    /// carries, when the CI can list them; `tickets` ids it names itself.
    /// The tickets whose linked commits are among `commits` and `revision`,
    /// plus `tickets`, run in the environment from `at` once it succeeds; a
    /// rollback, or a later deploy to an older revision, takes out what only
    /// the undone deploys carried; `started` and `failed` count for nothing.
    /// A deployment never changes a status. Yields exactly one event:
    /// `deployed` — the deployment as kept, the tickets it carried, its
    /// commits no ticket links, and the tickets that `entered` and `left`
    /// the environment; `duplicate: true` and nothing written when the same
    /// record (project, environment, state, revision, and `source.run`,
    /// else `at`) is kept already — or an in-band `error` naming every
    /// problem, with nothing kept. DEPLOY-21: `from` names the environment
    /// this one promotes from (`test`, for `prod`), set (or moved) with the
    /// record — `promotes` follows it on `watch`.
    #[plexus_macros::method(
        group = "write",
        target = "project",
        description = "Record a deployment (DEPLOY-10's record): the tickets its commits carry run in the environment; yields deployed { deployment, carried, unlinked, entered, left, duplicate }",
        params(
            project = "Whose environment: a shortcode, any case (menger is MENGER)",
            environment = "Its name: test, staging, prod, pr-412 …",
            state = "started | succeeded | failed | rolled_back",
            revision = "The commit now running (rolled_back: the one returned to; failed: the one tried); repo@sha outside the project's repository",
            previous = "The commit that ran before (rolled_back: the one taken out) (optional)",
            commits = "Every commit in previous..revision, when the CI can list them (optional)",
            tickets = "Ticket ids the CI names itself, added to what the commits give (optional)",
            release = "The artifact's name (optional)",
            at = "When: RFC 3339 or unix seconds (optional; default now)",
            source = "Where to read the run: { system, run, url } (optional; run makes a retry one deployment)",
            from = "The environment this one promotes from, e.g. test for prod (optional; set with the record)"
        ),
        values(state = ["started", "succeeded", "failed", "rolled_back"]),
        example(note = "a promote to prod", params = r#"{"project": "menger", "environment": "prod", "state": "succeeded", "revision": "235ff62", "previous": "29a50be", "commits": ["824e943", "235ff62"], "release": "20261012T220402Z-235ff62", "at": "2026-10-12T22:04:19Z", "source": {"system": "menger-pipeline", "run": "promote"}}"#)
    )]
    pub async fn record(
        &self, auth: Option<&AuthContext>, reach: Reach,
        project: Project,
        environment: EnvironmentName,
        state: DeployState,
        revision: CommitRef,
        previous: Option<CommitRef>,
        commits: Option<Vec<CommitRef>>,
        tickets: Option<Vec<TicketRef>>,
        release: Option<Release>,
        at: Option<Timestamp>,
        source: Option<DeploySource>,
        from: Option<EnvironmentName>,
    ) -> impl Stream<Item = DeploymentsEvent> + Send + 'static {
        let texts = |v: Option<Vec<_>>| v.unwrap_or_default().into_iter().map(String::from).collect::<Vec<String>>();
        let store = self.store.clone();
        let admitted = self.access.verify_call(auth, &reach);
        stream! {
            let caller = match admitted {
                Ok(caller) => caller,
                Err(message) => {
                    yield DeploymentsEvent::Error { message };
                    return;
                }
            };
            let by = caller.principal_text().unwrap_or_else(|| crate::types::ANONYMOUS_ACTOR.to_string());
            let rec = Record {
                project: project.into_string(),
                environment: environment.into_string(),
                state: state.as_str().to_string(),
                revision: revision.into_string(),
                previous: previous.map(String::from),
                commits: texts(commits),
                tickets: tickets.unwrap_or_default().into_iter().map(String::from).collect(),
                release: release.map(String::from),
                at: at.map(String::from),
                source,
                from: from.map(String::from),
            };
            // ACCESS-11: deployer on the project's shortcode (DEPLOY-14).
            let on = match crate::activation::scope_named(&store, &rec.project).and_then(|s| caller.on::<crate::access::Deployer>(&s, &store).map_err(|r| r.to_string())) {
                Ok(on) => on,
                Err(message) => {
                    yield DeploymentsEvent::Error { message };
                    return;
                }
            };
            yield store.record_deployment(on, rec, by).await.unwrap_or_else(refused);
        }
    }

    /// DEPLOY-21: where an environment promotes from — `from` (`test`, for
    /// menger's `prod`), or, without it, from none (its own track, as the
    /// landing page's `edge`). Admin on the project's shortcode. A ticket's
    /// `ready for <environment>` is its current version running on `from`
    /// and not here; the environment's track (it and every environment it
    /// promotes from or to) is where `waiting` looks. Yields exactly one
    /// `promotes { project, environment, from }` — the same when nothing
    /// moved — or an in-band `error` (itself, a cycle), nothing written.
    #[plexus_macros::method(
        group = "write",
        target = "project",
        description = "Set where an environment promotes from (admin): prod from test; without from, from none; yields promotes { project, environment, from }",
        params(
            project = "Whose environment: a shortcode, any case (menger is MENGER)",
            environment = "The environment that promotes, e.g. prod",
            from = "The environment it promotes from, e.g. test (optional; omitted: from none)"
        ),
        example(note = "menger's prod promotes from test", params = r#"{"project": "menger", "environment": "prod", "from": "test"}"#)
    )]
    pub async fn promotes(
        &self, auth: Option<&AuthContext>, reach: Reach,
        project: Project,
        environment: EnvironmentName,
        from: Option<EnvironmentName>,
    ) -> impl Stream<Item = DeploymentsEvent> + Send + 'static {
        let store = self.store.clone();
        let admitted = self.access.verify_call(auth, &reach);
        stream! {
            let caller = match admitted {
                Ok(caller) => caller,
                Err(message) => {
                    yield DeploymentsEvent::Error { message };
                    return;
                }
            };
            let by = caller.principal_text().unwrap_or_else(|| crate::types::ANONYMOUS_ACTOR.to_string());
            // ACCESS-11: admin on the project's shortcode.
            let on = match crate::activation::scope_named(&store, &project).and_then(|s| caller.admin_of(&s, &store).map_err(|r| r.to_string())) {
                Ok(on) => on,
                Err(message) => {
                    yield DeploymentsEvent::Error { message };
                    return;
                }
            };
            yield store.set_promotes(on, &project, &environment, from.as_deref(), by).await.unwrap_or_else(refused);
        }
    }

    /// The deployments matching every filter given, newest first — one
    /// `deployment` event each (with the tickets it carried, its unlinked
    /// commits, and `live`: whether it still counts in its environment) —
    /// then exactly one `page { next }`. `after` starts strictly after
    /// (older than) that deployment; `limit` caps the page. From the cache.
    #[plexus_macros::method(
        group = "read",
        description = "List deployments newest first, by project, environment, state and ticket; each with the tickets it carried",
        params(
            project = "Only this project's (a shortcode) (optional)",
            environment = "Only to this environment (optional)",
            state = "Only in this state: started | succeeded | failed | rolled_back (optional)",
            ticket = "Only those that carried this ticket (optional)",
            after = "Cursor: start strictly after (older than) this deployment id (optional)",
            limit = "At most this many; the page event carries the next cursor (optional)"
        ),
        values(state = ["started", "succeeded", "failed", "rolled_back"]),
        example(note = "the last 20 to prod", params = r#"{"environment": "prod", "limit": 20}"#)
    )]
    pub async fn list(
        &self, auth: Option<&AuthContext>, reach: Reach,
        project: Option<Project>,
        environment: Option<EnvironmentName>,
        state: Option<DeployState>,
        ticket: Option<TicketRef>,
        after: Option<DeploymentId>,
        limit: Option<u32>,
    ) -> impl Stream<Item = DeploymentsEvent> + Send + 'static {
        let store = self.store.clone();
        let admitted = self.access.verify_call(auth, &reach);
        stream! {
            let allowed = match admitted.and_then(|c| allowed(&c, &store)) {
                Ok(a) => a,
                Err(message) => {
                    yield DeploymentsEvent::Error { message };
                    return;
                }
            };
            let q = DeployQuery {
                project: project.map(String::from),
                environment: environment.map(String::from),
                state,
                ticket: ticket.map(String::from),
            };
            match store.deployments(&q, allowed.as_ref(), after.map(DeploymentId::get), limit) {
                Ok(page) => {
                    for row in page.rows {
                        yield row;
                    }
                    yield DeploymentsEvent::Page { next: page.next };
                }
                Err(e) => yield refused(e),
            }
        }
    }

    /// Every environment — of `project` when given — one `environment`
    /// event each, by project and then in display order (test → staging →
    /// prod): what it runs, since when, its last deployment, how many
    /// tickets run there, and (DEPLOY-21) `from`, where it promotes from.
    #[plexus_macros::method(
        group = "read",
        description = "Each environment: what it runs, since when, its last deployment, how many tickets run there",
        params(project = "Only this project's (a shortcode) (optional)"),
        example(note = "menger's environments", params = r#"{"project": "menger"}"#)
    )]
    pub async fn environments(&self, auth: Option<&AuthContext>, reach: Reach, project: Option<Project>) -> impl Stream<Item = DeploymentsEvent> + Send + 'static {
        let store = self.store.clone();
        let admitted = self.access.verify_call(auth, &reach);
        stream! {
            let allowed = match admitted.and_then(|c| allowed(&c, &store)) {
                Ok(a) => a,
                Err(message) => {
                    yield DeploymentsEvent::Error { message };
                    return;
                }
            };
            match store.environments(project.as_deref(), allowed.as_ref()) {
                Ok(envs) => {
                    for environment in envs {
                        yield DeploymentsEvent::Environment { environment };
                    }
                }
                Err(e) => yield refused(e),
            }
        }
    }

    /// Where a ticket's work runs: one `running` event for each environment
    /// it runs in — since when, brought by which deployment, what the
    /// environment runs now — and one `not_running` for each other
    /// environment of its projects (its shortcode's, its hosts', and any it
    /// has run in). DEPLOY-20: a `running` whose `behind` is not empty runs
    /// an earlier version there — `behind` the commits not carried, in link
    /// order, `version` the newest carried one. An `error` for an id that
    /// names no ticket.
    #[plexus_macros::method(
        group = "read",
        target = "id",
        description = "Where a ticket's work runs: running { environment, since, deployment, behind, version } per environment, not_running for the rest",
        params(id = "Ticket id, e.g. MENGER-34"),
        example(note = "where MENGER-34 runs", params = r#"{"id": "MENGER-34"}"#)
    )]
    pub async fn for_ticket(&self, auth: Option<&AuthContext>, reach: Reach, id: TicketRef) -> impl Stream<Item = DeploymentsEvent> + Send + 'static {
        let store = self.store.clone();
        let admitted = self.access.verify_call(auth, &reach);
        stream! {
            let allowed = match admitted.and_then(|c| allowed(&c, &store)) {
                Ok(a) => a,
                Err(message) => {
                    yield DeploymentsEvent::Error { message };
                    return;
                }
            };
            match store.for_ticket(&id, allowed.as_ref()) {
                Ok(rows) => {
                    for row in rows {
                        yield row;
                    }
                }
                Err(StoreError::NotFound(id)) => yield DeploymentsEvent::Error { message: format!("no ticket {id}") },
                Err(e) => yield refused(e),
            }
        }
    }

    /// DEPLOY-13: the kept cells — one `cells { project, environment,
    /// running }` per environment (of `project`, named `environment`, when
    /// given), `running` every ticket there (only `ids`' when given, at most
    /// 1,000), each with since when and the deploy that brought it, in the
    /// store's order — an empty one too, so a copy can replace what it held.
    /// DEPLOY-20: a cell's `behind` (and `version`) when the ticket runs an
    /// earlier version there. DEPLOY-21: `staged`, each ticket deploying,
    /// ready for or failed there.
    /// What the app's board marks are drawn from, one read for every row.
    #[plexus_macros::method(
        group = "read",
        description = "The kept cells: cells { project, environment, running: [{ ticket, since, deployment, behind, version }], staged: [{ ticket, stage }] } per environment",
        params(
            project = "Only this project's (a shortcode) (optional)",
            environment = "Only environments of this name (optional)",
            ids = "Only these tickets' cells, at most 1,000 (optional)"
        ),
        example(note = "what runs on menger's prod", params = r#"{"project": "menger", "environment": "prod"}"#)
    )]
    pub async fn running(
        &self, auth: Option<&AuthContext>, reach: Reach,
        project: Option<Project>,
        environment: Option<EnvironmentName>,
        ids: Option<Vec<TicketRef>>,
    ) -> impl Stream<Item = DeploymentsEvent> + Send + 'static {
        let ids: Option<Vec<String>> = ids.map(|v| v.into_iter().map(String::from).collect());
        let store = self.store.clone();
        let admitted = self.access.verify_call(auth, &reach);
        stream! {
            let allowed = match admitted.and_then(|c| allowed(&c, &store)) {
                Ok(a) => a,
                Err(message) => {
                    yield DeploymentsEvent::Error { message };
                    return;
                }
            };
            match store.running(project.as_deref(), environment.as_deref(), ids.as_deref(), allowed.as_ref()) {
                Ok(rows) => {
                    for row in rows {
                        yield row;
                    }
                }
                Err(e) => yield refused(e),
            }
        }
    }

    /// Open-ended: every deployment recorded from now on — `deployed`, then
    /// `environment_changed` with the environment as it left it — of the
    /// projects the caller holds this on; `lagged` when it fell behind. A
    /// ticket entering or leaving a `deployed` filter is said on
    /// `tickets.watch { filter }`.
    #[plexus_macros::method(
        group = "read",
        streaming,
        description = "Watch deployments: deployed, then environment_changed, as each is recorded"
    )]
    pub async fn watch(&self, auth: Option<&AuthContext>, reach: Reach) -> impl Stream<Item = DeploymentsEvent> + Send + 'static {
        let mut rx = self.store.subscribe_deployments();
        // ACCESS-12: the caller followed — verified again when the grants
        // generation moves, a sign-out or the expiry comes.
        let opened = crate::access::Following::open(&self.access, &self.store, auth, &reach);
        let store = self.store.clone();
        stream! {
            let mut following = match opened {
                Ok(f) => f,
                Err(message) => {
                    yield DeploymentsEvent::Error { message };
                    return;
                }
            };
            loop {
                let next = tokio::select! {
                    biased;
                    () = following.changed() => None,
                    r = rx.recv() => Some(r),
                };
                if let Err(closed) = following.again() {
                    yield DeploymentsEvent::WatchClosed { reason: closed.reason, message: closed.message };
                    return;
                }
                let Some(next) = next else { continue };
                // The projects it sees, as its grants stand (a project
                // recorded since included).
                let allowed = match allowed(following.user(), &store) {
                    Ok(a) => a,
                    Err(message) => {
                        yield DeploymentsEvent::WatchClosed { reason: crate::types::WatchEnd::Revoked, message };
                        return;
                    }
                };
                match next {
                    Ok(ev) if project_of(&ev).is_some_and(|p| allowed.as_ref().is_some_and(|a| !a.contains(p))) => {}
                    Ok(ev) => yield ev,
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(missed)) => yield DeploymentsEvent::Lagged { missed },
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
        }
    }
}
