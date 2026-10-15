//! ORG-4: the organizations vocabulary — what the `orgs` activation says.
//! Wasm-safe, like [`crate::members`]: the app reads it through the
//! generated `OrgsClient` (its org switcher).
//!
//! Every ticket and every shortcode belongs to exactly one organization
//! (ORG-1); today's tracker is `hypermemetic`. An organization is a name —
//! lower-case letters, digits and `-`, unique app-wide ([`OrgId`]) — and
//! roles are granted on it (`acme:`, the whole org) or on a shortcode in
//! it (`acme:MENGER`, that shortcode and every one hosted under it).

use crate::address::OrgId;
use crate::members::Grant;
use serde::{Deserialize, Serialize};

/// One organization, as a reader of it sees it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Org {
    /// Its name now: `acme`. A grant's scope on it is `acme:`.
    pub name: OrgId,
    /// The names it had before a rename — each still names it in an
    /// address (`old:MENGER-1`) and a grant's scope.
    pub aliases: Vec<String>,
    /// Its root shortcodes' full addresses (`acme:MENGER`), in the store's
    /// order.
    pub roots: Vec<String>,
    /// How many shortcodes it holds, hosted ones included.
    pub shortcodes: u32,
    /// How many tickets it holds.
    pub tickets: u32,
}

/// What the `orgs` activation's methods yield.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum OrgsEvent {
    /// `list`: one organization the caller holds a role in.
    Org { org: Org },
    /// `create`'s answer: the organization, empty, and the grant that made
    /// its first admin (none when no `admin` was named).
    Created { org: Org, admin: Option<Grant> },
    /// `rename`'s answer: the organization under its new name; `from` is
    /// one of its aliases now.
    Renamed { from: String, org: Org },
    /// `delete`'s answer: the organization is gone, and every grant on it.
    Deleted { name: OrgId, grants: u32 },
    /// `name` is no organization the caller holds a role in.
    NotFound { name: String },
    /// A refusal, naming the problem; nothing was changed.
    Error { message: String },
}
