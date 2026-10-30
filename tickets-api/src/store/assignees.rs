//! TICKETS-9: the **assignments store** — who holds each ticket, and the
//! assignment history.
//!
//! **The set is on the row** (`assignees`, a column of `tickets` like
//! `labels`): every read that carries a row carries it, and every write
//! that writes a row writes it as the cache holds it. Only `assign` (and
//! `update_many { assign, unassign }`, the same step) moves it.
//!
//! **Its history is a log of its own** (`assignment_log`), as the queue's
//! is (MENGER-39): a transition is a status's or a kind's, which `changed`,
//! `by` and `--verify-derived` read as such, and an older release reading
//! a `transitions` row it cannot parse would refuse to open the file. Every
//! principal assigned or unassigned is an [`AssignRecord`] — who, when, by
//! whom, why — and `history` yields a ticket's after its queue records.
//!
//! **The cells** `assigned <who>`, `assigned me`, `assigned` (anyone) and
//! `unassigned` ([`crate::filter::Event::Assigned`]) are answered from a
//! kept index, principal → tickets ([`Assignments::holds`]), never by
//! testing a ticket. The index is built from the rows at open (and at every
//! read-back of the file) and moved by the assignment step alone, between
//! the open watches' before and after — so a `watch { filter }` asking it
//! hears `entered` / `left` in the write that moved it.

use super::cascade::Gathered;
use super::history::Act;
use super::*;
use crate::assignees::{AssignAction, AssignRecord};
use crate::vocab::Principal;
use roaring::RoaringBitmap;
use std::collections::BTreeSet;

/// The log's table, gained empty on open ([`migrate`]).
const TABLES: &str = "
        CREATE TABLE IF NOT EXISTS assignment_log (
            id        INTEGER PRIMARY KEY,
            ticket    TEXT NOT NULL,
            principal TEXT NOT NULL,
            action    TEXT NOT NULL,
            at_unix   INTEGER NOT NULL,
            actor     TEXT NOT NULL,
            name      TEXT,
            source    TEXT NOT NULL,
            message   TEXT
        );
        CREATE INDEX IF NOT EXISTS assignment_log_by_ticket ON assignment_log (ticket, id);
    ";

/// A file without the assignment log gains it, empty, here — on every open
/// a no-op once it is there.
pub(crate) fn migrate(conn: &mut Connection) -> anyhow::Result<()> {
    if !db::has_column(conn, "assignment_log", "ticket")? {
        conn.execute_batch(TABLES)?;
        tracing::warn!("TICKETS-9: the file gained the assignment log, empty, durably");
    }
    Ok(())
}

/// The assignments as the cache holds them: each principal's tickets (by
/// key), and each ticket's records, oldest first.
#[derive(Debug, Clone, Default)]
pub(crate) struct Assignments {
    by: HashMap<String, BTreeSet<String>>,
    log: HashMap<String, Vec<AssignRecord>>,
    next_id: u64,
}

impl Assignments {
    /// `ticket`'s records, oldest first.
    pub(crate) fn records(&self, ticket: &str) -> &[AssignRecord] {
        self.log.get(ticket).map_or(&[][..], Vec::as_slice)
    }

    /// The tickets `assigned <who>` holds for (`None`: anyone), per
    /// shortcode — each placed by `at` (a deleted ticket is in none).
    pub(super) fn holds(&self, who: Option<&str>, at: &HashMap<String, OrderKey>) -> BTreeMap<ShortcodeId, RoaringBitmap> {
        let mut out: BTreeMap<ShortcodeId, RoaringBitmap> = BTreeMap::new();
        let sets: Vec<&BTreeSet<String>> = match who {
            Some(p) => self.by.get(p).into_iter().collect(),
            None => self.by.values().collect(),
        };
        for key in sets.into_iter().flatten().filter_map(|t| at.get(t)) {
            out.entry(key.shortcode.clone()).or_default().insert(key.seq);
        }
        out
    }

    /// The cache's half of a committed step: `ticket`'s set moved from
    /// `before` to `after`, and its records.
    fn put(&mut self, ticket: &str, before: &[String], after: &[String], records: Vec<AssignRecord>) {
        for p in before.iter().filter(|p| !after.contains(p)) {
            if let Some(set) = self.by.get_mut(p) {
                set.remove(ticket);
                if set.is_empty() {
                    self.by.remove(p);
                }
            }
        }
        for p in after {
            self.by.entry(p.clone()).or_default().insert(ticket.to_string());
        }
        if let Some(last) = records.last() {
            self.next_id = last.id + 1;
        }
        self.log.entry(ticket.to_string()).or_default().extend(records);
    }

    /// The id the next record takes.
    fn next_id(&self) -> u64 {
        self.next_id.max(1)
    }
}

/// The assignments as stored: the log, and the index from the rows.
pub(super) fn load<'r>(conn: &Connection, rows: impl Iterator<Item = &'r Row>) -> anyhow::Result<Assignments> {
    let bad = |at: usize, e: String| rusqlite::Error::FromSqlConversionFailure(at, rusqlite::types::Type::Text, e.into());
    let mut a = Assignments::default();
    for row in rows {
        for p in &row.assignees {
            a.by.entry(p.clone()).or_default().insert(row.id.to_string());
        }
    }
    let mut stmt = conn.prepare("SELECT id, ticket, principal, action, at_unix, actor, name, source, message FROM assignment_log ORDER BY id")?;
    let records = stmt.query_map([], |r| {
        Ok(AssignRecord {
            id: r.get::<_, i64>(0)? as u64,
            ticket: r.get(1)?,
            principal: r.get(2)?,
            action: r.get::<_, String>(3)?.parse().map_err(|e| bad(3, e))?,
            at_unix: r.get(4)?,
            actor: r.get(5)?,
            name: r.get(6)?,
            source: r.get::<_, String>(7)?.parse().map_err(|e| bad(7, e))?,
            message: r.get(8)?,
        })
    })?;
    for rec in records {
        let rec = rec?;
        a.next_id = rec.id + 1;
        a.log.entry(rec.ticket.clone()).or_default().push(rec);
    }
    Ok(a)
}

/// One record, inside the caller's transaction.
fn store(tx: &Connection, r: &AssignRecord) -> rusqlite::Result<()> {
    tx.prepare_cached(
        "INSERT INTO assignment_log (id, ticket, principal, action, at_unix, actor, name, source, message)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
    )?
    .execute(rusqlite::params![r.id as i64, r.ticket, r.principal, r.action.as_str(), r.at_unix, r.actor, r.name, r.source.as_str(), r.message])?;
    Ok(())
}

/// `row`'s assignees with `add` and `remove` applied — the set after, or
/// `None` when nothing moves; refused past the cap, naming the ticket.
pub(super) fn planned(row: &Row, add: &[Principal], remove: &[Principal]) -> Result<Option<Vec<String>>, StoreError> {
    let (after, added, removed) =
        crate::assignees::apply(&row.assignees, add, remove).map_err(|why| StoreError::Invalid(format!("{why} (nothing changed for '{}')", row.id)))?;
    Ok((!added.is_empty() || !removed.is_empty()).then_some(after))
}

/// The assignment step: `id`'s set becomes `after` — the row and its
/// records written, the index moved between the open watches' before and
/// after, and one `AssigneesChanged`.
pub(super) fn step(
    txn: &rusqlite::Transaction<'_>,
    cache: &mut Cache,
    unders: &lineage::UnderWatches,
    id: &str,
    after: Vec<String>,
    act: &Act,
    now: i64,
    out: &mut Gathered,
) -> Result<(), StoreError> {
    let mut row = cache.get(id).cloned().ok_or_else(|| StoreError::NotFound(id.to_string()))?;
    let before = std::mem::replace(&mut row.assignees, after.clone());
    let added: Vec<String> = after.iter().filter(|p| !before.contains(p)).cloned().collect();
    let removed: Vec<String> = before.iter().filter(|p| !after.contains(p)).cloned().collect();
    row.updated_unix = now;
    let first = cache.assignments.next_id();
    let record = |i: usize, principal: &String, action| AssignRecord {
        id: first + i as u64,
        ticket: id.to_string(),
        principal: principal.clone(),
        action,
        at_unix: now,
        actor: act.actor.clone(),
        name: act.name.clone(),
        source: act.source,
        message: act.message.clone(),
    };
    let records: Vec<AssignRecord> = added
        .iter()
        .map(|p| (p, AssignAction::Assigned))
        .chain(removed.iter().map(|p| (p, AssignAction::Unassigned)))
        .enumerate()
        .map(|(i, (p, a))| record(i, p, a))
        .collect();
    let derived = cache.cell_change(&row, None);
    let under = lineage::under_before_with(unders, cache, || lineage::Touched::of_moves(derived.announced().as_deref()).row(&row));
    let announced = derived.announced();
    let Derived { stored, moved } = derived;
    db::update_row(txn, &row)?;
    stored.write(txn)?;
    for r in &records {
        store(txn, r)?;
    }
    let body = db::body(txn, &row.id)?.ok_or_else(|| missing(&row.id))?;
    cache.assignments.put(id, &before, &after, records);
    cache.commits_changed(row.clone(), moved);
    out.tickets.push(Change::AssigneesChanged { ticket: whole(row, body), added, removed });
    out.filtered(announced, under.and_then(|u| u.after_in(cache)));
    Ok(())
}

impl Store {
    /// TICKETS-9: moves the assignees of every ticket `on` is for — `add`
    /// added and `remove` taken off each ([`crate::assignees::apply`]), in
    /// one write by `act`, all or none (a ticket past the cap refuses the
    /// write, named). Who may be assigned — a principal that reads the
    /// ticket — is the caller's to have asked. One `AssigneesChanged` per
    /// ticket, in the order named: `added` and `removed` empty where
    /// nothing moved, which writes and broadcasts nothing for it.
    pub async fn assign_by(&self, on: Vec<Permit<Editor>>, add: Vec<Principal>, remove: Vec<Principal>, act: Act) -> Result<Vec<Change>, StoreError> {
        let mut ids: Vec<String> = Vec::with_capacity(on.len());
        for p in &on {
            let id = self.keyed(p.id())?;
            if !ids.contains(&id) {
                ids.push(id);
            }
        }
        let serial = self.serial().await;
        let (mut steps, mut refusals) = (Vec::with_capacity(ids.len()), Vec::new());
        {
            let cache = self.tickets.read().expect("store lock poisoned");
            for id in &ids {
                match cache.get(id).ok_or_else(|| StoreError::NotFound(id.clone())).and_then(|row| planned(row, &add, &remove)) {
                    Ok(after) => steps.push((id.clone(), after)),
                    Err(e) => refusals.push((id.clone(), vec![e])),
                }
            }
        }
        match refusals.len() {
            0 => {}
            1 if ids.len() == 1 => return Err(refusals.pop().expect("one").1.pop().expect("one reason")),
            _ => return Err(StoreError::RefusedMany { refusals }),
        }
        if steps.iter().all(|(_, after)| after.is_none()) {
            let mut answer = Vec::with_capacity(steps.len());
            for (id, _) in steps {
                answer.push(Change::AssigneesChanged { ticket: self.get_unscoped(&id).await?, added: Vec::new(), removed: Vec::new() });
            }
            return Ok(answer);
        }
        self.in_one_write(serial, move |conn, cache, unders| {
            super::revisions::act(&act);
            let txn = conn.transaction()?;
            let now = chrono::Utc::now().timestamp();
            let mut out = Gathered::default();
            let mut answer = Vec::with_capacity(steps.len());
            for (id, after) in steps {
                match after {
                    Some(after) => {
                        step(&txn, cache, unders, &id, after, &act, now, &mut out)?;
                        answer.push(out.tickets.last().cloned().expect("the step's own change"));
                    }
                    None => {
                        let row = cache.get(&id).cloned().ok_or_else(|| StoreError::NotFound(id.clone()))?;
                        let body = db::body(&txn, &id)?.ok_or_else(|| missing(&id))?;
                        answer.push(Change::AssigneesChanged { ticket: whole(row, body), added: Vec::new(), removed: Vec::new() });
                    }
                }
            }
            super::revisions::commit(txn)?;
            Ok((answer, out.finish()))
        })
        .await
    }

    /// TICKETS-9: `id`'s assignment history, oldest first — ACCESS-10: when
    /// `who` sees it. From the cache.
    pub fn assign_history(&self, who: &(impl Reader + ?Sized), id: &str) -> Vec<AssignRecord> {
        let Ok(seen) = Seen::of(who) else { return Vec::new() };
        let cache = self.tickets.read().expect("store lock poisoned");
        seen.sees(&cache).resolve(id).map(|key| cache.assignments.records(&key).to_vec()).unwrap_or_default()
    }
}
