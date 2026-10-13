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

use crate::deployments::{DeploySource, DeploymentsEvent};
#[cfg(feature = "server")]
use crate::access::{Access, Caller};
#[cfg(feature = "server")]
use crate::deployments::{DeployState, Record};
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
fn allowed(caller: &Caller, store: &Store) -> Result<Option<HashSet<String>>, String> {
    let Some(grant) = caller.grant.clone() else { return Ok(None) };
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
    description = "Deployments: every deploy from any CI to any environment — record, list, environments, for_ticket, watch",
    client,
    server_cfg = "feature = \"server\"",
    crate_path = "plexus_core",
    // AUTH-7: reads are a viewer's; recording is a deployer's alone (a
    // CI's key on the project's shortcode) — and admin's, who holds all.
    roles(
        viewer = ["list", "environments", "for_ticket", "watch"],
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
    /// problem, with nothing kept.
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
            source = "Where to read the run: { system, run, url } (optional; run makes a retry one deployment)"
        ),
        values(state = ["started", "succeeded", "failed", "rolled_back"]),
        example(note = "a promote to prod", params = r#"{"project": "menger", "environment": "prod", "state": "succeeded", "revision": "235ff62", "previous": "29a50be", "commits": ["824e943", "235ff62"], "release": "20261012T220402Z-235ff62", "at": "2026-10-12T22:04:19Z", "source": {"system": "menger-pipeline", "run": "promote"}}"#)
    )]
    pub async fn record(
        &self, auth: Option<&AuthContext>, reach: Reach,
        project: String,
        environment: String,
        state: String,
        revision: String,
        previous: Option<String>,
        commits: Option<Vec<String>>,
        tickets: Option<Vec<String>>,
        release: Option<String>,
        at: Option<String>,
        source: Option<DeploySource>,
    ) -> impl Stream<Item = DeploymentsEvent> + Send + 'static {
        let store = self.store.clone();
        let admitted = self.access.admit(auth, &reach);
        stream! {
            let caller = match admitted {
                Ok(caller) => caller,
                Err(message) => {
                    yield DeploymentsEvent::Error { message };
                    return;
                }
            };
            let by = caller.principal.clone().unwrap_or_else(|| crate::types::ANONYMOUS_ACTOR.to_string());
            let rec = Record {
                project, environment, state, revision, previous,
                commits: commits.unwrap_or_default(),
                tickets: tickets.unwrap_or_default(),
                release, at, source,
            };
            yield store.record_deployment(rec, by).await.unwrap_or_else(refused);
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
        project: Option<String>,
        environment: Option<String>,
        state: Option<String>,
        ticket: Option<String>,
        after: Option<u64>,
        limit: Option<u32>,
    ) -> impl Stream<Item = DeploymentsEvent> + Send + 'static {
        let store = self.store.clone();
        let admitted = self.access.admit(auth, &reach);
        stream! {
            let allowed = match admitted.and_then(|c| allowed(&c, &store)) {
                Ok(a) => a,
                Err(message) => {
                    yield DeploymentsEvent::Error { message };
                    return;
                }
            };
            let state = match state.as_deref().map(str::trim).filter(|s| !s.is_empty()).map(str::parse::<DeployState>).transpose() {
                Ok(s) => s,
                Err(message) => {
                    yield DeploymentsEvent::Error { message };
                    return;
                }
            };
            let blank = |v: Option<String>| v.filter(|v| !v.trim().is_empty());
            let q = DeployQuery { project: blank(project), environment: blank(environment), state, ticket: blank(ticket) };
            match store.deployments(&q, allowed.as_ref(), after, limit) {
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
    /// prod): what it runs, since when, its last deployment, and how many
    /// tickets run there.
    #[plexus_macros::method(
        group = "read",
        description = "Each environment: what it runs, since when, its last deployment, how many tickets run there",
        params(project = "Only this project's (a shortcode) (optional)"),
        example(note = "menger's environments", params = r#"{"project": "menger"}"#)
    )]
    pub async fn environments(&self, auth: Option<&AuthContext>, reach: Reach, project: Option<String>) -> impl Stream<Item = DeploymentsEvent> + Send + 'static {
        let store = self.store.clone();
        let admitted = self.access.admit(auth, &reach);
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
    /// has run in). An `error` for an id that names no ticket.
    #[plexus_macros::method(
        group = "read",
        target = "id",
        description = "Where a ticket's work runs: running { environment, since, deployment } per environment, not_running for the rest",
        params(id = "Ticket id, e.g. MENGER-34"),
        example(note = "where MENGER-34 runs", params = r#"{"id": "MENGER-34"}"#)
    )]
    pub async fn for_ticket(&self, auth: Option<&AuthContext>, reach: Reach, id: String) -> impl Stream<Item = DeploymentsEvent> + Send + 'static {
        let store = self.store.clone();
        let admitted = self.access.admit(auth, &reach);
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
        let access = self.access.clone();
        let who = auth.cloned();
        let store = self.store.clone();
        stream! {
            if let Err(message) = access.admit(who.as_ref(), &reach) {
                yield DeploymentsEvent::Error { message };
                return;
            }
            loop {
                let next = rx.recv().await;
                // The grants as they stand now: held nowhere, the watch ends.
                let Some(now) = reach.current() else {
                    yield DeploymentsEvent::Error { message: "your access changed: you hold this nowhere any more".into() };
                    return;
                };
                let allowed = match access.admit(who.as_ref(), &now).and_then(|c| allowed(&c, &store)) {
                    Ok(a) => a,
                    Err(message) => {
                        yield DeploymentsEvent::Error { message };
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
