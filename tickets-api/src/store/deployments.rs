//! DEPLOY-11: the **deployments store** — every deployment any CI records,
//! to any environment, kept beside the tickets; what each environment runs;
//! and, for every ticket, where its work runs and since when.
//!
//! **A record's tickets** (DEPLOY-9, decided: `mapping` = both): the tickets
//! whose linked commits (`link_commits`, SDLC-9) are among the record's
//! `commits` and `revision`, plus the ids the record names in `tickets`.
//! menger never runs git, so two references name one commit when one hash
//! is a prefix of the other (a CI's whole SHA and a merge's seven), the
//! repository before `@` aside; everything menger knows of ancestry comes
//! from the records ([`fold`]). **The join is kept**, not computed once: a
//! commit linked after it was deployed counts wherever it runs — every row
//! written relinks its ticket ([`Deploys::relink`], from `Cache::insert`).
//!
//! **What an environment runs** is a fold of its records in `(at, id)`
//! order: the succeeded deploys that still count, oldest first (its
//! *stack*). A `succeeded` deploy goes on top — after cutting the stack back
//! to its revision when that revision is already there (a later deploy to an
//! older revision undoes what came after it), else to its `previous`. A
//! `rolled_back` record cuts the stack back to its revision (or to below its
//! `previous`, else by one) and carries nothing itself. `started` and
//! `failed` are recorded and shown, and count for nothing. A ticket runs in
//! an environment when a deploy on its stack carried it — since the earliest
//! such deploy — so a rollback takes out exactly what only the undone deploys
//! carried. A deployment never changes a ticket's status (DEPLOY-9:
//! `status` = a separate fact).
//!
//! **The kept cells** are each environment's running tickets
//! ([`Kept::running`]): `deployed <env> [window]` (FILTER-3's language,
//! [`crate::filter::Event::Deployed`]) is answered from them
//! ([`Deploys::holds`]), never by testing a ticket; `--verify-derived` holds
//! them to a scan and the stored environments to a fold ([`check`]).
//!
//! **Stored** in three tables, written through in each record's own
//! transaction ([`Store::record_deployment`]): `deployments` (a row each,
//! unique by project, environment, state, revision and run — else `at` — so
//! a CI's retry is one deployment), `deployment_commits` (each record's
//! commits, in order) and `deploy_environments` (what each environment runs,
//! derived, checked). A file without them gains them, empty, on open
//! ([`migrate`]). A **project** is a shortcode the store holds (`menger` is
//! MENGER) — what a `deployer` grant names and a ticket lives in.
//!
//! **A ticket runs a version** (DEPLOY-20): its commits are grouped into
//! versions by the link call that brought each ([`super::versions`]), and
//! a ticket that runs in an environment is **behind** there when a version
//! linked after the newest one the environment carries is not carried —
//! the environment runs an earlier version. [`Kept::behind`] keeps, per
//! environment, each such ticket's commits not carried (in link order) and
//! the version that runs (its newest carried commit); a record that named
//! the ticket carries every commit linked by then. `behind <env>` is
//! answered from it ([`Deploys::holds_behind`]); a rollback refolds it as
//! it refolds the cells.
//!
//! **A ticket's delivery stage** (DEPLOY-21): an environment may name the
//! one it promotes from ([`Deploys::sources`], the `deploy_sources` table:
//! `deployments promotes`, or a record's `from`) — menger's `prod` from
//! `test`. Short of running its current version in an environment, a
//! ticket is, there, the first of: **deploying** (an open `started` record
//! — none has ended a deploy since — carries its current version,
//! [`Kept::deploying`]), **failed** (the last record that carried its
//! current version there failed, or was rolled back, [`Kept::failed`]),
//! **ready for** (its current version runs where the environment promotes
//! from) and **waiting** (Complete, with commits, in the project, its
//! current version running nowhere on the environment's track). The first
//! two are kept per environment beside the cells and moved with them; the
//! other two are set algebra over the kept cells and the cell table at
//! read time ([`Cache::holds_stage`]). A record "carries a version" as
//! DEPLOY-20 says a deploy does ([`Touch::current`]).

use super::history::Clock;
use super::versions::{self, Linked};
use super::{lineage, Authority, Cache, OrderKey, Row, Store, StoreError};
use crate::address::ShortcodeId;
use crate::vocab::CommitRef;
use crate::deployments::{
    environment_of, project_of, rank, DeliveryStage, DeploySource, DeployState, Deployment, DeploymentsEvent, Environment, Record, RunningCell, StagedCell,
    COMMITS_MAX, TICKETS_MAX,
};
use crate::filter::{When, Window};
use roaring::RoaringBitmap;
use rusqlite::{params, Connection};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::sync::Arc;

/// DEPLOY-13: the most ids one `running` may name.
pub const RUNNING_IDS_MAX: usize = 1_000;

/// The tables.
pub(super) const TABLES: &str = "
        CREATE TABLE IF NOT EXISTS deployments (
            id          INTEGER PRIMARY KEY,
            project     TEXT NOT NULL,
            environment TEXT NOT NULL,
            state       TEXT NOT NULL,
            revision    TEXT NOT NULL,
            previous    TEXT,
            tickets     TEXT NOT NULL DEFAULT '',
            release     TEXT,
            at_unix     INTEGER NOT NULL,
            system      TEXT,
            run         TEXT,
            url         TEXT,
            recorded_at INTEGER NOT NULL,
            recorded_by TEXT NOT NULL,
            dedupe      TEXT NOT NULL UNIQUE,
            request     INTEGER
        );
        CREATE INDEX IF NOT EXISTS deployments_by_environment ON deployments (project, environment, at_unix, id);
        CREATE TABLE IF NOT EXISTS deployment_commits (
            deployment INTEGER NOT NULL,
            pos        INTEGER NOT NULL,
            sha        TEXT NOT NULL,
            PRIMARY KEY (deployment, pos)
        );
        CREATE INDEX IF NOT EXISTS deployment_commits_by_sha ON deployment_commits (sha);
        CREATE TABLE IF NOT EXISTS deploy_environments (
            project    TEXT NOT NULL,
            name       TEXT NOT NULL,
            revision   TEXT,
            since      INTEGER,
            release    TEXT,
            deployment INTEGER,
            last       INTEGER,
            PRIMARY KEY (project, name)
        );
    ";

/// DEPLOY-21: which environment each promotes from — set, not derived.
const SOURCES: &str = "
        CREATE TABLE IF NOT EXISTS deploy_sources (
            project TEXT NOT NULL,
            name    TEXT NOT NULL,
            source  TEXT NOT NULL,
            set_at  INTEGER NOT NULL,
            set_by  TEXT NOT NULL,
            PRIMARY KEY (project, name)
        );
    ";

fn has_table(conn: &Connection) -> rusqlite::Result<bool> {
    conn.query_row("SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name = 'deployments'", [], |r| {
        r.get::<_, i64>(0).map(|n| n > 0)
    })
}

/// A file without the tables gains them, empty, here — on every open, a
/// no-op once they are there. Returns whether they were made.
pub(crate) fn migrate(conn: &mut Connection) -> anyhow::Result<bool> {
    // DEPLOY-21: and one without the sources gains that table, empty.
    conn.execute_batch(SOURCES)?;
    // DEPLOY-25: and the requests', the deliveries' and the webhooks'.
    conn.execute_batch(requests::TABLES)?;
    if has_table(conn)? {
        // DEPLOY-25: a record names the request it answers.
        if !has_request_column(conn)? {
            conn.execute_batch("ALTER TABLE deployments ADD COLUMN request INTEGER")?;
            tracing::warn!("DEPLOY-25: deployments gained its request column, empty");
        }
        return Ok(false);
    }
    conn.execute_batch(TABLES)?;
    tracing::warn!("DEPLOY-11: the file gained the deployments tables, empty, durably; every later open reads them");
    Ok(true)
}

/// DEPLOY-25: whether `deployments` has its `request` column (a file
/// `--verify-derived` reads, unmigrated, may not).
fn has_request_column(conn: &Connection) -> rusqlite::Result<bool> {
    conn.query_row("SELECT count(*) FROM pragma_table_info('deployments') WHERE name = 'request'", [], |r| r.get::<_, i64>(0).map(|n| n > 0))
}

/// DEPLOY-25: a request — what menger asked a CI for — and its webhook.
mod requests;
pub use requests::{Due, RequestAsk};
#[cfg(test)]
pub(super) use requests::NOTE_MAX;

// ---------------------------------------------------------------------
// Commits, without git

/// A reference's hash, its repository off.
fn hash_of(c: &str) -> &str {
    c.rsplit_once('@').map_or(c, |(_, h)| h)
}

/// What a commit is filed under: the first seven of its hash, which every
/// reference has (SDLC-9's grammar).
fn key_of(c: &str) -> String {
    let h = hash_of(c);
    h[..h.len().min(7)].to_ascii_lowercase()
}

/// Whether two references name one commit, as far as menger can tell
/// without a repository: one hash is a prefix of the other.
pub(super) fn same(a: &str, b: &str) -> bool {
    let (a, b) = (hash_of(a), hash_of(b));
    a.len().min(b.len()) >= 7 && (a.starts_with(b) || b.starts_with(a))
}

/// Every ticket's linked commits, filed by [`key_of`] — the join's other
/// side, moved by every row written.
#[derive(Debug, Clone, Default)]
pub(crate) struct Links {
    by_key: HashMap<String, Vec<(String, String)>>,
    of: HashMap<String, Vec<CommitRef>>,
    /// DEPLOY-20: each ticket's link record — which version each commit
    /// linked since DEPLOY-20 is, and when.
    record: HashMap<String, Linked>,
}

/// DEPLOY-20: a ticket running an earlier version in an environment — its
/// commits not carried there, in link order, and the version that runs
/// (its newest carried commit; `None` when only a record's naming carried
/// it).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Behind {
    pub(crate) version: Option<String>,
    pub(crate) not: Vec<String>,
}

impl Links {
    /// `ticket`'s commits are `commits` now; whether that moved anything.
    fn set(&mut self, ticket: &str, commits: &[CommitRef]) -> bool {
        let had: Vec<CommitRef> = self.of.get(ticket).cloned().unwrap_or_default();
        if had == commits {
            return false;
        }
        for c in &had {
            let k = key_of(c);
            if let Some(v) = self.by_key.get_mut(&k) {
                v.retain(|(h, t)| !(t == ticket && h == hash_of(c)));
                if v.is_empty() {
                    self.by_key.remove(&k);
                }
            }
        }
        for c in commits {
            self.by_key.entry(key_of(c)).or_default().push((hash_of(c).to_string(), ticket.to_string()));
        }
        if commits.is_empty() {
            self.of.remove(ticket);
        } else {
            self.of.insert(ticket.to_string(), commits.to_vec());
        }
        true
    }

    /// The tickets linking the commit `c` names (a ticket may come twice).
    fn tickets<'a>(&'a self, c: &'a str) -> impl Iterator<Item = &'a str> + 'a {
        self.by_key.get(&key_of(c)).into_iter().flatten().filter(move |(h, _)| same(h, c)).map(|(_, t)| t.as_str())
    }

    /// DEPLOY-20: whether `ticket`, running where `carries` says which
    /// commits are (and, when a record there named it, `named` the latest
    /// such record's moment), runs an earlier version: the versions linked
    /// after its newest carried commit's, when there are any.
    fn behind(&self, ticket: &str, carries: impl Fn(&str) -> bool, named: Option<i64>) -> Option<Behind> {
        let commits = self.of.get(ticket)?;
        let linked = self.record.get(ticket);
        let version = versions::groups(commits, linked);
        let carried = |c: &CommitRef| {
            carries(c.as_str()) || named.is_some_and(|t| versions::at(c.as_str(), linked).is_none_or(|at| at <= t))
        };
        let newest = commits.iter().rposition(carried);
        let after = newest.map(|i| version[i]);
        let not: Vec<String> =
            commits.iter().zip(&version).filter(|(_, v)| after.is_none_or(|a| **v > a)).map(|(c, _)| c.to_string()).collect();
        (!not.is_empty()).then(|| Behind { version: newest.map(|i| commits[i].to_string()), not })
    }
}

// ---------------------------------------------------------------------
// An environment: its fold, and the kept cells

/// One environment as kept: its records, the fold of them, and its cells.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Kept {
    /// Every deployment to it, by `(at, id)`.
    records: Vec<(i64, u64)>,
    /// The succeeded deploys that still count, oldest first.
    stack: Vec<u64>,
    revision: Option<String>,
    release: Option<String>,
    since: Option<i64>,
    /// The deployment that made it run `revision`.
    deployment: Option<u64>,
    /// The latest deployment to it, whatever its state.
    last: Option<u64>,
    /// The commits running there, by [`key_of`]: each hash, since when, and
    /// the deploy that first brought it.
    carried: HashMap<String, Vec<(String, i64, u64)>>,
    /// The ids the stack's records named: since when, brought by which.
    named: HashMap<String, (i64, u64)>,
    /// DEPLOY-20: …and the latest such record's moment — what it carried of
    /// the ticket is every commit linked by then.
    named_last: HashMap<String, i64>,
    /// The kept cells: each ticket running there — since when, brought by
    /// which deployment. An id that names no ticket (a deleted one, a name a
    /// CI made up) is kept and never answered.
    pub(super) running: BTreeMap<String, (i64, u64)>,
    /// DEPLOY-20: of `running`, each ticket that runs an earlier version.
    pub(super) behind: BTreeMap<String, Behind>,
    /// DEPLOY-21: the `started` records no record has ended since, in order.
    starts: Vec<Touch>,
    /// DEPLOY-21: every record that ended a deploy — succeeded, failed, a
    /// rollback (as what it undid) — in order.
    ends: Vec<Touch>,
    /// DEPLOY-21: the kept stages — each ticket an open start carries at
    /// its current version, not running it here…
    pub(super) deploying: BTreeSet<String>,
    /// …and each whose last ending here failed or was rolled back, neither
    /// running its current version here nor deploying.
    pub(super) failed: BTreeSet<String>,
}

/// DEPLOY-21: a record as the stages read it — its state, the commits it
/// carries (its own, or a rollback's: what it undid) by [`key_of`], the ids
/// it names, and the moment a naming carries the commits linked by.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Touch {
    state: DeployState,
    refs: HashMap<String, Vec<String>>,
    tickets: Vec<String>,
    at: i64,
}

impl Touch {
    /// `ds` carried, as one record in `state` — `at` when `ds` is empty.
    fn of(state: DeployState, at: i64, ds: &[&Deployment]) -> Self {
        let mut t = Touch { state, refs: HashMap::new(), tickets: Vec::new(), at: ds.iter().map(|d| d.at).max().unwrap_or(at) };
        for d in ds {
            for c in d.commits.iter().chain(std::iter::once(&d.revision)) {
                let list = t.refs.entry(key_of(c)).or_default();
                if !list.iter().any(|h| h == hash_of(c)) {
                    list.push(hash_of(c).to_string());
                }
            }
            for n in &d.tickets {
                if !t.tickets.contains(n) {
                    t.tickets.push(n.clone());
                }
            }
        }
        t
    }

    fn carries(&self, c: &str) -> bool {
        self.refs.get(&key_of(c)).is_some_and(|l| l.iter().any(|h| same(h, c)))
    }

    /// Whether it carries `ticket`'s current version: its newest version
    /// is carried (DEPLOY-20's rule) — a ticket with no commits, when it
    /// names it.
    fn current(&self, ticket: &str, links: &Links) -> bool {
        let named = self.tickets.iter().any(|t| t == ticket).then_some(self.at);
        match links.of.contains_key(ticket) {
            false => named.is_some(),
            true => links.behind(ticket, |c| self.carries(c), named).is_none(),
        }
    }

    /// The tickets it may carry: those linking its commits, and those it names.
    fn candidates<'a>(&'a self, links: &'a Links) -> impl Iterator<Item = &'a str> + 'a {
        self.refs.values().flatten().flat_map(move |h| links.tickets(h)).chain(self.tickets.iter().map(String::as_str))
    }
}

/// The earlier of two `(since, deployment)`.
fn earlier(a: Option<(i64, u64)>, b: (i64, u64)) -> (i64, u64) {
    a.map_or(b, |a| a.min(b))
}

/// The fold of an environment's deployments, `ds` in `(at, id)` order —
/// its stack, what it runs, and what the stack carries — without the cells
/// ([`Kept::join`]).
pub(super) fn fold(ds: &[&Deployment]) -> Kept {
    let by_id: HashMap<u64, &Deployment> = ds.iter().map(|d| (d.id, *d)).collect();
    let mut k = Kept::default();
    let at = |stack: &[u64], c: &str| stack.iter().rposition(|id| same(&by_id[id].revision, c));
    for d in ds {
        k.records.push((d.at, d.id));
        k.last = Some(d.id);
        // DEPLOY-21: a start stays open until a record ends a deploy.
        match d.state {
            DeployState::Started => k.starts.push(Touch::of(d.state, d.at, &[d])),
            DeployState::Failed | DeployState::Succeeded => {
                k.starts.clear();
                k.ends.push(Touch::of(d.state, d.at, &[d]));
            }
            DeployState::RolledBack => k.starts.clear(),
            // DEPLOY-25: a request is folded nowhere (never reaches here).
            DeployState::Requested => {}
        }
        let before = (d.state == DeployState::RolledBack).then(|| k.stack.clone());
        match d.state {
            DeployState::Started | DeployState::Failed | DeployState::Requested => continue,
            DeployState::Succeeded => {
                if let Some(i) = at(&k.stack, &d.revision) {
                    k.stack.truncate(i + 1);
                } else if let Some(j) = d.previous.as_deref().and_then(|p| at(&k.stack, p)) {
                    k.stack.truncate(j + 1);
                }
                k.stack.push(d.id);
            }
            DeployState::RolledBack => {
                if let Some(i) = at(&k.stack, &d.revision) {
                    k.stack.truncate(i + 1);
                } else if let Some(j) = d.previous.as_deref().and_then(|p| at(&k.stack, p)) {
                    k.stack.truncate(j);
                } else {
                    k.stack.pop();
                }
            }
        }
        // DEPLOY-21: a rollback ends what it undid.
        if let Some(before) = before {
            let undone: Vec<&Deployment> = before.iter().filter(|id| !k.stack.contains(id)).map(|id| by_id[id]).collect();
            k.ends.push(Touch::of(d.state, d.at, &undone));
        }
        k.revision = Some(d.revision.clone());
        k.release = d.release.clone();
        k.since = Some(d.at);
        k.deployment = Some(d.id);
    }
    for id in &k.stack {
        let d = by_id[id];
        for c in d.commits.iter().chain(std::iter::once(&d.revision)) {
            let list = k.carried.entry(key_of(c)).or_default();
            if !list.iter().any(|(h, ..)| h == hash_of(c)) {
                list.push((hash_of(c).to_string(), d.at, d.id));
            }
        }
        for t in &d.tickets {
            k.named.entry(t.clone()).or_insert((d.at, d.id));
            k.named_last.insert(t.clone(), d.at);
        }
    }
    k
}

impl Kept {
    /// DEPLOY-21: whether `ticket` runs its current version here.
    pub(super) fn current(&self, ticket: &str) -> bool {
        self.running.contains_key(ticket) && !self.behind.contains_key(ticket)
    }

    /// DEPLOY-21: the tickets running their current version here.
    pub(super) fn currents(&self) -> impl Iterator<Item = &str> + '_ {
        self.running.keys().map(String::as_str).filter(|t| !self.behind.contains_key(*t))
    }

    /// DEPLOY-21: the stages, from the starts and ends joined with `links`.
    fn stage(&mut self, links: &Links) {
        let mut deploying = BTreeSet::new();
        for s in &self.starts {
            for t in s.candidates(links) {
                if !deploying.contains(t) && !self.current(t) && s.current(t, links) {
                    deploying.insert(t.to_string());
                }
            }
        }
        let mut last: HashMap<&str, DeployState> = HashMap::new();
        for e in &self.ends {
            for t in e.candidates(links) {
                if e.current(t, links) {
                    last.insert(t, e.state);
                }
            }
        }
        let failed: BTreeSet<String> = last
            .into_iter()
            .filter(|(t, s)| matches!(s, DeployState::Failed | DeployState::RolledBack) && !self.current(t) && !deploying.contains(*t))
            .map(|(t, _)| t.to_string())
            .collect();
        (self.deploying, self.failed) = (deploying, failed);
    }

    /// DEPLOY-21: `ticket`'s stages again — its links moved.
    fn restage(&mut self, ticket: &str, links: &Links) {
        let current = self.current(ticket);
        let deploying = !current && self.starts.iter().any(|s| s.current(ticket, links));
        let last = self.ends.iter().rev().find(|e| e.current(ticket, links)).map(|e| e.state);
        let failed = !current && !deploying && matches!(last, Some(DeployState::Failed | DeployState::RolledBack));
        for (set, on) in [(&mut self.deploying, deploying), (&mut self.failed, failed)] {
            match on {
                true => set.insert(ticket.to_string()),
                false => set.remove(ticket),
            };
        }
    }

    /// Whether the stack carries the commit `c` names.
    fn carries(&self, c: &str) -> bool {
        self.carried.get(&key_of(c)).is_some_and(|l| l.iter().any(|(h, ..)| same(h, c)))
    }

    /// DEPLOY-20: `ticket`'s behind, as it runs here.
    fn behind_of(&self, ticket: &str, links: &Links) -> Option<Behind> {
        links.behind(ticket, |c| self.carries(c), self.named_last.get(ticket).copied())
    }

    /// The cells, from the stack's commits and names joined with `links`.
    fn join(&mut self, links: &Links) {
        self.running.clear();
        for list in self.carried.values() {
            for (h, at, id) in list {
                for t in links.tickets(h) {
                    let was = self.running.get(t).copied();
                    self.running.insert(t.to_string(), earlier(was, (*at, *id)));
                }
            }
        }
        for (t, v) in &self.named {
            let was = self.running.get(t).copied();
            self.running.insert(t.clone(), earlier(was, *v));
        }
        self.behind = self.running.keys().filter_map(|t| self.behind_of(t, links).map(|b| (t.clone(), b))).collect();
        self.stage(links);
    }

    /// `ticket`'s cell again — its links moved.
    fn rejoin(&mut self, ticket: &str, links: &Links) {
        let mut best = self.named.get(ticket).copied();
        for c in links.of.get(ticket).into_iter().flatten() {
            for (h, at, id) in self.carried.get(&key_of(c)).into_iter().flatten() {
                if same(h, c) {
                    best = Some(earlier(best, (*at, *id)));
                }
            }
        }
        match best {
            Some(v) => self.running.insert(ticket.to_string(), v),
            None => self.running.remove(ticket),
        };
        match best.and_then(|_| self.behind_of(ticket, links)) {
            Some(b) => self.behind.insert(ticket.to_string(), b),
            None => self.behind.remove(ticket),
        };
        self.restage(ticket, links);
    }
}

// ---------------------------------------------------------------------
// Every deployment, as the cache keeps them

/// The store's deployments, environments and cells, as the cache holds them.
#[derive(Debug, Clone, Default)]
pub(crate) struct Deploys {
    by_id: BTreeMap<u64, Deployment>,
    /// Every deployment by `(at, id)` — what `list` pages, newest first.
    by_time: BTreeSet<(i64, u64)>,
    /// The key a retry repeats ([`dedupe`]) → the deployment kept for it.
    keys: HashMap<String, u64>,
    pub(super) envs: BTreeMap<(String, String), Kept>,
    links: Links,
    /// DEPLOY-21: `(project, environment)` → the environment it promotes from.
    pub(super) sources: BTreeMap<(String, String), String>,
    /// DEPLOY-25: the pending requests' ids — what `requested <env>` reads.
    pending: BTreeSet<u64>,
    /// DEPLOY-25: each environment's webhook, its secret with it (never
    /// answered; the deliveries' alone).
    webhooks: BTreeMap<(String, String), requests::Hook>,
}

/// What makes two records one deployment: the project, the environment, the
/// state, the revision, and the run when the CI names one, else `at`.
fn dedupe(d: &Deployment) -> String {
    // DEPLOY-25: a request is its own — two asks are two requests.
    if d.state == DeployState::Requested {
        return format!("request:{}", d.id);
    }
    let when = match &d.source.run {
        Some(run) => format!("run:{run}"),
        None => format!("at:{}", d.at),
    };
    format!("{}\u{1f}{}\u{1f}{}\u{1f}{}\u{1f}{when}", d.project, d.environment, d.state.as_str(), hash_of(&d.revision))
}

impl Deploys {
    /// No deployments, the links of `rows`.
    pub(super) fn linked<'r>(rows: impl IntoIterator<Item = &'r Row>) -> Self {
        let mut d = Self::default();
        for r in rows {
            d.links.set(&r.id, &r.commits);
        }
        d
    }

    /// `ticket`'s row was written with `commits`: when they moved, its cell
    /// in every environment again.
    pub(super) fn relink(&mut self, ticket: &str, commits: &[CommitRef]) {
        if self.links.set(ticket, commits) {
            for k in self.envs.values_mut() {
                k.rejoin(ticket, &self.links);
            }
        }
    }

    /// DEPLOY-20: the version the next link call to `ticket` makes.
    pub(super) fn next_version(&self, ticket: &str) -> u32 {
        versions::next(self.links.record.get(ticket))
    }

    /// DEPLOY-20: `refs` linked to `ticket` as `version` at `at` — recorded
    /// before the row goes in, whose relink rejoins the cells with it.
    pub(super) fn versioned(&mut self, ticket: &str, refs: &[CommitRef], version: u32, at: i64) {
        let l = self.links.record.entry(ticket.to_string()).or_default();
        for r in refs {
            l.insert(r.to_string(), (version, at));
        }
    }

    /// DEPLOY-20: `refs` unlinked from `ticket`, out of its record.
    pub(super) fn unversioned(&mut self, ticket: &str, refs: &[CommitRef]) {
        if let Some(l) = self.links.record.get_mut(ticket) {
            for r in refs {
                l.remove(r.as_str());
            }
            if l.is_empty() {
                self.links.record.remove(ticket);
            }
        }
    }

    fn next_id(&self) -> u64 {
        self.by_id.keys().next_back().map_or(1, |n| n + 1)
    }

    /// The environment `(project, name)` folded again, `extra` among its
    /// records — what a record would leave it as.
    fn refold(&self, project: &str, name: &str, extra: Option<&Deployment>) -> Kept {
        let mut ds: Vec<&Deployment> = self
            .envs
            .get(&(project.to_string(), name.to_string()))
            .map(|k| k.records.iter().map(|(_, id)| &self.by_id[id]).collect())
            .unwrap_or_default();
        ds.extend(extra.filter(|d| d.state != DeployState::Requested));
        ds.sort_by_key(|d| (d.at, d.id));
        let mut k = fold(&ds);
        k.join(&self.links);
        k
    }

    /// The cache's half of a committed record: `d` kept, its environment as
    /// `kept` says.
    fn install(&mut self, d: Deployment, kept: Kept) {
        self.keys.insert(dedupe(&d), d.id);
        self.by_time.insert((d.at, d.id));
        self.envs.insert((d.project.clone(), d.environment.clone()), kept);
        // DEPLOY-25: a record naming a request moves it.
        if let Some(r) = d.request {
            self.answered(r, &d);
        }
        self.by_id.insert(d.id, d);
    }

    /// `records` kept — at open, and for `--verify-derived` — every
    /// environment folded and joined.
    fn load_records(&mut self, records: Vec<Deployment>) {
        let mut envs: BTreeSet<(String, String)> = BTreeSet::new();
        for d in records {
            // DEPLOY-25: a request is kept and listed, and folded nowhere.
            if d.state != DeployState::Requested {
                envs.insert((d.project.clone(), d.environment.clone()));
            }
            self.keys.insert(dedupe(&d), d.id);
            self.by_time.insert((d.at, d.id));
            self.by_id.insert(d.id, d);
        }
        self.settle_requests();
        for (p, e) in envs {
            let mut ds: Vec<&Deployment> =
                self.by_id.values().filter(|d| d.project == p && d.environment == e && d.state != DeployState::Requested).collect();
            ds.sort_by_key(|d| (d.at, d.id));
            let mut k = fold(&ds);
            k.join(&self.links);
            self.envs.insert((p, e), k);
        }
    }

    /// Whether `env` (`prod`, or `menger/prod`) names `(project, name)`.
    fn names(env: &str, project: &str, name: &str) -> bool {
        env == name || env.split_once('/').is_some_and(|(p, n)| p.eq_ignore_ascii_case(project) && n == name)
    }

    /// DEPLOY-11: the tickets `deployed <env> [window]` holds for, per
    /// shortcode — each running in an environment `env` names, since a
    /// moment in the window. From the kept cells, never a scan.
    pub(super) fn holds(&self, env: &str, window: Option<&Window>, clock: &Clock, at: &HashMap<String, OrderKey>) -> BTreeMap<ShortcodeId, RoaringBitmap> {
        let mut out: BTreeMap<ShortcodeId, RoaringBitmap> = BTreeMap::new();
        let Some((lo, hi)) = clock.bounds(window) else { return out };
        for ((p, n), k) in &self.envs {
            if !Self::names(env, p, n) {
                continue;
            }
            for (t, (since, _)) in &k.running {
                if lo.is_some_and(|l| *since < l) || hi.is_some_and(|h| *since >= h) {
                    continue;
                }
                if let Some(key) = at.get(t) {
                    out.entry(key.shortcode.clone()).or_default().insert(key.seq);
                }
            }
        }
        out
    }

    /// DEPLOY-20: the tickets `behind <env>` holds for, per shortcode — each
    /// running an earlier version in an environment `env` names. From the
    /// kept cells, never a scan.
    pub(super) fn holds_behind(&self, env: &str, at: &HashMap<String, OrderKey>) -> BTreeMap<ShortcodeId, RoaringBitmap> {
        let mut out: BTreeMap<ShortcodeId, RoaringBitmap> = BTreeMap::new();
        for ((p, n), k) in &self.envs {
            if !Self::names(env, p, n) {
                continue;
            }
            for key in k.behind.keys().filter_map(|t| at.get(t)) {
                out.entry(key.shortcode.clone()).or_default().insert(key.seq);
            }
        }
        out
    }

    /// DEPLOY-21: every environment kept or named by a source (either end),
    /// of every project.
    fn env_keys(&self) -> BTreeSet<(String, String)> {
        let named = self.sources.iter().flat_map(|((p, e), f)| [(p.clone(), e.clone()), (p.clone(), f.clone())]);
        self.envs.keys().cloned().chain(named).collect()
    }

    /// DEPLOY-21: the environment `(project, name)`'s track — it and every
    /// environment it promotes from or to, transitively.
    pub(super) fn track(&self, project: &str, name: &str) -> BTreeSet<String> {
        let mut out: BTreeSet<String> = BTreeSet::from([name.to_string()]);
        let mut todo = vec![name.to_string()];
        while let Some(n) = todo.pop() {
            let up = self.sources.get(&(project.to_string(), n.clone())).cloned();
            let down = self.sources.iter().filter(|((p, _), f)| p == project && **f == n).map(|((_, e), _)| e.clone());
            for e in up.into_iter().chain(down.collect::<Vec<_>>()) {
                if out.insert(e.clone()) {
                    todo.push(e);
                }
            }
        }
        out
    }

    /// DEPLOY-21: the tickets ready for `(project, name)`: running their
    /// current version where it promotes from, not here, neither deploying
    /// nor failed here.
    pub(super) fn ready_for<'a>(&'a self, project: &str, name: &str, k: &'a Kept) -> Vec<&'a str> {
        let Some(from) = self.sources.get(&(project.to_string(), name.to_string())) else { return Vec::new() };
        let Some(src) = self.envs.get(&(project.to_string(), from.clone())) else { return Vec::new() };
        src.currents().filter(|t| !k.current(t) && !k.deploying.contains(*t) && !k.failed.contains(*t)).collect()
    }

    /// DEPLOY-21: the tickets whose stage anywhere in `project` a change to
    /// its sources may move — each running its current version somewhere
    /// there, and each deploying or failed there.
    fn staged_in(&self, project: &str) -> BTreeSet<&str> {
        let mut out = BTreeSet::new();
        for ((p, _), k) in &self.envs {
            if p == project {
                out.extend(k.currents());
                out.extend(k.deploying.iter().chain(&k.failed).map(String::as_str));
            }
        }
        out
    }

    /// DEPLOY-21: why `(project, name)` may not promote from `from`: itself,
    /// or an environment that (transitively) promotes from it.
    fn source_refused(&self, project: &str, name: &str, from: &str) -> Option<String> {
        let mut at = from.to_string();
        let mut seen = BTreeSet::new();
        loop {
            if at == name {
                return Some(match from == name {
                    true => format!("{project}/{name} cannot promote from itself (nothing was written)"),
                    false => format!("{project}/{name} cannot promote from {from}: {from} promotes from {name}, a cycle (nothing was written)"),
                });
            }
            if !seen.insert(at.clone()) {
                return None;
            }
            match self.sources.get(&(project.to_string(), at.clone())) {
                Some(next) => at = next.clone(),
                None => return None,
            }
        }
    }

    /// DEPLOY-11: the next moment after the clock's now at which time alone
    /// moves what `deployed <env> <window>` holds for — as
    /// [`super::history::Log::next_boundary`] says it for a transition.
    pub(super) fn next_boundary(&self, env: &str, window: Option<&Window>, clock: &Clock) -> Option<i64> {
        let w = window?;
        let now = clock.now();
        let mut since: Vec<i64> = self
            .envs
            .iter()
            .filter(|((p, n), _)| Self::names(env, p, n))
            .flat_map(|(_, k)| k.running.values().map(|(s, _)| *s))
            .collect();
        since.sort_unstable();
        let first_from = |from: i64| since.get(since.partition_point(|s| *s < from)).copied();
        let edge = |len: i64| first_from(now - len).map(|at| at + len + 1);
        let ago = |x: &When| match x {
            When::Ago(n, u) => edge(i64::from(*n) * u.seconds()),
            When::At(_) => None,
        };
        match w {
            Window::Within(n, u) => edge(i64::from(*n) * u.seconds()),
            Window::Today | Window::Yesterday | Window::ThisWeek => clock.next_day(matches!(w, Window::ThisWeek)),
            Window::Since(x) | Window::Before(x) => ago(x),
            Window::Between(a, b) => match (ago(a), ago(b)) {
                (Some(x), Some(y)) => Some(x.min(y)),
                (x, y) => x.or(y),
            },
        }
    }
}

impl Cache {
    /// MENGER-13 / DEPLOY-11: the tickets a history predicate holds for, per
    /// shortcode — `deployed` from the deployments' cells, the rest from the
    /// transition log.
    pub(super) fn holds(&self, h: &crate::filter::History, clock: &Clock) -> BTreeMap<ShortcodeId, RoaringBitmap> {
        match &h.event {
            crate::filter::Event::Deployed(env) => self.deploys.holds(env, h.window.as_ref(), clock, &self.at),
            // DEPLOY-20: from the kept behind.
            crate::filter::Event::Behind(env) => self.deploys.holds_behind(env, &self.at),
            // DEPLOY-21: from the kept stages, the cells and the cell table.
            crate::filter::Event::Stage(stage, env) => self.holds_stage(*stage, env),
            // DEPLOY-25: from the pending requests and the kept cells.
            crate::filter::Event::Requested(env) => self.holds_requested(env),
            // MENGER-39: `queued` from the queue's kept entries.
            crate::filter::Event::Queued(u) => self.queue.holds(*u, &self.at),
            // CHAT-3: for the reader, from the chats.
            crate::filter::Event::Chatting => super::chats::holds(self, &clock.me, false),
            crate::filter::Event::Unread => super::chats::holds(self, &clock.me, true),
            _ => self.log.holds(h, clock),
        }
    }

    /// …and when time alone next moves what it holds for.
    pub(super) fn next_boundary(&self, h: &crate::filter::History, clock: &Clock) -> Option<i64> {
        match &h.event {
            crate::filter::Event::Deployed(env) => self.deploys.next_boundary(env, h.window.as_ref(), clock),
            // MENGER-39 / DEPLOY-20: time alone never moves the queue, or
            // which version runs.
            crate::filter::Event::Queued(_)
            | crate::filter::Event::Behind(_)
            | crate::filter::Event::Stage(..)
            | crate::filter::Event::Requested(_)
            | crate::filter::Event::Chatting
            | crate::filter::Event::Unread => None,
            _ => self.log.next_boundary(h, clock),
        }
    }

    /// DEPLOY-21: the tickets `<stage> <env>` holds for, per shortcode — in
    /// each environment `env` names. Deploying and failed from the kept
    /// stages; ready for from the kept cells of the environment it promotes
    /// from; waiting from the cell table (Complete, with commits, in the
    /// project or a shortcode hosted under it) less every ticket running its
    /// current version on the track, deploying or failed there. Never a scan
    /// of tickets.
    pub(super) fn holds_stage(&self, stage: DeliveryStage, env: &str) -> BTreeMap<ShortcodeId, RoaringBitmap> {
        let mut out: BTreeMap<ShortcodeId, RoaringBitmap> = BTreeMap::new();
        let none = Kept::default();
        for (p, n) in self.deploys.env_keys() {
            if !Deploys::names(env, &p, &n) {
                continue;
            }
            let k = self.deploys.envs.get(&(p.clone(), n.clone())).unwrap_or(&none);
            let ids: Vec<&str> = match stage {
                DeliveryStage::Deploying => k.deploying.iter().map(String::as_str).collect(),
                DeliveryStage::Failed => k.failed.iter().map(String::as_str).collect(),
                DeliveryStage::ReadyFor => self.deploys.ready_for(&p, &n, k),
                DeliveryStage::Waiting => {
                    self.waiting_in(&p, &n, k, &mut out);
                    continue;
                }
            };
            for key in ids.into_iter().filter_map(|t| self.at.get(t)) {
                out.entry(key.shortcode.clone()).or_default().insert(key.seq);
            }
        }
        out
    }

    /// DEPLOY-21: `waiting` in `(project, name)`, added to `out`.
    fn waiting_in(&self, project: &str, name: &str, k: &Kept, out: &mut BTreeMap<ShortcodeId, RoaringBitmap>) {
        let mut gone: BTreeMap<&ShortcodeId, RoaringBitmap> = BTreeMap::new();
        let track = self.deploys.track(project, name);
        let running = track.iter().filter_map(|e| self.deploys.envs.get(&(project.to_string(), e.clone()))).flat_map(Kept::currents);
        for key in running.chain(k.deploying.iter().chain(&k.failed).map(String::as_str)).filter_map(|t| self.at.get(t)) {
            gone.entry(&key.shortcode).or_default().insert(key.seq);
        }
        let home = ShortcodeId::new(project);
        let keys = self.cells.keys();
        for (sc, held) in self.cells.shortcodes() {
            if *sc != home && !lineage::host_chain(self, sc).contains(&home) {
                continue;
            }
            let mut acc = RoaringBitmap::new();
            for (cell, bm) in held {
                if keys.get(*cell as usize).is_some_and(|c| c.commits() && c.status() == super::Status::Complete) {
                    acc |= bm;
                }
            }
            if let Some(g) = gone.get(sc) {
                acc -= g;
            }
            if !acc.is_empty() {
                *out.entry(sc.clone()).or_default() |= acc;
            }
        }
    }

    /// Whether the store holds a ticket of `epic` — ORG-3: a project names a
    /// shortcode by its identity (its code, for every one before ORG-3).
    fn holds_shortcode(&self, epic: &str) -> bool {
        let sc = ShortcodeId::new(epic);
        self.order.range(OrderKey { shortcode: sc.clone(), seq: 0 }..).next().is_some_and(|(k, _)| k.shortcode == sc)
    }

    /// `ids` that name tickets, each once, in the store's order.
    fn in_order<'a>(&self, ids: impl IntoIterator<Item = &'a str>) -> Vec<String> {
        let mut keys: Vec<&OrderKey> = ids.into_iter().filter_map(|t| self.at.get(t)).collect();
        keys.sort();
        keys.dedup();
        keys.into_iter().filter_map(|k| self.order.get(k)).map(|r| r.id.to_string()).collect()
    }

    /// What `d` carried, and the commits of it no ticket links, and whether
    /// it still counts in its environment.
    fn carried(&self, d: &Deployment) -> (Vec<String>, Vec<String>, bool) {
        // DEPLOY-25: a request carries what it would, while it is pending.
        if d.state == DeployState::Requested {
            return (self.request_carries(d), Vec::new(), false);
        }
        let links = &self.deploys.links;
        let mut tickets: Vec<&str> = Vec::new();
        let mut unlinked: Vec<String> = Vec::new();
        let mut seen: HashSet<&str> = HashSet::new();
        for c in d.commits.iter().chain(std::iter::once(&d.revision)) {
            if !seen.insert(hash_of(c)) {
                continue;
            }
            let before = tickets.len();
            tickets.extend(links.tickets(c).filter(|t| self.at.contains_key(*t)));
            if tickets.len() == before {
                unlinked.push(c.clone());
            }
        }
        tickets.extend(d.tickets.iter().map(String::as_str));
        let live = d.state == DeployState::Succeeded
            && self.deploys.envs.get(&(d.project.clone(), d.environment.clone())).is_some_and(|k| k.stack.contains(&d.id));
        (self.in_order(tickets), unlinked, live)
    }

    /// `d` as `list` says it.
    fn deployment_event(&self, d: &Deployment) -> DeploymentsEvent {
        let (carried, unlinked, live) = self.carried(d);
        DeploymentsEvent::Deployment { deployment: d.clone(), carried, unlinked, live }
    }

    /// The environment `(project, name)` as it stands.
    fn environment(&self, project: &str, name: &str) -> Option<Environment> {
        let k = self.deploys.envs.get(&(project.to_string(), name.to_string()))?;
        let mut names: Vec<&str> = self.deploys.envs.keys().filter(|(p, _)| p == project).map(|(_, n)| n.as_str()).collect();
        names.sort_by_key(|n| (rank(n), n.to_string()));
        let last = k.last.and_then(|id| self.deploys.by_id.get(&id));
        Some(Environment {
            project: project.to_string(),
            name: name.to_string(),
            order: names.iter().position(|n| *n == name).unwrap_or(0) as u32,
            revision: k.revision.clone(),
            release: k.release.clone(),
            since: k.since,
            deployment: k.deployment,
            last: k.last,
            last_state: last.map(|d| d.state),
            last_at: last.map(|d| d.at),
            tickets: k.running.keys().filter(|t| self.at.contains_key(*t)).count() as u64,
            from: self.deploys.sources.get(&(project.to_string(), name.to_string())).cloned(),
        })
    }

    /// Every environment of `project` (all, when `None`), by project, then
    /// in display order.
    fn environments_of(&self, project: Option<&str>) -> Vec<Environment> {
        let mut out: Vec<Environment> = self
            .deploys
            .envs
            .keys()
            .filter(|(p, _)| project.is_none_or(|x| x == p))
            .filter_map(|(p, n)| self.environment(p, n))
            .collect();
        out.sort_by(|a, b| (&a.project, a.order).cmp(&(&b.project, b.order)));
        out
    }
}

// ---------------------------------------------------------------------
// A record, read

/// A record read: its deployment, but for the id and who.
fn read(rec: Record, now: i64) -> Result<Deployment, StoreError> {
    let bad = |m: String| StoreError::Invalid(format!("{m} (nothing was recorded)"));
    let project = project_of(&rec.project).map_err(bad)?;
    let environment = environment_of(&rec.environment).map_err(bad)?;
    let state: DeployState = rec.state.parse().map_err(bad)?;
    if state == DeployState::Requested {
        return Err(bad("a CI records started, succeeded, failed or rolled_back — a request is made with `deployments request`".into()));
    }
    let reference = |what: &str, c: &str| {
        super::commits::reference(c.trim())
            .ok_or_else(|| bad(format!("{what} `{c}` is not a commit reference: 7–64 hex characters, `repo@` before them outside the project's repository")))
    };
    let revision = reference("revision", &rec.revision)?;
    let previous = rec.previous.as_deref().map(str::trim).filter(|p| !p.is_empty()).map(|p| reference("previous", p)).transpose()?;
    if rec.commits.len() > COMMITS_MAX {
        return Err(bad(format!("a record lists at most {COMMITS_MAX} commits; this one lists {}", rec.commits.len())));
    }
    let (mut commits, mut wrong) = (Vec::with_capacity(rec.commits.len()), Vec::new());
    for c in &rec.commits {
        match super::commits::reference(c.trim()) {
            Some(r) if !commits.contains(&r) => commits.push(r),
            Some(_) => {}
            None => wrong.push(format!("`{c}`")),
        }
    }
    if !wrong.is_empty() {
        return Err(bad(format!("not commit references: {}", wrong.join(", "))));
    }
    if rec.tickets.len() > TICKETS_MAX {
        return Err(bad(format!("a record names at most {TICKETS_MAX} tickets; this one names {}", rec.tickets.len())));
    }
    let mut tickets: Vec<String> = Vec::new();
    for t in &rec.tickets {
        let t = t.trim().to_ascii_uppercase();
        let ok = t.rsplit_once('-').is_some_and(|(c, n)| project_of(c).is_ok() && !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()));
        if !ok {
            return Err(bad(format!("`{t}` is not a ticket id (e.g. MENGER-34)")));
        }
        if !tickets.contains(&t) {
            tickets.push(t);
        }
    }
    let text = |what: &str, v: Option<String>, max: usize| -> Result<Option<String>, StoreError> {
        match v.map(|v| v.trim().to_string()).filter(|v| !v.is_empty()) {
            Some(v) if v.len() > max => Err(bad(format!("{what} is at most {max} bytes; this one is {}", v.len()))),
            v => Ok(v),
        }
    };
    let release = text("release", rec.release, 256)?;
    let src = rec.source.unwrap_or_default();
    let source = DeploySource { system: text("source.system", src.system, 128)?, run: text("source.run", src.run, 256)?, url: text("source.url", src.url, 2048)? };
    let at = match rec.at.as_deref().map(str::trim).filter(|a| !a.is_empty()) {
        None => now,
        Some(a) if a.bytes().all(|b| b.is_ascii_digit()) => a.parse().map_err(|_| bad(format!("`{a}` is not a moment")))?,
        Some(a) => chrono::DateTime::parse_from_rfc3339(a)
            .map(|t| t.timestamp())
            .map_err(|_| bad(format!("at `{a}` is not RFC 3339 (2026-10-12T22:04:19Z) or unix seconds")))?,
    };
    Ok(Deployment {
        id: 0,
        project,
        environment,
        state,
        revision,
        previous,
        commits,
        tickets,
        release,
        at,
        source,
        recorded_at: now,
        recorded_by: String::new(),
        request: rec.request,
        requested: None,
    })
}

/// What `list` asks: every filter given must hold.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DeployQuery {
    pub project: Option<String>,
    pub environment: Option<String>,
    pub state: Option<DeployState>,
    /// Only the deployments that carried this ticket.
    pub ticket: Option<String>,
}

/// One page of `list`: each deployment's event, newest first, and the
/// cursor to pass as the next page's `after`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeployPage {
    pub rows: Vec<DeploymentsEvent>,
    pub next: Option<u64>,
}

impl Store {
    /// DEPLOY-11: keeps a deployment — `rec` read (a project that is a
    /// shortcode the store holds, an environment name, a state, commit
    /// references, ticket ids; each refusal named, nothing kept) and, unless
    /// the same record is kept already (then that one is the answer,
    /// `duplicate`, and nothing is written), committed with its commits and
    /// its environment as the fold leaves it, in one transaction; then the
    /// cache; then `deployed` and `environment_changed` on
    /// `deployments.watch`, and on `tickets.watch { filter }` each ticket a
    /// `deployed` filter admits or no longer admits because of it. `by` is
    /// who sent it.
    ///
    /// ACCESS-11: `allowed` — the system, or a deployer (or admin) on
    /// exactly the project's shortcode, checked under the write-serial lock.
    pub async fn record_deployment(&self, allowed: impl Into<Authority>, rec: Record, by: String) -> Result<DeploymentsEvent, StoreError> {
        let allowed = allowed.into();
        let now = chrono::Utc::now().timestamp();
        // DEPLOY-21: where the environment promotes from, when the record says.
        let from = rec.from.as_deref().map(str::trim).filter(|f| !f.is_empty()).map(environment_of).transpose();
        let from = from.map_err(|m| StoreError::Invalid(format!("from: {m} (nothing was recorded)")))?;
        let mut d = read(rec, now)?;
        d.recorded_by = by;
        let serial = self.serial().await;
        let (kept, entered, left, under, source) = {
            let cache = self.tickets.read().expect("store lock poisoned");
            if !cache.holds_shortcode(&d.project) {
                return Err(StoreError::Invalid(format!(
                    "no shortcode {}: a project is a shortcode menger holds (nothing was recorded)",
                    d.project
                )));
            }
            allowed.deployer_on(&super::writs::scope_of(&cache, &ShortcodeId::new(&d.project)), "a deployment needs deployer on its project")?;
            if let Some(kept) = cache.deploys.keys.get(&dedupe(&d)).and_then(|id| cache.deploys.by_id.get(id)) {
                let (carried, unlinked, live) = cache.carried(kept);
                let deployment = kept.clone();
                return Ok(DeploymentsEvent::Deployed { deployment, carried, unlinked, live, entered: Vec::new(), left: Vec::new(), duplicate: true });
            }
            // DEPLOY-25: the request it answers is a request to this environment.
            if let Some(r) = d.request {
                cache.deploys.answers(r, &d)?;
            }
            let at = (d.project.clone(), d.environment.clone());
            let source = from.filter(|f| cache.deploys.sources.get(&at) != Some(f));
            if let Some(why) = source.as_deref().and_then(|f| cache.deploys.source_refused(&d.project, &d.environment, f)) {
                return Err(StoreError::Invalid(why.replace("nothing was written", "nothing was recorded")));
            }
            d.id = cache.deploys.next_id();
            let kept = cache.deploys.refold(&d.project, &d.environment, Some(&d));
            let empty = BTreeMap::new();
            let (empty_behind, had) = (BTreeMap::new(), cache.deploys.envs.get(&at));
            let before = had.map_or(&empty, |k| &k.running);
            let behind = had.map_or(&empty_behind, |k| &k.behind);
            // DEPLOY-21: a ticket whose stage there moved is moved too — and,
            // when the record moves where it promotes from, every ticket a
            // stage of the project may name.
            let no = BTreeSet::new();
            let (deploying, failed) = (had.map_or(&no, |k| &k.deploying), had.map_or(&no, |k| &k.failed));
            let staged = source.as_ref().map(|_| cache.deploys.staged_in(&d.project)).unwrap_or_default();
            // DEPLOY-25: what a pending request in the project would carry moves with what runs.
            let requested = cache.requested_moves(&d.project, &kept);
            // DEPLOY-20: a ticket whose version there moved is moved too.
            let moved: Vec<&str> = before
                .iter()
                .filter(|(t, v)| kept.running.get(*t) != Some(*v))
                .map(|(t, _)| t.as_str())
                .chain(kept.running.iter().filter(|(t, v)| before.get(*t) != Some(*v)).map(|(t, _)| t.as_str()))
                .chain(behind.keys().chain(kept.behind.keys()).filter(|t| behind.get(*t) != kept.behind.get(*t)).map(String::as_str))
                .chain(deploying.symmetric_difference(&kept.deploying).map(String::as_str))
                .chain(failed.symmetric_difference(&kept.failed).map(String::as_str))
                .chain(staged)
                .chain(requested.iter().map(String::as_str))
                .collect();
            let entered = cache.in_order(kept.running.keys().map(String::as_str).filter(|t| !before.contains_key(*t)));
            let left = cache.in_order(before.keys().map(String::as_str).filter(|t| !kept.running.contains_key(*t)));
            let keys: Vec<(&ShortcodeId, u32)> = moved.iter().filter_map(|t| cache.at.get(*t)).map(|k| (&k.shortcode, k.seq)).collect();
            let under = lineage::deployed_before_with(&self.unders, &cache, &keys);
            (kept, entered, left, under, source)
        };
        let (stored, env, set) = (d.clone(), kept.clone(), source.clone());
        let tickets = Arc::clone(&self.tickets);
        self.write(
            serial,
            move |tx| {
                store(tx, &stored)?;
                store_environment(tx, &stored.project, &stored.environment, &env)?;
                // DEPLOY-25: a request picked up sends no more.
                if let Some(r) = stored.request {
                    requests::stop_delivery(tx, r)?;
                }
                if let Some(f) = &set {
                    store_source(tx, &stored.project, &stored.environment, Some(f), stored.recorded_at, &stored.recorded_by)?;
                }
                Ok(())
            },
            move |cache, _| {
                let (project, name) = (d.project.clone(), d.environment.clone());
                if let Some(f) = &source {
                    cache.deploys.sources.insert((project.clone(), name.clone()), f.clone());
                }
                let request = d.request;
                cache.deploys.install(d.clone(), kept);
                let (carried, unlinked, live) = cache.carried(&d);
                let deployed = DeploymentsEvent::Deployed { deployment: d, carried, unlinked, live, entered, left, duplicate: false };
                let promotes = source.map(|from| DeploymentsEvent::Promotes { project: project.clone(), environment: name.clone(), from: Some(from) });
                let request = request.and_then(|r| cache.request_event(r));
                (deployed, cache.environment(&project, &name), (promotes, request))
            },
            move |(), (deployed, environment, (promotes, request)), tx| {
                tx.deployment(deployed.clone());
                if let Some(p) = promotes {
                    tx.deployment(p);
                }
                if let Some(r) = request {
                    tx.deployment(r);
                }
                if let Some(environment) = environment {
                    tx.deployment(DeploymentsEvent::EnvironmentChanged { environment });
                }
                if let Some(change) = under.and_then(|u| u.after(&tickets)) {
                    tx.send(change);
                }
                deployed
            },
        )
        .await
    }

    /// DEPLOY-21: the environment `(project, environment)` promotes from
    /// `from` now — `None`: from none. A project is a shortcode the store
    /// holds; an environment need not have a deployment yet. Refused when it
    /// would promote from itself or close a cycle. Committed in its own
    /// transaction, then the cache, then `promotes` on `deployments.watch`
    /// and, on `tickets.watch { filter }`, each ticket a stage filter admits
    /// or no longer admits because of it. The same source again writes
    /// nothing and answers the same.
    ///
    /// ACCESS-11: `allowed` — the system, or admin on exactly the project's
    /// shortcode, checked under the write-serial lock.
    pub async fn set_promotes(&self, allowed: impl Into<Authority>, project: &str, environment: &str, from: Option<&str>, by: String) -> Result<DeploymentsEvent, StoreError> {
        let allowed = allowed.into();
        let bad = |m: String| StoreError::Invalid(format!("{m} (nothing was written)"));
        let project = project_of(project).map_err(bad)?;
        let environment = environment_of(environment).map_err(bad)?;
        let from = from.map(environment_of).transpose().map_err(bad)?;
        let answer = DeploymentsEvent::Promotes { project: project.clone(), environment: environment.clone(), from: from.clone() };
        let serial = self.serial().await;
        let key = (project.clone(), environment.clone());
        let under = {
            let cache = self.tickets.read().expect("store lock poisoned");
            if !cache.holds_shortcode(&project) {
                return Err(StoreError::Invalid(format!("no shortcode {project}: a project is a shortcode menger holds (nothing was written)")));
            }
            allowed.admin_on(&super::writs::scope_of(&cache, &ShortcodeId::new(&project)), "where an environment promotes from needs admin on its project")?;
            if cache.deploys.sources.get(&key) == from.as_ref() {
                return Ok(answer);
            }
            if let Some(why) = from.as_deref().and_then(|f| cache.deploys.source_refused(&project, &environment, f)) {
                return Err(StoreError::Invalid(why));
            }
            let keys: Vec<(&ShortcodeId, u32)> = cache.deploys.staged_in(&project).into_iter().filter_map(|t| cache.at.get(t)).map(|k| (&k.shortcode, k.seq)).collect();
            lineage::deployed_before_with(&self.unders, &cache, &keys)
        };
        let now = chrono::Utc::now().timestamp();
        let (stored, set) = (key.clone(), from.clone());
        let tickets = Arc::clone(&self.tickets);
        self.write(
            serial,
            move |tx| {
                store_source(tx, &stored.0, &stored.1, set.as_deref(), now, &by)?;
                Ok(())
            },
            move |cache, _| {
                match from {
                    Some(f) => cache.deploys.sources.insert(key, f),
                    None => cache.deploys.sources.remove(&key),
                };
            },
            move |(), (), tx| {
                tx.deployment(answer.clone());
                if let Some(change) = under.and_then(|u| u.after(&tickets)) {
                    tx.send(change);
                }
                answer
            },
        )
        .await
    }

    /// DEPLOY-11: one page of the deployments `q` admits — of the projects
    /// in `allowed` when given — newest first by `(at, id)`, strictly after
    /// the deployment `after` when given, at most `limit`. From the cache.
    pub fn deployments(&self, q: &DeployQuery, allowed: Option<&HashSet<String>>, after: Option<u64>, limit: Option<u32>) -> Result<DeployPage, StoreError> {
        let cache = self.tickets.read().expect("store lock poisoned");
        let deploys = &cache.deploys;
        let start = match after {
            None => None,
            Some(a) => Some(deploys.by_id.get(&a).map(|d| (d.at, d.id)).ok_or_else(|| StoreError::Invalid(format!("no deployment {a} to page after")))?),
        };
        let project = q.project.as_deref().map(project_of).transpose().map_err(StoreError::Invalid)?;
        let environment = q.environment.as_deref().map(environment_of).transpose().map_err(StoreError::Invalid)?;
        let ticket = q.ticket.as_deref().map(|t| t.trim().to_ascii_uppercase());
        let admits = |d: &Deployment| -> bool {
            allowed.is_none_or(|a| a.contains(&d.project))
                && project.as_ref().is_none_or(|p| *p == d.project)
                && environment.as_ref().is_none_or(|e| *e == d.environment)
                && q.state.is_none_or(|s| s == d.state)
                && ticket.as_ref().is_none_or(|t| cache.carried(d).0.contains(t))
        };
        let newest: Box<dyn Iterator<Item = &(i64, u64)>> = match start {
            Some(s) => Box::new(deploys.by_time.range(..s).rev()),
            None => Box::new(deploys.by_time.iter().rev()),
        };
        let take = limit.map_or(usize::MAX, |n| n as usize);
        let mut matches = newest.map(|(_, id)| &deploys.by_id[id]).filter(|d| admits(d));
        let page: Vec<&Deployment> = matches.by_ref().take(take).collect();
        let more = limit.is_some() && matches.next().is_some();
        let next = more.then(|| page.last().map(|d| d.id).or(after)).flatten();
        Ok(DeployPage { rows: page.into_iter().map(|d| cache.deployment_event(d)).collect(), next })
    }

    /// DEPLOY-11: every environment of `project` (every project's when
    /// `None`), of the projects in `allowed` when given — what each runs,
    /// since when, its last deployment, how many tickets run there.
    pub fn environments(&self, project: Option<&str>, allowed: Option<&HashSet<String>>) -> Result<Vec<Environment>, StoreError> {
        let project = project.map(str::trim).filter(|p| !p.is_empty()).map(project_of).transpose().map_err(StoreError::Invalid)?;
        let cache = self.tickets.read().expect("store lock poisoned");
        let mut out = cache.environments_of(project.as_deref());
        out.retain(|e| allowed.is_none_or(|a| a.contains(&e.project)));
        Ok(out)
    }

    /// DEPLOY-11: the projects deployments have been recorded for.
    pub fn deploy_projects(&self) -> Vec<String> {
        let cache = self.tickets.read().expect("store lock poisoned");
        let mut projects: BTreeSet<String> = cache.deploys.envs.keys().map(|(p, _)| p.clone()).collect();
        // DEPLOY-25: and those with only a request so far.
        projects.extend(cache.deploys.request_projects());
        projects.into_iter().collect()
    }

    /// DEPLOY-11: where the ticket `id` runs — `running` for each
    /// environment it runs in (since when, brought by which deployment) and
    /// `not_running` for each other environment of its projects (its
    /// shortcode, the shortcodes hosting it, and any it has run in), of the
    /// projects in `allowed` when given; by project, then in display order.
    pub fn for_ticket(&self, id: &str, allowed: Option<&HashSet<String>>) -> Result<Vec<DeploymentsEvent>, StoreError> {
        let cache = self.tickets.read().expect("store lock poisoned");
        let row = cache.get(id.trim()).ok_or_else(|| StoreError::NotFound(id.trim().to_string()))?;
        let mut projects: BTreeSet<String> = BTreeSet::from([row.shortcode.as_str().to_string()]);
        projects.extend(lineage::host_chain(&cache, &row.shortcode).iter().map(|h| h.as_str().to_string()));
        for ((p, _), k) in &cache.deploys.envs {
            if k.running.contains_key(row.id.as_str()) {
                projects.insert(p.clone());
            }
        }
        let mut out = Vec::new();
        for env in cache.environments_of(None) {
            if !projects.contains(&env.project) || allowed.is_some_and(|a| !a.contains(&env.project)) {
                continue;
            }
            let k = &cache.deploys.envs[&(env.project.clone(), env.name.clone())];
            let behind = k.behind.get(row.id.as_str()).cloned();
            out.push(match k.running.get(row.id.as_str()) {
                Some((since, deployment)) => DeploymentsEvent::Running {
                    ticket: row.id.to_string(),
                    project: env.project,
                    environment: env.name,
                    since: *since,
                    deployment: *deployment,
                    revision: env.revision,
                    release: env.release,
                    behind: behind.as_ref().map(|b| b.not.clone()).unwrap_or_default(),
                    version: behind.and_then(|b| b.version),
                },
                None => DeploymentsEvent::NotRunning { ticket: row.id.to_string(), project: env.project, environment: env.name },
            });
        }
        // DEPLOY-25: and each pending request that would carry it.
        out.extend(cache.requested_for(row.id.as_str(), allowed));
        Ok(out)
    }

    /// DEPLOY-13: the kept cells — one `cells` per environment (of
    /// `project`, named `environment` when given; of the projects in
    /// `allowed` when given), by project then in display order, each with
    /// every ticket running there — only `ids`' when given (at most
    /// [`RUNNING_IDS_MAX`]) — in the store's order. What the app's board
    /// marks are drawn from: asked once, then per environment a deploy moves.
    pub fn running(
        &self,
        project: Option<&str>,
        environment: Option<&str>,
        ids: Option<&[String]>,
        allowed: Option<&HashSet<String>>,
    ) -> Result<Vec<DeploymentsEvent>, StoreError> {
        let blank = |v: Option<&str>| v.map(str::trim).filter(|v| !v.is_empty()).map(str::to_string);
        let project = blank(project).as_deref().map(project_of).transpose().map_err(StoreError::Invalid)?;
        let environment = blank(environment).as_deref().map(environment_of).transpose().map_err(StoreError::Invalid)?;
        if ids.is_some_and(|i| i.len() > RUNNING_IDS_MAX) {
            return Err(StoreError::Invalid(format!("`running` takes at most {RUNNING_IDS_MAX} ids")));
        }
        let only: Option<HashSet<String>> = ids.map(|i| i.iter().map(|t| t.trim().to_ascii_uppercase()).collect());
        let cache = self.tickets.read().expect("store lock poisoned");
        let mut out = Vec::new();
        for env in cache.environments_of(project.as_deref()) {
            if allowed.is_some_and(|a| !a.contains(&env.project)) || environment.as_ref().is_some_and(|e| *e != env.name) {
                continue;
            }
            let k = &cache.deploys.envs[&(env.project.clone(), env.name.clone())];
            let mut cells: Vec<(&OrderKey, RunningCell)> = k
                .running
                .iter()
                .filter(|(t, _)| only.as_ref().is_none_or(|o| o.contains(*t)))
                .filter_map(|(t, (since, deployment))| {
                    let b = k.behind.get(t);
                    let cell = RunningCell {
                        ticket: t.clone(),
                        since: *since,
                        deployment: *deployment,
                        behind: b.map(|b| b.not.clone()).unwrap_or_default(),
                        version: b.and_then(|b| b.version.clone()),
                    };
                    cache.at.get(t).map(|key| (key, cell))
                })
                .collect();
            cells.sort_by(|a, b| a.0.cmp(b.0));
            // DEPLOY-21: and each ticket deploying, ready for or failed there.
            let ready = cache.deploys.ready_for(&env.project, &env.name, k);
            let mut staged: Vec<(&OrderKey, StagedCell)> = [(DeliveryStage::Deploying, k.deploying.iter().map(String::as_str).collect::<Vec<_>>()), (DeliveryStage::Failed, k.failed.iter().map(String::as_str).collect()), (DeliveryStage::ReadyFor, ready)]
                .into_iter()
                .flat_map(|(stage, ids)| ids.into_iter().map(move |t| (stage, t)))
                .filter(|(_, t)| only.as_ref().is_none_or(|o| o.contains(*t)))
                .filter_map(|(stage, t)| cache.at.get(t).map(|key| (key, StagedCell { ticket: t.to_string(), stage })))
                .collect();
            staged.sort_by(|a, b| a.0.cmp(b.0));
            out.push(DeploymentsEvent::Cells {
                project: env.project,
                environment: env.name,
                running: cells.into_iter().map(|(_, c)| c).collect(),
                staged: staged.into_iter().map(|(_, c)| c).collect(),
            });
        }
        Ok(out)
    }

    /// DEPLOY-11: `deployments.watch`'s channel — every deployment recorded
    /// and the environment it left, in the order the writes were
    /// acknowledged.
    pub fn subscribe_deployments(&self) -> tokio::sync::broadcast::Receiver<DeploymentsEvent> {
        self.tx.deployments.subscribe()
    }
}

// ---------------------------------------------------------------------
// The tables

/// Writes `d` and its commits inside the caller's transaction.
fn store(conn: &Connection, d: &Deployment) -> rusqlite::Result<()> {
    conn.prepare_cached(
        "INSERT INTO deployments (id, project, environment, state, revision, previous, tickets, release, at_unix, system, run, url,
                                  recorded_at, recorded_by, dedupe, request)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)",
    )?
    .execute(params![
        d.id as i64,
        d.project,
        d.environment,
        d.state.as_str(),
        d.revision,
        d.previous,
        d.tickets.join("\n"),
        d.release,
        d.at,
        d.source.system,
        d.source.run,
        d.source.url,
        d.recorded_at,
        d.recorded_by,
        dedupe(d),
        d.request.map(|r| r as i64),
    ])?;
    let mut insert = conn.prepare_cached("INSERT INTO deployment_commits (deployment, pos, sha) VALUES (?1, ?2, ?3)")?;
    for (pos, c) in d.commits.iter().enumerate() {
        insert.execute(params![d.id as i64, pos as i64, c])?;
    }
    Ok(())
}

/// Writes what `(project, name)` runs, as `k` says, inside the caller's
/// transaction.
fn store_environment(conn: &Connection, project: &str, name: &str, k: &Kept) -> rusqlite::Result<()> {
    conn.prepare_cached(
        "INSERT INTO deploy_environments (project, name, revision, since, release, deployment, last) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT (project, name) DO UPDATE SET revision = excluded.revision, since = excluded.since,
                 release = excluded.release, deployment = excluded.deployment, last = excluded.last",
    )?
    .execute(params![project, name, k.revision, k.since, k.release, k.deployment.map(|d| d as i64), k.last.map(|d| d as i64)])?;
    Ok(())
}

/// DEPLOY-21: `(project, name)` promotes from `from` (`None`: from none),
/// inside the caller's transaction.
fn store_source(conn: &Connection, project: &str, name: &str, from: Option<&str>, at: i64, by: &str) -> rusqlite::Result<()> {
    match from {
        Some(f) => conn
            .prepare_cached(
                "INSERT INTO deploy_sources (project, name, source, set_at, set_by) VALUES (?1, ?2, ?3, ?4, ?5)
                     ON CONFLICT (project, name) DO UPDATE SET source = excluded.source, set_at = excluded.set_at, set_by = excluded.set_by",
            )?
            .execute(params![project, name, f, at, by])?,
        None => conn.prepare_cached("DELETE FROM deploy_sources WHERE project = ?1 AND name = ?2")?.execute(params![project, name])?,
    };
    Ok(())
}

/// DEPLOY-21: every environment's source, as stored.
fn read_sources(conn: &Connection) -> rusqlite::Result<BTreeMap<(String, String), String>> {
    let mut stmt = conn.prepare("SELECT project, name, source FROM deploy_sources")?;
    let rows = stmt.query_map([], |r| Ok(((r.get(0)?, r.get(1)?), r.get(2)?)))?;
    rows.collect()
}

/// Every deployment as stored, its commits in order.
fn read_all(conn: &Connection) -> anyhow::Result<Vec<Deployment>> {
    let mut commits: HashMap<u64, Vec<String>> = HashMap::new();
    {
        let mut stmt = conn.prepare("SELECT deployment, sha FROM deployment_commits ORDER BY deployment, pos")?;
        let mut rows = stmt.query([])?;
        while let Some(r) = rows.next()? {
            commits.entry(r.get::<_, i64>(0)? as u64).or_default().push(r.get(1)?);
        }
    }
    // DEPLOY-25: a file `--verify-derived` reads unmigrated has no request column.
    let request = if has_request_column(conn)? { "request" } else { "NULL" };
    let mut stmt = conn.prepare(&format!(
        "SELECT id, project, environment, state, revision, previous, tickets, release, at_unix, system, run, url, recorded_at, recorded_by, {request}
             FROM deployments ORDER BY id"
    ))?;
    let mut rows = stmt.query([])?;
    let mut out = Vec::new();
    while let Some(r) = rows.next()? {
        let id = r.get::<_, i64>(0)? as u64;
        let state: String = r.get(3)?;
        let tickets: String = r.get(6)?;
        out.push(Deployment {
            id,
            project: r.get(1)?,
            environment: r.get(2)?,
            state: state.parse().map_err(|e: String| anyhow::anyhow!("deployment {id}: {e}"))?,
            revision: r.get(4)?,
            previous: r.get(5)?,
            commits: commits.remove(&id).unwrap_or_default(),
            tickets: tickets.lines().filter(|t| !t.is_empty()).map(str::to_string).collect(),
            release: r.get(7)?,
            at: r.get(8)?,
            source: DeploySource { system: r.get(9)?, run: r.get(10)?, url: r.get(11)? },
            recorded_at: r.get(12)?,
            recorded_by: r.get(13)?,
            request: r.get::<_, Option<i64>>(14)?.map(|r| r as u64),
            requested: None,
        });
    }
    Ok(out)
}

impl Deploys {
    /// Every deployment as stored, kept — at open, over the links already
    /// made from the rows.
    pub(super) fn load(&mut self, conn: &Connection) -> anyhow::Result<()> {
        let started = std::time::Instant::now();
        // DEPLOY-20: the link record first, so the cells join with it.
        self.links.record = versions::load(conn)?;
        // DEPLOY-21: and where each environment promotes from.
        self.sources = read_sources(conn)?;
        let mut records = read_all(conn)?;
        // DEPLOY-25: each request's note, delivery and attempts, and the webhooks.
        requests::load(conn, &mut records, &mut self.webhooks)?;
        self.load_records(records);
        tracing::info!(
            "DEPLOY-11: read {} deployment(s) to {} environment(s) and kept their cells in {:.1} ms",
            self.by_id.len(),
            self.envs.len(),
            started.elapsed().as_secs_f64() * 1000.0
        );
        Ok(())
    }
}

/// What the stored environments say: `(project, name)` → (revision, since,
/// release, deployment, last).
type EnvRow = (Option<String>, Option<i64>, Option<String>, Option<u64>, Option<u64>);

fn read_environments(conn: &Connection) -> rusqlite::Result<BTreeMap<(String, String), EnvRow>> {
    let mut stmt = conn.prepare("SELECT project, name, revision, since, release, deployment, last FROM deploy_environments")?;
    let rows = stmt.query_map([], |r| {
        Ok((
            (r.get(0)?, r.get(1)?),
            (r.get(2)?, r.get(3)?, r.get(4)?, r.get::<_, Option<i64>>(5)?.map(|d| d as u64), r.get::<_, Option<i64>>(6)?.map(|d| d as u64)),
        ))
    })?;
    rows.collect()
}

/// A ticket's cell in each environment by a scan: every ticket, every
/// environment, tested against the stack's deployments one by one — the
/// reference the kept cells are held to.
pub(super) fn scanned_cells(d: &Deploys, rows: &[&Row]) -> BTreeMap<(String, String), BTreeMap<String, (i64, u64)>> {
    let mut out = BTreeMap::new();
    for (env, k) in &d.envs {
        let mut cells: BTreeMap<String, (i64, u64)> = BTreeMap::new();
        for r in rows {
            let mut best: Option<(i64, u64)> = None;
            for id in &k.stack {
                let dep = &d.by_id[id];
                let carries = dep.tickets.contains(&r.id.to_string())
                    || r.commits.iter().any(|c| dep.commits.iter().chain(std::iter::once(&dep.revision)).any(|x| same(c, x)));
                if carries {
                    best = Some(earlier(best, (dep.at, dep.id)));
                }
            }
            if let Some(b) = best {
                cells.insert(r.id.to_string(), b);
            }
        }
        out.insert(env.clone(), cells);
    }
    out
}

/// DEPLOY-20: each environment's behind by a scan — every running ticket
/// tested against the stack's deployments one by one, as
/// [`scanned_cells`] tests where it runs.
pub(super) fn scanned_behind(d: &Deploys, cells: &BTreeMap<(String, String), BTreeMap<String, (i64, u64)>>) -> BTreeMap<(String, String), BTreeMap<String, Behind>> {
    let mut out = BTreeMap::new();
    for (env, k) in &d.envs {
        let stack: Vec<&Deployment> = k.stack.iter().map(|id| &d.by_id[id]).collect();
        let carries = |c: &str| stack.iter().any(|dep| dep.commits.iter().chain(std::iter::once(&dep.revision)).any(|x| same(c, x)));
        let mut behind = BTreeMap::new();
        for t in cells.get(env).into_iter().flat_map(BTreeMap::keys) {
            let named = stack.iter().filter(|dep| dep.tickets.contains(t)).map(|dep| dep.at).max();
            if let Some(b) = d.links.behind(t, carries, named) {
                behind.insert(t.clone(), b);
            }
        }
        out.insert(env.clone(), behind);
    }
    out
}

/// DEPLOY-11: `--verify-derived`'s look at the deployments, against
/// `cache` (a scan of the same snapshot's tickets): `None` when the file has
/// no deployments table yet; else how many deployments it holds and each
/// place it disagrees — a stored environment that is not what its records
/// fold to, a deployment its key does not single out, and a kept cell —
/// built as a running server builds it, the deployments folded first and
/// each ticket's links moved after — that is not what a scan of every
/// ticket against every deployment gives.
pub(super) fn check(conn: &Connection, cache: &Cache) -> anyhow::Result<Option<(usize, Vec<String>)>> {
    if !has_table(conn)? {
        return Ok(None);
    }
    let records = read_all(conn)?;
    let n = records.len();
    let mut out = Vec::new();
    let mut keys: HashMap<String, u64> = HashMap::new();
    for d in &records {
        if let Some(first) = keys.insert(dedupe(d), d.id) {
            out.push(format!("deployment {}: the same record as deployment {first}", d.id));
        }
        for c in d.commits.iter().chain(std::iter::once(&d.revision)) {
            if super::commits::reference(c).as_deref() != Some(c.as_str()) {
                out.push(format!("deployment {}: `{c}` is not a commit reference as stored", d.id));
            }
        }
    }
    // The cells as a server keeps them: folded first, every link after.
    let mut kept = Deploys::default();
    kept.links.record = versions::load(conn)?;
    kept.load_records(records);
    let rows: Vec<&Row> = cache.order.values().collect();
    for r in &rows {
        kept.relink(&r.id, &r.commits);
    }
    let stored = read_environments(conn)?;
    for ((p, e), k) in &kept.envs {
        let want: EnvRow = (k.revision.clone(), k.since, k.release.clone(), k.deployment, k.last);
        match stored.get(&(p.clone(), e.clone())) {
            Some(row) if *row == want => {}
            Some(row) => out.push(format!("environment {p}/{e}: stored as {row:?}; its deployments fold to {want:?}")),
            None => out.push(format!("environment {p}/{e}: not stored; its deployments fold to {want:?}")),
        }
    }
    for (p, e) in stored.keys().filter(|k| !kept.envs.contains_key(*k)) {
        out.push(format!("environment {p}/{e}: stored, with no deployment"));
    }
    let scanned = scanned_cells(&kept, &rows);
    for (env, k) in &kept.envs {
        let have: BTreeMap<&String, &(i64, u64)> = k.running.iter().filter(|(t, _)| cache.at.contains_key(*t)).collect();
        let want: BTreeMap<&String, &(i64, u64)> = scanned.get(env).map(|m| m.iter().collect()).unwrap_or_default();
        for t in have.keys().chain(want.keys()).collect::<BTreeSet<_>>() {
            if have.get(t) != want.get(t) {
                out.push(format!(
                    "deployed cell {t} in {}/{}: kept {:?}; a scan gives {:?}",
                    env.0,
                    env.1,
                    have.get(t),
                    want.get(t)
                ));
            }
        }
    }
    // DEPLOY-20: and each ticket's behind, kept against a scan.
    for (env, want) in scanned_behind(&kept, &scanned) {
        let have: BTreeMap<&String, &Behind> = kept.envs[&env].behind.iter().filter(|(t, _)| cache.at.contains_key(*t)).collect();
        for t in have.keys().copied().chain(want.keys()).collect::<BTreeSet<_>>() {
            if have.get(t).copied() != want.get(t) {
                out.push(format!("behind cell {t} in {}/{}: kept {:?}; a scan gives {:?}", env.0, env.1, have.get(t), want.get(t)));
            }
        }
    }
    // DEPLOY-21: and the stages kept link by link, against a fresh join of
    // the same records with every link made first — as a reopen keeps them.
    let mut fresh = Deploys::linked(rows.iter().copied());
    fresh.links.record = versions::load(conn)?;
    fresh.load_records(read_all(conn)?);
    for (env, k) in &kept.envs {
        let Some(f) = fresh.envs.get(env) else { continue };
        for (what, have, want) in [("deploying", &k.deploying, &f.deploying), ("failed", &k.failed, &f.failed)] {
            for t in have.symmetric_difference(want).filter(|t| cache.at.contains_key(*t)) {
                out.push(format!("{what} cell {t} in {}/{}: kept {}; a fresh join {}", env.0, env.1, have.contains(t), want.contains(t)));
            }
        }
    }
    out.sort();
    Ok(Some((n, out)))
}
