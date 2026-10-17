//! ACT-2: **`update_many`** — the same update to many tickets in one write,
//! all of it or none of it. Owner, 2026-10-13: *"all the updates that are
//! available should be available one at a time and in aggregate."*
//!
//! **What it moves.** Each ticket gets [`super::update`]'s change, by
//! `update`'s rules (MENGER-30): its kind, status, parent, severity,
//! blockers — and its labels, `label`'s. A title and a body are each
//! ticket's own and are refused here: `update` changes them one at a time.
//!
//! **Which tickets.** Named ([`Selection::Ids`]: trimmed, upper-cased, each
//! once, in the order given), or the tickets a filter admits *at the write*
//! ([`Selection::Filter`]: chosen under the write's serial lock, in the
//! store's order), at most `max` of them — [`UPDATE_MANY_MAX`] unless asked
//! — and more is refused, naming the count. Before any rule, the caller's
//! `may` is asked of the whole selection (the activation's grants: one
//! missing refuses the write, naming it).
//!
//! **All or none.** Every ticket's update is planned against the cache as
//! it stands, as `update` plans one; a ticket that names nothing, or that
//! any rule refuses, refuses the write, and the answer lists each refused
//! id with every reason ([`StoreError::RefusedMany`]). Since every ticket
//! gets the same parent and the same blockers, a cycle among the moved
//! tickets is a cycle for one of them as the cache stands — a parent under
//! one of its own, a blocker that waits on it — refused for the tickets
//! involved. A ticket nothing would change for is no write and is answered
//! unchanged.
//!
//! **One write** ([`Store::in_one_write`]): each ticket's steps in turn,
//! against the cache as the tickets before it left it — `update`'s steps,
//! a status a cascade earlier in the write already gave it skipped — in one
//! transaction under the cache's write lock; a failed commit reads the cache
//! back from the file. The write's transitions carry its `message`; its
//! first is the cause of every other one of the caller's ([`Act::cause`]),
//! so the write reads as one in each ticket's history. Its events are one
//! write's ([`Gathered`]), then one [`Change::UpdatedMany`] — before the
//! filtered moves, which are a write's last.
//!
//! **`label` and `link_commits` over many ids** ([`Store::label_many`],
//! [`Store::link_commits_many`]) are the same shape: every ticket's change
//! checked first, all refused or none, one write, one answer per ticket.

use super::cascade::Gathered;
use super::history::Act;
use super::update::{apply, cell_step, plan};
use super::*;

/// ACT-2: the most tickets a filtered `update_many` takes unless it asks
/// for more (`max`).
pub const UPDATE_MANY_MAX: usize = 500;

/// ACT-2: the most any `update_many` takes, `max` or not — and the most ids
/// one names.
pub const UPDATE_MANY_CEILING: usize = 10_000;

/// ACT-2: which tickets an `update_many` changes.
#[derive(Debug, Clone)]
pub enum Selection {
    /// These, in this order.
    Ids(Vec<String>),
    /// The tickets `lens` admits at the write, at most `max`.
    Filter { lens: Lens, max: usize },
}

/// ACT-2: an `update_many`'s answer — every ticket selected as the write
/// left it (in the selection's order), the ids it changed and the ids it
/// did not, and the aspects that moved in any (in `update`'s order).
#[derive(Debug, Clone, Default)]
pub struct UpdatedMany {
    pub tickets: Vec<Ticket>,
    pub ids: Vec<String>,
    pub unchanged: Vec<String>,
    pub changed: Vec<String>,
}

/// The aspects, in the order `changed` names them.
const ASPECTS: [&str; 8] = ["title", "body", "kind", "status", "parent", "severity", "blocked_by", "labels"];

/// `changed` with `more` merged in, in [`ASPECTS`]' order.
fn merged(changed: &[String], more: &[String]) -> Vec<String> {
    ASPECTS.iter().filter(|a| changed.iter().chain(more).any(|c| c == *a)).map(|a| a.to_string()).collect()
}

/// Ids as a write names them — trimmed, upper-cased, blanks dropped, each
/// once, in the order given.
fn named(ids: &[String]) -> Vec<String> {
    let ids: Vec<String> = ids.iter().map(|i| i.trim().to_ascii_uppercase()).filter(|i| !i.is_empty()).collect();
    rules::distinct(&ids)
}

/// A shortcode's part of an `UpdatedMany` — what `watch { epic }` passes
/// on: the ids in `epic`, when any is. ORG-3: a ticket's shortcode is the one
/// its key was minted in — nothing in ORG-3 moves a ticket out of it (ACT-6,
/// which will, carries the move on the change itself).
pub(super) fn in_epic(ids: Vec<String>, changed: Vec<String>, epic: &ShortcodeId) -> Option<Change> {
    let ids: Vec<String> = ids.into_iter().filter(|id| id.rsplit_once('-').is_some_and(|(e, _)| e == epic.as_str())).collect();
    (!ids.is_empty()).then_some(Change::UpdatedMany { ids, changed })
}

/// [`StoreError::RefusedMany`] in words — each ticket with its reasons,
/// already said (by `to_string`, or the activation's redaction).
pub fn refused(each: Vec<(String, Vec<String>)>) -> String {
    let n = each.len();
    let each: Vec<String> = each.into_iter().map(|(id, why)| format!("{id}: {}", why.join("; "))).collect();
    format!("{n} ticket{} refused, nothing written — {}", if n == 1 { "" } else { "s" }, each.join(" · "))
}

/// What a cell-only change of many tickets moves on one: its labels, or its
/// commits.
enum Moved {
    Labels { added: Vec<Label>, removed: Vec<Label> },
    Commits { added: Vec<CommitRef> },
}

impl Moved {
    fn nothing(&self) -> bool {
        match self {
            Self::Labels { added, removed } => added.is_empty() && removed.is_empty(),
            Self::Commits { added } => added.is_empty(),
        }
    }

    fn change(self, ticket: Ticket) -> Change {
        match self {
            Self::Labels { added, removed } => Change::LabelsChanged { ticket, added, removed },
            Self::Commits { added } => Change::CommitsLinked { ticket, added },
        }
    }
}

impl Store {
    /// [`Store::update_many_by`] as nobody signed in, through the API, with
    /// every ticket allowed.
    pub async fn update_many(&self, sys: &System, select: Selection, update: Update) -> Result<UpdatedMany, StoreError> {
        self.update_many_by(select, update, Act::default(), |ids| Ok(ids.iter().map(|id| sys.updating(id)).collect())).await
    }

    /// ACT-2: the same `update` to every ticket `select` names, in one write
    /// by `act`, all or none (the module doc). `may` is asked of the whole
    /// selection before any rule; its refusal is the write's. ACCESS-11:
    /// it answers the proofs — one [`Updating`] per ticket selected, each
    /// checked, under the write-serial lock, to be for exactly that ticket
    /// and what the update names beyond it.
    pub async fn update_many_by(
        &self,
        select: Selection,
        update: Update,
        mut act: Act,
        may: impl FnOnce(&[String]) -> Result<Vec<Updating>, String> + Send,
    ) -> Result<UpdatedMany, StoreError> {
        if !update.refused.is_empty() {
            return Err(StoreError::Invalid(update.refused.join("; ")));
        }
        if update.title.is_some() || update.body.is_some() {
            return Err(StoreError::Invalid(
                "update_many: a title and a body are each ticket's own — change them one at a time with update".into(),
            ));
        }
        let serial = self.serial().await;
        let ids = self.selected(select)?;
        if ids.is_empty() {
            return Ok(UpdatedMany::default());
        }
        let proofs = may(&ids).map_err(StoreError::Invalid)?;
        {
            let cache = self.tickets.read().expect("store lock poisoned");
            writs::covers_exactly(&cache, proofs.iter().map(Updating::ticket), &ids)?;
        }
        let (mut plans, mut refusals, mut unchanged) = (Vec::new(), Vec::new(), Vec::new());
        for id in &ids {
            let current = match self.get_unscoped(id).await {
                Ok(t) => t,
                Err(e) => {
                    refusals.push((id.clone(), vec![e]));
                    continue;
                }
            };
            let mut refused = Vec::new();
            let p = {
                let cache = self.tickets.read().expect("store lock poisoned");
                let proof = proofs.iter().find(|u| writs::key_of(&cache, u.id()).as_deref() == Some(current.id.as_str()));
                let row = cache.get(&current.id);
                match (proof, row) {
                    (Some(u), Some(row)) => u.check(&cache, row, &update)?,
                    _ => return Err(StoreError::Invalid(format!("{}: the write was not given the proof it needs (nothing written)", current.id))),
                }
                plan(&cache, &current, update.clone(), None, None, &mut refused)
            };
            if !refused.is_empty() {
                refusals.push((String::from(current.id), refused));
            } else if p.changed.is_empty() {
                unchanged.push(current.id);
            } else {
                plans.push(p);
            }
        }
        if !refusals.is_empty() {
            return Err(StoreError::RefusedMany { refusals });
        }
        if plans.is_empty() {
            let mut tickets = Vec::with_capacity(ids.len());
            for id in &ids {
                tickets.push(self.get_unscoped(id).await?);
            }
            return Ok(UpdatedMany { tickets, ids: Vec::new(), unchanged: unchanged.into_iter().map(String::from).collect::<Vec<String>>(), changed: Vec::new() });
        }
        self.in_one_write(serial, move |conn, cache, unders| {
            super::revisions::act(&act);
            let txn = conn.transaction()?;
            let now = chrono::Utc::now().timestamp();
            let mut out = Gathered::default();
            let (mut moved, mut changed) = (Vec::with_capacity(plans.len()), Vec::new());
            for p in plans {
                moved.push(p.id.clone());
                changed = merged(&changed, &p.changed);
                apply(&txn, cache, unders, p, &act, now, &mut out)?;
                if act.cause.is_none() {
                    act.cause = out.first_transition();
                }
            }
            let mut tickets = Vec::with_capacity(ids.len());
            for id in &ids {
                let row = cache.get(id).cloned().ok_or_else(|| StoreError::NotFound(id.clone()))?;
                let body = db::body(&txn, id)?.ok_or_else(|| missing(id))?;
                tickets.push(whole(row, body));
            }
            super::revisions::commit(txn)?;
            let changes = out.finish_with(Change::UpdatedMany { ids: moved.clone(), changed: changed.clone() });
            Ok((UpdatedMany { tickets, ids: moved, unchanged: unchanged.into_iter().map(String::from).collect::<Vec<String>>(), changed }, changes))
        })
        .await
    }

    /// ACT-2: the ids a selection names, as the store stands — at most the
    /// ceiling named, at most `max` a filter admits (more is refused, naming
    /// the count).
    pub(super) fn selected(&self, select: Selection) -> Result<Vec<String>, StoreError> {
        match select {
            Selection::Ids(ids) => {
                let ids = named(&ids);
                if ids.len() > UPDATE_MANY_CEILING {
                    return Err(StoreError::Invalid(format!(
                        "{} ids named — one write takes at most {UPDATE_MANY_CEILING}; nothing written",
                        ids.len()
                    )));
                }
                Ok(ids)
            }
            Selection::Filter { lens, max } => {
                if max > UPDATE_MANY_CEILING {
                    return Err(StoreError::Invalid(format!("max {max}: one write takes at most {UPDATE_MANY_CEILING}")));
                }
                let page = self.find_filtered_unscoped("", &lens, None, Some(max as u32))?;
                if page.remaining > 0 {
                    let n = page.tickets.len() as u64 + page.remaining;
                    return Err(StoreError::Invalid(format!(
                        "the filter admits {n} tickets, over the {max} one write takes (raise max, or narrow the filter); nothing written"
                    )));
                }
                Ok(page.tickets.into_iter().map(|t| t.id.into_string()).collect())
            }
        }
    }

    /// ACT-2: `label` over many tickets — `add` added to and `remove` taken
    /// off each, by `label`'s rules, in one write, all or none (a ticket
    /// that names nothing, or whose labels would be refused, refuses the
    /// write, each named). One `LabelsChanged` per ticket, in the order
    /// named — empty where nothing moved, which is no write.
    /// ACCESS-11: of the tickets the permits `on` are for.
    pub async fn label_many(&self, on: Vec<Permit<Editor>>, add: Vec<Label>, remove: Vec<Label>) -> Result<Vec<Change>, StoreError> {
        let ids: Vec<String> = on.iter().map(|p| p.id().to_string()).collect();
        self.cells_many(&ids, |row| {
            let (after, added, removed) =
                crate::labels::apply(&row.labels, &add, &remove).map_err(|why| StoreError::Labels { id: row.id.to_string(), why })?;
            let mut row = row.clone();
            row.labels = after;
            Ok((row, Moved::Labels { added, removed }))
        })
        .await
    }

    /// ACT-2: `link_commits` over many tickets — every reference each does
    /// not carry yet appended, by `link_commits`' rules, in one write, all
    /// or none. One `CommitsLinked` per ticket, in the order named — empty
    /// where nothing was added, which is no write.
    /// ACCESS-11: of the tickets the permits `on` are for.
    pub async fn link_commits_many(&self, on: Vec<Permit<Editor>>, refs: Vec<CommitRef>) -> Result<Vec<Change>, StoreError> {
        let ids: Vec<String> = on.iter().map(|p| p.id().to_string()).collect();
        self.cells_many(&ids, |row| {
            let mut known: HashSet<CommitRef> = row.commits.iter().cloned().collect();
            let added: Vec<CommitRef> = refs.iter().filter(|r| known.insert((*r).clone())).cloned().collect();
            if row.commits.len() + added.len() > commits::MAX_PER_TICKET {
                return Err(StoreError::TooManyCommits { id: row.id.to_string(), linked: row.commits.len(), adding: added.len() });
            }
            let mut row = row.clone();
            row.commits.extend(added.iter().cloned());
            Ok((row, Moved::Commits { added }))
        })
        .await
    }

    /// A change of each ticket `ids` names that moves only its own row and
    /// cell — `what` of it, as the cache stands — checked for every ticket
    /// first, then one write.
    async fn cells_many(&self, ids: &[String], what: impl Fn(&Row) -> Result<(Row, Moved), StoreError>) -> Result<Vec<Change>, StoreError> {
        let ids = named(ids);
        if ids.is_empty() || ids.len() > UPDATE_MANY_CEILING {
            return Err(StoreError::Invalid(format!("name 1 to {UPDATE_MANY_CEILING} tickets")));
        }
        let serial = self.serial().await;
        let (mut steps, mut refusals) = (Vec::with_capacity(ids.len()), Vec::new());
        {
            let cache = self.tickets.read().expect("store lock poisoned");
            for id in &ids {
                match cache.get(id).ok_or_else(|| StoreError::NotFound(id.clone())).and_then(&what) {
                    Ok(step) => steps.push(step),
                    Err(e) => refusals.push((id.clone(), vec![e])),
                }
            }
        }
        if !refusals.is_empty() {
            return Err(StoreError::RefusedMany { refusals });
        }
        if steps.iter().all(|(_, m)| m.nothing()) {
            let mut answer = Vec::with_capacity(steps.len());
            for (row, moved) in steps {
                answer.push(moved.change(self.get_unscoped(&row.id).await?));
            }
            return Ok(answer);
        }
        self.in_one_write(serial, move |conn, cache, unders| {
            let txn = conn.transaction()?;
            let now = chrono::Utc::now().timestamp();
            let mut out = Gathered::default();
            let mut answer = Vec::with_capacity(steps.len());
            for (mut row, moved) in steps {
                if moved.nothing() {
                    let body = db::body(&txn, &row.id)?.ok_or_else(|| missing(&row.id))?;
                    let now_row = cache.get(&row.id).cloned().unwrap_or(row);
                    answer.push(moved.change(whole(now_row, body)));
                    continue;
                }
                row.updated_unix = now;
                // DEPLOY-20: one call's commits are each ticket's next version.
                if let Moved::Commits { added } = &moved {
                    let version = cache.deploys.next_version(&row.id);
                    super::versions::store(&txn, &row.id, added, version, now)?;
                    cache.deploys.versioned(&row.id, added, version, now);
                }
                cell_step(&txn, cache, unders, row, &mut out, |t| moved.change(t))?;
                answer.push(out.tickets.last().cloned().expect("the step's own change"));
            }
            super::revisions::commit(txn)?;
            Ok((answer, out.finish()))
        })
        .await
    }
}
