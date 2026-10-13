//! INPUT-3: the **inputs store** — every input a ticket's body declares,
//! and every answer and state change it has been through (INPUT-1 decisions
//! 1, 2 and 4; pins 5 and 6). Beside the tickets: a click never rewrites a
//! body.
//!
//! **What a body declares** is read by [`crate::inputs::read`] on every
//! `create` and every `amend` of a body ([`Inputs::plan`]), under the
//! write-serial lock, from the new body in hand and the store's own record
//! of the ticket's inputs (the old body is never read): an input the body
//! declares for the first time is born `open`; one it declares again takes
//! the new declaration (kind, question, options, recommendation) and keeps
//! its history; one it no longer declares is `removed`, history kept — and
//! declared again, it comes back to the state its history says. The change
//! is stored in the write's transaction and taken by the cache after it
//! commits; `watch` hears one `input_changed` per input it moved, in the
//! body's order, the removed after, after every other event of the write.
//!
//! **What an input goes through** ([`apply`]) — `answer` (a value checked
//! against its kind and options, [`crate::inputs::check`]), `clear_answer`,
//! `close_input`, `reopen_input` — is one entry of its history each, with
//! who (`app` / `api`) and when. The state is the history's: the last
//! `answered` or `cleared` says whether it holds an answer, the last
//! `closed` or `reopened` whether it is frozen. A closed input takes no
//! answer and no clear; a removed one takes nothing. A change that changes
//! nothing — closing what is closed, reopening what is not, clearing what
//! holds no answer — writes nothing.
//!
//! **Stored** in two tables: `inputs` — a row per input (`ticket`, `id`,
//! `position` in its ticket's list, `kind`, `question`, `options` as JSON,
//! `recommendation`, `state`); `answers` — a row per history entry
//! (`ticket`, `input`, `action`, `value` as JSON, `rationale`, `source`,
//! `at_unix`), in the order recorded. Read whole at open ([`load`]); a
//! database without them gains both on first open, durably, once, with
//! every body's inputs read in ([`migrate`]). The history is the record, not
//! a cache of the bodies.

use crate::inputs::{self as markup, Declared, Problem};
use crate::types::{Answer, AnswerSource, Input, InputAction, InputHistoryEntry, InputKind, InputState, InputValue};
use rusqlite::{params, Connection};
use std::collections::{HashMap, HashSet};

/// The two tables, and the history's index by input.
const TABLES: &str = "
        CREATE TABLE IF NOT EXISTS inputs (
            ticket         TEXT NOT NULL,
            id             TEXT NOT NULL,
            position       INTEGER NOT NULL,
            kind           TEXT NOT NULL,
            question       TEXT,
            options        TEXT NOT NULL,
            recommendation TEXT,
            state          TEXT NOT NULL,
            PRIMARY KEY (ticket, id)
        ) WITHOUT ROWID;
        CREATE TABLE IF NOT EXISTS answers (
            seq       INTEGER PRIMARY KEY,
            ticket    TEXT NOT NULL,
            input     TEXT NOT NULL,
            action    TEXT NOT NULL,
            value     TEXT,
            rationale TEXT,
            source    TEXT NOT NULL,
            at_unix   INTEGER NOT NULL
        );
        CREATE INDEX IF NOT EXISTS answers_by_input ON answers (ticket, input, seq);
    ";

/// The answer `history` holds now: its last `answered`, unless a `cleared`
/// came after it.
pub(crate) fn answer_of(history: &[InputHistoryEntry]) -> Option<Answer> {
    let last = history
        .iter()
        .rev()
        .find(|e| matches!(e.action, InputAction::Answered | InputAction::Cleared))?;
    Some(Answer {
        value: last.value.clone().filter(|_| last.action == InputAction::Answered)?,
        rationale: last.rationale.clone(),
        source: last.source,
        at_unix: last.at_unix,
    })
}

/// Whether `history` leaves the input frozen: its last `closed` or
/// `reopened` is a `closed`.
fn frozen(history: &[InputHistoryEntry]) -> bool {
    history
        .iter()
        .rev()
        .find(|e| matches!(e.action, InputAction::Closed | InputAction::Reopened))
        .is_some_and(|e| e.action == InputAction::Closed)
}

/// MENGER-13: whether `history` leaves the input withdrawn: its last
/// `withdrawn` or `restored` is a `withdrawn`.
pub(crate) fn withdrawn(history: &[InputHistoryEntry]) -> bool {
    history
        .iter()
        .rev()
        .find(|e| matches!(e.action, InputAction::Withdrawn | InputAction::Restored))
        .is_some_and(|e| e.action == InputAction::Withdrawn)
}

/// The state an input is in — `declared`: its body declares it.
pub(crate) fn state_of(declared: bool, history: &[InputHistoryEntry]) -> InputState {
    if !declared {
        InputState::Removed
    } else if withdrawn(history) {
        InputState::Withdrawn
    } else if frozen(history) {
        InputState::Closed
    } else if answer_of(history).is_some() {
        InputState::Answered
    } else {
        InputState::Open
    }
}

/// A change to one input ([`apply`]).
#[derive(Debug, Clone)]
pub(crate) enum Op {
    Answer {
        value: InputValue,
        rationale: Option<String>,
        source: AnswerSource,
    },
    /// INPUT-9: with who cleared it — `app` when the host clears it for a
    /// frame (`menger.input.clear`), else `api`.
    Clear {
        source: AnswerSource,
    },
    Close,
    Reopen,
}

/// Why an [`Op`] is refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Refused {
    /// Its body no longer declares it.
    Removed,
    /// MENGER-13: its ticket is canceled — it asks no one until the ticket
    /// is not.
    Withdrawn,
    /// It is closed: an answer and a clear wait for `reopen_input`.
    Closed,
    /// The value does not answer it — why ([`markup::check`]).
    Invalid(String),
}

/// `op` on `input` at `now`: the input as it leaves it and the history
/// entry that records it — `None` when it changes nothing — or why not.
pub(crate) fn apply(input: &Input, op: Op, now: i64) -> Result<Option<(Input, InputHistoryEntry)>, Refused> {
    if input.state == InputState::Removed {
        return Err(Refused::Removed);
    }
    if input.state == InputState::Withdrawn {
        return Err(Refused::Withdrawn);
    }
    let closed = input.state == InputState::Closed;
    let entry = |action, value, rationale, source| InputHistoryEntry {
        action,
        value,
        rationale,
        source,
        at_unix: now,
    };
    let entry = match op {
        Op::Answer { .. } | Op::Clear { .. } if closed => return Err(Refused::Closed),
        Op::Answer { value, rationale, source } => {
            let value = markup::check(input.kind, &input.options, value).map_err(Refused::Invalid)?;
            let rationale = rationale.filter(|r| !r.trim().is_empty());
            entry(InputAction::Answered, Some(value), rationale, source)
        }
        Op::Clear { .. } if input.answer.is_none() => return Ok(None),
        Op::Clear { source } => entry(InputAction::Cleared, None, None, source),
        Op::Close if closed => return Ok(None),
        Op::Close => entry(InputAction::Closed, None, None, AnswerSource::Api),
        Op::Reopen if !closed => return Ok(None),
        Op::Reopen => entry(InputAction::Reopened, None, None, AnswerSource::Api),
    };
    let mut next = input.clone();
    next.history.push(entry.clone());
    next.answer = answer_of(&next.history);
    next.state = state_of(true, &next.history);
    Ok(Some((next, entry)))
}

/// MENGER-13: `input` with one system entry more — `withdrawn` or
/// `restored`, `reason` its rationale — and the state its history now says.
fn system_entry(input: &Input, action: InputAction, reason: &str, now: i64) -> (Input, InputHistoryEntry) {
    let entry = InputHistoryEntry {
        action,
        value: None,
        rationale: Some(reason.to_string()),
        source: AnswerSource::System,
        at_unix: now,
    };
    let mut next = input.clone();
    next.history.push(entry.clone());
    next.answer = answer_of(&next.history);
    next.state = state_of(input.state != InputState::Removed, &next.history);
    (next, entry)
}

impl Plan {
    /// MENGER-13: a body written while its ticket is canceled — every input
    /// the plan leaves `open` withdrawn instead, `reason` its rationale (a
    /// canceled ticket asks no one): the entries to record, by input id.
    pub(crate) fn withdraw_open(&mut self, reason: &str, now: i64) -> Vec<(Input, InputHistoryEntry)> {
        let mut out = Vec::new();
        let Some(list) = self.list.as_mut() else { return out };
        for input in list.iter_mut().filter(|i| i.state == InputState::Open) {
            let (next, entry) = system_entry(input, InputAction::Withdrawn, reason, now);
            if let Some(c) = self.changed.iter_mut().find(|c| c.id == next.id) {
                *c = next.clone();
            } else {
                self.changed.push(next.clone());
            }
            *input = next.clone();
            out.push((next, entry));
        }
        out
    }
}

/// One write's change to a ticket's inputs, worked out before it commits.
#[derive(Debug, Default, Clone)]
pub(crate) struct Plan {
    /// The ticket's inputs as the write leaves them, in order — `None` when
    /// the write moves nothing (not even an order).
    pub(crate) list: Option<Vec<Input>>,
    /// Each input the write declared, re-declared, removed or restored, as
    /// it leaves it — the write's `input_changed` events, in order.
    pub(crate) changed: Vec<Input>,
}

/// FILTER-30: whether a history entry touches its ticket — an answer or a
/// clear, the owner's word on it; a close, a reopen, a withdrawal or a
/// restore does not.
pub(crate) fn touches(entry: &InputHistoryEntry) -> bool {
    matches!(entry.action, InputAction::Answered | InputAction::Cleared)
}

/// The store's inputs, as the cache holds them: each ticket's, in its
/// order — the body's, then the removed ones.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct Inputs {
    by_ticket: HashMap<String, Vec<Input>>,
}

/// MENGER-12: whether `list` asks the owner something — holds an input in
/// the state `open`: the filter cell `asking`.
pub(crate) fn asking_in(list: &[Input]) -> bool {
    list.iter().any(|i| i.state == InputState::Open)
}

impl Inputs {
    /// `ticket`'s inputs, in order.
    pub(crate) fn of(&self, ticket: &str) -> &[Input] {
        self.by_ticket.get(ticket).map_or(&[], Vec::as_slice)
    }

    /// MENGER-12: whether `ticket` is `asking` — one of its inputs is
    /// `open` ([`asking_in`]).
    pub(crate) fn asking(&self, ticket: &str) -> bool {
        asking_in(self.of(ticket))
    }

    /// MENGER-12: whether `ticket` is asking once `input` (one of its own,
    /// as a write leaves it) stands in its list.
    pub(crate) fn asking_with(&self, ticket: &str, input: &Input) -> bool {
        self.of(ticket)
            .iter()
            .map(|i| if i.id == input.id { input } else { i })
            .any(|i| i.state == InputState::Open)
    }

    /// MENGER-13: whether `ticket` is asking once each of `moved` (its own,
    /// as a write leaves them) stands in its list — `None` when the write
    /// moves none of its inputs.
    pub(crate) fn asking_after(&self, ticket: &str, moved: &[(Input, InputHistoryEntry)]) -> Option<bool> {
        if moved.is_empty() {
            return None;
        }
        Some(
            self.of(ticket)
                .iter()
                .map(|i| moved.iter().map(|(m, _)| m).find(|m| m.id == i.id).unwrap_or(i))
                .any(|i| i.state == InputState::Open),
        )
    }

    /// One input.
    pub(crate) fn get(&self, ticket: &str, id: &str) -> Option<&Input> {
        self.of(ticket).iter().find(|i| i.id == id)
    }

    /// Every input of `ticket` (or of every ticket), whose ticket is in
    /// `epic`, in `state` — in the store's order of tickets, each ticket's
    /// in its order.
    pub(crate) fn query(&self, ticket: Option<&str>, epic: Option<&str>, state: Option<InputState>) -> Vec<Input> {
        self.query_where(ticket, epic, state, |_| true)
    }

    /// FILTER-15: [`Inputs::query`] of the tickets `keep` admits — asked once
    /// a ticket that declares an input, before anything is cloned.
    pub(crate) fn query_where(
        &self,
        ticket: Option<&str>,
        epic: Option<&str>,
        state: Option<InputState>,
        keep: impl Fn(&str) -> bool,
    ) -> Vec<Input> {
        let mut tickets: Vec<&String> = match ticket {
            Some(t) => self.by_ticket.get_key_value(t).map(|(k, _)| k).into_iter().collect(),
            None => self.by_ticket.keys().collect(),
        };
        tickets.retain(|t| epic.is_none_or(|e| super::db::by_epic_seq(t).0 == e) && keep(t));
        tickets.sort_by_cached_key(|t| super::db::by_epic_seq(t));
        tickets
            .into_iter()
            .flat_map(|t| self.of(t))
            .filter(|i| state.is_none_or(|s| i.state == s))
            .cloned()
            .collect()
    }

    /// What a write of `ticket`'s body declaring `declared` does to its
    /// inputs — see the module doc. Nothing changes here.
    pub(crate) fn plan(&self, ticket: &str, declared: Vec<Declared>) -> Plan {
        let before = self.of(ticket);
        if before.is_empty() && declared.is_empty() {
            return Plan::default();
        }
        let mut list = Vec::with_capacity(declared.len());
        let mut changed = Vec::new();
        let ids: HashSet<&str> = declared.iter().map(|d| d.id.as_str()).collect();
        // The ones the body no longer declares: removed, history kept, after
        // the body's in their order — heard once, when they go.
        let mut gone = Vec::new();
        let mut went = Vec::new();
        for prev in before.iter().filter(|i| !ids.contains(i.id.as_str())) {
            let mut input = prev.clone();
            input.state = InputState::Removed;
            if prev.state != InputState::Removed {
                went.push(input.clone());
            }
            gone.push(input);
        }
        for d in declared {
            let prev = before.iter().find(|i| i.id == d.id);
            let history = prev.map(|p| p.history.clone()).unwrap_or_default();
            let input = Input {
                ticket: ticket.to_string(),
                answer: answer_of(&history),
                state: state_of(true, &history),
                id: d.id,
                kind: d.kind,
                question: d.question,
                options: d.options,
                recommendation: d.recommendation,
                history,
            };
            if prev != Some(&input) {
                changed.push(input.clone());
            }
            list.push(input);
        }
        changed.extend(went);
        list.extend(gone);
        if list == before {
            return Plan::default();
        }
        Plan { list: Some(list), changed }
    }

    /// MENGER-13: what canceling `ticket` does to its inputs — each `open`
    /// one withdrawn, `reason` its rationale, by the system: the input as it
    /// leaves it and the history entry that records it, in the ticket's
    /// order. An answered or closed input asks no one, and stays as it is.
    /// Nothing changes here.
    pub(crate) fn withdrawing(&self, ticket: &str, reason: &str, now: i64) -> Vec<(Input, InputHistoryEntry)> {
        self.of(ticket)
            .iter()
            .filter(|i| i.state == InputState::Open)
            .map(|i| system_entry(i, InputAction::Withdrawn, reason, now))
            .collect()
    }

    /// MENGER-13: what `ticket` leaving `Canceled` does to its inputs — each
    /// its history leaves withdrawn (declared or removed since) restored,
    /// back to what its history says. Nothing changes here.
    pub(crate) fn restoring(&self, ticket: &str, reason: &str, now: i64) -> Vec<(Input, InputHistoryEntry)> {
        self.of(ticket)
            .iter()
            .filter(|i| withdrawn(&i.history))
            .map(|i| system_entry(i, InputAction::Restored, reason, now))
            .collect()
    }

    /// The cache's half of a committed [`Plan`]: `ticket`'s inputs are
    /// `list` now.
    pub(crate) fn take(&mut self, ticket: &str, list: Vec<Input>) {
        if list.is_empty() {
            self.by_ticket.remove(ticket);
        } else {
            self.by_ticket.insert(ticket.to_string(), list);
        }
    }

    /// ACT-5: only the inputs of the tickets `keep` names — a deleted
    /// ticket's stay in the file, out of every read, for its undo.
    pub(crate) fn retain(&mut self, keep: impl Fn(&str) -> bool) {
        self.by_ticket.retain(|t, _| keep(t));
    }

    /// FILTER-30: each answer to and clear of an input, as its ticket and
    /// when — a touch of that ticket ([`touches`]).
    pub(crate) fn activity(&self) -> impl Iterator<Item = (&str, i64)> + '_ {
        self.by_ticket
            .iter()
            .flat_map(|(t, list)| list.iter().flat_map(|i| &i.history).filter(|e| touches(e)).map(move |e| (t.as_str(), e.at_unix)))
    }

    /// The cache's half of a committed [`apply`]: `input` as it is now.
    pub(crate) fn put(&mut self, input: Input) {
        if let Some(slot) = self
            .by_ticket
            .get_mut(&input.ticket)
            .and_then(|list| list.iter_mut().find(|i| i.id == input.id))
        {
            *slot = input;
        }
    }

    /// How many tickets declare inputs, and how many inputs there are — for
    /// a log line.
    fn census(&self) -> (usize, usize) {
        (self.by_ticket.len(), self.by_ticket.values().map(Vec::len).sum())
    }
}

/// A JSON column's text.
fn to_json<T: serde::Serialize>(v: &T) -> String {
    serde_json::to_string(v).expect("serialize a wire type")
}

/// Writes `ticket`'s inputs as `list` — every row of the ticket's replaced,
/// inside the caller's transaction. The history is not touched.
pub(crate) fn store(conn: &Connection, ticket: &str, list: &[Input]) -> rusqlite::Result<()> {
    conn.prepare_cached("DELETE FROM inputs WHERE ticket = ?1")?.execute([ticket])?;
    let mut insert = conn.prepare_cached(
        "INSERT INTO inputs (ticket, id, position, kind, question, options, recommendation, state)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
    )?;
    for (position, i) in list.iter().enumerate() {
        insert.execute(params![
            ticket,
            i.id,
            position as i64,
            i.kind.as_str(),
            i.question,
            to_json(&i.options),
            i.recommendation,
            i.state.as_str(),
        ])?;
    }
    Ok(())
}

/// Records one history entry of `input` and the state it leaves it in,
/// inside the caller's transaction.
pub(crate) fn record(conn: &Connection, input: &Input, entry: &InputHistoryEntry) -> rusqlite::Result<()> {
    conn.prepare_cached(
        "INSERT INTO answers (ticket, input, action, value, rationale, source, at_unix)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
    )?
    .execute(params![
        input.ticket,
        input.id,
        entry.action.as_str(),
        entry.value.as_ref().map(to_json),
        entry.rationale,
        entry.source.as_str(),
        entry.at_unix,
    ])?;
    conn.prepare_cached("UPDATE inputs SET state = ?1 WHERE ticket = ?2 AND id = ?3")?
        .execute(params![input.state.as_str(), input.ticket, input.id])?;
    Ok(())
}

/// A column read and parsed — a JSON column, or a word with a `FromStr`.
fn column<T>(r: &rusqlite::Row<'_>, at: usize, parse: impl FnOnce(&str) -> Result<T, String>) -> rusqlite::Result<T> {
    parse(r.get_ref(at)?.as_str()?).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(at, rusqlite::types::Type::Text, e.into())
    })
}

fn json<T: serde::de::DeserializeOwned>(s: &str) -> Result<T, String> {
    serde_json::from_str(s).map_err(|e| e.to_string())
}

/// Every input as stored, with its history — what a start reads. The state
/// is the history's ([`state_of`]), the stored one saying only whether the
/// input is removed.
pub(crate) fn load(conn: &Connection) -> rusqlite::Result<Inputs> {
    let mut inputs = Inputs::default();
    let mut stmt = conn.prepare(
        "SELECT ticket, id, kind, question, options, recommendation, state FROM inputs ORDER BY ticket, position",
    )?;
    let mut rows = stmt.query([])?;
    while let Some(r) = rows.next()? {
        let ticket: String = r.get(0)?;
        let input = Input {
            ticket: ticket.clone(),
            id: r.get(1)?,
            kind: column(r, 2, |s| s.parse::<InputKind>())?,
            question: r.get(3)?,
            options: column(r, 4, json)?,
            recommendation: r.get(5)?,
            state: column(r, 6, |s| s.parse::<InputState>())?,
            answer: None,
            history: Vec::new(),
        };
        inputs.by_ticket.entry(ticket).or_default().push(input);
    }
    let mut stmt =
        conn.prepare("SELECT ticket, input, action, value, rationale, source, at_unix FROM answers ORDER BY seq")?;
    let mut rows = stmt.query([])?;
    while let Some(r) = rows.next()? {
        let ticket: String = r.get(0)?;
        let id: String = r.get(1)?;
        let value: Option<String> = r.get(3)?;
        let entry = InputHistoryEntry {
            action: column(r, 2, |s| s.parse::<InputAction>())?,
            value: value.as_deref().map(json).transpose().map_err(|e| {
                rusqlite::Error::FromSqlConversionFailure(3, rusqlite::types::Type::Text, e.into())
            })?,
            rationale: r.get(4)?,
            source: column(r, 5, |s| s.parse::<AnswerSource>())?,
            at_unix: r.get(6)?,
        };
        match inputs.by_ticket.get_mut(&ticket).and_then(|l| l.iter_mut().find(|i| i.id == id)) {
            Some(input) => input.history.push(entry),
            None => tracing::warn!("INPUT-3: an answer of {ticket}'s '{id}', which has no input row — not loaded"),
        }
    }
    for input in inputs.by_ticket.values_mut().flatten() {
        input.answer = answer_of(&input.history);
        input.state = state_of(input.state != InputState::Removed, &input.history);
    }
    Ok(inputs)
}

/// MENGER-12: [`load`], or none for a file without the tables — what
/// `--verify-derived` reads, which runs no migration.
pub(crate) fn load_if_any(conn: &Connection) -> rusqlite::Result<Inputs> {
    match has_tables(conn)? {
        true => load(conn),
        false => Ok(Inputs::default()),
    }
}

/// Whether the file has the tables.
fn has_tables(conn: &Connection) -> rusqlite::Result<bool> {
    conn.query_row(
        "SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name IN ('inputs', 'answers')",
        [],
        |r| r.get::<_, i64>(0).map(|n| n == 2),
    )
}

/// One-shot, durable migration of a database written before INPUT-3 — and a
/// new one's first open. If the file lacks the tables, this makes them and
/// fills `inputs` from every body that names `m-decision` or `m-input`
/// (SQLite answers which), each input `open`, with no history — in one
/// transaction committed before the cache loads: a SIGKILL before the commit
/// leaves no table, and the next open reads the bodies again; after it,
/// every open reads ([`load`]). A body's malformed input is logged and
/// skipped (of two with one id, the second); the ticket's next body change
/// is refused while it still holds it. No ticket is written. Returns
/// whether the tables were made.
pub(crate) fn migrate(conn: &mut Connection) -> anyhow::Result<bool> {
    if has_tables(conn)? {
        return Ok(false);
    }
    let started = std::time::Instant::now();
    let tx = conn.transaction()?;
    tx.execute_batch(TABLES)?;
    let mut inputs = Inputs::default();
    let mut read_bodies = 0usize;
    {
        let mut stmt = tx.prepare(
            "SELECT id, body FROM tickets
                 WHERE instr(lower(body), '<m-decision') > 0 OR instr(lower(body), '<m-input') > 0",
        )?;
        let mut rows = stmt.query([])?;
        while let Some(r) = rows.next()? {
            read_bodies += 1;
            let id = r.get_ref(0)?.as_str()?.to_string();
            let (declared, problems) = markup::read(r.get_ref(1)?.as_str()?);
            for problem in problems {
                let skipped = match &problem {
                    Problem::Twice { .. } => "the first is kept",
                    _ => "not kept",
                };
                tracing::warn!("INPUT-3 migration: {id}: {problem} — {skipped}; its next body change is refused while it holds it");
            }
            if let Some(list) = inputs.plan(&id, declared).list {
                store(&tx, &id, &list)?;
                inputs.take(&id, list);
            }
        }
    }
    tx.commit()?;
    let (tickets, count) = inputs.census();
    tracing::info!(
        "INPUT-3: added the inputs and answers tables — {count} input(s) in {tickets} ticket(s), read from \
         {read_bodies} body(ies) that name one, in {:.1} ms, durably",
        started.elapsed().as_secs_f64() * 1000.0
    );
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::InputOption;

    fn declared(id: &str, options: &[&str]) -> Declared {
        Declared {
            id: id.into(),
            kind: InputKind::Choice,
            question: Some(format!("{id}?")),
            options: options
                .iter()
                .map(|o| InputOption {
                    value: o.to_string(),
                    label: o.to_string(),
                    recommended: false,
                })
                .collect(),
            recommendation: None,
        }
    }

    fn ids(list: &[Input]) -> Vec<(&str, InputState)> {
        list.iter().map(|i| (i.id.as_str(), i.state)).collect()
    }

    /// A body's inputs over three writes: born open, one removed (heard
    /// once, kept after the body's), one re-declared, the removed one back.
    #[test]
    fn a_plan_declares_removes_and_restores() {
        let mut inputs = Inputs::default();
        let plan = inputs.plan("T-1", vec![declared("a", &["x"]), declared("b", &["y"])]);
        assert_eq!(ids(&plan.changed), [("a", InputState::Open), ("b", InputState::Open)]);
        inputs.take("T-1", plan.list.unwrap());

        let plan = inputs.plan("T-1", vec![declared("b", &["y", "z"])]);
        assert_eq!(ids(&plan.changed), [("b", InputState::Open), ("a", InputState::Removed)]);
        let list = plan.list.unwrap();
        assert_eq!(ids(&list), [("b", InputState::Open), ("a", InputState::Removed)]);
        inputs.take("T-1", list);

        // The same body again moves nothing; a removed input is heard once.
        let plan = inputs.plan("T-1", vec![declared("b", &["y", "z"])]);
        assert!(plan.list.is_none() && plan.changed.is_empty(), "{plan:?}");

        let plan = inputs.plan("T-1", vec![declared("a", &["x"]), declared("b", &["y", "z"])]);
        assert_eq!(ids(&plan.changed), [("a", InputState::Open)]);
        inputs.take("T-1", plan.list.unwrap());

        // Order alone moves the list and no input.
        let plan = inputs.plan("T-1", vec![declared("b", &["y", "z"]), declared("a", &["x"])]);
        assert!(plan.changed.is_empty());
        assert_eq!(ids(&plan.list.unwrap()), [("b", InputState::Open), ("a", InputState::Open)]);

        // A body without inputs, of a ticket without any: nothing at all.
        let plan = inputs.plan("T-2", vec![]);
        assert!(plan.list.is_none() && plan.changed.is_empty());
    }

    /// The state machine: answer, change, clear, close (frozen), reopen;
    /// the no-ops; removed takes nothing.
    #[test]
    fn apply_moves_the_state_and_records_each_change() {
        let mut inputs = Inputs::default();
        inputs.take("T-1", inputs.plan("T-1", vec![declared("a", &["x", "y"])]).list.unwrap());
        let a = inputs.get("T-1", "a").unwrap().clone();
        let answer = |o: &str| Op::Answer {
            value: InputValue::Choice { option: o.into() },
            rationale: Some("because".into()),
            source: AnswerSource::App,
        };
        let clear = || Op::Clear { source: AnswerSource::Api };
        assert!(matches!(apply(&a, answer("z"), 1), Err(Refused::Invalid(_))));
        assert_eq!(apply(&a, clear(), 1).unwrap(), None, "nothing to clear");
        assert_eq!(apply(&a, Op::Reopen, 1).unwrap(), None, "not closed");
        let (a, e) = apply(&a, answer("x"), 10).unwrap().unwrap();
        assert_eq!((a.state, e.action, e.source), (InputState::Answered, InputAction::Answered, AnswerSource::App));
        assert_eq!(a.answer.as_ref().map(|x| (&x.value, x.at_unix)), Some((&InputValue::Choice { option: "x".into() }, 10)));
        let (a, _) = apply(&a, answer("y"), 11).unwrap().unwrap();
        let (a, _) = apply(&a, Op::Close, 12).unwrap().unwrap();
        assert_eq!(a.state, InputState::Closed);
        assert_eq!(a.answer.as_ref().map(|x| x.value.clone()), Some(InputValue::Choice { option: "y".into() }), "frozen with its answer");
        assert_eq!(apply(&a, answer("x"), 13), Err(Refused::Closed));
        assert_eq!(apply(&a, clear(), 13), Err(Refused::Closed));
        assert_eq!(apply(&a, Op::Close, 13).unwrap(), None);
        let (a, _) = apply(&a, Op::Reopen, 14).unwrap().unwrap();
        assert_eq!(a.state, InputState::Answered, "reopened to what its history says");
        let (a, e) = apply(&a, clear(), 15).unwrap().unwrap();
        assert_eq!((a.state, a.answer.as_ref(), e.source), (InputState::Open, None, AnswerSource::Api));
        // INPUT-9: a clear made for a frame is the app's.
        let (b, _) = apply(&a, answer("x"), 16).unwrap().unwrap();
        let (b, e) = apply(&b, Op::Clear { source: AnswerSource::App }, 17).unwrap().unwrap();
        assert_eq!((b.state, b.answer.as_ref(), e.action, e.source), (InputState::Open, None, InputAction::Cleared, AnswerSource::App));
        let actions: Vec<InputAction> = a.history.iter().map(|e| e.action).collect();
        use InputAction::*;
        assert_eq!(actions, [Answered, Answered, Closed, Reopened, Cleared]);
        let mut gone = a.clone();
        gone.state = InputState::Removed;
        for op in [answer("x"), clear(), Op::Close, Op::Reopen] {
            assert_eq!(apply(&gone, op, 16), Err(Refused::Removed));
        }
    }

    /// A blank rationale is none.
    #[test]
    fn a_blank_rationale_is_none() {
        let mut inputs = Inputs::default();
        inputs.take("T-1", inputs.plan("T-1", vec![declared("a", &["x"])]).list.unwrap());
        let a = inputs.get("T-1", "a").unwrap();
        let op = Op::Answer {
            value: InputValue::Choice { option: "x".into() },
            rationale: Some("  ".into()),
            source: AnswerSource::Api,
        };
        let (a, _) = apply(a, op, 1).unwrap().unwrap();
        assert_eq!(a.answer.unwrap().rationale, None);
    }
}
