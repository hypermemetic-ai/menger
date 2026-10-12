//! MENGER-13: **cancellation cascades**, and their undoing.
//!
//! Setting a ticket to `Canceled` cancels, in the same write, every
//! descendant that is still open work — every ticket of its subtree (the
//! children index, into hosted shortcodes too) whose status is not
//! `Complete`, `Superseded` or `Canceled`; a doc has no lifecycle and keeps
//! `Doc`. Each is a transition of its own: `system`'s, from `cascade`, its
//! message naming its cause and who made it ("canceled because MENGER-7 was
//! canceled by ada"), its `cause` the canceled ticket's transition. Each
//! canceled ticket's open inputs are withdrawn with it (the ticket's own
//! too, cascade or none).
//!
//! **Undo.** A ticket leaving `Canceled` gets its withdrawn inputs back; and,
//! asked (`restore_children`), the descendants its latest cancel canceled —
//! found through `cause`, each still `Canceled` with nothing moved since —
//! go back to the statuses they had, in the same write, each a transition of
//! `system`'s caused by the restore. A descendant canceled on its own, or by
//! another ticket's cancel, or moved since, stays as it is.
//!
//! **One write** ([`Store::batch`]). A step's derived change — the roll-ups,
//! the status counts and the ready set, the placements, the cells, the kept
//! tree — is worked out against the cache as the steps before it left it,
//! so the batch holds the cache's write lock from its first step to its
//! commit: each step's rows and derived state go into one transaction and
//! into the cache in turn, no reader sees a step the file does not hold
//! (none sees the cache at all until the commit), and a commit that fails
//! puts the cache back to the file, which is as it was ([`Store::load_cache`]).
//! Then every step's events are broadcast, in order. A status change that
//! touches no other ticket takes the ordinary write path
//! ([`Store::set_status_by`]), lock-free for readers as every write is.

use super::history::Act;
use super::*;
use crate::types::{Transition, TransitionField, TransitionSource, SYSTEM_ACTOR};

/// Whether `t` is open work a cancel carries down to: it has a lifecycle and
/// is not done with.
fn open_work(t: &Row) -> bool {
    t.kind.has_lifecycle() && !matches!(t.status, Status::Complete | Status::Superseded | Status::Canceled)
}

/// The descendants of `root` its cancel cancels — its subtree's open work,
/// in the store's order.
pub(super) fn to_cancel<'c>(cache: &'c Cache, root: &Row) -> Vec<&'c Row> {
    let mut out = Vec::new();
    for (epic, bm) in lineage::subtree(cache, &root.id, Some(root)) {
        for seq in bm {
            if let Some(t) = cache.order.get(&OrderKey { epic: epic.clone(), seq }) {
                if t.id != root.id && open_work(t) {
                    out.push(t);
                }
            }
        }
    }
    out
}

/// The descendants `row`'s latest cancel canceled and nothing has moved
/// since — each with the status it had — in the store's order: what a
/// restore puts back. Nothing unless `row` is canceled.
pub(super) fn restorable<'c>(cache: &'c Cache, row: &Row) -> Vec<(&'c Row, Status)> {
    if row.status != Status::Canceled {
        return Vec::new();
    }
    let log = &cache.log;
    let Some(cancel) = log.latest(&row.epic, row.seq, TransitionField::Status) else { return Vec::new() };
    if log.mark(cancel).status_after() != Some(Status::Canceled) {
        return Vec::new();
    }
    let mut out: Vec<(&Row, Status)> = Vec::new();
    for &p in log.caused_by(log.mark(cancel).id) {
        let m = log.mark(p);
        if m.source != TransitionSource::Cascade || m.status_after() != Some(Status::Canceled) {
            continue;
        }
        let key = log.key_of(p);
        let Some(t) = cache.order.get(&key) else { continue };
        if t.status != Status::Canceled || log.latest(&key.epic, key.seq, TransitionField::Status) != Some(p) {
            continue;
        }
        if let Some(from) = m.status_before() {
            out.push((t, from));
        }
    }
    out.sort_by_key(|(t, _)| OrderKey::of(t));
    out
}

/// A status transition of `ticket`, by `act`.
fn status_transition(id: u64, ticket: &str, from: Status, to: Status, at: i64, act: &Act, cause: Option<(u64, &str)>) -> Transition {
    Transition {
        id,
        ticket: ticket.to_string(),
        field: TransitionField::Status,
        from: Some(from.as_str().to_string()),
        to: to.as_str().to_string(),
        at_unix: at,
        actor: act.actor.clone(),
        name: act.name.clone(),
        source: act.source,
        message: act.message.clone(),
        cause: cause.map(|(c, _)| c),
        cause_ticket: cause.map(|(_, t)| t.to_string()),
    }
}

/// The store's own act: `system`, from `cascade`, saying `message`.
fn system(message: String) -> Act {
    Act { actor: SYSTEM_ACTOR.into(), name: None, source: TransitionSource::Cascade, message: Some(message) }
}

/// One step of a batch: a ticket's new status, the transition that records
/// it (none when the status is the one it has), and its inputs' moves.
pub(super) struct Step {
    pub(super) id: String,
    pub(super) to: Status,
    pub(super) transition: Option<Transition>,
    pub(super) inputs: Vec<(Input, crate::types::InputHistoryEntry)>,
}

/// MENGER-13: the steps a change of `row`'s status to `status` by `act`
/// makes, from the cache as it stands — its own (the transition, none when
/// the status is the one it has; the inputs a crossing of `Canceled`
/// moves), then, into `Canceled`, each open descendant's; out of it, with
/// `restore_children`, each one its cancel canceled. MENGER-30: shared by
/// `set_status` and `update`.
pub(super) fn status_steps(cache: &Cache, row: &Row, status: Status, act: &Act, restore_children: bool, now: i64) -> Vec<Step> {
    let previous = row.status;
    let canceling = previous != Status::Canceled && status == Status::Canceled;
    let uncanceling = previous == Status::Canceled && status != Status::Canceled;
    let mut next = cache.log.next_id();
    let own = (previous != status).then(|| {
        let t = status_transition(next, &row.id, previous, status, now, act, None);
        next += 1;
        t
    });
    let cause = own.as_ref().map(|t| (t.id, row.id.as_str()));
    let who = act.who().to_string();
    let mut steps = Vec::new();
    let own_inputs = if canceling {
        let reason = format!("{} was canceled by {who}", row.id);
        cache.inputs.withdrawing(&row.id, &reason, now)
    } else if uncanceling {
        let reason = format!("{} is {} again (by {who})", row.id, status.as_str());
        cache.inputs.restoring(&row.id, &reason, now)
    } else {
        Vec::new()
    };
    steps.push(Step { id: row.id.clone(), to: status, transition: own, inputs: own_inputs });
    if canceling {
        for t in to_cancel(cache, row) {
            let message = format!("canceled because {} was canceled by {who}", row.id);
            let tr = status_transition(next, &t.id, t.status, Status::Canceled, now, &system(message.clone()), cause);
            next += 1;
            let inputs = cache.inputs.withdrawing(&t.id, &message, now);
            steps.push(Step { id: t.id.clone(), to: Status::Canceled, transition: Some(tr), inputs });
        }
    } else if uncanceling && restore_children {
        for (t, from) in restorable(cache, row) {
            let message = format!("restored to {} because {} was restored by {who}", from.as_str(), row.id);
            let tr = status_transition(next, &t.id, Status::Canceled, from, now, &system(message.clone()), cause);
            next += 1;
            let inputs = cache.inputs.restoring(&t.id, &message, now);
            steps.push(Step { id: t.id.clone(), to: from, transition: Some(tr), inputs });
        }
    }
    steps
}

/// MENGER-13: what `history { id }` answers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoryAnswer {
    /// The ticket's own transitions, oldest first.
    pub own: Vec<Transition>,
    /// The transitions they caused in other tickets, in the order written.
    pub caused: Vec<Transition>,
    /// When the log began (`None`: a log nobody began — a test's).
    pub began: Option<i64>,
    /// For a canceled ticket, what a restore would put back.
    pub restorable: Vec<String>,
}

impl Store {
    /// MENGER-13: `id`'s history — its own transitions, the ones they caused
    /// (a cascade's), when the log began, and (canceled) what a restore puts
    /// back. From the cache alone. [`StoreError::NotFound`] for an id that
    /// names no ticket.
    pub fn history(&self, id: &str) -> Result<HistoryAnswer, StoreError> {
        let id = id.trim().to_ascii_uppercase();
        let cache = self.tickets.read().expect("store lock poisoned");
        let row = cache.get(&id).ok_or_else(|| StoreError::NotFound(id.clone()))?;
        let log = &cache.log;
        let ticket_of = |p: u32| cache.order.get(&log.key_of(p)).map_or_else(|| id.clone(), |t| t.id.clone());
        let cause_of = |p: u32| {
            let c = log.mark(p).cause;
            (c != 0).then(|| log.pos_of(c)).flatten().map(ticket_of)
        };
        let mine = log.of(&row.epic, row.seq);
        let own: Vec<Transition> = mine.iter().map(|p| log.transition(*p, row.id.clone(), cause_of(*p))).collect();
        let caused: Vec<Transition> = mine
            .iter()
            .flat_map(|p| log.caused_by(log.mark(*p).id).iter().copied())
            .map(|p| log.transition(p, ticket_of(p), cause_of(p)))
            .collect();
        let restorable = restorable(&cache, row).into_iter().map(|(t, _)| t.id.clone()).collect();
        Ok(HistoryAnswer { own, caused, began: log.began(), restorable })
    }

    /// [`Store::set_status_by`] as nobody signed in, through the API, with
    /// no children restored — the write as it was before MENGER-13, its
    /// transition and its cascade included.
    pub async fn set_status(&self, id: &str, status: Status) -> Result<Change, StoreError> {
        self.set_status_by(id, status, Act::default(), false).await
    }

    /// Changes the status, write-through: durable in the database before
    /// the cache is updated and subscribers are notified. A change into or
    /// out of `Complete` moves `done` for the group the ticket is drawn in
    /// and every group around it (MNG-10): `StatusChanged`, then one
    /// `GroupChanged` per group, innermost first. Any other change moves
    /// no count and broadcasts no group — AGS-23: unless the ticket is a
    /// shortcode's root, whose status is its group's row
    /// ([`Group::root_row`]): any change of it broadcasts that group.
    ///
    /// SDLC-11: a doc has no lifecycle — `set_status` on one is refused
    /// ([`StoreError::DocHasNoLifecycle`]), whatever status is asked for —
    /// and `Doc` is a doc's own status, refused for any other kind
    /// ([`StoreError::DocStatusOnWork`]). Both before anything is written:
    /// a doc stays in the state it was born in, and only a doc is in it.
    ///
    /// MENGER-13: a change of status is a transition, by `act`, recorded in
    /// the write and said right after its `StatusChanged`. Into `Canceled`,
    /// the ticket's open inputs are withdrawn and its open descendants
    /// canceled with it ([`to_cancel`]); out of it, its withdrawn inputs are
    /// restored and — `restore_children` — the descendants its cancel
    /// canceled put back ([`restorable`]). A write that moves any ticket
    /// beside its own is one batch ([`Store::batch`]). The status it already
    /// has writes as before, and records no transition.
    pub async fn set_status_by(&self, id: &str, status: Status, act: Act, restore_children: bool) -> Result<Change, StoreError> {
        let serial = self.serial().await;
        let (mut row, _) = self.row(id)?;
        rules::status_refusal(&row, status)?;
        let previous = row.status;
        let now = chrono::Utc::now().timestamp();
        // Worked out under the write-serial lock, from the cache as it
        // stands: the transitions, the other tickets, the inputs.
        let mut steps: Vec<Step> = {
            let cache = self.tickets.read().expect("store lock poisoned");
            status_steps(&cache, &row, status, &act, restore_children, now)
        };
        if steps.len() > 1 {
            return self.batch(serial, steps).await;
        }
        let Step { transition, inputs: moved_inputs, .. } = steps.pop().expect("the ticket's own step");
        row.status = status;
        row.updated_unix = now;
        // SCALE-28: the roll-ups it moves, stored with the row. FILTER-13:
        // and what an open `watch { filter }` asking `under` sees of it.
        let (derived, under) = {
            let cache = self.tickets.read().expect("store lock poisoned");
            let derived = cache.status_change_asking(&row, previous, cache.inputs.asking_after(&row.id, &moved_inputs));
            let under = self.under_before(&cache, || lineage::Touched::of_moves(derived.announced().as_deref()));
            (derived, under)
        };
        // FILTER-4: the cells it moves — its own and its fan-out's — said
        // last.
        let announced = derived.announced();
        let Derived { stored, moved } = derived;
        // WAVE-6: across the satisfied line, its own placement moves with it
        // (its `status_changed` carries the new one), and the ones downstream
        // that move are said last.
        if let Some(placement) = moved.placement() {
            row.placement = placement;
        }
        let placed = moved.placed.as_ref().and_then(Change::placed);
        let key = OrderKey::of(&row);
        let epic = row.epic.clone();
        let commit = Self::row_commit(row);
        let cache = Arc::clone(&self.tickets);
        let (logged, kept_inputs) = (transition.clone(), moved_inputs.clone());
        self.write(
            serial,
            move |tx| {
                let out = commit(tx)?;
                stored.write(tx)?;
                if let Some(t) = &logged {
                    history::record(tx, t)?;
                }
                for (input, entry) in &moved_inputs {
                    inputs::record(tx, input, entry)?;
                }
                Ok(out)
            },
            move |cache, (row, _)| {
                let groups = cache.status_changed(row.clone(), previous, moved);
                if let Some(t) = &transition {
                    cache.log.append(t, &key);
                }
                for (input, _) in &kept_inputs {
                    cache.inputs.put(input.clone());
                }
                (groups, transition, kept_inputs)
            },
            move |(row, body), (groups, transition, kept_inputs), tx| {
                let change = Change::StatusChanged {
                    ticket: whole(row, body),
                    previous,
                };
                tx.send(change.clone());
                if let Some(transition) = transition {
                    tx.send(Change::Transition { epic: epic.clone(), transition });
                }
                broadcast_groups(tx, groups);
                if let Some(placed) = placed {
                    tx.send(placed);
                }
                for (input, _) in kept_inputs {
                    tx.send(Change::InputChanged { epic: epic.clone(), input });
                }
                if let Some(moves) = announced {
                    tx.send(Change::CellsMoved { moves });
                }
                if let Some(change) = under.and_then(|u| u.after(&cache)) {
                    tx.send(change);
                }
                change
            },
        )
        .await
    }

    /// MENGER-13: a status change that moves other tickets beside its own —
    /// `steps`, the first the call's own — as one write: every step's row,
    /// derived state, transition and inputs in one transaction, the cache
    /// moved step by step under its write lock (each step's derived change
    /// worked out against the cache the steps before it left), then every
    /// step's events, in order. Its answer is the first step's
    /// `StatusChanged`. A commit that fails changes nothing: the cache is
    /// read back from the file, which is as it was.
    pub(super) async fn batch(&self, serial: OwnedMutexGuard<()>, steps: Vec<Step>) -> Result<Change, StoreError> {
        self.in_one_write(serial, move |conn, cache, unders| {
            let changes = Self::batch_steps(conn, cache, unders, steps)?;
            let first = changes.first().cloned().expect("a batch has its own step");
            Ok((first, changes))
        })
        .await
    }

    /// MENGER-13: a write made of steps that each work out their derived
    /// change against the cache as the steps before it left it — `steps`,
    /// run with the cache's write lock held from the first step to the
    /// commit and the database's connection in hand, answering the write's
    /// answer and its events. Committed, the events are broadcast in order;
    /// failed, the cache is read back from the file, which is as it was.
    /// MENGER-30: [`Store::batch`]'s, and `update`'s.
    pub(super) async fn in_one_write<T: Send + 'static>(
        &self,
        serial: OwnedMutexGuard<()>,
        steps: impl FnOnce(&mut Connection, &mut Cache, &lineage::UnderWatches) -> Result<(T, Vec<Change>), StoreError> + Send + 'static,
    ) -> Result<T, StoreError> {
        let (db, tickets, tx) = (Arc::clone(&self.db), Arc::clone(&self.tickets), self.tx.clone());
        let (committing, unders) = (Arc::clone(&self.committing), Arc::clone(&self.unders));
        committing.fetch_add(1, Ordering::SeqCst);
        tokio::task::spawn_blocking(move || {
            let _serial = serial;
            let mut cache = tickets.write().expect("store lock poisoned");
            let mut conn = db.lock().expect("db lock poisoned");
            let written = cache.written;
            match steps(&mut conn, &mut cache, &unders) {
                Ok((answer, changes)) => {
                    cache.written += 1;
                    drop(cache);
                    drop(conn);
                    for c in &changes {
                        tx.send(c.clone());
                    }
                    Ok(answer)
                }
                Err(e) => {
                    tracing::error!("MENGER-13: a batch write failed ({e}); the cache is read back from the file");
                    match Self::load_cache(&mut conn) {
                        Ok(mut fresh) => {
                            fresh.written = written;
                            *cache = fresh;
                        }
                        Err(again) => tracing::error!("MENGER-13: reading the cache back failed too: {again}"),
                    }
                    committing.fetch_sub(1, Ordering::SeqCst);
                    Err(e)
                }
            }
        })
        .await
        .map_err(|e| StoreError::Db(format!("write task failed: {e}")))?
    }

    /// The steps of a batch, into one transaction and the cache — the
    /// changes to broadcast, once it has committed.
    fn batch_steps(
        conn: &mut Connection,
        cache: &mut Cache,
        unders: &lineage::UnderWatches,
        steps: Vec<Step>,
    ) -> Result<Vec<Change>, StoreError> {
        let txn = conn.transaction()?;
        let now = chrono::Utc::now().timestamp();
        let mut out = Gathered::default();
        for step in steps {
            status_step(&txn, cache, unders, step, now, &mut out)?;
        }
        txn.commit()?;
        Ok(out.finish())
    }
}

/// MENGER-13: one status step of a batch — its row, derived state,
/// transition and inputs into `txn` and the cache, worked out against the
/// cache as the steps before it left it; its events into `out`. MENGER-30:
/// `update`'s status change is these steps too.
pub(super) fn status_step(
    txn: &rusqlite::Transaction<'_>,
    cache: &mut Cache,
    unders: &lineage::UnderWatches,
    step: Step,
    now: i64,
    out: &mut Gathered,
) -> Result<(), StoreError> {
    let mut row = cache.get(&step.id).cloned().ok_or_else(|| StoreError::NotFound(step.id.clone()))?;
    let previous = row.status;
    row.status = step.to;
    row.updated_unix = now;
    let derived = cache.status_change_asking(&row, previous, cache.inputs.asking_after(&row.id, &step.inputs));
    let under = lineage::under_before_with(unders, cache, || lineage::Touched::of_moves(derived.announced().as_deref()));
    let announced = derived.announced();
    let Derived { stored, moved } = derived;
    if let Some(placement) = moved.placement() {
        row.placement = placement;
    }
    let placed = moved.placed.as_ref().and_then(Change::placed);
    db::update_row(txn, &row)?;
    stored.write(txn)?;
    if let Some(t) = &step.transition {
        history::record(txn, t)?;
    }
    for (input, entry) in &step.inputs {
        inputs::record(txn, input, entry)?;
    }
    let body = db::body(txn, &row.id)?.ok_or_else(|| missing(&row.id))?;
    let key = OrderKey::of(&row);
    let epic = row.epic.clone();
    let moved_groups = cache.status_changed(row.clone(), previous, moved);
    if let Some(t) = &step.transition {
        cache.log.append(t, &key);
    }
    for (input, _) in &step.inputs {
        cache.inputs.put(input.clone());
    }
    out.tickets.push(Change::StatusChanged { ticket: whole(row, body), previous });
    if let Some(transition) = step.transition {
        out.tickets.push(Change::Transition { epic: epic.clone(), transition });
    }
    out.groups(moved_groups);
    out.placed(placed);
    out.inputs.extend(step.inputs.into_iter().map(|(input, _)| Change::InputChanged { epic: epic.clone(), input }));
    out.filtered(announced, under.and_then(|u| u.after_in(cache)));
    Ok(())
}

/// MENGER-13: the events of a write made of several steps, as one write's
/// are: every ticket's, in the order made; then each group that moved (once,
/// as the write leaves it); then one `placements_changed` (each ticket once,
/// as the write leaves it); then — MENGER-30 — the components a body moved;
/// then the inputs; then, for `watch { filter }`, each step's cell and
/// lineage moves, in the order made.
#[derive(Default)]
pub(super) struct Gathered {
    pub(super) tickets: Vec<Change>,
    groups: Vec<Group>,
    placements: Vec<(String, crate::types::PlacementChange)>,
    pub(super) components: Vec<Change>,
    pub(super) inputs: Vec<Change>,
    filtered: Vec<Change>,
}

impl Gathered {
    /// Groups a step moved — each kept once, as the latest step left it.
    pub(super) fn groups(&mut self, moved: Vec<Group>) {
        for g in moved {
            match self.groups.iter_mut().find(|h| h.epic == g.epic) {
                Some(h) => *h = g,
                None => self.groups.push(g),
            }
        }
    }

    /// A step's `placements_changed` — each ticket kept once, as the latest
    /// step placed it.
    pub(super) fn placed(&mut self, placed: Option<Change>) {
        if let Some(Change::PlacementsChanged { changes }) = placed {
            for (e, c) in changes {
                match self.placements.iter_mut().find(|(_, p)| p.id == c.id) {
                    Some(p) => *p = (e, c),
                    None => self.placements.push((e, c)),
                }
            }
        }
    }

    /// A step's cell moves and lineage moves, for `watch { filter }`.
    pub(super) fn filtered(&mut self, cells: Option<std::sync::Arc<cells::Moves>>, under: Option<Change>) {
        if let Some(moves) = cells {
            self.filtered.push(Change::CellsMoved { moves });
        }
        self.filtered.extend(under);
    }

    /// The write's events, in the order they are sent.
    pub(super) fn finish(self) -> Vec<Change> {
        let mut changes = self.tickets;
        changes.extend(self.groups.into_iter().map(|group| Change::GroupChanged { group }));
        if !self.placements.is_empty() {
            changes.push(Change::PlacementsChanged { changes: self.placements });
        }
        changes.extend(self.components);
        changes.extend(self.inputs);
        changes.extend(self.filtered);
        changes
    }
}
