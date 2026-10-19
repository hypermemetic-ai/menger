//! MENGER-39: **the queue** — a ticket put in front of the owner with an
//! urgency and a reason. The wire vocabulary, wasm-safe: the server's store
//! keeps it ([`crate::store`]'s `queue` module), the app's tray draws it.
//!
//! - **An entry** ([`QueueEntry`]): the ticket, its [`Urgency`], why, the
//!   input that answers it (required for `blocking`), who queued it, from
//!   where, when it was first queued and last asked, and until when it is
//!   snoozed. A ticket is in the queue at most once; queueing it again
//!   raises the urgency and never lowers it.
//! - **Several questions** (APP-86): an entry may name several inputs
//!   ([`QueueEntry::inputs`], in the order asked) and the one the caller is
//!   idle on ([`QueueEntry::blocking`]); it leaves the queue when every one
//!   it names is closed. `input` stays, for every reader before APP-86: the
//!   blocking one, else the first.
//! - **The order** ([`order`]): most urgent first, then the oldest.
//! - **Its history** ([`QueueRecord`]): every queueing, raise, snooze and
//!   unqueue — who, from where, why — kept in a log of its own beside the
//!   transitions.

use std::cmp::Ordering;
use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::types::TransitionSource;

/// How soon the owner should look — in rising order, so `max` raises.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Urgency {
    /// When there is time.
    Whenever,
    /// Before the day is out.
    Today,
    /// Now: something is wrong or waiting.
    Now,
    /// The caller is idle until the named input is answered.
    Blocking,
}

impl Urgency {
    /// The four, quietest first.
    pub const ALL: [Urgency; 4] = [Urgency::Whenever, Urgency::Today, Urgency::Now, Urgency::Blocking];

    /// The wire spelling.
    pub const fn as_str(self) -> &'static str {
        match self {
            Urgency::Whenever => "whenever",
            Urgency::Today => "today",
            Urgency::Now => "now",
            Urgency::Blocking => "blocking",
        }
    }

    /// `now` and `blocking`: the loud ones (the tray's taller, nudging tab).
    pub const fn loud(self) -> bool {
        matches!(self, Urgency::Now | Urgency::Blocking)
    }
}

impl fmt::Display for Urgency {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Case-insensitive; an unknown word names the four.
impl FromStr for Urgency {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Urgency::ALL.into_iter().find(|u| u.as_str().eq_ignore_ascii_case(s.trim())).ok_or_else(|| {
            format!("unknown urgency `{}` — the urgencies are whenever, today, now, blocking", s.trim())
        })
    }
}

/// The longest reason the queue keeps, in bytes.
pub const REASON_MAX: usize = 2_000;

/// APP-86: the most inputs one entry names.
pub const INPUTS_MAX: usize = 16;

/// The longest snooze, in minutes: a week.
pub const SNOOZE_MAX_MINUTES: u32 = 7 * 24 * 60;

/// A ticket in the queue, as it stands.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct QueueEntry {
    pub ticket: String,
    pub urgency: Urgency,
    /// Why the owner should look.
    pub reason: String,
    /// The input on the ticket whose answer is what is asked — required at
    /// `blocking`. APP-86: with several named, the blocking one, else the
    /// first ([`QueueEntry::inputs`] holds them all).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input: Option<String>,
    /// APP-86: every input the entry names, in the order asked — the
    /// questions the owner answers in turn. It leaves the queue when every
    /// one is closed (one closed is progress). Written only with two or
    /// more; [`QueueEntry::named`] reads both forms.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub inputs: Vec<String>,
    /// APP-86: the input the caller is idle on — one of the named; at
    /// urgency `blocking` with one input it is implied, and not written
    /// ([`QueueEntry::blocking_input`] reads both).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blocking: Option<String>,
    /// Who queued it last: a principal (`anonymous` on one kept before ACCESS-24, when login could be off).
    pub by: String,
    /// Their display name, when they have one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub source: TransitionSource,
    /// When it was first queued — "oldest" in the queue's order.
    pub queued_unix: i64,
    /// When it was last queued (a requeue moves it; a snooze does not).
    pub asked_unix: i64,
    /// Out of the alerting surfaces until then; `None`: not snoozed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snoozed_until_unix: Option<i64>,
}

impl QueueEntry {
    /// Whether it is snoozed at `now`.
    pub fn snoozed_at(&self, now: i64) -> bool {
        self.snoozed_until_unix.is_some_and(|u| u > now)
    }

    /// APP-86: the inputs it names, in order — `inputs`, or (an entry kept
    /// before APP-86) its one `input`.
    pub fn named(&self) -> Vec<&str> {
        match self.inputs.is_empty() {
            true => self.input.iter().map(String::as_str).collect(),
            false => self.inputs.iter().map(String::as_str).collect(),
        }
    }

    /// APP-86: the input the caller is idle on — `blocking`, or an entry at
    /// urgency `blocking` naming one input: that one.
    pub fn blocking_input(&self) -> Option<&str> {
        self.blocking.as_deref().or_else(|| (self.urgency == Urgency::Blocking && self.named().len() == 1).then_some(self.input.as_deref()).flatten())
    }

    /// APP-86: as the wire and the file keep it — an entry naming at most one
    /// input is MENGER-39's, byte for byte (`input` alone; `blocking` only
    /// where the urgency does not already say it); with several, `inputs`
    /// lists them and `input` is the blocking one, else the first.
    pub fn compact(mut self) -> Self {
        let named: Vec<String> = self.named().into_iter().map(str::to_string).collect();
        self.input = self.blocking.clone().filter(|b| named.contains(b)).or_else(|| named.first().cloned());
        if named.len() <= 1 {
            self.inputs = Vec::new();
            if self.urgency == Urgency::Blocking && self.blocking == self.input {
                self.blocking = None;
            }
        } else {
            self.inputs = named;
        }
        self
    }
}

/// The queue's order: most urgent first, then the oldest, then by id.
pub fn order(a: &QueueEntry, b: &QueueEntry) -> Ordering {
    b.urgency.cmp(&a.urgency).then(a.queued_unix.cmp(&b.queued_unix)).then_with(|| a.ticket.cmp(&b.ticket))
}

/// What one queue write did — a [`QueueRecord`]'s action.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum QueueAction {
    /// Put in the queue.
    Queued,
    /// Queued again, at a higher urgency.
    Raised,
    /// Queued again at the same urgency or a lower one: the urgency kept,
    /// the reason (and input) the new ones.
    Requeued,
    /// Snoozed until `until_unix`.
    Snoozed,
    /// Taken out — the owner done with it, or the caller withdrawing it.
    Unqueued,
    /// Taken out because its named input was closed.
    Closed,
    /// APP-23: handed back — the owner requested changes (the record names
    /// the comment that says which).
    ChangesRequested,
    /// ARCHIVE-2: taken out because the ticket was archived — an archived
    /// ticket asks nothing.
    Archived,
}

impl QueueAction {
    /// The wire spelling.
    pub const fn as_str(self) -> &'static str {
        match self {
            QueueAction::Queued => "queued",
            QueueAction::Raised => "raised",
            QueueAction::Requeued => "requeued",
            QueueAction::Snoozed => "snoozed",
            QueueAction::Unqueued => "unqueued",
            QueueAction::Closed => "closed",
            QueueAction::ChangesRequested => "changes_requested",
            QueueAction::Archived => "archived",
        }
    }
}

impl FromStr for QueueAction {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s.trim() {
            "queued" => QueueAction::Queued,
            "raised" => QueueAction::Raised,
            "requeued" => QueueAction::Requeued,
            "snoozed" => QueueAction::Snoozed,
            "unqueued" => QueueAction::Unqueued,
            "closed" => QueueAction::Closed,
            "changes_requested" => QueueAction::ChangesRequested,
            "archived" => QueueAction::Archived,
            other => return Err(format!("unknown queue action `{other}`")),
        })
    }
}

/// One line of a ticket's queue history — what `history` yields beside its
/// transitions: who queued it, why and at what urgency; who snoozed it and
/// until when; who took it out.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct QueueRecord {
    /// The queue log's order (its own, not the transitions').
    pub id: u64,
    pub ticket: String,
    pub action: QueueAction,
    /// The urgency the write left (queued, raised, requeued, snoozed), or
    /// the one it had (unqueued, closed).
    pub urgency: Urgency,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input: Option<String>,
    /// APP-86: every input the entry named after the write.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub inputs: Vec<String>,
    /// APP-86: the one the caller was idle on.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blocking: Option<String>,
    pub at_unix: i64,
    pub actor: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub source: TransitionSource,
    /// A snooze's end.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub until_unix: Option<i64>,
    /// APP-23: a change request's comment (its id).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub comment: Option<u64>,
}

/// APP-24: what answered an agent waiting on a ticket (`tickets.await_reply`)
/// — the first response on it since the call's `since`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Reply {
    /// An input on the ticket was answered (with `input` named: that one):
    /// the value, the rationale, from where (`app` — a reader's click — or
    /// `api`), when.
    Answered {
        input: String,
        value: crate::types::InputValue,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        rationale: Option<String>,
        source: crate::types::AnswerSource,
        at_unix: i64,
    },
    /// APP-23: the owner requested changes — the comment that says which
    /// (intent `changes`); the ticket is out of the queue.
    ChangesRequested { comment: crate::comments::Comment },
    /// Someone other than the caller commented on the ticket.
    Commented { comment: crate::comments::Comment },
    /// Someone other than the caller took the ticket out of the queue
    /// (`action`: `unqueued`, or `closed` — its named input was closed).
    Unqueued { action: QueueAction, by: String, at_unix: i64 },
    /// The ticket was canceled or deleted (`how`): nothing more will come.
    Ended { how: Ending, by: String, at_unix: i64 },
    /// ARCHIVE-2: the ticket was archived (by `by`, itself or with an
    /// ancestor): it asks nothing any more.
    Archived { by: String, at_unix: i64 },
}

impl Reply {
    /// When it happened, unix seconds.
    pub fn at_unix(&self) -> i64 {
        match self {
            Reply::Answered { at_unix, .. } | Reply::Unqueued { at_unix, .. } | Reply::Ended { at_unix, .. } | Reply::Archived { at_unix, .. } => *at_unix,
            Reply::ChangesRequested { comment } | Reply::Commented { comment } => comment.at,
        }
    }
}

/// APP-24: how a ticket ended for an agent waiting on it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Ending {
    Canceled,
    Deleted,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(t: &str, u: Urgency, at: i64) -> QueueEntry {
        QueueEntry {
            ticket: t.into(),
            urgency: u,
            reason: "r".into(),
            input: None,
            inputs: Vec::new(),
            blocking: None,
            by: "a".into(),
            name: None,
            source: TransitionSource::Api,
            queued_unix: at,
            asked_unix: at,
            snoozed_until_unix: None,
        }
    }

    #[test]
    fn most_urgent_first_then_oldest() {
        let mut v = [
            entry("A-1", Urgency::Today, 5),
            entry("A-2", Urgency::Blocking, 9),
            entry("A-3", Urgency::Today, 1),
            entry("A-4", Urgency::Whenever, 0),
            entry("A-5", Urgency::Now, 3),
        ];
        v.sort_by(order);
        let ids: Vec<&str> = v.iter().map(|e| e.ticket.as_str()).collect();
        assert_eq!(ids, ["A-2", "A-5", "A-3", "A-1", "A-4"]);
    }

    #[test]
    fn urgencies_rise_and_parse() {
        assert!(Urgency::Blocking > Urgency::Now && Urgency::Now > Urgency::Today && Urgency::Today > Urgency::Whenever);
        assert_eq!("NOW".parse::<Urgency>(), Ok(Urgency::Now));
        assert!("soon".parse::<Urgency>().unwrap_err().contains("whenever, today, now, blocking"));
        for u in Urgency::ALL {
            assert_eq!(serde_json::to_value(u).unwrap(), serde_json::json!(u.as_str()));
        }
    }

    /// APP-24: the wire shape the ticket's example shows.
    #[test]
    fn a_reply_is_its_kind_beside_its_fields() {
        let r = Reply::Answered {
            input: "agent-owner".into(),
            value: crate::types::InputValue::Choice { option: "granter".into() },
            rationale: None,
            source: crate::types::AnswerSource::App,
            at_unix: 7,
        };
        assert_eq!(
            serde_json::to_value(&r).unwrap(),
            serde_json::json!({"kind": "answered", "input": "agent-owner", "value": {"type": "choice", "option": "granter"}, "source": "app", "at_unix": 7})
        );
        let e = Reply::Ended { how: Ending::Canceled, by: "idp:ada".into(), at_unix: 9 };
        assert_eq!(serde_json::to_value(&e).unwrap(), serde_json::json!({"kind": "ended", "how": "canceled", "by": "idp:ada", "at_unix": 9}));
        assert_eq!((r.at_unix(), e.at_unix()), (7, 9));
    }

    #[test]
    fn a_snooze_ends() {
        let mut e = entry("A-1", Urgency::Now, 0);
        e.snoozed_until_unix = Some(100);
        assert!(e.snoozed_at(99) && !e.snoozed_at(100));
    }
}
