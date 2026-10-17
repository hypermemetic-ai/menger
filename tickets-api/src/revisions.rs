//! TICKETS-7: **a ticket's revisions** — the vocabulary. Every write that
//! changes a ticket's own fields keeps the ticket as that write left it, as
//! a numbered revision (1, 2, … per ticket): who, when, from where, why,
//! and which fields moved. A ticket's History lists them; any one opens as
//! it was, diffs against another, and can be restored — a restore is a new
//! revision, so nothing is lost. The store keeps them
//! (`store/revisions.rs`); the wire carries these. Wasm-safe.
//!
//! **A ticket's own fields**, the ones a revision keeps: `title`, `body`,
//! `status`, `kind`, `severity`, `parent`, `blocked_by`, `labels` and
//! `commits` ([`FIELDS`]). Not what is derived from other tickets (its
//! `unlocks`, `child_count`, placement), nor the archive's mark (a mark
//! apart, ARCHIVE-2: the transition log keeps it).

use crate::address::TicketId;
use crate::diff::TextDiff;
use serde::{Deserialize, Serialize};

/// The fields a revision keeps, in the order a revision names them.
pub const FIELDS: [&str; 9] = ["title", "body", "status", "kind", "severity", "parent", "blocked_by", "labels", "commits"];

/// The fields `restore` puts back when none is named — every one but the
/// commits, which record what finished the work and are linked, never
/// restored.
pub const RESTORABLE: [&str; 8] = ["title", "body", "status", "kind", "severity", "parent", "blocked_by", "labels"];

/// What a revision's `fields` says beside the fields: the ticket was born
/// (its first revision), deleted (ACT-5: kept with its tombstone) or brought
/// back by `undelete`.
pub const CREATED: &str = "created";
pub const DELETED: &str = "deleted";
pub const UNDELETED: &str = "undeleted";

/// Where a revision came from. `app`, `api`, `pipeline` and `cascade` as a
/// transition's (MENGER-13); `baseline`: the ticket as it stood before the
/// first write that kept a revision of it — kept by that write, dated by
/// the ticket's last update, by nobody known (a ticket older than TICKETS-7
/// has this as its first revision); `backup`: a revision recovered from a
/// backup of the database, dated by the backup and by nobody known —
/// reserved for an import (TICKETS-7's `backfill`), which nothing writes yet.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RevisionSource {
    App,
    #[default]
    Api,
    Pipeline,
    Cascade,
    Baseline,
    Backup,
}

impl RevisionSource {
    /// The stored and wire spelling.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::App => "app",
            Self::Api => "api",
            Self::Pipeline => "pipeline",
            Self::Cascade => "cascade",
            Self::Baseline => "baseline",
            Self::Backup => "backup",
        }
    }

    /// From its stored spelling; anything else is `api`.
    pub fn parse(s: &str) -> Self {
        match s {
            "app" => Self::App,
            "pipeline" => Self::Pipeline,
            "cascade" => Self::Cascade,
            "baseline" => Self::Baseline,
            "backup" => Self::Backup,
            _ => Self::Api,
        }
    }
}

impl From<crate::types::TransitionSource> for RevisionSource {
    fn from(s: crate::types::TransitionSource) -> Self {
        use crate::types::TransitionSource as T;
        match s {
            T::App => Self::App,
            T::Api => Self::Api,
            T::Pipeline => Self::Pipeline,
            T::Cascade => Self::Cascade,
        }
    }
}

/// One revision, as `revisions list` answers it: the ticket and its number
/// (`rev`, from 1), when, who (`actor`: a principal, `system`, `anonymous`
/// with login off; `name`, the caller's display name), from where
/// (`source`), why (`message`), what caused it (`cause`: the ticket whose
/// change carried this one — a cancel's cascade, the first ticket of a
/// write that revised several), the fields it changed against the revision
/// before it ([`FIELDS`], or `created` / `deleted` / `undeleted`; empty for
/// a `baseline`), and its body: its content's hash (`body_hash`, SHA-256
/// hex — equal bodies, equal hashes: a body is stored once however many
/// revisions share it), its size in bytes, and the lines it added and
/// removed against the revision before.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Revision {
    pub ticket: TicketId,
    pub rev: u32,
    pub at_unix: i64,
    pub actor: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub source: RevisionSource,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cause: Option<String>,
    pub fields: Vec<String>,
    pub body_hash: String,
    pub body_bytes: u64,
    pub added: u32,
    pub removed: u32,
}

/// One field that differs between two revisions — its value in each, as
/// text (a list as its items joined by `, `; `null`: none).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct FieldChange {
    pub field: String,
    pub from: Option<String>,
    pub to: Option<String>,
}

/// `revisions diff`'s answer: `from` → `to`, the fields that differ (the
/// body's by its line counts), the body's frontmatter key by key, and the
/// body's line diff (`null` when the bodies are equal) — of its source, or
/// (`view: text`) of the text a reader sees.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct RevisionDiff {
    pub ticket: TicketId,
    pub from: u32,
    pub to: u32,
    pub fields: Vec<FieldChange>,
    pub frontmatter: Vec<FieldChange>,
    pub body: Option<TextDiff>,
}
