//! DEPLOY-11: the deployments vocabulary — [`Record`] (what a CI sends,
//! DEPLOY-10's shape), [`Deployment`] (a record as kept), [`DeployState`],
//! [`DeploySource`], [`Environment`] and [`DeploymentsEvent`] — and the
//! order environments are shown in ([`rank`]). Wasm-safe, like
//! [`crate::notes`]: the app reads it through the generated
//! `DeploymentsClient`.
//!
//! A **deployment** is one thing a CI did to one environment: it started,
//! succeeded, failed, or was rolled back. An **environment** is a project's
//! (DEPLOY-9, decided: `env-identity` = a project's environment) — the pair
//! (project, name), shown per project in the order test → staging → prod. A
//! **project** is a shortcode (`menger` is `MENGER`): the simplest mapping
//! that is sound, since a grant and a ticket's place are both shortcodes.
//!
//! Where a ticket runs is a fact beside its status, never in it (DEPLOY-9,
//! decided: `status` = a separate fact): a deployment moves no status.

use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

/// The most commits one record may list — a release's range, never a
/// repository's history.
pub const COMMITS_MAX: usize = 10_000;

/// The most ticket ids one record may name.
pub const TICKETS_MAX: usize = 1_000;

/// The longest environment name, in bytes.
pub const ENVIRONMENT_MAX: usize = 64;

/// What a deployment did.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DeployState {
    /// Under way: recorded and shown; nothing runs because of it yet.
    Started,
    /// Done: the environment runs `revision`, and what it carried.
    Succeeded,
    /// Recorded and shown; nothing runs because of it.
    Failed,
    /// The environment went back to `revision`: what only the undone
    /// deploys carried leaves it.
    RolledBack,
    /// DEPLOY-25: asked for from menger (`deployments request`), not yet
    /// done — kept, shown, and handed to the environment's CI by its
    /// webhook. Like `started`, it runs nothing and completes nothing; a
    /// CI's record names it (`request`) when it picks it up.
    Requested,
}

impl DeployState {
    pub const ALL: [DeployState; 5] = [DeployState::Started, DeployState::Succeeded, DeployState::Failed, DeployState::RolledBack, DeployState::Requested];

    /// What a CI records (`deployments record`): every state but
    /// `requested`, which only `deployments request` makes.
    pub const RECORDED: [DeployState; 4] = [DeployState::Started, DeployState::Succeeded, DeployState::Failed, DeployState::RolledBack];

    pub const fn as_str(self) -> &'static str {
        match self {
            DeployState::Started => "started",
            DeployState::Succeeded => "succeeded",
            DeployState::Failed => "failed",
            DeployState::RolledBack => "rolled_back",
            DeployState::Requested => "requested",
        }
    }

    /// Whether it changes what the environment runs.
    pub const fn effective(self) -> bool {
        matches!(self, DeployState::Succeeded | DeployState::RolledBack)
    }
}

impl fmt::Display for DeployState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for DeployState {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().replace('-', "_").as_str() {
            "started" => Ok(Self::Started),
            "succeeded" => Ok(Self::Succeeded),
            "failed" => Ok(Self::Failed),
            "rolled_back" => Ok(Self::RolledBack),
            "requested" => Ok(Self::Requested),
            other => Err(format!("unknown deployment state '{other}' (expected started, succeeded, failed, rolled_back or requested)")),
        }
    }
}

/// Where to read the run that made a deployment.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, Serialize, Deserialize, schemars::JsonSchema)]
pub struct DeploySource {
    /// The CI: `menger-pipeline`, `github-actions`, …
    #[serde(default)]
    pub system: Option<String>,
    /// Its run, as the CI names it — what a retry of the same record repeats.
    #[serde(default)]
    pub run: Option<String>,
    #[serde(default)]
    pub url: Option<String>,
}

/// What a CI sends (DEPLOY-10's record): every field as the CI says it.
/// `deployments.record` takes its fields as parameters.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Record {
    /// Whose environment: a shortcode, any case (`menger` is MENGER).
    pub project: String,
    /// Its name: `test`, `staging`, `prod`, `pr-412` … (kept lower case).
    pub environment: String,
    /// `started` · `succeeded` · `failed` · `rolled_back`.
    pub state: String,
    /// The commit now running (`repo@sha` outside the project's repository).
    pub revision: String,
    /// The commit that ran before, when known.
    #[serde(default)]
    pub previous: Option<String>,
    /// Every commit in `previous..revision`, when the CI can list them.
    #[serde(default)]
    pub commits: Vec<String>,
    /// Ticket ids the CI names itself, added to what the commits give.
    #[serde(default)]
    pub tickets: Vec<String>,
    /// The artifact's name.
    #[serde(default)]
    pub release: Option<String>,
    /// When: RFC 3339 (`2026-10-12T22:04:19Z`) or unix seconds; omitted, now.
    #[serde(default)]
    pub at: Option<String>,
    #[serde(default)]
    pub source: Option<DeploySource>,
    /// DEPLOY-21: the environment this one promotes from (`test`, for
    /// menger's `prod`) — set (or moved) by the record; omitted, left as it
    /// was.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from: Option<String>,
    /// DEPLOY-25: the request this record answers — the id of a
    /// `requested` deployment to the same environment, which then shows
    /// this record's outcome.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request: Option<u64>,
}

/// DEPLOY-25: where a request stands — `pending` until a CI's record names
/// it (then that record's state) or it is canceled.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RequestState {
    /// Kept; no CI has picked it up. It may be canceled.
    Pending,
    /// Withdrawn before a CI picked it up.
    Canceled,
    /// A CI's `started` names it.
    Started,
    /// A CI's `succeeded` names it.
    Succeeded,
    /// A CI's `failed` names it.
    Failed,
    /// A CI's `rolled_back` names it.
    RolledBack,
}

impl RequestState {
    pub const fn as_str(self) -> &'static str {
        match self {
            RequestState::Pending => "pending",
            RequestState::Canceled => "canceled",
            RequestState::Started => "started",
            RequestState::Succeeded => "succeeded",
            RequestState::Failed => "failed",
            RequestState::RolledBack => "rolled_back",
        }
    }

    /// The state a CI's record in `state` leaves its request in.
    pub const fn of(state: DeployState) -> Self {
        match state {
            DeployState::Started => RequestState::Started,
            DeployState::Succeeded => RequestState::Succeeded,
            DeployState::Failed => RequestState::Failed,
            DeployState::RolledBack => RequestState::RolledBack,
            DeployState::Requested => RequestState::Pending,
        }
    }

    /// Whether it is still open: pending, or under way (started).
    pub const fn open(self) -> bool {
        matches!(self, RequestState::Pending | RequestState::Started)
    }
}

/// DEPLOY-25: where a request's webhook delivery stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum WebhookState {
    /// No webhook is set for the environment: kept, shown as "no CI
    /// configured", never sent.
    None,
    /// Waiting for its first attempt.
    Pending,
    /// An attempt failed (a non-2xx answer, or none); another follows.
    Retrying,
    /// An attempt was answered 2xx.
    Delivered,
    /// Every attempt failed; no more are made.
    Failed,
    /// No more attempts: the request was canceled, or a CI picked it up,
    /// before one was answered 2xx.
    Stopped,
}

impl WebhookState {
    pub const fn as_str(self) -> &'static str {
        match self {
            WebhookState::None => "none",
            WebhookState::Pending => "pending",
            WebhookState::Retrying => "retrying",
            WebhookState::Delivered => "delivered",
            WebhookState::Failed => "failed",
            WebhookState::Stopped => "stopped",
        }
    }

    /// Whether another attempt is due.
    pub const fn due(self) -> bool {
        matches!(self, WebhookState::Pending | WebhookState::Retrying)
    }
}

impl FromStr for WebhookState {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        [WebhookState::None, WebhookState::Pending, WebhookState::Retrying, WebhookState::Delivered, WebhookState::Failed, WebhookState::Stopped]
            .into_iter()
            .find(|w| w.as_str() == s)
            .ok_or_else(|| format!("unknown webhook state '{s}'"))
    }
}

/// DEPLOY-25: one attempt to deliver a request's webhook.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct DeliveryAttempt {
    /// 1 for the first.
    pub attempt: u32,
    /// When it was made, unix seconds.
    pub at: i64,
    /// The receiver's HTTP status — `None` when nothing answered.
    #[serde(default)]
    pub status: Option<u16>,
    /// What went wrong, in a line (at most 200 bytes) — `None` on a 2xx.
    #[serde(default)]
    pub error: Option<String>,
}

/// DEPLOY-25: what a `requested` deployment carries beyond a record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct DeployRequest {
    /// Why, in the asker's words.
    #[serde(default)]
    pub note: Option<String>,
    /// The environment it takes its revision from, when it asked for "what
    /// the environment this one promotes from runs" (DEPLOY-21's `from`).
    #[serde(default)]
    pub from: Option<String>,
    pub state: RequestState,
    /// The latest CI record that names it.
    #[serde(default)]
    pub outcome: Option<u64>,
    #[serde(default)]
    pub canceled_at: Option<i64>,
    #[serde(default)]
    pub canceled_by: Option<String>,
    /// The webhook delivery's id (`X-Menger-Delivery`) — the same on every
    /// attempt; `None` when no webhook was set.
    #[serde(default)]
    pub delivery: Option<String>,
    pub webhook: WebhookState,
    /// Each attempt, oldest first.
    #[serde(default)]
    pub attempts: Vec<DeliveryAttempt>,
    /// When the next attempt is due, unix seconds — while `retrying`.
    #[serde(default)]
    pub next_attempt_at: Option<i64>,
}

/// DEPLOY-25: what a request's webhook POSTs, as JSON, signed —
/// `X-Menger-Signature: sha256=<hex HMAC-SHA256 of the body, keyed by the
/// secret>`, `X-Menger-Delivery: <id>`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct WebhookPayload {
    /// The request's id — what the CI's records name as `request`.
    pub request: u64,
    pub project: String,
    pub environment: String,
    pub revision: String,
    pub release: Option<String>,
    /// The environment the revision was taken from, when it was.
    pub from: Option<String>,
    pub requested_by: String,
    pub note: Option<String>,
    /// When it was asked for, RFC 3339.
    pub at: String,
}

/// DEPLOY-25: an environment's webhook as menger says it — never its
/// secret, only whether one is set.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct WebhookConfig {
    pub project: String,
    pub environment: String,
    /// `None`: no webhook — a request is kept and shown as "no CI configured".
    pub url: Option<String>,
    pub secret_set: bool,
    pub set_at: Option<i64>,
    pub set_by: Option<String>,
}

/// DEPLOY-21: a ticket's delivery stage in an environment, short of running
/// its current version there (`deployed <env>`, and not `behind`) — each a
/// filter cell, `<stage> <env>`; a ticket is in at most one of them per
/// environment, the first that holds of: deploying, failed, ready for,
/// waiting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DeliveryStage {
    /// Complete, its current version running nowhere on the environment's
    /// track (it, and the environments it promotes from and to).
    Waiting,
    /// A `started` record to the environment carries its current version,
    /// and no record has ended that deploy yet.
    Deploying,
    /// Its current version runs on the environment this one promotes from,
    /// not here (absent, or behind).
    ReadyFor,
    /// The last record that carried its current version here failed or was
    /// rolled back, and that version does not run here.
    Failed,
}

impl DeliveryStage {
    /// In the order a ticket moves through them.
    pub const ALL: [DeliveryStage; 4] = [DeliveryStage::Waiting, DeliveryStage::Deploying, DeliveryStage::ReadyFor, DeliveryStage::Failed];

    /// The filter cell's name: `waiting`, `deploying`, `ready for`, `failed`.
    pub const fn as_str(self) -> &'static str {
        match self {
            DeliveryStage::Waiting => "waiting",
            DeliveryStage::Deploying => "deploying",
            DeliveryStage::ReadyFor => "ready for",
            DeliveryStage::Failed => "failed",
        }
    }

    /// The cell named `name` (`ready for`, any case).
    pub fn of(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|s| s.as_str().eq_ignore_ascii_case(name))
    }
}

impl fmt::Display for DeliveryStage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// DEPLOY-21: one ticket's stage in an environment, as `running` answers it
/// beside the running cells — deploying, ready for or failed (waiting is
/// every other Complete ticket, asked with the filter cell).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct StagedCell {
    pub ticket: String,
    pub stage: DeliveryStage,
}

/// A deployment as menger keeps it: the record, read, with its id, `at` as
/// unix seconds, and who recorded it when.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Deployment {
    pub id: u64,
    /// The project's shortcode, upper case.
    pub project: String,
    /// The environment's name, lower case.
    pub environment: String,
    pub state: DeployState,
    pub revision: String,
    pub previous: Option<String>,
    /// As sent, each a commit reference (lower-case hash).
    pub commits: Vec<String>,
    /// The ids the record named, upper case — counted when they name a ticket.
    pub tickets: Vec<String>,
    pub release: Option<String>,
    /// When it happened, unix seconds — the record's `at`, else when it came.
    pub at: i64,
    pub source: DeploySource,
    /// When menger kept it, unix seconds.
    pub recorded_at: i64,
    /// Who sent it: a principal, `anonymous` with login off.
    pub recorded_by: String,
    /// DEPLOY-25: the request a CI's record answers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request: Option<u64>,
    /// DEPLOY-25: a `requested` deployment's note, source, state and
    /// webhook delivery.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub requested: Option<DeployRequest>,
}

/// An environment as it stands.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Environment {
    pub project: String,
    pub name: String,
    /// Its place among its project's environments ([`rank`]): 0 first.
    pub order: u32,
    /// What it runs — `None` until a deploy to it succeeds.
    pub revision: Option<String>,
    pub release: Option<String>,
    /// Since when it runs `revision`, unix seconds.
    pub since: Option<i64>,
    /// The deployment that made it run `revision` (a deploy or a rollback).
    pub deployment: Option<u64>,
    /// The latest deployment recorded to it, whatever its state.
    pub last: Option<u64>,
    pub last_state: Option<DeployState>,
    pub last_at: Option<i64>,
    /// How many tickets run there.
    pub tickets: u64,
    /// DEPLOY-21: the environment it promotes from, when it names one
    /// (`deployments promotes`, or a record's `from`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from: Option<String>,
}

/// DEPLOY-13: one ticket running in an environment — a kept cell, as
/// `running` answers it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct RunningCell {
    pub ticket: String,
    /// Since when, unix seconds — the earliest still-counting deploy that
    /// carried it.
    pub since: i64,
    /// That deploy.
    pub deployment: u64,
    /// DEPLOY-20: the ticket's commits not carried there, in link order —
    /// linked after the version that runs there. Empty: it runs the
    /// ticket's current version.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub behind: Vec<String>,
    /// DEPLOY-20: when `behind`, the version that runs there — its newest
    /// carried commit (`None` when only a record naming the ticket carried
    /// it).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
}

/// What the `deployments` activation streams. Every method returns
/// `impl Stream<Item = DeploymentsEvent>`; a refusal is the in-band `Error`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DeploymentsEvent {
    /// One deployment (`list`'s rows): `carried` the tickets it carried —
    /// whose linked commits are among its commits and revision, and the ids
    /// it named — `unlinked` its commits no ticket links, `live` whether it
    /// still counts in its environment (not undone by a rollback).
    Deployment { deployment: Deployment, carried: Vec<String>, unlinked: Vec<String>, live: bool },
    /// A deployment recorded — `record`'s answer and `watch`'s event:
    /// as `deployment`, with the tickets that began (`entered`) and stopped
    /// (`left`) running in its environment because of it. `duplicate`: the
    /// same record was kept already (a CI's retry) — this is that one, and
    /// nothing was written.
    Deployed {
        deployment: Deployment,
        carried: Vec<String>,
        unlinked: Vec<String>,
        live: bool,
        entered: Vec<String>,
        left: Vec<String>,
        duplicate: bool,
    },
    /// DEPLOY-25: a request — `request`'s and `cancel_request`'s answer, and
    /// `watch`'s each time one moves (asked, canceled, a delivery attempt,
    /// a CI record naming it): the `requested` deployment whole, and the
    /// tickets it would carry while it is pending.
    Request { deployment: Deployment, carried: Vec<String> },
    /// DEPLOY-25: `for_ticket`: a pending request to this environment would
    /// carry the ticket.
    Requested {
        ticket: String,
        project: String,
        environment: String,
        request: u64,
        requested_by: String,
        at: i64,
        revision: String,
        webhook: WebhookState,
    },
    /// DEPLOY-25: `webhook show`'s, `set`'s and `clear`'s answer — never
    /// the secret.
    Webhook { webhook: WebhookConfig },
    /// One environment (`environments`' rows).
    Environment { environment: Environment },
    /// `watch`: an environment as a recorded deployment left it — after its
    /// `deployed`.
    EnvironmentChanged { environment: Environment },
    /// `for_ticket`: the ticket runs in this environment, since `since`
    /// (unix seconds), brought by `deployment`; the environment runs
    /// `revision` now. DEPLOY-20: `behind` the ticket's commits not carried
    /// there, in link order, when it runs an earlier version — `version`,
    /// its newest carried commit; empty when it runs the current one.
    Running {
        ticket: String,
        project: String,
        environment: String,
        since: i64,
        deployment: u64,
        revision: Option<String>,
        release: Option<String>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        behind: Vec<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        version: Option<String>,
    },
    /// `for_ticket`: an environment of the ticket's project it does not run in.
    NotRunning { ticket: String, project: String, environment: String },
    /// DEPLOY-13: `running`: an environment's kept cells — every ticket
    /// running there (of the `ids` asked, when given), in the store's order.
    /// One per environment asked, an empty one included, so a copy can
    /// replace what it held for it.
    /// DEPLOY-21: `staged` each ticket deploying, ready for or failed there
    /// (of the `ids` asked, when given), in the store's order.
    Cells {
        project: String,
        environment: String,
        running: Vec<RunningCell>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        staged: Vec<StagedCell>,
    },
    /// DEPLOY-21: `promotes`' answer and `watch`'s event — the environment
    /// `(project, environment)` promotes from `from` now (`None`: from none).
    Promotes { project: String, environment: String, from: Option<String> },
    /// Where a `list` stands, once, last: `next` is the id to pass as the
    /// next call's `after`, `null` when nothing more matches.
    Page { next: Option<u64> },
    /// `watch` fell behind and lost `missed` events.
    Lagged { missed: u64 },
    /// ACCESS-12 (`watch`): the watch ended — its caller, verified again
    /// when the grants moved or its sign-in ended, was refused. Last.
    WatchClosed { reason: crate::types::WatchEnd, message: String },
    /// In-band refusal, naming the problem; nothing was written.
    Error { message: String },
}

/// An environment's place among its project's: development, then previews,
/// then test, then staging, then production — anything else between test
/// and staging. Ties go by name.
pub fn rank(name: &str) -> u8 {
    let n = name.to_ascii_lowercase();
    match n.as_str() {
        "local" | "dev" | "development" => 10,
        _ if n.starts_with("pr-") || n.starts_with("preview") || n.starts_with("review") => 15,
        "test" | "testing" | "ci" | "qa" => 20,
        "staging" | "stage" | "preprod" | "pre-prod" | "uat" => 30,
        "prod" | "production" | "live" => 40,
        _ => 25,
    }
}

/// An environment's name as kept — lower case, `[a-z0-9._/-]` (GitLab's
/// `review/pr-412` included), starting with
/// a letter or digit, at most [`ENVIRONMENT_MAX`] bytes — or why not.
pub fn environment_of(text: &str) -> Result<String, String> {
    let t = text.trim().to_ascii_lowercase();
    let ok = !t.is_empty()
        && t.len() <= ENVIRONMENT_MAX
        && t.bytes().next().is_some_and(|b| b.is_ascii_alphanumeric())
        && t.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'/'));
    if ok {
        Ok(t)
    } else {
        Err(format!("`{}` is not an environment name: letters, digits, `-`, `_`, `.` and `/`, at most {ENVIRONMENT_MAX}", text.trim()))
    }
}

/// DEPLOY-25: the longest webhook URL, in bytes.
pub const WEBHOOK_URL_MAX: usize = 2048;

/// DEPLOY-25: a webhook's URL as kept — `https://host[:port]/…`, or
/// `http://` to a loopback host (`localhost`, `127.x.x.x`, `[::1]`) for a
/// local CI; no userinfo (credentials go in the secret), no whitespace —
/// or why not.
pub fn webhook_url_of(text: &str) -> Result<String, String> {
    let t = text.trim();
    let bad = |why: &str| Err(format!("`{}` is not a webhook URL: {why}", t.escape_debug()));
    if t.len() > WEBHOOK_URL_MAX {
        return Err(format!("a webhook URL is at most {WEBHOOK_URL_MAX} bytes; this one is {}", t.len()));
    }
    if t.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return bad("no whitespace or control characters");
    }
    let lower = t.to_ascii_lowercase();
    let (https, rest) = match (lower.strip_prefix("https://"), lower.strip_prefix("http://")) {
        (Some(r), _) => (true, r),
        (None, Some(r)) => (false, r),
        _ => return bad("it is https://…, or http:// to this machine (localhost, 127.0.0.1, [::1])"),
    };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    if authority.contains('@') {
        return bad("no credentials in the URL — the secret signs every delivery");
    }
    let host = match authority.strip_prefix('[') {
        Some(v6) => match v6.split_once(']') {
            Some((h, port)) if port.is_empty() || port.strip_prefix(':').is_some_and(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit())) => h,
            _ => return bad("an IPv6 host is written [::1] or [::1]:port"),
        },
        None => match authority.rsplit_once(':') {
            Some((h, port)) if !port.is_empty() && port.bytes().all(|b| b.is_ascii_digit()) => h,
            Some(_) => return bad("a port is digits"),
            None => authority,
        },
    };
    if host.is_empty() || (host.contains(':') && !authority.starts_with('[')) {
        return bad("it names a host");
    }
    let loopback = host == "localhost" || host == "::1" || host.strip_prefix("127.").is_some_and(|r| r.split('.').count() == 3 && r.split('.').all(|o| o.parse::<u8>().is_ok()));
    if !https && !loopback {
        return bad("plain http only to this machine (localhost, 127.0.0.1, [::1]) — anywhere else it is https");
    }
    Ok(t.to_string())
}

/// A project as kept — its shortcode, upper case — or why not.
/// ACCESS-8: the one shortcode rule, [`crate::address::Code::parse`]'s (a
/// `_` it once took is in no shortcode).
pub fn project_of(text: &str) -> Result<String, String> {
    crate::address::Code::parse(text)
        .map(|c| c.as_str().to_string())
        .map_err(|_| format!("`{}` is not a project: a project is a shortcode (e.g. menger, MENGER)", text.trim()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn states_read_and_say_themselves() {
        for s in DeployState::ALL {
            assert_eq!(s.as_str().parse::<DeployState>(), Ok(s));
            assert_eq!(serde_json::to_value(s).unwrap(), serde_json::json!(s.as_str()));
        }
        assert_eq!("rolled-back".parse::<DeployState>(), Ok(DeployState::RolledBack));
        assert!("deployed".parse::<DeployState>().is_err());
        assert!(DeployState::Succeeded.effective() && DeployState::RolledBack.effective());
        assert!(!DeployState::Failed.effective() && !DeployState::Started.effective());
    }

    #[test]
    fn environments_are_ordered_test_staging_prod() {
        let mut names = vec!["prod", "staging", "test", "pr-412", "dev", "eu"];
        names.sort_by_key(|n| (rank(n), n.to_string()));
        assert_eq!(names, ["dev", "pr-412", "test", "eu", "staging", "prod"]);
    }

    #[test]
    fn names_and_projects_are_checked() {
        assert_eq!(environment_of(" Prod ").unwrap(), "prod");
        assert_eq!(environment_of("pr-412").unwrap(), "pr-412");
        assert_eq!(environment_of("review/pr-412").unwrap(), "review/pr-412");
        assert!(environment_of("").is_err() && environment_of("-x").is_err() && environment_of("a b").is_err());
        assert_eq!(project_of("menger").unwrap(), "MENGER");
        assert!(project_of("my project").is_err());
    }

    #[test]
    fn a_webhook_url_is_https_or_loopback() {
        for ok in ["https://ci.example.com/hooks/menger", "https://ci.example.com:8443/x?y=1", "http://127.0.0.1:6091/hook", "http://localhost/hook", "http://[::1]:9000/", "HTTPS://CI.EXAMPLE.COM/"] {
            assert!(webhook_url_of(ok).is_ok(), "{ok}");
        }
        for no in ["http://ci.example.com/hook", "ftp://x/", "https://", "https://user:pw@ci.example.com/", "http://127.0.0.1.evil.com/", "http://10.0.0.1/", "https://a b/", "ci.example.com"] {
            assert!(webhook_url_of(no).is_err(), "{no}");
        }
        assert!(webhook_url_of(&format!("https://x/{}", "a".repeat(WEBHOOK_URL_MAX))).is_err());
    }

    #[test]
    fn a_requests_states_and_a_webhooks() {
        assert_eq!(RequestState::of(DeployState::Succeeded), RequestState::Succeeded);
        assert!(RequestState::Pending.open() && RequestState::Started.open() && !RequestState::Canceled.open());
        for w in [WebhookState::None, WebhookState::Pending, WebhookState::Retrying, WebhookState::Delivered, WebhookState::Failed, WebhookState::Stopped] {
            assert_eq!(w.as_str().parse::<WebhookState>(), Ok(w));
            assert_eq!(serde_json::to_value(w).unwrap(), serde_json::json!(w.as_str()));
        }
        assert!(WebhookState::Retrying.due() && !WebhookState::Delivered.due());
        assert_eq!("requested".parse::<DeployState>(), Ok(DeployState::Requested));
        assert!(!DeployState::Requested.effective() && !DeployState::RECORDED.contains(&DeployState::Requested));
    }

    #[test]
    fn a_record_reads_from_deploy_9s_json() {
        let r: Record = serde_json::from_value(serde_json::json!({
            "project": "menger", "environment": "prod", "state": "succeeded",
            "revision": "235ff62", "previous": "29a50be", "commits": ["824e943", "235ff62"],
            "release": "20261012T220402Z-235ff62", "at": "2026-10-12T22:04:19Z",
            "source": { "system": "menger-pipeline", "run": "promote", "url": null }
        }))
        .unwrap();
        assert_eq!(r.commits.len(), 2);
        assert!(r.tickets.is_empty());
        assert_eq!(r.source.unwrap().run.as_deref(), Some("promote"));
    }
}

/// MACRO-21: the emitted client implements the activation's trait; a client
/// failure is the in-band error, the last item of its stream.
impl plexus_client::FromClientError for DeploymentsEvent {
    fn from_client_error(error: plexus_client::ClientError) -> Self {
        DeploymentsEvent::Error { message: error.to_string() }
    }
}
