//! Events emitted by the tickets activation.

use crate::store::{Kind, Severity, Status, Ticket};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
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

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
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
    /// One resolved blocker of the queried ticket, at its current status
    /// (yielded per-entry by `blockers`).
    Blocker { ticket: Ticket },
    /// A blocker id that names no existing ticket — a dangling forward
    /// reference (yielded per-entry by `blockers`).
    DanglingBlocker { id: String },
    /// Lookup miss.
    NotFound { id: String },
    /// In-band error (bad enum value, invalid epic, blocked_by cycle, …).
    Error { message: String },
}
