//! The service-wide ticket cache.
//!
//! This layer is the primary interface through which the whole
//! application touches tickets — activations, DAG queries, and the shell
//! all go through here, never to storage directly. Reads are async-safe:
//! no lock is held across an await and the read path does no I/O. MNG-2
//! puts the database (the system of record) behind this layer:
//! write-through on mutation, loaded into the cache at startup. Status/
//! type vocabulary matches the house ticketing convention.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::str::FromStr;
use std::sync::RwLock;
use tokio::sync::broadcast;

/// Lifecycle status. Serialized with frontmatter casing (`Pending`, `Ready`, …)
/// so a ticket round-trips into a plans/ file unchanged.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum Status {
    Pending,
    Ready,
    Blocked,
    Complete,
    Idea,
    Epic,
    Superseded,
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

/// Unit of work. Frontmatter uses lowercase (`type: implementation`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TicketType {
    Implementation,
    Analysis,
    Spike,
    Epic,
    Checkpoint,
}

impl FromStr for TicketType {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "implementation" => Ok(Self::Implementation),
            "analysis" => Ok(Self::Analysis),
            "spike" => Ok(Self::Spike),
            "epic" => Ok(Self::Epic),
            "checkpoint" => Ok(Self::Checkpoint),
            other => Err(format!(
                "unknown ticket type '{other}' (expected implementation, analysis, spike, epic, or checkpoint)"
            )),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum Severity {
    Critical,
    High,
    Medium,
    Low,
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

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Ticket {
    /// `EPIC-N`, allocated by the store.
    pub id: String,
    /// Epic prefix, uppercase (e.g. `MNG`).
    pub epic: String,
    /// Sequence number within the epic (the `N` in `EPIC-N`).
    pub seq: u32,
    pub title: String,
    pub status: Status,
    pub ticket_type: TicketType,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub severity: Option<Severity>,
    /// Ticket ids that must complete before this one starts.
    pub blocked_by: Vec<String>,
    /// Ticket ids that can start once this one completes.
    pub unlocks: Vec<String>,
    /// Markdown body (Problem / Required behavior / Acceptance criteria …).
    pub body: String,
    pub created_unix: i64,
    pub updated_unix: i64,
}

/// A mutation, broadcast to every `watch` subscriber.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Change {
    Created { ticket: Ticket },
    StatusChanged { ticket: Ticket, previous: Status },
    /// An existing ticket's `unlocks` gained `added` because a newly
    /// created ticket named it in `blocked_by`. Carries the full updated
    /// ticket so watch-only clients stay coherent.
    UnlocksChanged { ticket: Ticket, added: String },
}

impl Change {
    pub fn ticket(&self) -> &Ticket {
        match self {
            Self::Created { ticket }
            | Self::StatusChanged { ticket, .. }
            | Self::UnlocksChanged { ticket, .. } => ticket,
        }
    }
}

/// One entry of a ticket's `blocked_by`, resolved against current state.
#[derive(Debug, Clone)]
pub enum BlockerRef {
    /// The blocker exists; carries it at its current status.
    Resolved(Ticket),
    /// The id names no existing ticket (a dangling forward reference).
    Dangling(String),
}

pub struct NewTicket {
    pub epic: String,
    pub title: String,
    pub ticket_type: TicketType,
    pub severity: Option<Severity>,
    pub blocked_by: Vec<String>,
    pub body: String,
}

#[derive(Debug)]
pub enum StoreError {
    NotFound(String),
    InvalidEpic(String),
    /// Creating `id` with the requested `blocked_by` would close a cycle;
    /// `path` is the blocked_by chain from `id` back to `id`.
    Cycle { id: String, path: Vec<String> },
}

impl std::fmt::Display for StoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFound(id) => write!(f, "no ticket with id '{id}'"),
            Self::InvalidEpic(e) => write!(
                f,
                "invalid epic '{e}' (expected a short alphanumeric prefix like MNG)"
            ),
            Self::Cycle { id, path } => write!(
                f,
                "creating '{id}' would close a blocked_by cycle: {id} -> {}",
                path.join(" -> ")
            ),
        }
    }
}

/// Walks `blocked_by` edges from the proposed blockers through existing
/// tickets; if the allocated `new_id` is reachable (self-reference
/// included), returns the blocked_by chain that closes the cycle.
/// Dangling ids other than `new_id` terminate their branch.
fn find_cycle(
    tickets: &HashMap<String, Ticket>,
    new_id: &str,
    blocked_by: &[String],
) -> Option<Vec<String>> {
    let mut stack: Vec<Vec<String>> = blocked_by.iter().map(|b| vec![b.clone()]).collect();
    let mut visited: HashSet<String> = HashSet::new();
    while let Some(path) = stack.pop() {
        let last = path.last().expect("paths start non-empty");
        if last == new_id {
            return Some(path);
        }
        if !visited.insert(last.clone()) {
            continue;
        }
        if let Some(t) = tickets.get(last) {
            for b in &t.blocked_by {
                let mut next = path.clone();
                next.push(b.clone());
                stack.push(next);
            }
        }
    }
    None
}

/// A blocker satisfies the ready-set rule when it is done with.
fn satisfied(status: Status) -> bool {
    matches!(status, Status::Complete | Status::Superseded)
}

pub struct Store {
    tickets: RwLock<HashMap<String, Ticket>>,
    tx: broadcast::Sender<Change>,
}

impl Default for Store {
    fn default() -> Self {
        Self::new()
    }
}

impl Store {
    pub fn new() -> Self {
        let (tx, _) = broadcast::channel(256);
        Self {
            tickets: RwLock::new(HashMap::new()),
            tx,
        }
    }

    /// Allocates the next `EPIC-N` id and inserts the ticket.
    ///
    /// DAG maintenance happens here: a create whose allocated id would
    /// close a `blocked_by` cycle (self-reference included) is rejected
    /// and stores nothing; otherwise the new id is appended to each
    /// existing blocker's `unlocks`, and any existing tickets whose
    /// `blocked_by` already named the allocated id (dangling forward
    /// references) become the newborn's `unlocks` from birth.
    pub fn create(&self, new: NewTicket) -> Result<Ticket, StoreError> {
        let epic = new.epic.trim().to_ascii_uppercase();
        if epic.is_empty() || !epic.chars().all(|c| c.is_ascii_alphanumeric()) {
            return Err(StoreError::InvalidEpic(new.epic));
        }

        let now = chrono::Utc::now().timestamp();
        let mut tickets = self.tickets.write().expect("store lock poisoned");
        let seq = tickets
            .values()
            .filter(|t| t.epic == epic)
            .map(|t| t.seq)
            .max()
            .unwrap_or(0)
            + 1;
        let id = format!("{epic}-{seq}");

        if let Some(path) = find_cycle(&tickets, &id, &new.blocked_by) {
            return Err(StoreError::Cycle { id, path });
        }

        // Dangling forward references to this id resolve now: their
        // owners are the newborn's `unlocks` from birth.
        let mut referrers: Vec<(String, u32, String)> = tickets
            .values()
            .filter(|t| t.blocked_by.iter().any(|b| b == &id))
            .map(|t| (t.epic.clone(), t.seq, t.id.clone()))
            .collect();
        referrers.sort();
        let unlocks: Vec<String> = referrers.into_iter().map(|(_, _, rid)| rid).collect();

        let ticket = Ticket {
            id: id.clone(),
            epic,
            seq,
            title: new.title,
            status: Status::Pending,
            ticket_type: new.ticket_type,
            severity: new.severity,
            blocked_by: new.blocked_by,
            unlocks,
            body: new.body,
            created_unix: now,
            updated_unix: now,
        };

        // Symmetry: each existing blocker gains the new id in `unlocks`.
        let mut unlocks_changes = Vec::new();
        for blocker_id in &ticket.blocked_by {
            if let Some(blocker) = tickets.get_mut(blocker_id) {
                if !blocker.unlocks.contains(&id) {
                    blocker.unlocks.push(id.clone());
                    blocker.updated_unix = now;
                    unlocks_changes.push(Change::UnlocksChanged {
                        ticket: blocker.clone(),
                        added: id.clone(),
                    });
                }
            }
        }

        tickets.insert(ticket.id.clone(), ticket.clone());
        drop(tickets);
        let _ = self.tx.send(Change::Created {
            ticket: ticket.clone(),
        });
        for change in unlocks_changes {
            let _ = self.tx.send(change);
        }
        Ok(ticket)
    }

    pub fn get(&self, id: &str) -> Option<Ticket> {
        self.tickets
            .read()
            .expect("store lock poisoned")
            .get(id)
            .cloned()
    }

    /// Tickets ordered by (epic, seq), optionally filtered.
    pub fn list(&self, epic: Option<&str>, status: Option<Status>) -> Vec<Ticket> {
        let epic = epic.map(str::to_ascii_uppercase);
        let mut out: Vec<Ticket> = self
            .tickets
            .read()
            .expect("store lock poisoned")
            .values()
            .filter(|t| epic.as_deref().is_none_or(|e| t.epic == e))
            .filter(|t| status.is_none_or(|s| t.status == s))
            .cloned()
            .collect();
        out.sort_by(|a, b| a.epic.cmp(&b.epic).then(a.seq.cmp(&b.seq)));
        out
    }

    pub fn set_status(&self, id: &str, status: Status) -> Result<Change, StoreError> {
        let mut tickets = self.tickets.write().expect("store lock poisoned");
        let ticket = tickets
            .get_mut(id)
            .ok_or_else(|| StoreError::NotFound(id.to_string()))?;
        let previous = ticket.status;
        ticket.status = status;
        ticket.updated_unix = chrono::Utc::now().timestamp();
        let change = Change::StatusChanged {
            ticket: ticket.clone(),
            previous,
        };
        drop(tickets);
        let _ = self.tx.send(change.clone());
        Ok(change)
    }

    /// Tickets that can start now: status `Ready` and every `blocked_by`
    /// entry resolves to a ticket whose status is `Complete` or
    /// `Superseded`. A dangling blocker (id naming no existing ticket)
    /// keeps its ticket out. The epic filter applies to membership;
    /// blockers resolve globally. Ordered by (epic, seq).
    pub fn ready_set(&self, epic: Option<&str>) -> Vec<Ticket> {
        let epic = epic.map(str::to_ascii_uppercase);
        let tickets = self.tickets.read().expect("store lock poisoned");
        let mut out: Vec<Ticket> = tickets
            .values()
            .filter(|t| epic.as_deref().is_none_or(|e| t.epic == e))
            .filter(|t| t.status == Status::Ready)
            .filter(|t| {
                t.blocked_by
                    .iter()
                    .all(|b| tickets.get(b).is_some_and(|bt| satisfied(bt.status)))
            })
            .cloned()
            .collect();
        drop(tickets);
        out.sort_by(|a, b| a.epic.cmp(&b.epic).then(a.seq.cmp(&b.seq)));
        out
    }

    /// One ticket's `blocked_by`, each entry resolved to the current
    /// ticket or marked dangling, in `blocked_by` order.
    pub fn blockers(&self, id: &str) -> Result<Vec<BlockerRef>, StoreError> {
        let tickets = self.tickets.read().expect("store lock poisoned");
        let ticket = tickets
            .get(id)
            .ok_or_else(|| StoreError::NotFound(id.to_string()))?;
        Ok(ticket
            .blocked_by
            .iter()
            .map(|b| match tickets.get(b) {
                Some(t) => BlockerRef::Resolved(t.clone()),
                None => BlockerRef::Dangling(b.clone()),
            })
            .collect())
    }

    pub fn count(&self) -> usize {
        self.tickets.read().expect("store lock poisoned").len()
    }

    pub fn subscribe(&self) -> broadcast::Receiver<Change> {
        self.tx.subscribe()
    }
}
