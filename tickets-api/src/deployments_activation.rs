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
//!
//! DEPLOY-25: `request` and `cancel_request` are the deployer's too — a deploy
//! asked for from menger, kept as a `requested` deployment — and `webhook`
//! is a child (`deployments.webhook.set|clear|show`), admin's alone: where
//! a request is POSTed, signed. The new methods' logic is beside the
//! activation, in `deployments_activation/streams.rs`.

use crate::deployments::{DeploySource, DeployState, DeploymentsEvent};
// ACCESS-8: every parameter a type (MACRO-12's `typed_params` holds it).
use crate::vocab::{CommitRef, DeploymentId, EnvironmentName, Message, Project, Release, TicketRef, Timestamp};
#[cfg(feature = "server")]
use crate::access::Access;
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
use std::sync::Arc;

// DEPLOY-25: the new methods' logic, beside the activation; the webhook
// child (not gated: its attribute emits `WebhookClient` in the client build).
#[cfg(feature = "server")]
mod streams;
#[cfg(feature = "server")]
use streams::*;
mod webhook;
pub use webhook::WebhookClient;

/// The activation. Server-only: it holds the store.
#[cfg(feature = "server")]
#[derive(Clone)]
pub struct DeploymentsActivation {
    store: Arc<Store>,
    /// The rule every method asks first ([`crate::access`]).
    access: Access,
}

#[cfg(feature = "server")]
impl DeploymentsActivation {
    /// No login: every call admitted, as always.
    pub fn new(store: Arc<Store>) -> Self {
        Self::with_access(store, Access::open())
    }

    /// Behind [`Access`].
    pub fn with_access(store: Arc<Store>, access: Access) -> Self {
        Self { store, access }
    }
}


// Not `#[cfg]`-gated: the client-only build needs it too (see activation.rs).
#[plexus_macros::activation(
    description = "Deployments: record, request, cancel_request, promotes, list, environments, for_ticket, running, watch; webhook",
    typed_params,
    // AUTH-7: reads are a viewer's; recording is a deployer's alone (a
    // CI's key on the project's shortcode) — and admin's, who holds all.
    // DEPLOY-21: `promotes` is listed nowhere: admin's alone.
    roles(
        viewer = ["list", "environments", "for_ticket", "running", "watch"],
        editor = [viewer],
        deployer = [viewer, "record", "request", "cancel_request"],
    )
)]
impl DeploymentsActivation {
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
    /// record — `promotes` follows it on `watch`. DEPLOY-25: `request` names
    /// the request it answers (`requested` is not a state a CI records); the
    /// request then says this record's state, and `request` follows on
    /// `watch`.
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
            from = "The environment this one promotes from, e.g. test for prod (optional; set with the record)",
            request = "The request this record answers: a requested deployment's id, to this environment (optional)"
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
        request: Option<DeploymentId>,
    ) -> impl Stream<Item = DeploymentsEvent> + Send + 'static {
        let texts = |v: Option<Vec<_>>| v.unwrap_or_default().into_iter().map(String::from).collect::<Vec<String>>();
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
            request: request.map(DeploymentId::get),
        };
        record_stream(self.store.clone(), self.access.verify_call(auth, &reach), rec)
    }

    /// DEPLOY-25: queues a deploy from menger — kept as a deployment in the
    /// state `requested`: who asked, when, `project`'s `environment`, what
    /// to deploy (`revision`; else the latest recorded deploy of `release`;
    /// else what the environment it promotes from runs, DEPLOY-21's `from`)
    /// and `note`. It runs nothing and completes nothing. When the
    /// environment has a webhook (`webhook set`), menger POSTs the request
    /// to it, signed, retrying with backoff; without one it is kept and
    /// says `webhook: none` ("no CI configured"). A CI's `record` names it
    /// as `request` when it picks it up. Yields exactly one `request
    /// { deployment, carried }` — the request as kept and the tickets it
    /// would carry — or an in-band `error`, nothing kept. The deployer's
    /// (or admin's) on the project.
    #[plexus_macros::method(
        group = "write",
        target = "project",
        description = "Queue a deploy: kept as a requested deployment, POSTed to the environment's webhook (signed); yields request { deployment, carried }",
        params(
            project = "Whose environment: a shortcode, any case (menger is MENGER)",
            environment = "The environment to deploy to, e.g. prod",
            revision = "The commit to deploy (optional; default: what the environment it promotes from runs)",
            release = "The artifact to deploy (optional; alone, the revision of its latest recorded deploy)",
            note = "Why, for the CI and the reader (optional; at most 2 KiB)"
        ),
        example(note = "promote what test runs to prod", params = r#"{"project": "menger", "environment": "prod", "note": "the week's work"}"#)
    )]
    pub async fn request(
        &self, auth: Option<&AuthContext>, reach: Reach,
        project: Project,
        environment: EnvironmentName,
        revision: Option<CommitRef>,
        release: Option<Release>,
        note: Option<Message>,
    ) -> impl Stream<Item = DeploymentsEvent> + Send + 'static {
        let ask = crate::store::RequestAsk {
            project: project.into_string(),
            environment: environment.into_string(),
            revision: revision.map(String::from),
            release: release.map(String::from),
            note: note.and_then(Message::given),
        };
        request_stream(self.store.clone(), self.access.verify_call(auth, &reach), ask)
    }

    /// DEPLOY-25: withdraws a pending request — `canceled`, by the caller;
    /// its webhook sends no more. One a CI has picked up (a record names
    /// it), or one canceled already, is refused in band, naming why. Yields
    /// exactly one `request { deployment, carried }` or an in-band `error`.
    /// The deployer's (or admin's) on the request's project.
    #[plexus_macros::method(
        group = "write",
        description = "Cancel a pending request (one no CI has picked up); yields request { deployment, carried }",
        params(id = "The request's id (a requested deployment's), e.g. 41"),
        example(note = "withdraw request 41", params = r#"{"id": 41}"#)
    )]
    pub async fn cancel_request(&self, auth: Option<&AuthContext>, reach: Reach, id: DeploymentId) -> impl Stream<Item = DeploymentsEvent> + Send + 'static {
        cancel_stream(self.store.clone(), self.access.verify_call(auth, &reach), id.get())
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
        let (project, environment) = (project.into_string(), environment.into_string());
        promotes_stream(self.store.clone(), self.access.verify_call(auth, &reach), project, environment, from.map(String::from))
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
            state = "Only in this state: started | succeeded | failed | rolled_back | requested (optional)",
            ticket = "Only those that carried this ticket (optional)",
            after = "Cursor: start strictly after (older than) this deployment id (optional)",
            limit = "At most this many; the page event carries the next cursor (optional)"
        ),
        values(state = ["started", "succeeded", "failed", "rolled_back", "requested"]),
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

    /// DEPLOY-25: each environment's webhook — where a request is POSTed,
    /// signed: `deployments.webhook.set|clear|show`. Admin's alone.
    #[plexus_macros::child]
    fn webhook(&self) -> webhook::WebhookActivation {
        webhook::WebhookActivation::new(Arc::clone(&self.store), self.access.clone())
    }
}
