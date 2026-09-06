//! menger's ticket vocabulary — the one declaration.
//!
//! Moved here from `menger/src/store.rs` (`Status`, `Kind`, `Severity`,
//! `Ticket`) and `menger/src/activations/tickets/types.rs`
//! (`TicketSummary`, `TicketsEvent`) by MFE-9, unchanged in shape: the
//! server re-exports them from `menger::store` / the tickets activation, so
//! every existing call site still reads `crate::store::Status`, and the
//! browser app reads *these* types rather than a hand-written mirror of the
//! JSON they arrive as.
//!
//! What that buys is one property, and it is the reason the module moved:
//! **rename a field here and the browser app stops compiling.** The old
//! frontend mirror was `#[derive(Deserialize)]` with `#[serde(default)]` on
//! most fields, so a renamed field became an empty string at runtime.
//!
//! Wire casing is load-bearing and is *not* uniform: `Status` and
//! `Severity` serialize in frontmatter casing (`"Pending"`, `"Critical"`)
//! so a ticket round-trips into a `plans/` file unchanged, while `Kind`
//! serializes lowercase (`"build"`). The `rename_all` attributes below are
//! the wire contract, not style.

use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

/// Lifecycle status. Serialized with frontmatter casing (`Pending`, `Ready`, …)
/// so a ticket round-trips into a plans/ file unchanged.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema,
)]
pub enum Status {
    Pending,
    Ready,
    Blocked,
    Complete,
    Idea,
    Epic,
    Superseded,
}

impl Status {
    /// The closed vocabulary, in the order a chooser should offer it. The
    /// browser app's status control renders exactly this — so a status
    /// added here appears in the UI without a second edit, and one removed
    /// cannot linger in it.
    pub const ALL: [Status; 7] = [
        Status::Pending,
        Status::Ready,
        Status::Blocked,
        Status::Complete,
        Status::Idea,
        Status::Epic,
        Status::Superseded,
    ];

    /// The wire spelling — what serde writes and what `set_status` accepts.
    pub const fn as_str(self) -> &'static str {
        match self {
            Status::Pending => "Pending",
            Status::Ready => "Ready",
            Status::Blocked => "Blocked",
            Status::Complete => "Complete",
            Status::Idea => "Idea",
            Status::Epic => "Epic",
            Status::Superseded => "Superseded",
        }
    }
}

impl fmt::Display for Status {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for Status {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "pending" => Ok(Self::Pending),
            "ready" => Ok(Self::Ready),
            "blocked" => Ok(Self::Blocked),
            "complete" => Ok(Self::Complete),
            "idea" => Ok(Self::Idea),
            "epic" => Ok(Self::Epic),
            "superseded" => Ok(Self::Superseded),
            other => Err(format!(
                "unknown status '{other}' (expected Pending, Ready, Blocked, Complete, Idea, Epic, or Superseded)"
            )),
        }
    }
}

/// What shape of work a ticket is (AGS-13): the owner's closed five-value
/// set, one vocabulary owning the slot. Serialized lowercase (`kind:
/// build`) on the wire and in the store.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    /// A container that decomposes into children; done only when they
    /// are. Its acceptance is the operator's gate.
    Scope,
    /// Carries out a defined procedure (verify, run, migrate, release)
    /// without producing new capability.
    Execution,
    /// Produces new capability; gated on acceptance criteria.
    Build,
    /// A defect or missing piece discovered in shipped behavior (rim's
    /// "bug").
    Gap,
    /// A binary investigation that answers a question.
    Spike,
}

impl Kind {
    /// The phrase every "unknown kind" error ends with — the five, named.
    pub const EXPECTED: &'static str = "scope, execution, build, gap, or spike";

    /// The wire spelling (lowercase).
    pub const fn as_str(self) -> &'static str {
        match self {
            Kind::Scope => "scope",
            Kind::Execution => "execution",
            Kind::Build => "build",
            Kind::Gap => "gap",
            Kind::Spike => "spike",
        }
    }
}

impl fmt::Display for Kind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for Kind {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "scope" => Ok(Self::Scope),
            "execution" => Ok(Self::Execution),
            "build" => Ok(Self::Build),
            "gap" => Ok(Self::Gap),
            "spike" => Ok(Self::Spike),
            other => Err(format!(
                "unknown kind '{other}' (expected {})",
                Kind::EXPECTED
            )),
        }
    }
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema,
)]
pub enum Severity {
    Critical,
    High,
    Medium,
    Low,
}

impl Severity {
    /// The wire spelling.
    pub const fn as_str(self) -> &'static str {
        match self {
            Severity::Critical => "Critical",
            Severity::High => "High",
            Severity::Medium => "Medium",
            Severity::Low => "Low",
        }
    }
}

impl fmt::Display for Severity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for Severity {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "critical" => Ok(Self::Critical),
            "high" => Ok(Self::High),
            "medium" => Ok(Self::Medium),
            "low" => Ok(Self::Low),
            other => Err(format!(
                "unknown severity '{other}' (expected Critical, High, Medium, or Low)"
            )),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Ticket {
    /// `EPIC-N`, allocated by the store.
    pub id: String,
    /// Epic prefix, uppercase (e.g. `MNG`).
    pub epic: String,
    /// Sequence number within the epic (the `N` in `EPIC-N`).
    pub seq: u32,
    pub title: String,
    pub status: Status,
    pub kind: Kind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub severity: Option<Severity>,
    /// Ticket ids that must complete before this one starts.
    pub blocked_by: Vec<String>,
    /// Ticket ids that can start once this one completes.
    pub unlocks: Vec<String>,
    /// AGS-19: the ticket this one is an output of, if any. Any epic; the
    /// parent must exist at creation and the chain is acyclic.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
    /// AGS-19: the tickets whose `parent` is this one — derived and kept
    /// by the store (as `unlocks` is from `blocked_by`), in birth order.
    pub children: Vec<String>,
    /// Markdown body (Problem / Required behavior / Acceptance criteria …).
    pub body: String,
    pub created_unix: i64,
    pub updated_unix: i64,
}

/// MNG-9: a ticket row without its body — every field of [`Ticket`]
/// except `body`.
///
/// The body is ~70 % of a ticket's weight (PERF-4: 98 MiB of `list` at
/// 100,000 tickets, against ~30 MiB of everything-but-bodies), and a
/// board, a nav or a picker shows none of it — they show what is here.
/// A consumer that needs one body calls `get` for that one ticket.
///
/// Unlike `Ticket`, `severity` and `parent` are written out as `null`
/// rather than skipped: a row shape a client destructures should have
/// the same keys in every row.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct TicketSummary {
    pub id: String,
    pub epic: String,
    pub seq: u32,
    pub title: String,
    pub status: Status,
    pub kind: Kind,
    pub severity: Option<Severity>,
    pub parent: Option<String>,
    pub children: Vec<String>,
    pub blocked_by: Vec<String>,
    pub unlocks: Vec<String>,
    pub created_unix: i64,
    pub updated_unix: i64,
}

impl From<Ticket> for TicketSummary {
    fn from(t: Ticket) -> Self {
        Self {
            id: t.id,
            epic: t.epic,
            seq: t.seq,
            title: t.title,
            status: t.status,
            kind: t.kind,
            severity: t.severity,
            parent: t.parent,
            children: t.children,
            blocked_by: t.blocked_by,
            unlocks: t.unlocks,
            created_unix: t.created_unix,
            updated_unix: t.updated_unix,
        }
    }
}

/// MFE-9: the same narrowing from a borrowed ticket, for a consumer that
/// keeps summaries and holds the body elsewhere (the browser app's row
/// store: a `watch` event carries a whole `Ticket`, the row wants the
/// summary of it and nothing more).
impl From<&Ticket> for TicketSummary {
    fn from(t: &Ticket) -> Self {
        t.clone().into()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TicketsEvent {
    /// Heartbeat.
    Health {
        version: String,
        unix_now: i64,
        ticket_count: u64,
    },
    /// A ticket was created (yielded by `create`, and by `watch` on every creation).
    Created { ticket: Ticket },
    /// One ticket, as it exists now (yielded by `get`, and per-row by
    /// `list` and `ready_set`).
    Ticket { ticket: Ticket },
    /// MNG-9: one ticket row without its body — what `list` and
    /// `ready_set` yield per row instead of `ticket` when the call asked
    /// for `summary`.
    Summary { summary: TicketSummary },
    /// MNG-9: where a `list` / `ready_set` walk stands, yielded once as
    /// the last data event of the call. `next` is the cursor to pass as
    /// the next call's `after` — `null` when nothing more matches, which
    /// is how a client knows the walk is over — and `remaining` counts
    /// the matches beyond this page under the same filters.
    Page {
        next: Option<String>,
        remaining: u64,
    },
    /// A ticket's status changed (yielded by `set_status`, and by `watch`).
    StatusChanged { ticket: Ticket, previous: Status },
    /// An existing ticket's `unlocks` gained `added`: a newly created
    /// ticket named it in `blocked_by` (yielded by `watch`). Carries the
    /// full updated ticket so a watch-only client stays coherent.
    UnlocksChanged { ticket: Ticket, added: String },
    /// AGS-19: an existing ticket's `children` gained `added` — a child
    /// was born naming it as `parent` (yielded by `watch`). Carries the
    /// full updated parent so a watch-only client stays coherent.
    ChildrenChanged { ticket: Ticket, added: String },
    /// MNG-12: a ticket's `parent` was set after birth (yielded by
    /// `set_parent`, and by `watch`); `previous` is the parent it had, if
    /// any. Carries the full child.
    ParentChanged {
        ticket: Ticket,
        previous: Option<String>,
    },
    /// MNG-12: an existing ticket's `children` lost `removed` — that
    /// child was re-parented elsewhere (yielded by `watch`). Carries the
    /// full updated ex-parent.
    ChildRemoved { ticket: Ticket, removed: String },
    /// One resolved blocker of the queried ticket, at its current status
    /// (yielded per-entry by `blockers`).
    Blocker { ticket: Ticket },
    /// A blocker id that names no existing ticket — a dangling forward
    /// reference (yielded per-entry by `blockers`).
    DanglingBlocker { id: String },
    /// Lookup miss.
    NotFound { id: String },
    /// In-band error (bad enum value, invalid epic, blocked_by cycle, …).
    /// A *data* event: the call succeeded and the activation said no,
    /// which is not the same thing as a transport or remote failure.
    Error { message: String },
}

#[cfg(test)]
mod tests {
    use super::*;

    /// MFE-9: the browser app renders a status by its wire spelling and
    /// sends that spelling straight back to `set_status`. If `as_str` and
    /// serde ever disagreed the UI would show one word and change the
    /// ticket to another — so they are pinned to each other here, for
    /// every member of the closed vocabulary.
    #[test]
    fn every_status_renders_as_its_wire_spelling() {
        for s in Status::ALL {
            assert_eq!(
                serde_json::to_value(s).unwrap(),
                serde_json::Value::String(s.as_str().to_string()),
                "{s:?}"
            );
            assert_eq!(s.as_str().parse::<Status>().unwrap(), s);
        }
    }

    #[test]
    fn kind_and_severity_render_as_their_wire_spellings() {
        for k in [Kind::Scope, Kind::Execution, Kind::Build, Kind::Gap, Kind::Spike] {
            assert_eq!(
                serde_json::to_value(k).unwrap(),
                serde_json::Value::String(k.as_str().to_string()),
                "{k:?}"
            );
        }
        for s in [
            Severity::Critical,
            Severity::High,
            Severity::Medium,
            Severity::Low,
        ] {
            assert_eq!(
                serde_json::to_value(s).unwrap(),
                serde_json::Value::String(s.as_str().to_string()),
                "{s:?}"
            );
        }
    }
}
