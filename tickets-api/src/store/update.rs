//! MENGER-30: **`update`** — any part of a ticket in one write: its title,
//! body, kind, status, parent, severity and blockers. Owner, 2026-10-12:
//! *"we need to be able to fully update a ticket."*
//!
//! **The rules are the single writes'**, applied together
//! ([`super::rules`]): `amend`'s (a title not blank, the body cap, the
//! component registry, input markup), `set_kind`'s (the doc line, a root's
//! kind, a path holder's — the kind evaluated before the status, against the
//! blockers the update leaves), `set_status`'s (no status on a doc, `Doc` on
//! nothing else — a status beside a change of kind is `set_kind`'s, as
//! `set_kind { status }` takes it), `set_parent`'s, and `create`'s for
//! blockers (never itself or a cycle — named —, a forward reference
//! allowed, a doc never blocks nor is blocked); and `block` / `unblock`
//! naming one id in both is refused. Every rule is asked before anything is
//! written, and a refusal names every one ([`StoreError::Refused`]; one
//! alone is itself).
//!
//! **One write** ([`Store::in_one_write`]): every aspect that moves is a
//! step, made in turn against the cache as the steps before it left it —
//! so each moves what its single write moves (the roll-ups, the status
//! counts and the ready set, the placements, the cells, the kept tree, the
//! transition log, the forward references, the search index, the registry
//! and the inputs) — into one transaction, under the cache's write lock;
//! a failed commit reads the cache back from the file. The steps go: the
//! title and body (`amended`); the blockers, when the ticket becomes a doc
//! (it sheds them first); the kind with its status (`kind_changed`), or the
//! status alone (`status_changed` — into `Canceled`, the cascade, as
//! `set_status` makes it); the blockers otherwise (`blockers_changed`, then
//! each blocker's `unlocks_changed` / `unlocks_removed`); the parent
//! (`parent_changed`, `children_changed`, `child_removed`); the severity
//! (`severity_changed`). The write's events are one write's ([`Gathered`]).
//! Nothing that would change is no write and no event: `changed: []`.

// A step takes the write in hand (the transaction, the cache, the open
// watches, its clock) beside what it writes: the arguments are the step.
#![allow(clippy::too_many_arguments)]

use super::cascade::{status_step, status_steps, Gathered};
use super::history::Act;
use super::*;
use crate::types::{InputHistoryEntry, Transition, TransitionField};

/// MENGER-30: what an `update` asks for — every field optional, absent
/// meaning unchanged.
#[derive(Debug, Clone, Default)]
pub struct Update {
    pub title: Option<String>,
    pub body: Option<String>,
    pub kind: Option<Kind>,
    pub status: Option<Status>,
    pub parent: Option<String>,
    /// `Some(None)` clears it.
    pub severity: Option<Option<Severity>>,
    /// Ids it waits on from now on.
    pub block: Vec<crate::vocab::TicketRef>,
    /// Ids it no longer waits on.
    pub unblock: Vec<crate::vocab::TicketRef>,
    /// ACT-2: labels to add, and to take off — `label`'s rules
    /// ([`crate::labels::apply`]; one in both is added).
    pub label: Vec<Label>,
    pub unlabel: Vec<Label>,
    /// What the caller refused already — an unknown kind, status or
    /// severity — named with the store's own refusals, so one answer names
    /// every one ([`StoreError::Invalid`]).
    pub refused: Vec<String>,
}

/// MENGER-30: an update's answer — the ticket as it left it, and the
/// aspects that moved, in the order `changed` names them: `title`, `body`,
/// `kind`, `status`, `parent`, `severity`, `blocked_by` — ACT-2: `labels`.
#[derive(Debug, Clone)]
pub struct Updated {
    pub ticket: Ticket,
    pub changed: Vec<String>,
}

/// What an update will do, worked out and checked before anything moves.
pub(super) struct Plan {
    pub(super) id: String,
    title: Option<String>,
    body: Option<String>,
    registered: components::Plan,
    asked: inputs::Plan,
    /// A change of kind, with the status asked beside it.
    kind: Option<(Kind, Option<Status>)>,
    /// A change of status alone.
    status: Option<Status>,
    parent: Option<String>,
    severity: Option<Option<Severity>>,
    added: Vec<TicketName>,
    removed: Vec<TicketName>,
    /// The ticket becomes a doc: it sheds its blockers before its kind.
    blockers_first: bool,
    /// ACT-2: the labels after, and what moved — `None`: none did.
    labels: Option<Vec<Label>>,
    pub(super) changed: Vec<String>,
}

/// Ids as a write keeps them — each once.
///
/// ORG-3: each by any short form, as the key it names — or will be born as;
/// one naming several is a refusal, into `refusals`. ACCESS-48: each a
/// [`crate::vocab::TicketRef`], so none is blank.
fn ids_of(cache: &Cache, entries: &[crate::vocab::TicketRef], refusals: &mut Vec<StoreError>) -> Vec<TicketName> {
    let mut ids: Vec<TicketName> = Vec::with_capacity(entries.len());
    for e in entries {
        match cache.name_ticket(e) {
            Ok(key) => ids.push(key),
            Err(refused) => refusals.push(refused),
        }
    }
    rules::distinct(&ids)
}

/// SDLC-13 / MENGER-13: a change of kind's transitions — the kind's, then
/// the status's when it moves, caused by the first — numbered from the
/// log's next id, and the inputs a crossing of `Canceled` moves. `old` is
/// the row before, `row` after. Shared by `set_kind` and `update`.
pub(super) fn kind_moves(cache: &Cache, old: &Row, row: &Row, act: &Act) -> (Vec<Transition>, Vec<(Input, InputHistoryEntry)>) {
    let first = cache.log.next_id();
    let transition = |id, field, from: &str, to: &str, cause: Option<(u64, &str)>| Transition {
        id,
        ticket: row.id.clone(),
        field,
        from: Some(from.to_string()),
        to: to.to_string(),
        at_unix: row.updated_unix,
        actor: act.actor.clone(),
        name: act.name.clone(),
        source: act.source,
        message: act.message.clone(),
        cause_ticket: cause.map(|(_, t)| t.to_string()),
        cause: cause.map(|(c, _)| c),
    };
    let mut made = Vec::new();
    if row.kind != old.kind {
        made.push(transition(first, TransitionField::Kind, old.kind.as_str(), row.kind.as_str(), act.shared_cause()));
    }
    if row.status != old.status {
        // Caused by its kind's, when that moved; else the write's own cause.
        let cause = made.first().map(|t| (t.id, row.id.as_str())).or_else(|| act.shared_cause());
        let id = first + made.len() as u64;
        made.push(transition(id, TransitionField::Status, old.status.as_str(), row.status.as_str(), cause));
    }
    let reason = format!("{} is {} (by {})", row.id, row.status.as_str(), act.who());
    let moved_inputs = match (old.status == Status::Canceled, row.status == Status::Canceled) {
        (false, true) => cache.inputs.withdrawing(&row.id, &reason, row.updated_unix),
        (true, false) => cache.inputs.restoring(&row.id, &reason, row.updated_unix),
        _ => Vec::new(),
    };
    (made, moved_inputs)
}

impl Store {
    /// [`Store::update_by`] as nobody signed in, through the API.
    pub async fn update(&self, at: Updating, update: Update) -> Result<Updated, StoreError> {
        self.update_by(at, update, Act::default()).await
    }

    /// MENGER-30: changes any part of the ticket `id` in one write, by `act`
    /// (the module doc). [`StoreError::NotFound`] for an id that names no
    /// ticket; a refusal writes nothing.
    ///
    /// ACCESS-11: of the ticket `at` is for, its parent and blockers as
    /// `at`'s proofs name them ([`Updating`]) — checked under the
    /// write-serial lock.
    pub async fn update_by(&self, at: Updating, update: Update, act: Act) -> Result<Updated, StoreError> {
        let id = crate::address::folded(at.id());
        let mut refusals: Vec<StoreError> = update.refused.iter().cloned().map(StoreError::Invalid).collect();
        // amend's, on what is in hand before the lock.
        if update.title.as_deref().is_some_and(|t| t.trim().is_empty()) {
            refusals.push(StoreError::EmptyTitle { id: id.clone() });
        }
        if let Some(Err(e)) = update.title.as_deref().map(|t| self.check_title(Some(&id), t)) {
            refusals.push(e);
        }
        if let Some(body) = &update.body {
            if let Err(e) = self.check_body(Some(&id), body) {
                refusals.push(e);
            }
        }
        let uses = update.body.as_deref().map(components::read);
        let declares = update.body.as_deref().map(crate::inputs::read);
        let serial = self.serial().await;
        let current = self.get_unscoped(&id).await?;
        let plan = {
            let cache = self.tickets.read().expect("store lock poisoned");
            let row = cache.get(&current.id).ok_or_else(|| StoreError::NotFound(id.clone()))?;
            at.check(&cache, row, &update)?;
            plan(&cache, &current, update, uses, declares, &mut refusals)
        };
        if refusals.len() == 1 {
            return Err(refusals.pop().expect("one refusal"));
        }
        if !refusals.is_empty() {
            return Err(StoreError::Refused { id: String::from(current.id), refusals });
        }
        if plan.changed.is_empty() {
            return Ok(Updated { ticket: current, changed: Vec::new() });
        }
        self.in_one_write(serial, move |conn, cache, unders| {
            let txn = conn.transaction()?;
            let now = chrono::Utc::now().timestamp();
            let mut out = Gathered::default();
            let (id, changed) = (plan.id.clone(), plan.changed.clone());
            apply(&txn, cache, unders, plan, &act, now, &mut out)?;
            let row = cache.get(&id).cloned().ok_or_else(|| StoreError::NotFound(id.clone()))?;
            let body = db::body(&txn, &id)?.ok_or_else(|| missing(&id))?;
            txn.commit()?;
            Ok((Updated { ticket: whole(row, body), changed }, out.finish()))
        })
        .await
    }
}

/// MENGER-30: a planned update's steps, in the module doc's order, into
/// `txn` and the cache as the steps before it left it; its events into
/// `out`. ACT-2: `update_many` makes each ticket's this way, in turn —
/// and a status a cascade earlier in its write already gave the ticket is
/// no step.
pub(super) fn apply(
    txn: &rusqlite::Transaction<'_>,
    cache: &mut Cache,
    unders: &lineage::UnderWatches,
    plan: Plan,
    act: &Act,
    now: i64,
    out: &mut Gathered,
) -> Result<(), StoreError> {
    let Plan { id, title, body, registered, asked, kind, status, parent, severity, added, removed, blockers_first, labels, changed: _ } = plan;
    let blockers = !added.is_empty() || !removed.is_empty();
    if title.is_some() || body.is_some() {
        amend_step(txn, cache, unders, &id, title, body, registered, asked, now, out)?;
    }
    if blockers && blockers_first {
        blockers_step(txn, cache, unders, &id, &added, &removed, now, out)?;
    }
    if let Some((kind, status)) = kind {
        kind_step(txn, cache, unders, &id, kind, status, act, now, out)?;
    }
    if let Some(status) = status {
        let row = cache.get(&id).cloned().ok_or_else(|| StoreError::NotFound(id.clone()))?;
        // ACT-2: a cascade earlier in the write may have given it already.
        if row.status != status {
            rules::status_refusal(&row, status)?;
            for step in status_steps(cache, &row, status, act, false, now) {
                status_step(txn, cache, unders, step, now, out)?;
            }
        }
    }
    if blockers && !blockers_first {
        blockers_step(txn, cache, unders, &id, &added, &removed, now, out)?;
    }
    if let Some(parent) = parent {
        parent_step(txn, cache, unders, &id, parent, now, out)?;
    }
    if let Some(severity) = severity {
        severity_step(txn, cache, unders, &id, severity, now, out)?;
    }
    if let Some(labels) = labels {
        labels_step(txn, cache, unders, &id, labels, now, out)?;
    }
    Ok(())
}

/// Every rule, asked of the ticket as it stands (`current`) and the update
/// as asked — each refusal into `refusals` — and what moves.
pub(super) fn plan(
    cache: &Cache,
    current: &Ticket,
    u: Update,
    uses: Option<(components::Uses, Vec<components::Problem>)>,
    declares: Option<(Vec<crate::inputs::Declared>, Vec<crate::inputs::Problem>)>,
    refusals: &mut Vec<StoreError>,
) -> Plan {
    let id = current.id.clone();
    let row = cache.get(&id).cloned().unwrap_or_else(|| Row::from(current));
    let mut changed: Vec<String> = Vec::new();
    let title = u.title.filter(|t| *t != current.title);
    if title.is_some() {
        changed.push("title".into());
    }
    // amend's: a new body moves the registry and the inputs — refused, the
    // whole update with it, when a block or the markup is malformed or a
    // name it defines is another ticket's.
    let body = u.body.filter(|b| *b != current.body);
    let (mut registered, mut asked) = (components::Plan::default(), inputs::Plan::default());
    if body.is_some() {
        changed.push("body".into());
        if let Some((uses, problems)) = uses {
            match problems.into_iter().next() {
                Some(problem) => refusals.push(StoreError::component(Some(&id), problem)),
                None => match cache.plan_registry(&id, &uses, Some(&id)) {
                    Ok(p) => registered = p,
                    Err(e) => refusals.push(e),
                },
            }
        }
        if let Some((declared, problems)) = declares {
            match problems.into_iter().next() {
                Some(problem) => refusals.push(StoreError::InputMarkup { id: Some(id.to_string()), problem }),
                None => asked = cache.inputs.plan(&id, declared),
            }
        }
    }
    // The blockers as the update leaves them — what the kind is checked
    // against.
    let block = ids_of(cache, &u.block, refusals);
    let unblock = ids_of(cache, &u.unblock, refusals);
    let both: Vec<TicketName> = block.iter().filter(|b| unblock.contains(b)).cloned().collect();
    if !both.is_empty() {
        let named: Vec<&str> = both.iter().map(TicketName::as_str).collect();
        refusals.push(StoreError::Blockers { id: id.to_string(), why: format!("{} both blocked and unblocked", named.join(", ")) });
    }
    let added: Vec<TicketName> = block.into_iter().filter(|b| !row.blocked_by.contains(b) && !both.contains(b)).collect();
    let removed: Vec<TicketName> = unblock.into_iter().filter(|b| row.blocked_by.contains(b) && !both.contains(b)).collect();
    let mut after: Vec<TicketName> = row.blocked_by.iter().filter(|b| !removed.contains(b)).cloned().collect();
    after.extend(added.iter().cloned());
    // set_kind's, the kind before the status; set_status's for a status
    // alone (the status it has is no change).
    let (mut kind, mut status) = (None, None);
    match u.kind.filter(|k| *k != row.kind) {
        Some(k) => {
            changed.push("kind".into());
            match rules::kind_status(&id, row.kind, row.status, k, u.status) {
                Ok(s) if s != row.status => changed.push("status".into()),
                Ok(_) => {}
                Err(e) => refusals.push(e),
            }
            if let Err(e) = rules::kind_refusal(cache, &row, &after, k) {
                refusals.push(e);
            }
            kind = Some((k, u.status));
        }
        None => {
            if let Some(s) = u.status.filter(|s| *s != row.status) {
                changed.push("status".into());
                if let Err(e) = rules::status_refusal(&row, s) {
                    refusals.push(e);
                }
                status = Some(s);
            }
        }
    }
    let final_kind = kind.map_or(row.kind, |(k, _)| k);
    // set_parent's.
    // ORG-3: the parent by any short form.
    let parent = u
        .parent
        .map(|p| match cache.find_ticket(&p, shortcodes::OrgReach::All) {
            shortcodes::Found::One(key) => key,
            shortcodes::Found::Many(candidates) => {
                refusals.push(StoreError::Ambiguous { text: p.trim().to_string(), candidates });
                String::new()
            }
            shortcodes::Found::None => p.trim().to_ascii_uppercase(),
        })
        .filter(|p| !p.is_empty() && row.parent.as_deref() != Some(p.as_str()));
    if let Some(p) = &parent {
        changed.push("parent".into());
        if let Err(e) = rules::parent_refusal(cache, &row, p) {
            refusals.push(e);
        }
    }
    let severity = u.severity.filter(|s| *s != row.severity);
    if severity.is_some() {
        changed.push("severity".into());
    }
    // create's, for what it adds: a doc is never blocked and never blocks;
    // no cycle (itself included).
    if !added.is_empty() || !removed.is_empty() {
        changed.push("blocked_by".into());
    }
    if !added.is_empty() {
        if !final_kind.has_lifecycle() && kind.is_none() {
            refusals.push(StoreError::DocInGraph { id: id.to_string(), blocked_by: added.clone(), unlocks: Vec::new() });
        }
        let docs = rules::docs_among(cache, &added);
        if !docs.is_empty() {
            refusals.push(StoreError::BlockedByDoc { id: id.to_string(), docs });
        }
        if let Some(path) = find_cycle(cache, &id, &added) {
            refusals.push(StoreError::BlockCycle { id: id.to_string(), path });
        }
        // ACT-5: and never a deleted ticket.
        if let Err(e) = cache.tombstones.refuse_blockers(&id, &added) {
            refusals.push(e);
        }
    }
    // ACT-2: label's.
    let mut labels = None;
    if !u.label.is_empty() || !u.unlabel.is_empty() {
        match crate::labels::apply(&row.labels, &u.label, &u.unlabel) {
            Err(why) => refusals.push(StoreError::Labels { id: id.to_string(), why }),
            Ok((after, added, removed)) if !added.is_empty() || !removed.is_empty() => {
                changed.push("labels".into());
                labels = Some(after);
            }
            Ok(_) => {}
        }
    }
    Plan {
        id: String::from(id),
        title,
        body,
        registered,
        asked,
        kind,
        status,
        parent,
        severity,
        added,
        removed,
        blockers_first: !final_kind.has_lifecycle(),
        labels,
        changed,
    }
}

/// `amend`'s write, as a step: the row, the body and its search entry, the
/// registry and the inputs it moves, the cell an input opened or settled.
fn amend_step(
    txn: &rusqlite::Transaction<'_>,
    cache: &mut Cache,
    unders: &lineage::UnderWatches,
    id: &str,
    title: Option<String>,
    body: Option<String>,
    mut registered: components::Plan,
    mut asked: inputs::Plan,
    now: i64,
    out: &mut Gathered,
) -> Result<(), StoreError> {
    let mut row = cache.get(id).cloned().ok_or_else(|| StoreError::NotFound(id.to_string()))?;
    let mut changed = Vec::new();
    let title_changed = title.is_some();
    if let Some(t) = title {
        row.title = t;
        changed.push(crate::vocab::AmendedField::Title);
    }
    let text = match &body {
        Some(b) => {
            changed.push(crate::vocab::AmendedField::Body);
            b.clone()
        }
        None => db::body(txn, id)?.ok_or_else(|| missing(id))?,
    };
    row.updated_unix = now;
    // MENGER-13: a canceled ticket asks no one.
    let withdrawn = if row.status == Status::Canceled {
        asked.withdraw_open(&format!("{} is canceled", row.id), now)
    } else {
        Vec::new()
    };
    let registry_moves = std::mem::take(&mut registered.moves);
    let input_list = asked.list.take();
    let asks = input_list.as_deref().map(inputs::asking_in);
    let was = cache.inputs.asking(&row.id);
    let derived = asks.filter(|a| *a != was).map(|a| cache.cell_change(&row, Some(a)));
    // FILTER-30: the row is touched whether or not its cell moves.
    let under = lineage::under_before_with(unders, cache, || {
        lineage::Touched::of_moves(derived.as_ref().and_then(Derived::announced).as_deref()).row(&row)
    });
    let announced = derived.as_ref().and_then(Derived::announced);
    db::update_row(txn, &row)?;
    if let Some(b) = &body {
        db::set_body(txn, &row.id, b)?;
    }
    search::put(txn, &row.id, &row.title, &text)?;
    components::store(txn, &registry_moves)?;
    if let Some(list) = &input_list {
        inputs::store(txn, &row.id, list)?;
    }
    for (input, entry) in &withdrawn {
        inputs::record(txn, input, entry)?;
    }
    let moved = match derived {
        Some(Derived { stored, moved }) => {
            stored.write(txn)?;
            Some(moved)
        }
        None => None,
    };
    let groups = cache.amended(row.clone(), title_changed);
    cache.take_registry(registry_moves);
    if let Some(list) = input_list {
        cache.inputs.take(&row.id, list);
    }
    if let Some(moved) = moved {
        let more = cache.take(moved);
        debug_assert!(more.is_empty(), "an input moves no roll-up");
    }
    let epic = row.shortcode.clone();
    out.tickets.push(Change::Amended { ticket: whole(row, text), changed });
    out.groups(groups);
    out.components.extend(Change::components(registered));
    out.inputs.extend(Change::inputs(&epic, asked));
    out.filtered(announced, under.and_then(|u| u.after_in(cache)));
    Ok(())
}

/// `set_kind`'s write, as a step: the kind and the status with it, their
/// transitions, everything kept by kind and by status.
fn kind_step(
    txn: &rusqlite::Transaction<'_>,
    cache: &mut Cache,
    unders: &lineage::UnderWatches,
    id: &str,
    kind: Kind,
    status: Option<Status>,
    act: &Act,
    now: i64,
    out: &mut Gathered,
) -> Result<(), StoreError> {
    let mut row = cache.get(id).cloned().ok_or_else(|| StoreError::NotFound(id.to_string()))?;
    let old = row.clone();
    let previous = row.kind;
    let status = rules::kind_status(&row.id, previous, row.status, kind, status)?;
    rules::kind_refusal(cache, &row, &row.blocked_by, kind)?;
    row.kind = kind;
    row.status = status;
    row.updated_unix = now;
    let (made, moved_inputs) = kind_moves(cache, &old, &row, act);
    let derived = cache.kind_change(&row, &old, cache.inputs.asking_after(&row.id, &moved_inputs))?;
    // ORG-3: a ticket that now roots its shortcode may move its namespace —
    // its row takes the address the write leaves it.
    if let Some(m) = &derived.moved.namespaces {
        row = Cache::dressed(&m.after, row);
    }
    let under = lineage::under_before_with(unders, cache, || {
        let mut t = lineage::Touched::of_moves(derived.announced().as_deref()).row(&row);
        if let Some(after) = &derived.moved.rollups {
            t.hosts(cache, after, cache.rows().with(&row), None);
        }
        t
    });
    let announced = derived.announced();
    let Derived { stored, moved } = derived;
    if let Some(placement) = moved.placement() {
        row.placement = placement;
    }
    let placed = moved.placed.as_ref().and_then(Change::placed);
    db::update_row(txn, &row)?;
    stored.write(txn)?;
    for t in &made {
        history::record(txn, t)?;
    }
    for (input, entry) in &moved_inputs {
        inputs::record(txn, input, entry)?;
    }
    let body = db::body(txn, id)?.ok_or_else(|| missing(id))?;
    let key = OrderKey::of(&row);
    let epic = row.shortcode.clone();
    let groups = cache.kind_changed(row.clone(), moved);
    for t in &made {
        cache.log.append(t, &key);
    }
    for (input, _) in &moved_inputs {
        cache.inputs.put(input.clone());
    }
    out.tickets.push(Change::KindChanged { ticket: whole(row, body), previous });
    out.tickets.extend(made.into_iter().map(|transition| Change::Transition { shortcode: epic.clone(), transition }));
    out.groups(groups);
    out.placed(placed);
    out.inputs.extend(moved_inputs.into_iter().map(|(input, _)| Change::InputChanged { shortcode: epic.clone(), input }));
    out.filtered(announced, under.and_then(|u| u.after_in(cache)));
    Ok(())
}

/// `set_parent`'s write, as a step: the child's row, both parents' counts
/// and children, the roll-ups and the kept tree.
fn parent_step(
    txn: &rusqlite::Transaction<'_>,
    cache: &mut Cache,
    unders: &lineage::UnderWatches,
    id: &str,
    parent: String,
    now: i64,
    out: &mut Gathered,
) -> Result<(), StoreError> {
    let mut child = cache.get(id).cloned().ok_or_else(|| StoreError::NotFound(id.to_string()))?;
    rules::parent_refusal(cache, &child, &parent)?;
    let new_epic = cache.get(&parent).map(|p| p.shortcode.clone()).expect("the parent exists");
    let previous = child.parent.take();
    child.parent = Some(crate::address::TicketId::new(parent.clone()));
    child.updated_unix = now;
    let old_epic = previous.as_deref().and_then(|p| cache.get(p)).map(|t| t.shortcode.clone());
    let derived = cache.reparent(&child, previous.as_deref())?;
    // ORG-3: a rehomed root's every row takes its new address — the child's
    // own, as its event carries it, from the registry the write leaves.
    if let Some(m) = &derived.moved.namespaces {
        child = Cache::dressed(&m.after, child);
    }
    let under = lineage::under_before_with(unders, cache, || {
        let mut t = lineage::Touched::default();
        t.subtree(cache, &child.id, cache.get(&child.id));
        if let Some(after) = &derived.moved.rollups {
            t.hosts(cache, after, cache.rows().with(&child), None);
        }
        t
    });
    let Derived { stored, moved } = derived;
    db::update_row(txn, &child)?;
    stored.write(txn)?;
    let body = db::body(txn, id)?.ok_or_else(|| missing(id))?;
    let (groups, joined, left) = cache.reparented(child.clone(), previous.as_deref(), moved);
    out.tickets.push(Change::ParentChanged { ticket: whole(child, body), previous: previous.clone().map(String::from) });
    out.tickets.push(Change::ChildrenChanged { id: parent, shortcode: new_epic, added: id.to_string(), child_count: joined });
    if let (Some(old), Some(epic)) = (previous, old_epic) {
        out.tickets.push(Change::ChildRemoved { id: String::from(old), shortcode: epic, removed: id.to_string(), child_count: left });
    }
    out.groups(groups);
    out.filtered(None, under.and_then(|u| u.after_in(cache)));
    Ok(())
}

/// A severity's write: the row, and its cell (severity is in it).
fn severity_step(
    txn: &rusqlite::Transaction<'_>,
    cache: &mut Cache,
    unders: &lineage::UnderWatches,
    id: &str,
    severity: Option<Severity>,
    now: i64,
    out: &mut Gathered,
) -> Result<(), StoreError> {
    let mut row = cache.get(id).cloned().ok_or_else(|| StoreError::NotFound(id.to_string()))?;
    let previous = row.severity;
    row.severity = severity;
    row.updated_unix = now;
    cell_step(txn, cache, unders, row, out, |ticket| Change::SeverityChanged { ticket, previous })
}

/// ACT-2: `label`'s write, as a step: the row and its cell (`label <name>`).
fn labels_step(
    txn: &rusqlite::Transaction<'_>,
    cache: &mut Cache,
    unders: &lineage::UnderWatches,
    id: &str,
    labels: Vec<Label>,
    now: i64,
    out: &mut Gathered,
) -> Result<(), StoreError> {
    let mut row = cache.get(id).cloned().ok_or_else(|| StoreError::NotFound(id.to_string()))?;
    let added: Vec<Label> = labels.iter().filter(|l| !row.labels.contains(l)).cloned().collect();
    let removed: Vec<Label> = row.labels.iter().filter(|l| !labels.contains(l)).cloned().collect();
    row.labels = labels;
    row.updated_unix = now;
    cell_step(txn, cache, unders, row, out, |ticket| Change::LabelsChanged { ticket, added, removed })
}

/// A write that moves only `row`'s cell — its labels, its severity, its
/// commits: the row, its cell and the kept tree, and `change` of the whole
/// ticket.
pub(super) fn cell_step(
    txn: &rusqlite::Transaction<'_>,
    cache: &mut Cache,
    unders: &lineage::UnderWatches,
    row: Row,
    out: &mut Gathered,
    change: impl FnOnce(Ticket) -> Change,
) -> Result<(), StoreError> {
    let derived = cache.cell_change(&row, None);
    let under = lineage::under_before_with(unders, cache, || lineage::Touched::of_moves(derived.announced().as_deref()).row(&row));
    let announced = derived.announced();
    let Derived { stored, moved } = derived;
    db::update_row(txn, &row)?;
    stored.write(txn)?;
    let body = db::body(txn, &row.id)?.ok_or_else(|| missing(&row.id))?;
    cache.commits_changed(row.clone(), moved);
    out.tickets.push(change(whole(row, body)));
    out.filtered(announced, under.and_then(|u| u.after_in(cache)));
    Ok(())
}

/// The blockers' write: the ticket's `blocked_by`, each blocker's `unlocks`
/// (create's mirror, kept both ways), the forward references, its count of
/// unsatisfied blockers and the ready set, its cell and the kept tree, and
/// its placement with every one downstream that moves.
fn blockers_step(
    txn: &rusqlite::Transaction<'_>,
    cache: &mut Cache,
    unders: &lineage::UnderWatches,
    id: &str,
    added: &[TicketName],
    removed: &[TicketName],
    now: i64,
    out: &mut Gathered,
) -> Result<(), StoreError> {
    let mut row = cache.get(id).cloned().ok_or_else(|| StoreError::NotFound(id.to_string()))?;
    if !added.is_empty() {
        if !row.kind.has_lifecycle() {
            return Err(StoreError::DocInGraph { id: String::from(row.id), blocked_by: added.to_vec(), unlocks: Vec::new() });
        }
        let docs = rules::docs_among(cache, added);
        if !docs.is_empty() {
            return Err(StoreError::BlockedByDoc { id: String::from(row.id), docs });
        }
        if let Some(path) = find_cycle(cache, id, added) {
            return Err(StoreError::BlockCycle { id: String::from(row.id), path });
        }
    }
    row.blocked_by.retain(|b| !removed.contains(b));
    row.blocked_by.extend(added.iter().cloned());
    row.updated_unix = now;
    // Each blocker that exists mirrors it in `unlocks`, as `create` keeps
    // them — gained or lost.
    let mirror = |ids: &[TicketName], gain: bool| -> Vec<Row> {
        ids.iter()
            .filter_map(|b| cache.get(b))
            .filter(|b| b.unlocks.iter().any(|u| u == id) != gain)
            .map(|b| {
                let mut b = b.clone();
                if gain {
                    b.unlocks.push(TicketName::new(id));
                } else {
                    b.unlocks.retain(|u| u != id);
                }
                b.updated_unix = now;
                b
            })
            .collect()
    };
    let (gained, lost) = (mirror(added, true), mirror(removed, false));
    let (derived, reblock) = cache.reblock(&row, added, removed);
    // FILTER-30: the ticket and each blocker whose `unlocks` it writes.
    let under = lineage::under_before_with(unders, cache, || {
        gained.iter().chain(&lost).fold(lineage::Touched::of_moves(derived.announced().as_deref()).row(&row), |t, b| t.row(b))
    });
    let announced = derived.announced();
    let Derived { stored, moved } = derived;
    if let Some(placement) = moved.placement() {
        row.placement = placement;
    }
    let placed = moved.placed.as_ref().and_then(Change::placed);
    db::update_row(txn, &row)?;
    let mut blockers: Vec<(Row, String, bool)> = Vec::new();
    for (b, gain) in gained.into_iter().map(|b| (b, true)).chain(lost.into_iter().map(|b| (b, false))) {
        db::update_row(txn, &b)?;
        let body = db::body(txn, &b.id)?.ok_or_else(|| missing(&b.id))?;
        blockers.push((b, body, gain));
    }
    stored.write(txn)?;
    let body = db::body(txn, id)?.ok_or_else(|| missing(id))?;
    for (b, _, _) in &blockers {
        cache.insert(b.clone());
    }
    cache.insert(row.clone());
    cache.ids.reblocked(reblock);
    let groups = cache.take(moved);
    out.tickets.push(Change::BlockersChanged { ticket: whole(row, body), added: added.to_vec(), removed: removed.to_vec() });
    for (b, body, gain) in blockers {
        // The blocker as the write leaves it — its placement included.
        let b = cache.get(&b.id).cloned().unwrap_or(b);
        out.tickets.push(if gain {
            Change::UnlocksChanged { ticket: whole(b, body), added: TicketName::new(id) }
        } else {
            Change::UnlocksRemoved { ticket: whole(b, body), removed: TicketName::new(id) }
        });
    }
    out.groups(groups);
    out.placed(placed);
    out.filtered(announced, under.and_then(|u| u.after_in(cache)));
    Ok(())
}

impl Cache {
    /// MENGER-30: what `row`'s new `blocked_by` (`added` and `removed` from
    /// the cache's) does to the derived state, before it commits: the
    /// forward references it makes and drops ([`ids::Ids::reblock`]), its
    /// count of unsatisfied blockers and the ready set
    /// ([`ready::Ready::reblocked_touching`]), its cell and the kept tree
    /// ([`Cache::recell`]), and its placement and every one downstream that
    /// moves ([`placements::replace`]) — the roll-ups count no edge.
    fn reblock(&self, row: &Row, added: &[TicketName], removed: &[TicketName]) -> (Derived, ids::Reblock) {
        let rows = self.rows().with(row);
        let reblock = self.ids.reblock(rows, row, added, removed);
        let (ready, touched) = self.ready.reblocked_touching(rows, row);
        let (cells, tree) = self.recell(rows, row, false, &touched, None);
        let mut derived = Derived::none().readied(ready).celled(cells).treed(tree);
        reblock.store(&mut derived.stored);
        derived.moved.placed = Some(Self::replace(rows, row, false));
        (derived, reblock)
    }
}
