//! Events emitted by the tickets activation.

use crate::store::{Status, Ticket};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

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
