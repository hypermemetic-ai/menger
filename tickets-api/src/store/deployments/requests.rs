//! DEPLOY-25: **a request** — a deploy asked for from menger, kept as a
//! deployment in the state `requested`, and handed to the environment's CI
//! by a signed webhook.
//!
//! A request is a row of `deployments` (its id from the same sequence, so a
//! CI's record names it as `request`), its own rest in `deploy_requests`
//! (note, source, cancellation, the webhook delivery's id and state), each
//! delivery attempt in `deploy_deliveries`, and each environment's webhook
//! in `deploy_webhooks` (its secret too: it keys the HMAC; never answered).
//!
//! **It runs nothing.** A request is folded into no environment
//! ([`super::fold`] never sees one): it moves no cell, no stage, no status.
//! It is **pending** until a CI's record names it — then it says that
//! record's state ([`crate::deployments::RequestState::of`]) and its
//! outcome is that record — or it is canceled, which only a pending one may
//! be. While pending it *would carry* tickets ([`Cache::request_carries`]):
//! those running their current version where its revision runs, and those
//! linking the revision itself, less those running their current version on
//! its environment — what `requested <env>` holds for and `for_ticket`'s
//! `requested` says.
//!
//! **The webhook is delivered off the write path.** The store keeps the
//! delivery's state and every attempt; the server's worker (menger's
//! `webhooks`) asks [`Store::deliveries_due`] and [`Store::delivery`],
//! POSTs with no lock held, and records each attempt with
//! [`Store::delivery_attempted`], which decides nothing but the state the
//! attempt leaves: delivered on a 2xx, retrying when the worker names a
//! next attempt, failed when it does not.

use super::*;
use crate::deployments::{DeliveryAttempt, DeployRequest, RequestState, WebhookConfig, WebhookPayload, WebhookState};
use crate::store::system::System;

/// The tables — made on every open where they are not ([`super::migrate`]).
pub(super) const TABLES: &str = "
        CREATE TABLE IF NOT EXISTS deploy_requests (
            id          INTEGER PRIMARY KEY,
            note        TEXT,
            from_env    TEXT,
            canceled_at INTEGER,
            canceled_by TEXT,
            delivery    TEXT,
            webhook     TEXT NOT NULL,
            next_at     INTEGER
        );
        CREATE TABLE IF NOT EXISTS deploy_deliveries (
            request INTEGER NOT NULL,
            attempt INTEGER NOT NULL,
            at      INTEGER NOT NULL,
            status  INTEGER,
            error   TEXT,
            PRIMARY KEY (request, attempt)
        );
        CREATE TABLE IF NOT EXISTS deploy_webhooks (
            project TEXT NOT NULL,
            name    TEXT NOT NULL,
            url     TEXT NOT NULL,
            secret  TEXT NOT NULL,
            set_at  INTEGER NOT NULL,
            set_by  TEXT NOT NULL,
            PRIMARY KEY (project, name)
        );
    ";

/// The longest note a request keeps, in bytes.
pub const NOTE_MAX: usize = 2048;

/// The longest error an attempt keeps, in bytes.
pub const ATTEMPT_ERROR_MAX: usize = 200;

/// An environment's webhook as the cache keeps it. Its `Debug` says no
/// secret.
#[derive(Clone)]
pub(in crate::store) struct Hook {
    url: String,
    secret: String,
    set_at: i64,
    set_by: String,
}

impl std::fmt::Debug for Hook {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Hook").field("url", &self.url).field("set_at", &self.set_at).field("set_by", &self.set_by).finish_non_exhaustive()
    }
}

fn config(project: &str, environment: &str, hook: Option<&Hook>) -> WebhookConfig {
    WebhookConfig {
        project: project.to_string(),
        environment: environment.to_string(),
        url: hook.map(|h| h.url.clone()),
        secret_set: hook.is_some(),
        set_at: hook.map(|h| h.set_at),
        set_by: hook.map(|h| h.set_by.clone()),
    }
}

/// One delivery the worker is to make now: where, keyed by what, the body,
/// and which attempt it is. Its `Debug` says no secret.
pub struct Due {
    pub request: u64,
    /// `X-Menger-Delivery` — the same on every attempt.
    pub delivery: String,
    /// This attempt's number, from 1.
    pub attempt: u32,
    pub url: String,
    secret: String,
    pub payload: WebhookPayload,
    /// When the last attempt was made, unix seconds — `None` before the first.
    pub last_at: Option<i64>,
}

impl Due {
    /// The secret the body's HMAC is keyed by — for the signature alone.
    pub fn secret(&self) -> &str {
        &self.secret
    }
}

impl std::fmt::Debug for Due {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Due").field("request", &self.request).field("delivery", &self.delivery).field("attempt", &self.attempt).field("url", &self.url).finish_non_exhaustive()
    }
}

/// What a request asks: where, and what — a revision, a release, or
/// neither (what the environment it promotes from runs) — and why.
#[derive(Debug, Clone, Default)]
pub struct RequestAsk {
    pub project: String,
    pub environment: String,
    pub revision: Option<String>,
    pub release: Option<String>,
    pub note: Option<String>,
}

// ---------------------------------------------------------------------
// The tables

/// Writes a request's own row, inside the caller's transaction.
fn store_request(conn: &Connection, d: &Deployment) -> rusqlite::Result<()> {
    let Some(q) = &d.requested else { return Ok(()) };
    conn.prepare_cached(
        "INSERT INTO deploy_requests (id, note, from_env, canceled_at, canceled_by, delivery, webhook, next_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT (id) DO UPDATE SET canceled_at = excluded.canceled_at, canceled_by = excluded.canceled_by,
                 webhook = excluded.webhook, next_at = excluded.next_at",
    )?
    .execute(params![d.id as i64, q.note, q.from, q.canceled_at, q.canceled_by, q.delivery, q.webhook.as_str(), q.next_attempt_at])?;
    Ok(())
}

/// A request picked up by a CI's record sends no more, inside the caller's
/// transaction.
pub(super) fn stop_delivery(conn: &Connection, request: u64) -> rusqlite::Result<()> {
    conn.prepare_cached("UPDATE deploy_requests SET webhook = 'stopped', next_at = NULL WHERE id = ?1 AND webhook IN ('pending', 'retrying')")?
        .execute(params![request as i64])?;
    Ok(())
}

/// Each request's rest and attempts onto `records`, and every webhook.
pub(super) fn load(conn: &Connection, records: &mut [Deployment], webhooks: &mut BTreeMap<(String, String), Hook>) -> anyhow::Result<()> {
    let mut attempts: HashMap<u64, Vec<DeliveryAttempt>> = HashMap::new();
    {
        let mut stmt = conn.prepare("SELECT request, attempt, at, status, error FROM deploy_deliveries ORDER BY request, attempt")?;
        let mut rows = stmt.query([])?;
        while let Some(r) = rows.next()? {
            let a = DeliveryAttempt { attempt: r.get::<_, i64>(1)? as u32, at: r.get(2)?, status: r.get::<_, Option<i64>>(3)?.map(|s| s as u16), error: r.get(4)? };
            attempts.entry(r.get::<_, i64>(0)? as u64).or_default().push(a);
        }
    }
    let mut rest: HashMap<u64, DeployRequest> = HashMap::new();
    {
        let mut stmt = conn.prepare("SELECT id, note, from_env, canceled_at, canceled_by, delivery, webhook, next_at FROM deploy_requests")?;
        let mut rows = stmt.query([])?;
        while let Some(r) = rows.next()? {
            let id = r.get::<_, i64>(0)? as u64;
            let webhook: String = r.get(6)?;
            rest.insert(
                id,
                DeployRequest {
                    note: r.get(1)?,
                    from: r.get(2)?,
                    state: RequestState::Pending,
                    outcome: None,
                    canceled_at: r.get(3)?,
                    canceled_by: r.get(4)?,
                    delivery: r.get(5)?,
                    webhook: webhook.parse().map_err(|e: String| anyhow::anyhow!("request {id}: {e}"))?,
                    attempts: attempts.remove(&id).unwrap_or_default(),
                    next_attempt_at: r.get(7)?,
                },
            );
        }
    }
    for d in records.iter_mut().filter(|d| d.state == DeployState::Requested) {
        d.requested = Some(rest.remove(&d.id).unwrap_or_else(|| blank(None, None, None)));
    }
    let mut stmt = conn.prepare("SELECT project, name, url, secret, set_at, set_by FROM deploy_webhooks")?;
    let mut rows = stmt.query([])?;
    while let Some(r) = rows.next()? {
        webhooks.insert((r.get(0)?, r.get(1)?), Hook { url: r.get(2)?, secret: r.get(3)?, set_at: r.get(4)?, set_by: r.get(5)? });
    }
    Ok(())
}

/// A new request's rest: pending, with a delivery when a webhook is set.
fn blank(note: Option<String>, from: Option<String>, delivery: Option<String>) -> DeployRequest {
    let webhook = if delivery.is_some() { WebhookState::Pending } else { WebhookState::None };
    DeployRequest { note, from, state: RequestState::Pending, outcome: None, canceled_at: None, canceled_by: None, delivery, webhook, attempts: Vec::new(), next_attempt_at: None }
}

/// An error kept short — at most [`ATTEMPT_ERROR_MAX`] bytes, on a char boundary.
fn short(e: &str) -> String {
    let mut n = e.len().min(ATTEMPT_ERROR_MAX);
    while !e.is_char_boundary(n) {
        n -= 1;
    }
    e[..n].to_string()
}

// ---------------------------------------------------------------------
// The cache's half

impl Deploys {
    /// Every request's state again from the records naming it — at open,
    /// after [`Deploys::load_records`] kept them.
    pub(super) fn settle_requests(&mut self) {
        self.pending.clear();
        let mut answers: Vec<(i64, u64, u64)> = Vec::new();
        for d in self.by_id.values_mut() {
            if let Some(q) = d.requested.as_mut() {
                q.state = if q.canceled_at.is_some() { RequestState::Canceled } else { RequestState::Pending };
                q.outcome = None;
            }
            if let Some(r) = d.request {
                answers.push((d.at, d.id, r));
            }
        }
        answers.sort_unstable();
        for (_, id, r) in answers {
            let d = self.by_id[&id].clone();
            self.answered(r, &d);
        }
        self.pending = self.by_id.values().filter(|d| d.requested.as_ref().is_some_and(|q| q.state == RequestState::Pending)).map(|d| d.id).collect();
    }

    /// The record `d` names the request `r`: when it is the latest that
    /// does, the request says its state and it is the outcome — and the
    /// webhook sends no more.
    pub(super) fn answered(&mut self, r: u64, d: &Deployment) {
        let newest = self.by_id.get(&r).and_then(|q| q.requested.as_ref()).and_then(|q| q.outcome).and_then(|o| self.by_id.get(&o)).map(|o| (o.at, o.id));
        let Some(q) = self.by_id.get_mut(&r).and_then(|q| q.requested.as_mut()) else { return };
        if newest.is_none_or(|n| n < (d.at, d.id)) {
            q.outcome = Some(d.id);
            q.state = RequestState::of(d.state);
        }
        if q.webhook.due() {
            q.webhook = WebhookState::Stopped;
            q.next_attempt_at = None;
        }
        self.pending.remove(&r);
    }

    /// Whether a record `d` may name the request `r`: a request to its
    /// environment.
    pub(super) fn answers(&self, r: u64, d: &Deployment) -> Result<(), StoreError> {
        match self.by_id.get(&r) {
            Some(q) if q.state == DeployState::Requested && q.project == d.project && q.environment == d.environment => Ok(()),
            Some(q) if q.state == DeployState::Requested => Err(StoreError::Invalid(format!(
                "request {r} is to {}/{}, not {}/{} (nothing was recorded)",
                q.project, q.environment, d.project, d.environment
            ))),
            _ => Err(StoreError::Invalid(format!("no request {r}: `request` names a requested deployment (nothing was recorded)"))),
        }
    }

    /// What a request asking for `revision` / `release` / neither of
    /// `(project, environment)` deploys: the revision, the release, and the
    /// environment it was taken from — or why it cannot say.
    fn what(&self, project: &str, environment: &str, revision: Option<String>, release: Option<String>) -> Result<(String, Option<String>, Option<String>), String> {
        let recorded = |f: &dyn Fn(&Deployment) -> bool| {
            self.by_time.iter().rev().map(|(_, id)| &self.by_id[id]).find(|d| d.project == project && d.state != DeployState::Requested && f(d))
        };
        match (revision, release) {
            (Some(rev), release) => {
                let release = release.or_else(|| recorded(&|d: &Deployment| same(&d.revision, &rev) && d.release.is_some()).and_then(|d| d.release.clone()));
                Ok((rev, release, None))
            }
            (None, Some(rel)) => match recorded(&|d: &Deployment| d.release.as_deref() == Some(rel.as_str())) {
                Some(d) => Ok((d.revision.clone(), Some(rel), None)),
                None => Err(format!("{project} has recorded no deployment of release {rel}: name its revision too")),
            },
            (None, None) => {
                let from = self.sources.get(&(project.to_string(), environment.to_string())).ok_or_else(|| {
                    format!("{project}/{environment} promotes from no environment: name a revision or a release (or set where it promotes from)")
                })?;
                let src = self.envs.get(&(project.to_string(), from.clone()));
                match src.and_then(|k| k.revision.clone()) {
                    Some(rev) => {
                        // Its release, or the latest recorded deploy's of that revision.
                        let release = src.and_then(|k| k.release.clone()).or_else(|| recorded(&|d: &Deployment| same(&d.revision, &rev) && d.release.is_some()).and_then(|d| d.release.clone()));
                        Ok((rev, release, Some(from.clone())))
                    }
                    None => Err(format!("{project}/{from} runs nothing yet, so there is nothing to promote to {environment}: name a revision")),
                }
            }
        }
    }

    /// A request kept: listed, pending.
    fn install_request(&mut self, d: Deployment) {
        self.keys.insert(dedupe(&d), d.id);
        self.by_time.insert((d.at, d.id));
        if d.requested.as_ref().is_some_and(|q| q.state == RequestState::Pending) {
            self.pending.insert(d.id);
        }
        self.by_id.insert(d.id, d);
    }

    /// The request `id`'s rest, to change.
    fn request_mut(&mut self, id: u64) -> Option<&mut DeployRequest> {
        self.by_id.get_mut(&id).and_then(|d| d.requested.as_mut())
    }

    /// The projects with a request, of any state.
    pub(super) fn request_projects(&self) -> BTreeSet<String> {
        self.by_id.values().filter(|d| d.state == DeployState::Requested).map(|d| d.project.clone()).collect()
    }
}

impl Cache {
    /// The tickets the request `d` would carry, in the store's order — while
    /// it is pending; nothing once it is not. Those running their current
    /// version on an environment of its project that runs its revision, and
    /// those linking the revision itself, less those running their current
    /// version on its environment.
    pub(super) fn request_carries(&self, d: &Deployment) -> Vec<String> {
        if d.requested.as_ref().is_none_or(|q| q.state != RequestState::Pending) {
            return Vec::new();
        }
        let deploys = &self.deploys;
        let here = deploys.envs.get(&(d.project.clone(), d.environment.clone()));
        let mut ids: Vec<&str> = deploys.links.tickets(&d.revision).collect();
        for ((p, _), k) in &deploys.envs {
            if *p == d.project && k.revision.as_deref().is_some_and(|r| same(r, &d.revision)) {
                ids.extend(k.currents());
            }
        }
        ids.retain(|t| here.is_none_or(|k| !k.current(t)));
        self.in_order(ids)
    }

    /// The request `id` as `watch` says it.
    pub(super) fn request_event(&self, id: u64) -> Option<DeploymentsEvent> {
        let d = self.deploys.by_id.get(&id).filter(|d| d.state == DeployState::Requested)?;
        Some(DeploymentsEvent::Request { deployment: d.clone(), carried: self.request_carries(d) })
    }

    /// The tickets `requested <env>` holds for, per shortcode — each a
    /// pending request to an environment `env` names would carry.
    pub(super) fn holds_requested(&self, env: &str) -> BTreeMap<ShortcodeId, RoaringBitmap> {
        let mut out: BTreeMap<ShortcodeId, RoaringBitmap> = BTreeMap::new();
        for d in self.deploys.pending.iter().filter_map(|id| self.deploys.by_id.get(id)) {
            if !Deploys::names(env, &d.project, &d.environment) {
                continue;
            }
            for key in self.request_carries(d).iter().filter_map(|t| self.at.get(t.as_str())) {
                out.entry(key.shortcode.clone()).or_default().insert(key.seq);
            }
        }
        out
    }

    /// The tickets a record to `project` may move into or out of a
    /// `requested` cell — when a request there is pending: what each would
    /// carry now, and every ticket running its current version in the
    /// project, before the record and (`kept`) after it.
    pub(super) fn requested_moves(&self, project: &str, kept: &Kept) -> Vec<String> {
        let pending: Vec<&Deployment> = self.deploys.pending.iter().filter_map(|id| self.deploys.by_id.get(id)).filter(|d| d.project == project).collect();
        if pending.is_empty() {
            return Vec::new();
        }
        let mut out: BTreeSet<String> = pending.iter().flat_map(|d| self.request_carries(d)).collect();
        out.extend(kept.currents().map(str::to_string));
        for ((p, _), k) in &self.deploys.envs {
            if p == project {
                out.extend(k.currents().map(str::to_string));
            }
        }
        out.into_iter().collect()
    }

    /// `for_ticket`'s `requested` rows: each pending request that would
    /// carry `ticket`, of the projects in `allowed` when given, oldest first.
    pub(super) fn requested_for(&self, ticket: &str, allowed: Option<&HashSet<String>>) -> Vec<DeploymentsEvent> {
        let mut out = Vec::new();
        for d in self.deploys.pending.iter().filter_map(|id| self.deploys.by_id.get(id)) {
            if allowed.is_some_and(|a| !a.contains(&d.project)) || !self.request_carries(d).iter().any(|t| t == ticket) {
                continue;
            }
            out.push(DeploymentsEvent::Requested {
                ticket: ticket.to_string(),
                project: d.project.clone(),
                environment: d.environment.clone(),
                request: d.id,
                requested_by: d.recorded_by.clone(),
                at: d.at,
                revision: d.revision.clone(),
                webhook: d.requested.as_ref().map_or(WebhookState::None, |q| q.webhook),
            });
        }
        out
    }

    /// The `(shortcode, seq)` of each ticket in `ids` the store holds.
    fn keys_of<'a>(&'a self, ids: &'a [String]) -> Vec<(&'a ShortcodeId, u32)> {
        ids.iter().filter_map(|t| self.at.get(t.as_str())).map(|k| (&k.shortcode, k.seq)).collect()
    }
}

// ---------------------------------------------------------------------
// The writes and reads

impl Store {
    /// DEPLOY-25: asks for a deploy of `ask.environment` — `ask.revision`,
    /// else the latest recorded deploy of `ask.release` in the project, else
    /// what the environment it promotes from runs (DEPLOY-21's `from`) —
    /// kept as a deployment in the state `requested`, pending, with its
    /// note. When the environment has a webhook, a delivery is made for it
    /// (its id, `pending`) and the server's worker sends it; without one it
    /// is kept and says `webhook: none`. Committed in its own transaction,
    /// then `request` on `deployments.watch` and, on `tickets.watch
    /// { filter }`, each ticket a `requested` filter now admits. It runs
    /// nothing and moves no status. `by` is who asked.
    ///
    /// ACCESS-11: `allowed` — the system, or a deployer (or admin) on
    /// exactly the project's shortcode, checked under the write-serial lock.
    pub async fn request_deployment(&self, allowed: impl Into<Authority>, ask: RequestAsk, by: String) -> Result<DeploymentsEvent, StoreError> {
        let allowed = allowed.into();
        let bad = |m: String| StoreError::Invalid(format!("{m} (nothing was requested)"));
        let project = project_of(&ask.project).map_err(bad)?;
        let environment = environment_of(&ask.environment).map_err(bad)?;
        let revision = match ask.revision.as_deref().map(str::trim).filter(|r| !r.is_empty()) {
            Some(r) => Some(super::super::commits::reference(r).ok_or_else(|| bad(format!("revision `{r}` is not a commit reference: 7–64 hex characters, `repo@` before them outside the project's repository")))?),
            None => None,
        };
        let release = ask.release.map(|r| r.trim().to_string()).filter(|r| !r.is_empty());
        let note = ask.note.map(|n| n.trim().to_string()).filter(|n| !n.is_empty());
        if let Some(n) = note.as_ref().filter(|n| n.len() > NOTE_MAX) {
            return Err(bad(format!("a note is at most {NOTE_MAX} bytes; this one is {}", n.len())));
        }
        let now = chrono::Utc::now().timestamp();
        let serial = self.serial().await;
        let (d, under) = {
            let cache = self.tickets.read().expect("store lock poisoned");
            if !cache.holds_shortcode(&project) {
                return Err(bad(format!("no shortcode {project}: a project is a shortcode menger holds")));
            }
            allowed.deployer_on(&super::super::writs::scope_of(&cache, &ShortcodeId::new(&project)), "a request needs deployer on its project")?;
            let (revision, release, from) = cache.deploys.what(&project, &environment, revision, release).map_err(bad)?;
            let at = (project.clone(), environment.clone());
            let delivery = cache.deploys.webhooks.contains_key(&at).then(|| uuid::Uuid::new_v4().to_string());
            let d = Deployment {
                id: cache.deploys.next_id(),
                previous: cache.deploys.envs.get(&at).and_then(|k| k.revision.clone()),
                project,
                environment,
                state: DeployState::Requested,
                revision,
                commits: Vec::new(),
                tickets: Vec::new(),
                release,
                at: now,
                source: DeploySource { system: Some("menger".into()), run: None, url: None },
                recorded_at: now,
                recorded_by: by,
                request: None,
                requested: Some(blank(note, from, delivery)),
            };
            let carried = cache.request_carries(&d);
            let under = lineage::deployed_before_with(&self.unders, &cache, &cache.keys_of(&carried));
            (d, under)
        };
        let (stored, id) = (d.clone(), d.id);
        let tickets = Arc::clone(&self.tickets);
        self.write(
            serial,
            move |tx| {
                store(tx, &stored)?;
                store_request(tx, &stored)?;
                Ok(())
            },
            move |cache, _| {
                cache.deploys.install_request(d);
                cache.request_event(id)
            },
            move |(), event, tx| {
                let event = event.expect("a request just kept");
                tx.deployment(event.clone());
                if let Some(change) = under.and_then(|u| u.after(&tickets)) {
                    tx.send(change);
                }
                event
            },
        )
        .await
    }

    /// DEPLOY-25: withdraws the pending request `id` — `canceled`, by `by`,
    /// now; its webhook sends no more. A request a CI has picked up, or one
    /// already canceled, is refused, naming why; so is an id that is no
    /// request. Then `request` on `deployments.watch`, and each ticket a
    /// `requested` filter no longer admits on `tickets.watch { filter }`.
    ///
    /// ACCESS-11: `allowed` — the system, or a deployer (or admin) on
    /// exactly the request's project, checked under the write-serial lock.
    pub async fn cancel_request(&self, allowed: impl Into<Authority>, id: u64, by: String) -> Result<DeploymentsEvent, StoreError> {
        let allowed = allowed.into();
        let now = chrono::Utc::now().timestamp();
        let serial = self.serial().await;
        let (row, under) = {
            let cache = self.tickets.read().expect("store lock poisoned");
            let d = cache.deploys.by_id.get(&id).filter(|d| d.state == DeployState::Requested).ok_or_else(|| StoreError::Invalid(format!("no request {id} (nothing was canceled)")))?;
            allowed.deployer_on(&super::super::writs::scope_of(&cache, &ShortcodeId::new(&d.project)), "canceling a request needs deployer on its project")?;
            let q = d.requested.clone().unwrap_or_else(|| blank(None, None, None));
            match (q.state, q.outcome) {
                (RequestState::Pending, _) => {}
                (RequestState::Canceled, _) => return Err(StoreError::Invalid(format!("request {id} is canceled already (nothing was canceled)"))),
                (s, o) => {
                    return Err(StoreError::Invalid(format!(
                        "request {id} was picked up — deployment {} says {} — so it cannot be canceled (nothing was canceled)",
                        o.map_or_else(|| "?".to_string(), |o| o.to_string()),
                        s.as_str()
                    )))
                }
            }
            let carried = cache.request_carries(d);
            let under = lineage::deployed_before_with(&self.unders, &cache, &cache.keys_of(&carried));
            let mut row = d.clone();
            let q = row.requested.as_mut().expect("a request's rest");
            (q.state, q.canceled_at, q.canceled_by) = (RequestState::Canceled, Some(now), Some(by));
            if q.webhook.due() {
                (q.webhook, q.next_attempt_at) = (WebhookState::Stopped, None);
            }
            (row, under)
        };
        let stored = row.clone();
        let tickets = Arc::clone(&self.tickets);
        self.write(
            serial,
            move |tx| {
                store_request(tx, &stored)?;
                Ok(())
            },
            move |cache, _| {
                cache.deploys.pending.remove(&id);
                if let (Some(q), Some(new)) = (cache.deploys.request_mut(id), row.requested) {
                    *q = new;
                }
                cache.request_event(id)
            },
            move |(), event, tx| {
                let event = event.expect("a request just canceled");
                tx.deployment(event.clone());
                if let Some(change) = under.and_then(|u| u.after(&tickets)) {
                    tx.send(change);
                }
                event
            },
        )
        .await
    }

    /// DEPLOY-25: `(project, environment)`'s webhook is `url`, keyed by
    /// `secret`, from now — set, or replaced. A request made after it is
    /// delivered there; one already made keeps what it had. The answer says
    /// the URL and that a secret is set, never the secret.
    ///
    /// ACCESS-11: `allowed` — the system, or admin on exactly the project's
    /// shortcode, checked under the write-serial lock.
    pub async fn set_webhook(&self, allowed: impl Into<Authority>, project: &str, environment: &str, url: &str, secret: String, by: String) -> Result<DeploymentsEvent, StoreError> {
        let allowed = allowed.into();
        let bad = |m: String| StoreError::Invalid(format!("{m} (nothing was written)"));
        let project = project_of(project).map_err(bad)?;
        let environment = environment_of(environment).map_err(bad)?;
        let url = crate::deployments::webhook_url_of(url).map_err(bad)?;
        if !(16..=256).contains(&secret.len()) {
            return Err(bad("a webhook secret is 16 to 256 bytes".into()));
        }
        let serial = self.serial().await;
        self.webhook_writable(&allowed, &project)?;
        let hook = Hook { url, secret, set_at: chrono::Utc::now().timestamp(), set_by: by };
        let (key, stored) = ((project, environment), hook.clone());
        let answer = DeploymentsEvent::Webhook { webhook: config(&key.0, &key.1, Some(&hook)) };
        self.write(
            serial,
            move |tx| {
                tx.prepare_cached(
                    "INSERT INTO deploy_webhooks (project, name, url, secret, set_at, set_by) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                         ON CONFLICT (project, name) DO UPDATE SET url = excluded.url, secret = excluded.secret, set_at = excluded.set_at, set_by = excluded.set_by",
                )?
                .execute(params![key.0, key.1, stored.url, stored.secret, stored.set_at, stored.set_by])?;
                Ok(key)
            },
            move |cache, key| {
                cache.deploys.webhooks.insert(key.clone(), hook);
            },
            move |_, (), _| answer,
        )
        .await
    }

    /// DEPLOY-25: `(project, environment)` has no webhook from now; a
    /// delivery still due fails at its next attempt, saying so. Clearing
    /// none answers the same. Admin on the project's shortcode.
    pub async fn clear_webhook(&self, allowed: impl Into<Authority>, project: &str, environment: &str) -> Result<DeploymentsEvent, StoreError> {
        let allowed = allowed.into();
        let bad = |m: String| StoreError::Invalid(format!("{m} (nothing was written)"));
        let key = (project_of(project).map_err(bad)?, environment_of(environment).map_err(bad)?);
        let serial = self.serial().await;
        self.webhook_writable(&allowed, &key.0)?;
        let answer = DeploymentsEvent::Webhook { webhook: config(&key.0, &key.1, None) };
        self.write(
            serial,
            move |tx| {
                tx.prepare_cached("DELETE FROM deploy_webhooks WHERE project = ?1 AND name = ?2")?.execute(params![key.0, key.1])?;
                Ok(key)
            },
            move |cache, key| {
                cache.deploys.webhooks.remove(key);
            },
            move |_, (), _| answer,
        )
        .await
    }

    /// The project is one menger holds, and `allowed` admin on it.
    fn webhook_writable(&self, allowed: &Authority, project: &str) -> Result<(), StoreError> {
        let cache = self.tickets.read().expect("store lock poisoned");
        if !cache.holds_shortcode(project) {
            return Err(StoreError::Invalid(format!("no shortcode {project}: a project is a shortcode menger holds (nothing was written)")));
        }
        allowed.admin_on(&super::super::writs::scope_of(&cache, &ShortcodeId::new(project)), "an environment's webhook needs admin on its project")
    }

    /// DEPLOY-25: `(project, environment)`'s webhook — its URL, whether a
    /// secret is set, who set it when; never the secret. Admin on the
    /// project's shortcode. From the cache.
    pub fn webhook(&self, allowed: impl Into<Authority>, project: &str, environment: &str) -> Result<DeploymentsEvent, StoreError> {
        let key = (project_of(project).map_err(StoreError::Invalid)?, environment_of(environment).map_err(StoreError::Invalid)?);
        self.webhook_writable(&allowed.into(), &key.0)?;
        let cache = self.tickets.read().expect("store lock poisoned");
        Ok(DeploymentsEvent::Webhook { webhook: config(&key.0, &key.1, cache.deploys.webhooks.get(&key)) })
    }

    /// DEPLOY-25: the project of the request `id` — `None` when it is no
    /// request. From the cache.
    pub fn request_project(&self, id: u64) -> Option<String> {
        let cache = self.tickets.read().expect("store lock poisoned");
        cache.deploys.by_id.get(&id).filter(|d| d.state == DeployState::Requested).map(|d| d.project.clone())
    }

    /// DEPLOY-25: the requests whose webhook delivery is due — pending or
    /// retrying, oldest first. The worker's, at start and after it lags.
    pub fn deliveries_due(&self) -> Vec<u64> {
        let cache = self.tickets.read().expect("store lock poisoned");
        cache.deploys.by_id.values().filter(|d| d.requested.as_ref().is_some_and(|q| q.webhook.due())).map(|d| d.id).collect()
    }

    /// DEPLOY-25: the delivery of `request` to make now — `Ok(None)` when
    /// none is due (delivered, failed, stopped, or no request), `Err` when
    /// one is due and cannot be made — the attempt it would have been, and
    /// why: its environment's webhook was cleared. The URL and secret are
    /// the webhook's as it stands.
    pub fn delivery(&self, request: u64) -> Result<Option<Due>, (u32, String)> {
        let cache = self.tickets.read().expect("store lock poisoned");
        let Some(d) = cache.deploys.by_id.get(&request) else { return Ok(None) };
        let Some(q) = d.requested.as_ref().filter(|q| q.webhook.due()) else { return Ok(None) };
        let Some(hook) = cache.deploys.webhooks.get(&(d.project.clone(), d.environment.clone())) else {
            return Err((q.attempts.len() as u32 + 1, format!("{}/{} has no webhook any more", d.project, d.environment)));
        };
        let at = chrono::DateTime::from_timestamp(d.at, 0).map(|t| t.to_rfc3339_opts(chrono::SecondsFormat::Secs, true)).unwrap_or_default();
        Ok(Some(Due {
            request,
            delivery: q.delivery.clone().unwrap_or_default(),
            attempt: q.attempts.len() as u32 + 1,
            url: hook.url.clone(),
            secret: hook.secret.clone(),
            payload: WebhookPayload {
                request,
                project: d.project.clone(),
                environment: d.environment.clone(),
                revision: d.revision.clone(),
                release: d.release.clone(),
                from: q.from.clone(),
                requested_by: d.recorded_by.clone(),
                note: q.note.clone(),
                at,
            },
            last_at: q.attempts.last().map(|a| a.at),
        }))
    }

    /// DEPLOY-25: one attempt to deliver `request`'s webhook, made — kept
    /// with the request, which says what it leaves: `delivered` on a 2xx;
    /// else `retrying` until `next_at` (unix seconds) when the worker names
    /// one, `failed` when it does not. A request no longer due (canceled,
    /// picked up) keeps the attempt and its state. Then `request` on
    /// `deployments.watch`. The worker's, as the system.
    pub async fn delivery_attempted(&self, _system: &System, request: u64, attempt: DeliveryAttempt, next_at: Option<i64>) -> Result<DeploymentsEvent, StoreError> {
        let attempt = DeliveryAttempt { error: attempt.error.as_deref().map(short), ..attempt };
        let delivered = attempt.status.is_some_and(|s| (200..300).contains(&s));
        let serial = self.serial().await;
        let row = {
            let cache = self.tickets.read().expect("store lock poisoned");
            let d = cache.deploys.by_id.get(&request).filter(|d| d.state == DeployState::Requested).ok_or_else(|| StoreError::Invalid(format!("no request {request}")))?;
            let mut row = d.clone();
            let q = row.requested.as_mut().expect("a request's rest");
            if q.webhook.due() {
                q.webhook = match (delivered, next_at) {
                    (true, _) => WebhookState::Delivered,
                    (false, Some(_)) => WebhookState::Retrying,
                    (false, None) => WebhookState::Failed,
                };
                q.next_attempt_at = next_at.filter(|_| !delivered);
            }
            q.attempts.push(attempt.clone());
            row
        };
        let stored = row.clone();
        self.write(
            serial,
            move |tx| {
                tx.prepare_cached("INSERT OR REPLACE INTO deploy_deliveries (request, attempt, at, status, error) VALUES (?1, ?2, ?3, ?4, ?5)")?
                    .execute(params![request as i64, attempt.attempt, attempt.at, attempt.status, attempt.error])?;
                store_request(tx, &stored)?;
                Ok(())
            },
            move |cache, _| {
                if let (Some(q), Some(new)) = (cache.deploys.request_mut(request), row.requested) {
                    *q = new;
                }
                cache.request_event(request)
            },
            move |(), event, tx| {
                let event = event.expect("a request just attempted");
                tx.deployment(event.clone());
                event
            },
        )
        .await
    }
}
