//! ARCHIVE-2: **the archive** — `archive` and `restore`, a mark a ticket
//! carries apart from its status (ARCHIVE-1, decided by the owner
//! 2026-10-15).
//!
//! **The mark.** A row's `archived` ([`Archived`]: when, by whom, why, and
//! the write that set it). Any ticket may carry it, of any kind and in any
//! status; it moves nothing else — status, kind, place, blockers, labels,
//! inputs and history stay as they were, so an archived Complete ticket
//! still satisfies what waits on it and an archived open one still blocks.
//!
//! **Kept as a cell** ([`super::cells::Key::archived`]): an archived ticket
//! never shares a cell with a live one, so a view that does not name the
//! archive admits no cell that holds one ([`crate::filter::Filter::matches`]'s
//! default) and walks none of them. The roll-ups count it apart
//! (`archived` beside `docs`, [`super::rollup`]); `--verify-derived` builds
//! both from the rows and compares.
//!
//! **Archive** takes ids or a filter and archives each ticket named and its
//! whole subtree (the kept tree: children, theirs, and the shortcodes
//! hosted under it), live work included — never refused for it: every
//! ticket not in category done is named in the answer's `live`, the
//! warning. A ticket already archived is no-op. A queued ticket leaves the
//! queue (recorded as by the archive): an archived ticket asks nothing.
//!
//! **Restore** takes ids or a filter (of archived tickets) and brings each
//! back, with its archived ancestors — the chain, never their other
//! children (ARCHIVE-1 `restore-under`) — and, `with_children`, the
//! descendants archived by the same write as it. The rest of its subtree
//! stays archived, and the answer names it (`still_archived`).
//!
//! **A create or a move under an archived parent** restores that parent's
//! archived chain ([`Store::restore_chain_by`], ARCHIVE-1
//! `child-of-archived`), named in the answer — the write right after the
//! create or the move, under the caller's permits for the chain, minted
//! before the create or the move is sent.
//!
//! **One write each** ([`Store::in_one_write`]): every ticket's mark, its
//! cell and the kept tree, the roll-ups, its transition (field `archive`,
//! `live` ↔ `archived`, the write's first the cause of the rest) and its
//! queue entry, in one transaction; then one [`Change::Archived`] or
//! [`Change::Restored`], before the filtered moves (a filter's `left` /
//! `entered`). `dry_run` answers what the write would, and writes nothing.

use super::cascade::Gathered;
use super::history::Act;
use super::update_many::{Selection, UPDATE_MANY_CEILING};
use super::*;
use crate::filter::{category_of, Category};
use crate::queue::QueueAction;
use crate::types::{Archived, LiveTicket, Transition, TransitionField, ARCHIVE_ARCHIVED, ARCHIVE_LIVE};

/// ARCHIVE-2: what `archive` did (or, `dry_run`, would do).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArchiveAnswer {
    /// Every ticket archived, in the store's order.
    pub ids: Vec<TicketId>,
    /// Those of them not in category done — the warning.
    pub live: Vec<LiveTicket>,
    /// Those of them it took out of the queue.
    pub unqueued: Vec<TicketId>,
    /// The mark each now carries (`write` 0 on a dry run).
    pub mark: Archived,
    pub dry_run: bool,
}

/// ARCHIVE-2: what `restore` did (or, `dry_run`, would do).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RestoreAnswer {
    /// Every ticket brought back, in the store's order.
    pub ids: Vec<TicketId>,
    /// Those of them brought back as an archived ancestor of one named.
    pub ancestors: Vec<TicketId>,
    /// The named tickets' descendants that stay archived.
    pub still_archived: Vec<TicketId>,
    pub by: String,
    pub dry_run: bool,
}

/// The tickets' keys, in the store's order, each once.
fn ordered_ids(cache: &Cache, ids: impl IntoIterator<Item = String>) -> Vec<String> {
    let mut keyed: Vec<(OrderKey, String)> = ids.into_iter().filter_map(|id| cache.at.get(&id).map(|k| (k.clone(), id))).collect();
    keyed.sort();
    keyed.dedup();
    keyed.into_iter().map(|(_, id)| id).collect()
}

/// `row` and its whole subtree (the kept tree, into hosted shortcodes).
fn subtree_of<'c>(cache: &'c Cache, row: &Row) -> Vec<&'c Row> {
    let mut out = Vec::new();
    for (epic, seqs) in lineage::subtree(cache, &row.id, Some(row)) {
        for seq in seqs {
            if let Some(t) = cache.order.get(&OrderKey { shortcode: epic.clone(), seq }) {
                out.push(t);
            }
        }
    }
    out
}

/// `id`'s archived ancestors — its parent, that one's, … as far up as the
/// chain goes, each one archived.
fn archived_ancestors(cache: &Cache, row: &Row) -> Vec<String> {
    tree::ancestors(cache.rows(), &row.id, row.parent.as_deref())
        .into_iter()
        .filter(|a| cache.get(a).is_some_and(|r| r.archived.is_some()))
        .map(str::to_string)
        .collect()
}

/// The keys `ids` name (any address), or the refusal naming each that
/// names none.
fn resolved(cache: &Cache, ids: &[String]) -> Result<Vec<String>, StoreError> {
    let (mut out, mut refusals) = (Vec::new(), Vec::new());
    for id in ids {
        match cache.resolve(id) {
            Ok(k) if cache.get(&k).is_some() => out.push(k),
            Ok(k) => refusals.push((id.clone(), vec![StoreError::NotFound(k)])),
            Err(e) => refusals.push((id.clone(), vec![e])),
        }
    }
    match refusals.is_empty() {
        true => Ok(out),
        false => Err(StoreError::RefusedMany { refusals }),
    }
}

/// An archive transition of `ticket`, `to` archived or live, by `act`.
fn transition(id: u64, ticket: &str, archiving: bool, at: i64, act: &Act, cause: Option<(u64, String)>) -> Transition {
    let (from, to) = if archiving { (ARCHIVE_LIVE, ARCHIVE_ARCHIVED) } else { (ARCHIVE_ARCHIVED, ARCHIVE_LIVE) };
    Transition {
        id,
        ticket: TicketId::new(ticket),
        field: TransitionField::Archive,
        from: Some(from.to_string()),
        to: to.to_string(),
        at_unix: at,
        actor: act.actor.clone(),
        name: act.name.clone(),
        source: act.source,
        message: act.message.clone(),
        cause: cause.as_ref().map(|(c, _)| *c),
        cause_ticket: cause.map(|(_, t)| t),
    }
}

/// One ticket's mark set (`Some`) or cleared, in the write: its row, its
/// cell and the kept tree, the roll-ups, and its transition — into `txn`
/// and the cache, worked out against the cache as the steps before it left
/// it; its transition and groups into `out`.
fn mark_step(
    txn: &rusqlite::Transaction<'_>,
    cache: &mut Cache,
    unders: &lineage::UnderWatches,
    id: &str,
    mark: Option<Archived>,
    t: Transition,
    out: &mut Gathered,
) -> Result<(), StoreError> {
    let old = cache.get(id).cloned().ok_or_else(|| StoreError::NotFound(id.to_string()))?;
    let mut row = old.clone();
    row.archived = mark;
    let derived = {
        let rows = cache.rows().with(&row);
        let mut rollups = cache.rollups.clone();
        let groups = rollups.archive_moved(rows, &row, &old);
        let derived = if groups.is_empty() { Derived::none() } else { Derived::rolled(cache, rollups, groups) };
        let count = cache.ready.count(&row.id);
        let (cells, tree) = cache.recell(rows, &row, false, &[(&row, count, count)], None);
        derived.celled(cells).treed(tree)
    };
    let under = lineage::under_before_with(unders, cache, || lineage::Touched::of_moves(derived.announced().as_deref()).row(&row));
    let announced = derived.announced();
    let Derived { stored, moved } = derived;
    db::update_row(txn, &row)?;
    stored.write(txn)?;
    history::record(txn, &t)?;
    let key = OrderKey::of(&row);
    let shortcode = row.shortcode.clone();
    cache.insert(row);
    let groups = cache.take(moved);
    cache.log.append(&t, &key);
    out.tickets.push(Change::Transition { shortcode, transition: t });
    out.groups(groups);
    out.filtered(announced, under.and_then(|u| u.after_in(cache)));
    Ok(())
}

/// `id` out of the queue because it was archived — its record, the entry
/// gone, said on `watch` — when it is queued.
fn unqueue_step(txn: &rusqlite::Transaction<'_>, cache: &mut Cache, unders: &lineage::UnderWatches, id: &str, act: &Act, now: i64, out: &mut Gathered) -> Result<bool, StoreError> {
    let Some(entry) = cache.queue.get(id).cloned() else { return Ok(false) };
    let row = cache.get(id).cloned().ok_or_else(|| StoreError::NotFound(id.to_string()))?;
    let before = lineage::queued_before_with(unders, cache, &[(&row.shortcode, row.seq)]);
    let record = queue::record_of(cache.queue.next_id(), &entry, QueueAction::Archived, act, now);
    queue::store(txn, id, None, &record)?;
    cache.queue.put(id, None, record);
    let change = queue::QueueChange::Unqueued { id: id.to_string(), entry, action: QueueAction::Archived, by: act.actor.clone(), at_unix: now };
    out.tickets.push(Change::Queue { shortcode: row.shortcode.clone(), change });
    out.filtered(None, before.and_then(|u| u.after_in(cache)));
    Ok(true)
}

/// What a restore of `named` (keys) brings back, as the cache stands: each
/// named ticket that is archived, its archived ancestors, and —
/// `with_children` — its descendants archived by the same write; and the
/// named tickets' descendants that stay archived.
fn restoring(cache: &Cache, named: &[String], with_children: bool) -> (Vec<String>, Vec<String>, Vec<String>) {
    let (mut back, mut chain, mut below) = (Vec::new(), Vec::new(), Vec::new());
    for id in named {
        let Some(row) = cache.get(id) else { continue };
        if let Some(mark) = &row.archived {
            back.push(row.id.to_string());
            for t in subtree_of(cache, row).into_iter().filter(|t| t.id != row.id) {
                match &t.archived {
                    Some(m) if with_children && m.write == mark.write => back.push(t.id.to_string()),
                    Some(_) => below.push(t.id.to_string()),
                    None => {}
                }
            }
        }
        chain.extend(archived_ancestors(cache, row));
    }
    let chain: Vec<String> = ordered_ids(cache, chain.into_iter().filter(|a| !named.contains(a)));
    let back = ordered_ids(cache, back.into_iter().chain(chain.iter().cloned()));
    let below = ordered_ids(cache, below.into_iter().filter(|b| !back.contains(b)));
    (back, chain, below)
}

impl Store {
    /// [`Store::archive_by`] as nobody signed in, through the API, every
    /// ticket allowed.
    pub async fn archive(&self, sys: &System, select: Selection, dry_run: bool) -> Result<ArchiveAnswer, StoreError> {
        self.archive_by(select, Act::default(), dry_run, |ids| Ok(sys.permits(ids))).await
    }

    /// ARCHIVE-2: each ticket `select` names and its whole subtree archived
    /// in one write by `act` (the module doc); `dry_run`, the answer and
    /// nothing written. `may` is asked of every ticket the write would
    /// archive, descendants included, before anything is written — its
    /// refusal is the write's; ACCESS-11: it answers one permit per ticket,
    /// checked under the write-serial lock to cover exactly them.
    pub async fn archive_by(
        &self,
        select: Selection,
        act: Act,
        dry_run: bool,
        may: impl FnOnce(&[String]) -> Result<Vec<Permit<Editor>>, String> + Send,
    ) -> Result<ArchiveAnswer, StoreError> {
        let serial = self.serial().await;
        let now = chrono::Utc::now().timestamp();
        let named = self.selected(select)?;
        let (ids, live, queued, write) = {
            let cache = self.tickets.read().expect("store lock poisoned");
            let mut all = Vec::new();
            for key in resolved(&cache, &named)? {
                let row = cache.get(&key).expect("resolved");
                all.extend(subtree_of(&cache, row).into_iter().filter(|t| t.archived.is_none()).map(|t| t.id.to_string()));
            }
            let ids = ordered_ids(&cache, all);
            let live: Vec<LiveTicket> = ids
                .iter()
                .filter_map(|id| cache.get(id))
                .filter(|t| !matches!(category_of(t.status), Category::Done | Category::Dropped | Category::Static))
                .map(|t| LiveTicket { id: t.id.clone(), status: t.status })
                .collect();
            let queued: Vec<TicketId> = ids.iter().filter(|id| cache.queue.get(id).is_some()).map(TicketId::new).collect();
            (ids, live, queued, cache.log.next_id())
        };
        if ids.len() > UPDATE_MANY_CEILING {
            return Err(StoreError::Invalid(format!("archive: {} tickets with their subtrees — one write takes at most {UPDATE_MANY_CEILING}; nothing written", ids.len())));
        }
        let mark = Archived { at_unix: now, by: act.actor.clone(), message: act.message.clone(), write: if dry_run || ids.is_empty() { 0 } else { write } };
        let answer = ArchiveAnswer { ids: ids.iter().map(TicketId::new).collect(), live, unqueued: queued, mark: mark.clone(), dry_run };
        let proofs = may(&ids).map_err(StoreError::Invalid)?;
        {
            let cache = self.tickets.read().expect("store lock poisoned");
            writs::covers_exactly(&cache, proofs.iter(), &ids)?;
        }
        if dry_run || ids.is_empty() {
            return Ok(answer);
        }
        self.in_one_write(serial, move |conn, cache, unders| {
            let txn = conn.transaction()?;
            let mut out = Gathered::default();
            let (mut next, mut cause) = (cache.log.next_id(), None);
            for id in &ids {
                let t = transition(next, id, true, now, &act, cause.clone());
                cause = cause.or(Some((next, id.clone())));
                next += 1;
                mark_step(&txn, cache, unders, id, Some(mark.clone()), t, &mut out)?;
                unqueue_step(&txn, cache, unders, id, &act, now, &mut out)?;
            }
            txn.commit()?;
            let last = Change::Archived { ids: answer.ids.clone(), live: answer.live.clone(), unqueued: answer.unqueued.clone(), mark };
            Ok((answer, out.finish_with(last)))
        })
        .await
    }

    /// [`Store::restore_by`] as nobody signed in, through the API, every
    /// ticket allowed.
    pub async fn restore(&self, sys: &System, select: Selection, with_children: bool, dry_run: bool) -> Result<RestoreAnswer, StoreError> {
        self.restore_by(select, with_children, Act::default(), dry_run, |ids| Ok(sys.permits(ids))).await
    }

    /// ARCHIVE-2: each archived ticket `select` names brought back, with its
    /// archived ancestors and (`with_children`) the descendants archived by
    /// the same write, in one write by `act` (the module doc); `dry_run`,
    /// the answer and nothing written. `may` as [`Store::archive_by`]'s, of
    /// every ticket it would bring back.
    pub async fn restore_by(
        &self,
        select: Selection,
        with_children: bool,
        act: Act,
        dry_run: bool,
        may: impl FnOnce(&[String]) -> Result<Vec<Permit<Editor>>, String> + Send,
    ) -> Result<RestoreAnswer, StoreError> {
        let serial = self.serial().await;
        let named = self.selected(select)?;
        let (ids, chain, below) = {
            let cache = self.tickets.read().expect("store lock poisoned");
            restoring(&cache, &resolved(&cache, &named)?, with_children)
        };
        self.restore_write(serial, ids, chain, below, act, dry_run, may).await
    }

    /// ARCHIVE-2 (ARCHIVE-1 `child-of-archived`): the archived chain of
    /// `parent` — itself when it is archived, and every archived ancestor —
    /// as a create or a move under it leaves it: the tickets it would bring
    /// back, in the store's order. Empty: nothing above is archived.
    pub fn archived_chain(&self, parent: &str) -> Vec<String> {
        let cache = self.tickets.read().expect("store lock poisoned");
        let Some(row) = cache.resolve(parent).ok().and_then(|k| cache.get(&k)) else { return Vec::new() };
        let own = row.archived.is_some().then(|| row.id.to_string());
        ordered_ids(&cache, own.into_iter().chain(archived_ancestors(&cache, row)))
    }

    /// ARCHIVE-2: `chain` ([`Store::archived_chain`], as a create or a move
    /// found it) brought back, after the create or the move it follows — each
    /// still archived, nothing else (`still_archived` is empty: a chain
    /// restore names the chain). `None`: nothing of it is archived now.
    /// ACCESS-11: `permits` are the chain's, minted before the write it
    /// follows; those of the tickets still archived are the write's.
    pub async fn restore_chain_by(&self, chain: Vec<String>, act: Act, permits: Vec<Permit<Editor>>) -> Result<Option<RestoreAnswer>, StoreError> {
        let serial = self.serial().await;
        let ids = {
            let cache = self.tickets.read().expect("store lock poisoned");
            ordered_ids(&cache, chain.into_iter().filter(|id| cache.get(id).is_some_and(|r| r.archived.is_some())))
        };
        if ids.is_empty() {
            return Ok(None);
        }
        let kept: Vec<Permit<Editor>> = permits.into_iter().filter(|p| ids.iter().any(|i| i == p.id().as_str())).collect();
        self.restore_write(serial, ids.clone(), ids, Vec::new(), act, false, move |_| Ok(kept)).await.map(Some)
    }

    /// The restore of `ids` (`chain` the ancestors among them, `below` what
    /// stays archived) as one write — or, `dry_run`, its answer.
    #[allow(clippy::too_many_arguments)]
    async fn restore_write(
        &self,
        serial: OwnedMutexGuard<()>,
        ids: Vec<String>,
        chain: Vec<String>,
        below: Vec<String>,
        act: Act,
        dry_run: bool,
        may: impl FnOnce(&[String]) -> Result<Vec<Permit<Editor>>, String> + Send,
    ) -> Result<RestoreAnswer, StoreError> {
        if ids.len() > UPDATE_MANY_CEILING {
            return Err(StoreError::Invalid(format!("restore: {} tickets — one write takes at most {UPDATE_MANY_CEILING}; nothing written", ids.len())));
        }
        let tids = |v: &[String]| v.iter().map(TicketId::new).collect::<Vec<_>>();
        let answer = RestoreAnswer { ids: tids(&ids), ancestors: tids(&chain), still_archived: tids(&below), by: act.actor.clone(), dry_run };
        let proofs = may(&ids).map_err(StoreError::Invalid)?;
        {
            let cache = self.tickets.read().expect("store lock poisoned");
            writs::covers_exactly(&cache, proofs.iter(), &ids)?;
        }
        if dry_run || ids.is_empty() {
            return Ok(answer);
        }
        let now = chrono::Utc::now().timestamp();
        self.in_one_write(serial, move |conn, cache, unders| {
            let txn = conn.transaction()?;
            let mut out = Gathered::default();
            let (mut next, mut cause) = (cache.log.next_id(), None);
            for id in &ids {
                let t = transition(next, id, false, now, &act, cause.clone());
                cause = cause.or(Some((next, id.clone())));
                next += 1;
                mark_step(&txn, cache, unders, id, None, t, &mut out)?;
            }
            txn.commit()?;
            let last = Change::Restored { ids: answer.ids.clone(), ancestors: answer.ancestors.clone(), still_archived: answer.still_archived.clone(), by: answer.by.clone() };
            Ok((answer, out.finish_with(last)))
        })
        .await
    }
}

/// A shortcode's part of an archive or a restore — what `watch { epic }`
/// passes on: its ids in `epic` (by the key's shortcode, split at its last
/// hyphen: ORG-9), when any is.
pub(super) fn in_epic(change: Change, epic: &ShortcodeId) -> Option<Change> {
    let mine = |id: &TicketId| crate::address::split_key(id.as_str()).is_some_and(|(sc, _)| sc == epic.as_str());
    let keep = |v: Vec<TicketId>| v.into_iter().filter(|i| mine(i)).collect::<Vec<_>>();
    match change {
        Change::Archived { ids, live, unqueued, mark } => {
            let ids = keep(ids);
            let live = live.into_iter().filter(|l| mine(&l.id)).collect();
            (!ids.is_empty()).then(|| Change::Archived { ids, live, unqueued: keep(unqueued), mark })
        }
        Change::Restored { ids, ancestors, still_archived, by } => {
            let ids = keep(ids);
            (!ids.is_empty()).then(|| Change::Restored { ids, ancestors: keep(ancestors), still_archived: keep(still_archived), by })
        }
        other => Some(other),
    }
}
