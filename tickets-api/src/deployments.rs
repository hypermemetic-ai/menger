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
}

impl DeployState {
    pub const ALL: [DeployState; 4] = [DeployState::Started, DeployState::Succeeded, DeployState::Failed, DeployState::RolledBack];

    pub const fn as_str(self) -> &'static str {
        match self {
            DeployState::Started => "started",
            DeployState::Succeeded => "succeeded",
            DeployState::Failed => "failed",
            DeployState::RolledBack => "rolled_back",
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
            other => Err(format!("unknown deployment state '{other}' (expected started, succeeded, failed or rolled_back)")),
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
    /// One environment (`environments`' rows).
    Environment { environment: Environment },
    /// `watch`: an environment as a recorded deployment left it — after its
    /// `deployed`.
    EnvironmentChanged { environment: Environment },
    /// `for_ticket`: the ticket runs in this environment, since `since`
    /// (unix seconds), brought by `deployment`; the environment runs
    /// `revision` now.
    Running {
        ticket: String,
        project: String,
        environment: String,
        since: i64,
        deployment: u64,
        revision: Option<String>,
        release: Option<String>,
    },
    /// `for_ticket`: an environment of the ticket's project it does not run in.
    NotRunning { ticket: String, project: String, environment: String },
    /// DEPLOY-13: `running`: an environment's kept cells — every ticket
    /// running there (of the `ids` asked, when given), in the store's order.
    /// One per environment asked, an empty one included, so a copy can
    /// replace what it held for it.
    Cells { project: String, environment: String, running: Vec<RunningCell> },
    /// Where a `list` stands, once, last: `next` is the id to pass as the
    /// next call's `after`, `null` when nothing more matches.
    Page { next: Option<u64> },
    /// `watch` fell behind and lost `missed` events.
    Lagged { missed: u64 },
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
