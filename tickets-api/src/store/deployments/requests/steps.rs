//! DEPLOY-37: **a request's steps** — what its executor reports while it
//! runs (`deployments step`), and a cancel asked of one it has picked up.
//!
//! menger knows no step: the executor names them (`build`, `gate`,
//! `deploy`, `smoke`, …), each reported by name as it starts and ends, and
//! a step reported again is the same step — its place and start kept, its
//! state, end, detail and url the latest. They are kept with the request
//! (`requested.steps`) and said on `deployments.watch` as the request
//! whole. A step moves nothing: no cell, no stage, no status — the
//! executor's records (`record --request`) are what say the request
//! started and how it ended.
//!
//! **Cancel.** A pending request is canceled outright (DEPLOY-25). One an
//! executor has picked up is asked to stop: `cancel_request` keeps when and
//! by whom (`cancel_asked_at` / `_by`), and the executor reads it in the
//! answer to its next `step`, stops, and records how it ended.

use super::*;
use crate::deployments::{step_name_of, DeployStep, StepState, STEPS_MAX, STEP_DETAIL_MAX};

/// The tables — made with the requests' on every open.
pub(in crate::store) const TABLES: &str = "
        CREATE TABLE IF NOT EXISTS deploy_steps (
            request    INTEGER NOT NULL,
            name       TEXT NOT NULL,
            ord        INTEGER NOT NULL,
            state      TEXT NOT NULL,
            started_at INTEGER NOT NULL,
            ended_at   INTEGER,
            detail     TEXT,
            url        TEXT,
            by         TEXT NOT NULL,
            PRIMARY KEY (request, name)
        );
        CREATE TABLE IF NOT EXISTS deploy_cancel_asks (
            request INTEGER PRIMARY KEY,
            at      INTEGER NOT NULL,
            by      TEXT NOT NULL
        );
    ";

/// What an executor reports of one step.
#[derive(Debug, Clone)]
pub struct StepReport {
    pub request: u64,
    pub name: String,
    pub state: StepState,
    pub detail: Option<String>,
    pub url: Option<String>,
}

/// Each request's steps and cancel ask onto `rest`, at open.
pub(super) fn load(conn: &Connection, rest: &mut HashMap<u64, DeployRequest>) -> anyhow::Result<()> {
    let mut stmt = conn.prepare("SELECT request, name, state, started_at, ended_at, detail, url, by FROM deploy_steps ORDER BY request, ord")?;
    let mut rows = stmt.query([])?;
    while let Some(r) = rows.next()? {
        let id = r.get::<_, i64>(0)? as u64;
        let state: String = r.get(2)?;
        let step = DeployStep {
            name: r.get(1)?,
            state: state.parse().map_err(|e: String| anyhow::anyhow!("request {id}: {e}"))?,
            started_at: r.get(3)?,
            ended_at: r.get(4)?,
            detail: r.get(5)?,
            url: r.get(6)?,
            by: r.get(7)?,
        };
        if let Some(q) = rest.get_mut(&id) {
            q.steps.push(step);
        }
    }
    let mut stmt = conn.prepare("SELECT request, at, by FROM deploy_cancel_asks")?;
    let mut rows = stmt.query([])?;
    while let Some(r) = rows.next()? {
        if let Some(q) = rest.get_mut(&(r.get::<_, i64>(0)? as u64)) {
            (q.cancel_asked_at, q.cancel_asked_by) = (Some(r.get(1)?), Some(r.get(2)?));
        }
    }
    Ok(())
}

/// A detail kept to its last [`STEP_DETAIL_MAX`] bytes (a log's end), on a
/// char boundary.
fn tail(text: &str) -> String {
    let mut from = text.len().saturating_sub(STEP_DETAIL_MAX);
    while !text.is_char_boundary(from) {
        from += 1;
    }
    text[from..].to_string()
}

/// Whether a request in `state` still takes a step in `step`: pending (its
/// executor has it, its `started` record not sent yet) or started; a
/// canceled one only a step's end (how the cancel left it), never a start.
fn running(id: u64, q: &DeployRequest, step: StepState) -> Result<(), StoreError> {
    match q.state {
        RequestState::Pending | RequestState::Started => Ok(()),
        RequestState::Canceled if step.ended() => Ok(()),
        RequestState::Canceled => Err(StoreError::Invalid(format!("request {id} is canceled: stop (nothing was recorded)"))),
        s => Err(StoreError::Invalid(format!("request {id} has ended — {} (nothing was recorded)", s.as_str()))),
    }
}

impl Store {
    /// DEPLOY-37: one step of the request `report.request`, as its executor
    /// reports it — kept by name (a step reported again keeps its place and
    /// start). A request ended by a record takes none, a canceled one only
    /// a step's end; nor
    /// does one with [`STEPS_MAX`] steps a new name. Then `request` on
    /// `deployments.watch`. The answer is the request whole: its
    /// `cancel_asked_at` tells the executor to stop.
    ///
    /// ACCESS-11: `allowed` — the system, or a deployer (or admin) on
    /// exactly the request's project, checked under the write-serial lock.
    pub async fn request_step(&self, allowed: impl Into<Authority>, report: StepReport, by: String) -> Result<DeploymentsEvent, StoreError> {
        let allowed = allowed.into();
        let bad = |m: String| StoreError::Invalid(format!("{m} (nothing was recorded)"));
        let name = step_name_of(&report.name).map_err(bad)?;
        let url = match report.url.as_deref().map(str::trim).filter(|u| !u.is_empty()) {
            Some(u) => Some(crate::deployments::webhook_url_of(u).map_err(bad)?),
            None => None,
        };
        let detail = report.detail.as_deref().map(str::trim_end).filter(|d| !d.is_empty()).map(tail);
        let (id, now) = (report.request, chrono::Utc::now().timestamp());
        let serial = self.serial().await;
        let (q, ord) = {
            let cache = self.tickets.read().expect("store lock poisoned");
            let d = cache.deploys.by_id.get(&id).filter(|d| d.state == DeployState::Requested).ok_or_else(|| bad(format!("no request {id}")))?;
            allowed.deployer_on(&super::super::super::writs::scope_of(&cache, &ShortcodeId::new(&d.project)), "a step needs deployer on its request's project")?;
            let mut q = d.requested.clone().unwrap_or_else(|| blank(None, None, None));
            running(id, &q, report.state)?;
            let ended_at = report.state.ended().then_some(now);
            let ord = match q.steps.iter().position(|s| s.name == name) {
                Some(i) => {
                    let s = &mut q.steps[i];
                    (s.state, s.ended_at, s.detail, s.url, s.by) = (report.state, ended_at, detail, url, by);
                    i
                }
                None if q.steps.len() >= STEPS_MAX => return Err(bad(format!("request {id} has {STEPS_MAX} steps, the most one keeps"))),
                None => {
                    q.steps.push(DeployStep { name, state: report.state, started_at: now, ended_at, detail, url, by });
                    q.steps.len() - 1
                }
            };
            (q, ord)
        };
        let s = q.steps[ord].clone();
        self.write(
            serial,
            move |tx| {
                tx.prepare_cached(
                    "INSERT INTO deploy_steps (request, name, ord, state, started_at, ended_at, detail, url, by) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
                         ON CONFLICT (request, name) DO UPDATE SET state = excluded.state, ended_at = excluded.ended_at,
                             detail = excluded.detail, url = excluded.url, by = excluded.by",
                )?
                .execute(params![id as i64, s.name, ord as i64, s.state.as_str(), s.started_at, s.ended_at, s.detail, s.url, s.by])?;
                super::stop_delivery(tx, id)?;
                Ok(())
            },
            move |cache, _| {
                if let Some(r) = cache.deploys.request_mut(id) {
                    *r = q;
                }
                cache.deploys.stepped(id);
                cache.request_event(id)
            },
            move |(), event, tx| {
                let event = event.expect("a request just stepped");
                tx.deployment(event.clone());
                event
            },
        )
        .await
    }

    /// DEPLOY-37: asks the executor running the request `id` to stop —
    /// kept as `cancel_asked_at` / `_by`; the request runs on until its
    /// executor records how it ended. Refused for a request not under way,
    /// or one asked already. Then `request` on `deployments.watch`.
    ///
    /// ACCESS-11: `allowed` — as [`Store::cancel_request`]'s.
    pub(super) async fn ask_cancel(&self, allowed: Authority, id: u64, by: String) -> Result<DeploymentsEvent, StoreError> {
        let now = chrono::Utc::now().timestamp();
        let serial = self.serial().await;
        let q = {
            let cache = self.tickets.read().expect("store lock poisoned");
            let d = cache.deploys.by_id.get(&id).filter(|d| d.state == DeployState::Requested).ok_or_else(|| StoreError::Invalid(format!("no request {id} (nothing was canceled)")))?;
            allowed.deployer_on(&super::super::super::writs::scope_of(&cache, &ShortcodeId::new(&d.project)), "canceling a request needs deployer on its project")?;
            let mut q = d.requested.clone().unwrap_or_else(|| blank(None, None, None));
            if q.state != RequestState::Started {
                return Err(StoreError::Invalid(format!("request {id} is not under way — {} (nothing was canceled)", q.state.as_str())));
            }
            if q.cancel_asked_at.is_some() {
                return Err(StoreError::Invalid(format!("request {id}'s executor was asked to stop already (nothing was canceled)")));
            }
            (q.cancel_asked_at, q.cancel_asked_by) = (Some(now), Some(by.clone()));
            q
        };
        self.write(
            serial,
            move |tx| {
                tx.prepare_cached("INSERT OR REPLACE INTO deploy_cancel_asks (request, at, by) VALUES (?1, ?2, ?3)")?.execute(params![id as i64, now, by])?;
                Ok(())
            },
            move |cache, _| {
                if let Some(r) = cache.deploys.request_mut(id) {
                    *r = q;
                }
                cache.deploys.stepped(id);
                cache.request_event(id)
            },
            move |(), event, tx| {
                let event = event.expect("a request just asked to stop");
                tx.deployment(event.clone());
                event
            },
        )
        .await
    }
}
