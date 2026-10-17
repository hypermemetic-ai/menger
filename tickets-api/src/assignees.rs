//! TICKETS-9: a ticket's **assignees** — zero or more principals, people or
//! agents, a set on the row, moved by `tickets.assign { id, add, remove }`
//! (and `update_many { assign, unassign }`), each move a line of the
//! ticket's assignment history ([`AssignRecord`]), and asked by the filter
//! cells `assigned <who>`, `assigned me`, `assigned` and `unassigned`.
//!
//! An assignee is a principal ([`crate::vocab::Principal`]: `key:<hex>`,
//! `idp:<uuid>`, …) that reads the ticket — one that cannot is refused by
//! the activation, naming why, before anything is written. A ticket holds
//! at most [`MAX_PER_TICKET`], kept sorted, each once. Wasm-safe: the app
//! draws the set and its history with it.

use serde::{Deserialize, Serialize};
use std::str::FromStr;

use crate::types::TransitionSource;
use crate::vocab::Principal;

/// At most this many assignees on one ticket — SCALE-4's lesson: a list on
/// a row is bounded.
pub const MAX_PER_TICKET: usize = 16;

/// `current` with `add` added and `remove` taken off — sorted, each once —
/// and what moved: `(after, added, removed)`, `added` and `removed` in the
/// order given, each once. A principal named in both is added (the
/// `remove` loses), as `label` takes it. Refused whole for a result past
/// [`MAX_PER_TICKET`].
pub fn apply(current: &[String], add: &[Principal], remove: &[Principal]) -> Result<(Vec<String>, Vec<String>, Vec<String>), String> {
    let once = |raw: &[Principal]| -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for p in raw.iter().map(|p| p.as_str().to_string()) {
            if !out.contains(&p) {
                out.push(p);
            }
        }
        out
    };
    let (add, remove) = (once(add), once(remove));
    let mut after: Vec<String> = current.to_vec();
    let removed: Vec<String> = remove.into_iter().filter(|p| !add.contains(p) && after.contains(p)).collect();
    after.retain(|p| !removed.contains(p));
    let added: Vec<String> = add.into_iter().filter(|p| !after.contains(p)).collect();
    after.extend(added.iter().cloned());
    after.sort();
    if after.len() > MAX_PER_TICKET {
        return Err(format!("a ticket holds at most {MAX_PER_TICKET} assignees; this would leave {}", after.len()));
    }
    Ok((after, added, removed))
}

/// What one assignment write did to one principal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AssignAction {
    /// Made an assignee.
    Assigned,
    /// No longer an assignee.
    Unassigned,
}

impl AssignAction {
    /// The wire spelling.
    pub const fn as_str(self) -> &'static str {
        match self {
            AssignAction::Assigned => "assigned",
            AssignAction::Unassigned => "unassigned",
        }
    }
}

impl FromStr for AssignAction {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim() {
            "assigned" => Ok(AssignAction::Assigned),
            "unassigned" => Ok(AssignAction::Unassigned),
            other => Err(format!("unknown assignment action `{other}`")),
        }
    }
}

/// One line of a ticket's assignment history — what `history` yields after
/// its transitions and its queue records: who was assigned or unassigned,
/// when, and by whom.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct AssignRecord {
    /// The assignment log's order (its own, not the transitions').
    pub id: u64,
    pub ticket: String,
    /// The principal assigned or unassigned.
    pub principal: String,
    pub action: AssignAction,
    pub at_unix: i64,
    /// Who made the change: a principal.
    pub actor: String,
    /// Their display name, when they have one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub source: TransitionSource,
    /// Why, when the call said (`message`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: &str = "key:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const B: &str = "key:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

    fn p(s: &str) -> Principal {
        s.parse().expect("a principal")
    }

    #[test]
    fn apply_adds_removes_sorts_and_names_what_moved() {
        let (after, added, removed) = apply(&[], &[p(B), p(A), p(A)], &[]).unwrap();
        assert_eq!(after, vec![A.to_string(), B.to_string()]);
        assert_eq!(added, vec![B.to_string(), A.to_string()]);
        assert!(removed.is_empty());
        let (after, added, removed) = apply(&after, &[], &[p(B)]).unwrap();
        assert_eq!((after, added, removed), (vec![A.to_string()], vec![], vec![B.to_string()]));
        // In both: added wins; already there, nothing moves.
        let (after, added, removed) = apply(&[A.to_string()], &[p(A)], &[p(A)]).unwrap();
        assert_eq!((after, added, removed), (vec![A.to_string()], vec![], vec![]));
    }

    #[test]
    fn apply_refuses_too_many_whole() {
        let many: Vec<String> = (0..MAX_PER_TICKET).map(|i| format!("key:{:064x}", i + 100)).collect();
        assert!(apply(&many, &[p(A)], &[]).unwrap_err().contains("at most 16"));
        assert!(apply(&many, &[p(A)], &[p(&many[0])]).is_ok());
        assert!(AssignAction::from_str("assigned").is_ok() && AssignAction::from_str("x").is_err());
    }
}
