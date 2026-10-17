//! ACT-5: **`delete`** — tickets gone for good, and **`undelete`** while the
//! tombstone keeps the body. Owner, 2026-10-13: *"I want to be able to
//! delete, reshortcode, etc."*; the decision `keep` (2026-10-13): a
//! tombstone — id, title, who, when, why — and the history are kept; the
//! body for 30 days, so a delete can be undone, then purged.
//!
//! **Refused while something points at it** ([`plan`]), every reason named,
//! per ticket, nothing written ([`StoreError::RefusedMany`], as ACT-2's):
//!
//! - its children — unless `children: reparent` (each moves to the nearest
//!   ancestor that stays; a shortcode's hosted root never to the top level)
//!   or `children: delete` (its whole subtree goes, into hosted shortcodes
//!   too, every id listed in the answer);
//! - a ticket that waits on it — unless `unblock` (the block is dropped, and
//!   the blocked ticket's history names the delete: `history`'s `deletions`);
//! - its shortcode, when it is the root and the shortcode holds a ticket that
//!   stays;
//! - a component or a theme it defines; an input of its that is `open` or
//!   `answered` (live: close it first).
//!
//! **Which tickets.** ACT-2's selection ([`Selection`]): named, or what a
//! filter admits at the write; all of them or none.
//!
//! **What stays.** A [`Tombstone`] per ticket (the `tombstones` table, which
//! a file gains empty on open) with the whole ticket beside it, body and all,
//! until it is [`KEEP_BODY_FOR`] old — then the purge ([`Store::purge`]: at
//! start and every [`PURGE_EVERY`] inside the server) keeps the tombstone and
//! drops the rest. The transition log is not touched; its inputs stay in the
//! file (out of every read) for an undo. A number is never reused: the next
//! ticket of the shortcode takes the number after its highest, deleted or
//! not ([`Cache::next_seq`]); nothing may wait on a deleted id.
//!
//! **One write, the derived state rebuilt** ([`Cache::rederive`]). Removing
//! a ticket takes it out of every structure kept beside the rows — the
//! store's order, the paths, the runs, the roll-ups, the children index and
//! the kept tree, the ready set and the status counts, the cells, the
//! placements, the search index. In one transaction under the cache's write
//! lock: the rows move (the deleted rows out, moved children's parents,
//! dropped blocks, each blocker's `unlocks`), and then the derived state is
//! built from the rows by the scan a first open does ([`Cache::built`]) —
//! the cell ids the cache already gave kept, so every open filter still reads
//! them right — and stored over what the file held ([`stored::write_all`]).
//! So the cache after a delete is the one an open of the file would make, and
//! `--verify-derived` holds it. A cost of a scan per delete — deletes are
//! rare, admin's alone, and many go in one write; a few ms at menger's size,
//! about a second at 10⁵ tickets.
//!
//! **Its events** are a write's, from the cache before and after: each moved
//! child's `ParentChanged` (and its new parent's `ChildrenChanged`), each
//! unblocked ticket's `BlockersChanged`, each surviving parent's
//! `ChildRemoved`, each blocker's `UnlocksRemoved`; then [`Change::Deleted`];
//! then the components, the groups and the placements that moved; then, for
//! `watch { filter }`, the cells ([`cells::GONE`]: a deleted ticket leaves
//! every filter) and the lineage. `undelete` is the same write backwards
//! ([`Change::Undeleted`] first).

use super::cascade::Gathered;
use super::history::Act;
use super::update_many::UPDATE_MANY_CEILING;
use super::*;
use crate::access::AdminOf;
use crate::address::{ShortcodeId, TicketId, TicketName};
use crate::types::{InputState, Tombstone};
use rusqlite::OptionalExtension;
use roaring::RoaringBitmap;
use std::collections::BTreeSet;

/// ACT-5: how long a tombstone keeps the body (and the rest of the ticket)
/// — an undo's window: 30 days.
pub const KEEP_BODY_FOR: i64 = 30 * 24 * 60 * 60;

/// ACT-5: how often the server purges the bodies past [`KEEP_BODY_FOR`] —
/// at start, then daily.
pub const PURGE_EVERY: std::time::Duration = std::time::Duration::from_secs(24 * 60 * 60);

/// How many ids a refusal names before it counts the rest.
const NAMED: usize = 8;

/// The tombstones table: each deleted ticket's tombstone, and (`ticket`,
/// JSON, the body included) the whole ticket until it is purged.
const TABLE: &str = "
        CREATE TABLE IF NOT EXISTS tombstones (
            id          TEXT PRIMARY KEY,
            epic        TEXT NOT NULL,
            seq         INTEGER NOT NULL,
            title       TEXT NOT NULL,
            actor       TEXT NOT NULL,
            name        TEXT,
            at_unix     INTEGER NOT NULL,
            message     TEXT,
            parent      TEXT,
            reparented  TEXT NOT NULL DEFAULT '[]',
            unblocked   TEXT NOT NULL DEFAULT '[]',
            ticket      TEXT,
            purged_unix INTEGER
        );
    ";

/// A file without the table gains it, empty — no ticket was ever deleted.
pub(super) fn migrate(conn: &mut Connection) -> anyhow::Result<()> {
    conn.execute_batch(TABLE)?;
    Ok(())
}

fn has_table(conn: &Connection) -> rusqlite::Result<bool> {
    conn.query_row("SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name = 'tombstones'", [], |r| {
        r.get::<_, i64>(0).map(|n| n > 0)
    })
}

fn ids_json(ids: &[TicketId]) -> String {
    serde_json::to_string(ids).expect("serialize ids")
}

fn ids_of(column: usize, text: &str) -> rusqlite::Result<Vec<TicketId>> {
    serde_json::from_str(text).map_err(|e| rusqlite::Error::FromSqlConversionFailure(column, rusqlite::types::Type::Text, e.into()))
}

/// Every tombstone as stored — what an open reads.
pub(super) fn load(conn: &Connection) -> rusqlite::Result<Tombstones> {
    let mut out = Tombstones::default();
    if !has_table(conn)? {
        return Ok(out);
    }
    let mut stmt = conn.prepare(
        "SELECT id, epic, seq, title, actor, name, at_unix, message, parent, reparented, unblocked, ticket IS NOT NULL
             FROM tombstones",
    )?;
    let mut rows = stmt.query([])?;
    while let Some(r) = rows.next()? {
        let at_unix: i64 = r.get(6)?;
        let kept: bool = r.get(11)?;
        let t = Tombstone {
            id: r.get(0)?,
            title: r.get(3)?,
            by: r.get(4)?,
            name: r.get(5)?,
            at_unix,
            message: r.get(7)?,
            restorable_until_unix: kept.then_some(at_unix + KEEP_BODY_FOR),
            parent: r.get(8)?,
            reparented: ids_of(9, &r.get::<_, String>(9)?)?,
            unblocked: ids_of(10, &r.get::<_, String>(10)?)?,
        };
        out.put(t, r.get::<_, ShortcodeId>(1)?, r.get(2)?);
    }
    Ok(out)
}

/// `--verify-derived`'s look: an id that is both a ticket and a tombstone,
/// one line each. `None` when the file has no table yet.
pub(super) fn check(conn: &Connection, cache: &Cache) -> rusqlite::Result<Option<(usize, Vec<String>)>> {
    if !has_table(conn)? {
        return Ok(None);
    }
    let t = load(conn)?;
    let out = t.by_id.keys().filter(|id| cache.get(id).is_some()).map(|id| format!("tombstone {id}: the ticket is there too")).collect();
    Ok(Some((t.by_id.len(), out)))
}

/// ORG-3: where each deleted ticket was — its shortcode and number, as its
/// tombstone keeps them — what the log places a deleted ticket's
/// transitions by (a key is never parsed).
pub(super) fn deleted_places(conn: &Connection) -> rusqlite::Result<HashMap<String, OrderKey>> {
    if !has_table(conn)? {
        return Ok(HashMap::new());
    }
    let mut stmt = conn.prepare("SELECT id, epic, seq FROM tombstones")?;
    let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, OrderKey { shortcode: r.get(1)?, seq: r.get(2)? })))?;
    rows.collect()
}

/// The ids of the tickets the file holds a tombstone of — what the log's
/// check reads (a deleted ticket's transitions stay).
pub(super) fn deleted_ids(conn: &Connection) -> rusqlite::Result<HashSet<String>> {
    if !has_table(conn)? {
        return Ok(HashSet::new());
    }
    let mut stmt = conn.prepare("SELECT id FROM tombstones")?;
    let rows = stmt.query_map([], |r| r.get(0))?;
    rows.collect()
}

/// ACT-5: every tombstone, as the cache holds it — and each shortcode's
/// highest deleted sequence, which the next ticket of it is born past.
#[derive(Debug, Clone, Default)]
pub(super) struct Tombstones {
    by_id: HashMap<String, (Tombstone, ShortcodeId, u32)>,
    last: HashMap<ShortcodeId, u32>,
}

impl Tombstones {
    pub(super) fn get(&self, id: &str) -> Option<&Tombstone> {
        self.by_id.get(id).map(|(t, _, _)| t)
    }

    fn put(&mut self, t: Tombstone, epic: ShortcodeId, seq: u32) {
        let last = self.last.entry(epic.clone()).or_insert(0);
        *last = (*last).max(seq);
        self.by_id.insert(t.id.to_string(), (t, epic, seq));
    }

    fn remove(&mut self, id: &str) {
        if let Some((_, epic, _)) = self.by_id.remove(id) {
            match self.by_id.values().filter(|(_, e, _)| *e == epic).map(|(_, _, s)| *s).max() {
                Some(s) => self.last.insert(epic, s),
                None => self.last.remove(&epic),
            };
        }
    }

    /// `epic`'s highest deleted sequence — 0 for none.
    pub(super) fn after(&self, epic: &ShortcodeId) -> u32 {
        self.last.get(epic).copied().unwrap_or(0)
    }

    /// ORG-3: where a deleted ticket was — its shortcode and number, as its
    /// tombstone keeps them.
    pub(super) fn place_of(&self, id: &str) -> Option<(&ShortcodeId, u32)> {
        self.by_id.get(id).map(|(_, sc, seq)| (sc, *seq))
    }

    /// ORG-3: the shortcode a deleted ticket was in, as its tombstone keeps it.
    pub(super) fn shortcode_of(&self, id: &str) -> Option<&ShortcodeId> {
        self.by_id.get(id).map(|(_, sc, _)| sc)
    }

    /// ORG-3: every shortcode a tombstone is of — registered with the rest.
    pub(super) fn shortcodes(&self) -> impl Iterator<Item = ShortcodeId> + '_ {
        self.last.keys().cloned()
    }

    /// ORG-3: every deleted ticket's key — pinned, as a live one's is.
    pub(super) fn ids(&self) -> impl Iterator<Item = String> + '_ {
        self.by_id.keys().cloned()
    }

    /// A write that would have `id` wait on a deleted ticket, refused.
    pub(super) fn refuse_blockers(&self, id: &str, blockers: &[TicketName]) -> Result<(), StoreError> {
        let dead: Vec<&str> = blockers.iter().map(TicketName::as_str).filter(|b| self.by_id.contains_key(*b)).collect();
        if dead.is_empty() {
            return Ok(());
        }
        Err(StoreError::Invalid(format!(
            "{id} can't wait on {}: deleted (undelete it first, or name another blocker) — nothing written",
            dead.join(", ")
        )))
    }

    /// The tombstones whose delete moved `id` or dropped its block, oldest
    /// first — `history`'s `deletions`.
    pub(super) fn touching(&self, id: &str) -> Vec<Tombstone> {
        let mut out: Vec<Tombstone> = self
            .by_id
            .values()
            .map(|(t, _, _)| t)
            .filter(|t| t.reparented.iter().chain(&t.unblocked).any(|x| x == id))
            .cloned()
            .collect();
        out.sort_by(|a, b| (a.at_unix, &a.id).cmp(&(b.at_unix, &b.id)));
        out
    }
}

/// ACT-5: what becomes of a deleted ticket's children.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Children {
    /// Each moves to the nearest ancestor that stays.
    Reparent,
    /// Its whole subtree goes with it.
    Delete,
}

impl std::str::FromStr for Children {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, String> {
        match s.trim().to_ascii_lowercase().as_str() {
            "reparent" => Ok(Self::Reparent),
            "delete" => Ok(Self::Delete),
            other => Err(format!("children '{other}': reparent (they move to its parent) or delete (they go too)")),
        }
    }
}

/// ACT-5: what a delete is told: what becomes of the children, and whether
/// the blocks on a deleted ticket are dropped.
#[derive(Debug, Clone, Copy, Default)]
pub struct Deletion {
    pub children: Option<Children>,
    pub unblock: bool,
}

/// ACT-5: a delete's answer — the ids that went, in the store's order, and
/// each one's tombstone.
#[derive(Debug, Clone, Default)]
pub struct Deleted {
    pub ids: Vec<TicketId>,
    pub tombstones: Vec<Tombstone>,
}

/// ACT-5: an undelete's answer — the tickets back, whole, in the store's
/// order.
#[derive(Debug, Clone, Default)]
pub struct Undeleted {
    pub ids: Vec<TicketId>,
    pub tickets: Vec<Ticket>,
}

/// What [`Change::in_epic`] passes on of a delete or an undelete: the ids
/// (and their tombstones or tickets) in `epic`, when any is. ORG-3: a
/// deleted ticket's shortcode is the one its key was minted in — a deleted
/// ticket never moves (ACT-6, which moves live ones, keeps its own record).
pub(super) fn in_epic(change: Change, epic: &ShortcodeId) -> Option<Change> {
    let mine = |id: &str| id.rsplit_once('-').is_some_and(|(e, _)| e == epic.as_str());
    match change {
        Change::Deleted { ids, tombstones } => {
            let ids: Vec<TicketId> = ids.into_iter().filter(|i| mine(i)).collect();
            let tombstones = tombstones.into_iter().filter(|t| mine(&t.id)).collect();
            (!ids.is_empty()).then_some(Change::Deleted { ids, tombstones })
        }
        Change::Undeleted { ids, tickets } => {
            let ids: Vec<TicketId> = ids.into_iter().filter(|i| mine(i)).collect();
            let tickets = tickets.into_iter().filter(|t| t.shortcode == *epic).collect();
            (!ids.is_empty()).then_some(Change::Undeleted { ids, tickets })
        }
        other => Some(other),
    }
}

/// `ids` as a refusal names them — the first [`NAMED`], then how many more.
fn named<'a>(ids: impl IntoIterator<Item = &'a str>) -> String {
    let ids: Vec<&str> = ids.into_iter().collect();
    let shown = ids.iter().take(NAMED).copied().collect::<Vec<_>>().join(", ");
    match ids.len().saturating_sub(NAMED) {
        0 => shown,
        more => format!("{shown} and {more} more"),
    }
}

fn plural(n: usize, one: &str, many: &str) -> String {
    if n == 1 {
        one.to_string()
    } else {
        many.to_string()
    }
}

fn refuse(why: String) -> StoreError {
    StoreError::Invalid(why)
}

/// A delete worked out against the cache as it stands.
#[derive(Debug, Default)]
pub(super) struct Plan {
    /// The tickets that go, in the store's order.
    pub(super) gone: Vec<Row>,
    /// Each child that moves (as it is), the ticket it leaves and where it
    /// goes — in the store's order.
    pub(super) moves: Vec<(Row, String, Option<String>)>,
    /// Each ticket that stops waiting, and on which of the gone — in the
    /// store's order.
    pub(super) unblocks: Vec<(TicketId, Vec<TicketName>)>,
}

/// ACT-5: the delete of `ids` as `how` says — every refusal of every ticket,
/// or what moves (the module doc). Pure: the cache as it stands.
pub(super) fn plan(cache: &Cache, ids: &[String], how: Deletion) -> Result<Plan, Vec<(String, Vec<StoreError>)>> {
    let mut refusals: Vec<(String, Vec<StoreError>)> = Vec::new();
    let mut gone: BTreeMap<OrderKey, Row> = BTreeMap::new();
    for id in ids {
        match cache.get(id) {
            Some(r) => {
                gone.insert(OrderKey::of(r), r.clone());
            }
            None if cache.tombstones.get(id).is_some() => refusals.push((id.clone(), vec![refuse(format!("{id} is already deleted"))])),
            None => refusals.push((id.clone(), vec![StoreError::NotFound(id.clone())])),
        }
    }
    if !refusals.is_empty() {
        return Err(refusals);
    }
    if how.children == Some(Children::Delete) {
        let named: Vec<Row> = gone.values().cloned().collect();
        for r in &named {
            for (epic, seqs) in lineage::subtree(cache, &r.id, Some(r)) {
                for seq in seqs {
                    let key = OrderKey { shortcode: epic.clone(), seq };
                    if let Some(t) = cache.order.get(&key) {
                        gone.entry(key).or_insert_with(|| t.clone());
                    }
                }
            }
        }
    }
    let leaving = |id: &str| cache.at.get(id).is_some_and(|k| gone.contains_key(k));
    let owners: HashMap<String, Vec<String>> = {
        let mut by: HashMap<String, Vec<String>> = HashMap::new();
        for c in cache.components.components() {
            if let Some(o) = c.owner {
                by.entry(o).or_default().push(format!("the component <{}>", c.name));
            }
        }
        for th in cache.themes.by_name.values() {
            if let Some(o) = &th.owner {
                by.entry(o.clone()).or_default().push(format!("the theme `{}`", th.name));
            }
        }
        by
    };
    let mut held_gone: HashMap<&ShortcodeId, u64> = HashMap::new();
    for t in gone.values() {
        *held_gone.entry(&t.shortcode).or_default() += 1;
    }
    let mut moves: Vec<(Row, String, Option<String>)> = Vec::new();
    let mut unblocks: BTreeMap<OrderKey, (TicketId, Vec<TicketName>)> = BTreeMap::new();
    for t in gone.values() {
        let mut why = Vec::new();
        let kids: Vec<&Row> = cache
            .children
            .of(&t.id)
            .into_iter()
            .filter_map(|(epic, seq)| cache.order.get(&OrderKey { shortcode: epic, seq }))
            .filter(|c| !leaving(&c.id))
            .collect();
        // The nearest ancestor that stays — where `reparent` moves them.
        let mut target = t.parent.clone();
        let mut seen = HashSet::from([t.id.to_string()]);
        while let Some(p) = target.clone().filter(|p| leaving(p)) {
            if !seen.insert(p.to_string()) {
                target = None;
                break;
            }
            target = cache.get(&p).and_then(|r| r.parent.clone());
        }
        if !kids.is_empty() {
            let where_to = target.as_deref().map_or("the top level".to_string(), str::to_string);
            match how.children {
                None => why.push(refuse(format!(
                    "{} has {} {} — say what becomes of {}: children reparent (to {where_to}) or children delete",
                    t.id,
                    plural(kids.len(), "a child", "children"),
                    named(kids.iter().map(|c| c.id.as_str())),
                    plural(kids.len(), "it", "them"),
                ))),
                Some(_) => {
                    for c in kids {
                        if target.is_none() && c.shortcode != t.shortcode {
                            why.push(refuse(format!(
                                "{} roots the shortcode {} hosted under {}, and a hosted root can't return to the top level — \
                                 move it first (set_parent), or delete it too",
                                c.id, c.epic, t.id
                            )));
                        } else {
                            moves.push((c.clone(), t.id.to_string(), target.clone().map(String::from)));
                        }
                    }
                }
            }
        }
        let waiting: Vec<&Row> = t
            .unlocks
            .iter()
            .filter(|u| !leaving(u))
            .filter_map(|u| cache.get(u))
            .filter(|w| w.blocked_by.iter().any(|b| *b == t.id))
            .collect();
        if !waiting.is_empty() {
            if how.unblock {
                for w in waiting {
                    unblocks.entry(OrderKey::of(w)).or_insert_with(|| (w.id.clone(), Vec::new())).1.push(TicketName::from(&t.id));
                }
            } else {
                why.push(refuse(format!(
                    "{} {} on it — unblock drops {} block",
                    named(waiting.iter().map(|w| w.id.as_str())),
                    plural(waiting.len(), "waits", "wait"),
                    plural(waiting.len(), "its", "their"),
                )));
            }
        }
        if shortcode_root(cache, &t.shortcode).is_some_and(|r| r.id == t.id) {
            let others = cache.ids.held(&t.shortcode).saturating_sub(held_gone.get(&t.shortcode).copied().unwrap_or(0));
            if others > 0 {
                why.push(refuse(format!(
                    "{} roots the shortcode {}, which holds {others} other {} — delete them with it (children delete), or move them first",
                    t.id,
                    t.epic,
                    plural(others as usize, "ticket", "tickets"),
                )));
            }
        }
        if let Some(owned) = owners.get(t.id.as_str()) {
            why.push(refuse(format!("{} defines {} — move the definition to another ticket first", t.id, owned.join(", "))));
        }
        let live: Vec<&str> = cache
            .inputs
            .of(&t.id)
            .iter()
            .filter(|i| matches!(i.state, InputState::Open | InputState::Answered))
            .map(|i| i.id.as_str())
            .collect();
        if !live.is_empty() {
            why.push(refuse(format!(
                "{} asks the owner {} ({}) — close {} first (close_input)",
                t.id,
                plural(live.len(), "an input", "inputs"),
                named(live.iter().copied()),
                plural(live.len(), "it", "them"),
            )));
        }
        if !why.is_empty() {
            refusals.push((t.id.to_string(), why));
        }
    }
    if !refusals.is_empty() {
        return Err(refusals);
    }
    moves.sort_by_key(|(c, _, _)| OrderKey::of(c));
    Ok(Plan { gone: gone.into_values().collect(), moves, unblocks: unblocks.into_values().collect() })
}

/// One ticket an undelete brings back: as the tombstone kept it, and the
/// tombstone.
type Back = (Ticket, Tombstone);

/// ACT-5: the undelete of `found` (each id with the ticket its tombstone
/// kept, `None` once purged) — every refusal, or what comes back. Pure: the
/// cache as it stands.
fn unplan(cache: &Cache, found: Vec<(String, Option<Ticket>)>) -> Result<Vec<Back>, Vec<(String, Vec<StoreError>)>> {
    let mut refusals = Vec::new();
    let back: HashSet<String> = found.iter().map(|(id, _)| id.clone()).collect();
    let mut out: Vec<Back> = Vec::new();
    for (id, ticket) in found {
        let Some(tomb) = cache.tombstones.get(&id) else {
            let why = match cache.get(&id) {
                Some(_) => refuse(format!("{id} is not deleted")),
                None => StoreError::NotFound(id.clone()),
            };
            refusals.push((id, vec![why]));
            continue;
        };
        let Some(mut ticket) = ticket else {
            refusals.push((id.clone(), vec![refuse(format!("{id}'s body was purged — a tombstone keeps it {} days", KEEP_BODY_FOR / 86_400))]));
            continue;
        };
        // ORG-3: its shortcode as its tombstone keeps it — a ticket kept
        // before ORG-3 says only its letters.
        if let Some(sc) = cache.tombstones.shortcode_of(&id) {
            ticket.shortcode = sc.clone();
        }
        let mut why = Vec::new();
        let gone = |x: &str| cache.get(x).is_none() && !back.contains(x);
        if let Some(p) = ticket.parent.as_deref().filter(|p| gone(p)) {
            let what = if cache.tombstones.get(p).is_some() { "deleted — undelete it too" } else { "no ticket" };
            why.push(refuse(format!("{id} hung under {p}, which is {what}")));
        }
        let dead: Vec<&str> = ticket.blocked_by.iter().map(TicketName::as_str).filter(|b| gone(b) && cache.tombstones.get(b).is_some()).collect();
        if !dead.is_empty() {
            why.push(refuse(format!("{id} waited on {}, deleted — undelete {} too", named(dead.iter().copied()), plural(dead.len(), "it", "them"))));
        }
        if let Some(path) = &ticket.path {
            if let Some(holder) = cache.paths.get(&(ticket.shortcode.clone(), path.clone())) {
                why.push(StoreError::PathTaken { epic: ticket.epic.to_string(), path: path.clone(), holder: holder.clone() });
            }
        }
        let (uses, _) = components::read(&ticket.body);
        if let Err(e) = cache.plan_registry(&id, &uses, Some(&id)) {
            why.push(e);
        }
        if why.is_empty() {
            out.push((ticket, tomb.clone()));
        } else {
            refusals.push((id, why));
        }
    }
    if !refusals.is_empty() {
        return Err(refusals);
    }
    out.sort_by_key(|(t, _)| (t.shortcode.clone(), t.seq));
    Ok(out)
}

/// What a rebuilding write remembers of the cache before it: each group as
/// drawn, each row's placement, and the cells of the tickets it touches.
pub(super) struct Before {
    groups: HashMap<ShortcodeId, Group>,
    placed: BTreeMap<OrderKey, Option<crate::types::Placement>>,
    cells: BTreeMap<OrderKey, u32>,
}

impl Before {
    pub(super) fn of(cache: &Cache, touched: impl IntoIterator<Item = OrderKey>) -> Self {
        let groups = cache.rollups.groups().into_iter().map(|g| (g.shortcode.clone(), cache.dress(g))).collect();
        let placed = cache.order.iter().map(|(k, r)| (k.clone(), r.placement)).collect();
        let cells = touched
            .into_iter()
            .filter_map(|k| {
                let id = cache.order.get(&k).and_then(|r| cache.cells.id(cache.key(r)))?;
                Some((k, id))
            })
            .collect();
        Self { groups, placed, cells }
    }

    /// The groups, placements and cells that moved, as `cache` (after the
    /// write) has them — into `out`. `touched` in the store's order.
    pub(super) fn moved(self, cache: &Cache, touched: &[OrderKey], out: &mut Gathered) -> Option<Arc<cells::Moves>> {
        let groups: Vec<Group> = cache
            .rollups
            .groups()
            .into_iter()
            .map(|g| cache.dress(g))
            .filter(|g| self.groups.get(&g.shortcode) != Some(g))
            .collect();
        out.groups(groups);
        let shifts: Vec<(ShortcodeId, crate::types::PlacementChange)> = cache
            .order
            .iter()
            .filter(|(k, r)| r.placement.is_some() && self.placed.get(*k).copied().flatten() != r.placement)
            .map(|(k, r)| (k.shortcode.clone(), crate::types::PlacementChange { id: r.id.to_string(), placement: r.placement.expect("filtered") }))
            .collect();
        if !shifts.is_empty() {
            out.placed(Some(Change::PlacementsChanged { changes: shifts }));
        }
        let mut moves = Vec::new();
        for k in touched {
            let from = self.cells.get(k).copied();
            let to = match cache.order.get(k) {
                Some(r) => cache.cells.id(cache.key(r)),
                None => Some(cells::GONE),
            };
            if let Some(to) = to.filter(|to| from != Some(*to)) {
                moves.push(cells::CellMove { shortcode: k.shortcode.clone(), seq: k.seq, from, to });
            }
        }
        if moves.is_empty() {
            return None;
        }
        let counts = (moves.len() > cells::RECOUNT_OVER).then(|| {
            let epics: BTreeSet<&ShortcodeId> = moves.iter().map(|m| &m.shortcode).collect();
            epics
                .into_iter()
                .map(|e| (e.clone(), cache.cells.held_in(e).map_or_else(Vec::new, |h| h.iter().map(|(c, b)| (*c, b.len())).collect())))
                .collect()
        });
        Some(Arc::new(cells::Moves { moves, counts }))
    }
}

/// The open `under` watches' view of the tickets at `keys` and the subtrees
/// of `roots`, before a rebuilding write.
pub(super) fn under_of(unders: &lineage::UnderWatches, cache: &Cache, keys: &[OrderKey], roots: &[&Row]) -> Option<lineage::UnderBefore> {
    lineage::under_before_with(unders, cache, || {
        let marks = cells::Moves {
            moves: keys.iter().map(|k| cells::CellMove { shortcode: k.shortcode.clone(), seq: k.seq, from: None, to: 0 }).collect(),
            counts: None,
        };
        let mut t = lineage::Touched::of_moves(Some(&marks));
        for r in roots {
            t.subtree(cache, &r.id, Some(r));
        }
        t
    })
}

impl Cache {
    /// ACT-5: the sequence `epic`'s next ticket gets — past its highest, and
    /// past every number a delete freed: none is reused.
    pub(super) fn next_seq(&self, epic: &ShortcodeId) -> u32 {
        // ACT-6: and past every number a move took out of it.
        self.ids.next_seq(epic).max(self.tombstones.after(epic) + 1).max(self.shortcodes.moved_after(epic) + 1)
    }

    /// ACT-5: the sequences below `last` that `epic` does not hold — a
    /// deleted ticket's, or a hand edit's hole. Nothing for a dense run.
    pub(super) fn holes(&self, epic: &ShortcodeId, last: u32) -> Vec<u32> {
        if self.ids.held(epic) >= u64::from(last) {
            return Vec::new();
        }
        let from = OrderKey { shortcode: epic.clone(), seq: 1 };
        let to = OrderKey { shortcode: epic.clone(), seq: last };
        let held: RoaringBitmap = self.order.range(from..=to).map(|(k, _)| k.seq).collect();
        (1..=last).filter(|s| !held.contains(*s)).collect()
    }

    /// ACT-5: the derived state replaced by `built`'s — built by a scan of
    /// the rows the write leaves ([`Cache::built`]) — with `inputs`; the
    /// placements and the log's times from the rows again. Every record
    /// beside the tickets (comments, notes, members, roles, themes and their
    /// channel, the log's marks) stays as it is.
    ///
    /// ORG-3: and the shortcodes as `built` has them — its rows dressed by
    /// them, a namespace a reparented root moved included ([`Cache::rebuilt`]).
    fn rederive(&mut self, built: Cache, inputs: inputs::Inputs) {
        let Cache { order, at, paths, rollups, ids, children, ready, cells, shortcodes, .. } = built;
        (self.order, self.at, self.paths, self.rollups, self.ids) = (order, at, paths, rollups, ids);
        self.shortcodes = shortcodes;
        (self.children, self.ready, self.cells) = (children, ready, cells);
        self.inputs = inputs;
        self.place_all();
        self.log.time_all(self.order.values());
    }

    /// ORG-3: a rebuilding write's cache (`built`, from the rows its
    /// transaction leaves) given the shortcodes — every namespace a moved
    /// root changes, checked (refused, naming the one there, into a
    /// namespace holding its code) and written in `txn` — and its rows
    /// dressed by them.
    fn rebuilt(&self, txn: &rusqlite::Transaction<'_>, mut built: Cache) -> Result<Cache, StoreError> {
        built.shortcodes = self.shortcodes.clone();
        if let Some(m) = self.namespaces(built.rows(), &built.rollups, None, None)? {
            m.write(txn)?;
            built.shortcodes = m.after;
        }
        built.readdress_all();
        Ok(built)
    }
}

/// `id`'s row as `rows` (the write's) has it, else the cache's.
fn edited<'r>(rows: &'r mut BTreeMap<String, Row>, cache: &Cache, id: &str) -> Option<&'r mut Row> {
    if !rows.contains_key(id) {
        rows.insert(id.to_string(), cache.get(id)?.clone());
    }
    rows.get_mut(id)
}

/// A ticket as the write leaves it — its row from `built`, its body from the
/// write's transaction.
pub(super) fn after(txn: &rusqlite::Transaction<'_>, built: &Cache, id: &str) -> Result<Ticket, StoreError> {
    let row = built.get(id).cloned().ok_or_else(|| StoreError::NotFound(id.to_string()))?;
    let body = db::body(txn, id)?.ok_or_else(|| missing(id))?;
    Ok(whole(row, body))
}

impl Store {
    /// ACT-5: `id`'s tombstone, when it names a deleted ticket.
    ///
    /// ORG-3: by any address the resolver takes to it (today's ids are
    /// pinned, deleted or not).
    pub(super) fn tombstone_unscoped(&self, id: &str) -> Option<Tombstone> {
        let cache = self.tickets.read().expect("store lock poisoned");
        let key = match cache.find_ticket(id, shortcodes::OrgReach::All) {
            shortcodes::Found::One(key) => key,
            _ => crate::address::folded(id),
        };
        cache.tombstones.get(&key).cloned()
    }

    /// ACT-5: what `extents` answers — each shortcode's numbers as given out,
    /// in shortcode order: the highest sequence any ticket of it has had (a
    /// deleted one's too, whose number is never reused, so a shortcode whose
    /// every ticket was deleted is still here) and the ones below it that it
    /// does not hold ([`Cache::holes`]). A walk only of a shortcode that is
    /// not dense; nothing else past [`Store::extents`].
    ///
    /// ORG-3: each with its letters, beside its identity.
    pub(super) fn extents_holed_unscoped(&self) -> Vec<(ShortcodeId, crate::address::Code, u32, Vec<u32>)> {
        let cache = self.tickets.read().expect("store lock poisoned");
        let mut last: BTreeMap<&ShortcodeId, u32> = cache.ids.last_seqs().collect();
        for (epic, seq) in &cache.tombstones.last {
            let l = last.entry(epic).or_insert(0);
            *l = (*l).max(*seq);
        }
        last.into_iter().map(|(epic, l)| (epic.clone(), cache.shortcodes.code(epic), l, cache.holes(epic, l))).collect()
    }

    /// [`Store::delete_by`] as nobody signed in, through the API, with every
    /// ticket allowed.
    pub async fn delete(&self, sys: &System, select: Selection, how: Deletion, message: Option<String>) -> Result<Deleted, StoreError> {
        self.delete_by(select, how, Act { message, ..Act::default() }, |ids| Ok(ids.iter().map(|id| sys.permit(id)).collect())).await
    }

    /// ACT-5: the tickets `select` names deleted by `act`, as `how` says —
    /// all of them or none (the module doc). `may` is asked of every ticket
    /// that would go (the selection and, `children: delete`, the subtrees)
    /// before any rule; its refusal is the write's.
    pub async fn delete_by(
        &self,
        select: Selection,
        how: Deletion,
        act: Act,
        may: impl FnOnce(&[String]) -> Result<Vec<Permit<Admin>>, String> + Send,
    ) -> Result<Deleted, StoreError> {
        let serial = self.serial().await;
        let ids = self.selected(select)?;
        if ids.is_empty() {
            return Ok(Deleted::default());
        }
        let planned = {
            let cache = self.tickets.read().expect("store lock poisoned");
            plan(&cache, &ids, how)
        };
        let p = planned.map_err(|refusals| StoreError::RefusedMany { refusals })?;
        let all: Vec<String> = p.gone.iter().map(|t| t.id.to_string()).collect();
        // ACCESS-11: an admin's permit for every ticket that goes.
        let permits = may(&all).map_err(StoreError::Invalid)?;
        writs::covers_exactly(&self.tickets.read().expect("store lock poisoned"), &permits, &all)?;
        self.in_one_write(serial, move |conn, cache, unders| {
            super::revisions::act(&act);
            Self::deleting(conn, cache, unders, p, act)
        })
        .await
    }

    /// The delete's one write — see the module doc.
    fn deleting(
        conn: &mut Connection,
        cache: &mut Cache,
        unders: &lineage::UnderWatches,
        p: Plan,
        act: Act,
    ) -> Result<(Deleted, Vec<Change>), StoreError> {
        let txn = conn.transaction()?;
        let now = chrono::Utc::now().timestamp();
        let leaving: HashSet<&str> = p.gone.iter().map(|t| t.id.as_str()).collect();
        let mut touched: Vec<OrderKey> = p.gone.iter().map(OrderKey::of).collect();
        touched.extend(p.unblocks.iter().filter_map(|(x, _)| cache.at.get(x.as_str()).cloned()));
        touched.sort();
        let roots: Vec<&Row> = p.moves.iter().map(|(c, _, _)| c).collect();
        let under = under_of(unders, cache, &touched, &roots);
        let before = Before::of(cache, touched.iter().cloned());
        // The rows that stay and move: each moved child's parent, each
        // unblocked ticket's blockers, each blocker's `unlocks`.
        let mut rows: BTreeMap<String, Row> = BTreeMap::new();
        for (c, _, to) in &p.moves {
            let r = edited(&mut rows, cache, &c.id).ok_or_else(|| missing(&c.id))?;
            (r.parent, r.updated_unix) = (to.clone().map(crate::address::TicketId::new), now);
        }
        for (x, from) in &p.unblocks {
            let r = edited(&mut rows, cache, x).ok_or_else(|| missing(x))?;
            r.blocked_by.retain(|b| !from.contains(b));
            r.updated_unix = now;
        }
        let mut unlocks: Vec<(TicketName, TicketName)> = Vec::new();
        for t in &p.gone {
            for b in t.blocked_by.iter().filter(|b| !leaving.contains(b.as_str())) {
                if let Some(r) = edited(&mut rows, cache, b) {
                    if r.unlocks.iter().any(|u| *u == t.id) {
                        r.unlocks.retain(|u| *u != t.id);
                        r.updated_unix = now;
                        unlocks.push((b.clone(), TicketName::from(&t.id)));
                    }
                }
            }
        }
        for r in rows.values() {
            db::update_row(&txn, r)?;
        }
        // The registry: each deleted ticket's references dropped, as a body
        // with none would drop them (it defines nothing: refused above).
        let mut registry = Vec::new();
        for t in &p.gone {
            let plan = cache.plan_registry(&t.id, &components::Uses::default(), Some(&t.id))?;
            components::store(&txn, &plan.moves)?;
            cache.take_registry(plan.moves.clone());
            registry.extend(Change::components(plan));
        }
        // The tombstones, beside the whole tickets; the rows and the index out.
        let mut tombstones = Vec::with_capacity(p.gone.len());
        for t in &p.gone {
            let whole = db::ticket(&txn, &t.id)?.ok_or_else(|| missing(&t.id))?;
            let tomb = Tombstone {
                id: t.id.clone(),
                title: t.title.clone(),
                by: act.actor.clone(),
                name: act.name.clone(),
                at_unix: now,
                message: act.message.clone(),
                restorable_until_unix: Some(now + KEEP_BODY_FOR),
                parent: t.parent.clone(),
                reparented: p.moves.iter().filter(|(_, from, _)| *from == t.id).map(|(c, _, _)| c.id.clone()).collect(),
                unblocked: p.unblocks.iter().filter(|(_, from)| from.iter().any(|b| *b == t.id)).map(|(x, _)| x.clone()).collect(),
            };
            txn.prepare_cached(
                "INSERT INTO tombstones (id, epic, seq, title, actor, name, at_unix, message, parent, reparented, unblocked, ticket)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            )?
            .execute(rusqlite::params![
                tomb.id,
                t.shortcode,
                t.seq,
                tomb.title,
                tomb.by,
                tomb.name,
                tomb.at_unix,
                tomb.message,
                tomb.parent,
                ids_json(&tomb.reparented),
                ids_json(&tomb.unblocked),
                serde_json::to_string(&whole).expect("serialize a ticket"),
            ])?;
            search::remove(&txn, &t.id)?;
            // TICKETS-7: its revisions stay with the tombstone, the last saying so.
            super::revisions::gone(&txn, &t.id)?;
            txn.prepare_cached("DELETE FROM tickets WHERE id = ?1")?.execute([&t.id])?;
            tombstones.push(tomb);
        }
        // The derived state, built from the rows as the write leaves them.
        let mut inputs = cache.inputs.clone();
        for t in &p.gone {
            inputs.take(&t.id, Vec::new());
        }
        let built = Cache::built(db::load_rows(&txn).map_err(|e| StoreError::Db(e.to_string()))?, inputs.clone(), cache.cells.keys());
        let built = cache.rebuilt(&txn, built)?;
        stored::write_all(&txn, &built.form())?;
        // The events of the tickets that stay and moved, as the write leaves them.
        let mut out = Gathered::default();
        for (c, from, to) in &p.moves {
            out.tickets.push(Change::ParentChanged { ticket: after(&txn, &built, &c.id)?, previous: Some(from.clone()) });
            if let Some(to) = to.as_deref().and_then(|t| built.get(t)) {
                out.tickets.push(Change::ChildrenChanged { id: to.id.to_string(), shortcode: to.shortcode.clone(), added: c.id.to_string(), child_count: to.child_count });
            }
        }
        for (x, from) in &p.unblocks {
            out.tickets.push(Change::BlockersChanged { ticket: after(&txn, &built, x)?, added: Vec::new(), removed: from.clone() });
        }
        for t in &p.gone {
            if let Some(parent) = t.parent.as_deref().filter(|q| !leaving.contains(q)).and_then(|q| built.get(q)) {
                out.tickets.push(Change::ChildRemoved {
                    id: parent.id.to_string(),
                    shortcode: parent.shortcode.clone(),
                    removed: t.id.to_string(),
                    child_count: parent.child_count,
                });
            }
        }
        for (b, removed) in unlocks {
            out.tickets.push(Change::UnlocksRemoved { ticket: after(&txn, &built, &b)?, removed });
        }
        super::revisions::commit(txn)?;
        let ids: Vec<TicketId> = p.gone.iter().map(|t| t.id.clone()).collect();
        cache.rederive(built, inputs);
        for (t, tomb) in p.gone.iter().zip(&tombstones) {
            cache.tombstones.put(tomb.clone(), t.shortcode.clone(), t.seq);
        }
        out.tickets.push(Change::Deleted { ids: ids.clone(), tombstones: tombstones.clone() });
        out.components = registry;
        let cells = before.moved(cache, &touched, &mut out);
        out.filtered(cells, under.and_then(|u| u.after_in(cache)));
        Ok((Deleted { ids, tombstones }, out.finish()))
    }

    /// ACCESS-11: whether `proofs` hold, for each of `ids`, admin on the
    /// scope its shortcode is — `*`, its organization, or the shortcode.
    fn undelete_covered(&self, ids: &[String], proofs: &[(String, AdminOf)]) -> Result<(), StoreError> {
        let cache = self.tickets.read().expect("store lock poisoned");
        let r = &cache.shortcodes;
        for id in ids {
            let sc = id.rsplit_once('-').map(|(e, _)| ShortcodeId::new(e));
            let covered = proofs.iter().filter(|(p, _)| p.trim().eq_ignore_ascii_case(id)).any(|(_, a)| match a.scope() {
                crate::members::Scope::Everywhere => true,
                crate::members::Scope::Org(o) => sc.as_ref().and_then(|s| r.org(s)).is_some_and(|w| r.canonical(w).is_some() && r.canonical(w) == r.canonical(o)),
                s @ crate::members::Scope::Shortcode { .. } => sc.as_ref().is_some_and(|sc| *s == writs::scope_of(&cache, sc)),
                // ACCESS-21: undeleting is its shortcode's admin's.
                crate::members::Scope::Under(_) | crate::members::Scope::Ticket(_) => false,
            });
            if !covered {
                return Err(StoreError::Invalid(format!("{id}: the write was not given the proof it needs (nothing written)")));
            }
        }
        Ok(())
    }

    /// [`Store::undelete_by`] as nobody signed in, with every ticket allowed.
    pub async fn undelete(&self, sys: &System, ids: &[String]) -> Result<Undeleted, StoreError> {
        self.undelete_by(ids, Act::default(), |ids| Ok(ids.iter().map(|id| (id.clone(), sys.admin_of(&crate::members::Scope::Everywhere))).collect())).await
    }

    /// ACT-5: the tickets `ids` names brought back, whole, as their
    /// tombstones kept them — each child its delete moved back under it and
    /// each block it dropped put back, where nothing has moved them since —
    /// in one write, all or none. Refused: an id that is not deleted, one
    /// whose body was purged, one whose parent or blocker is still deleted
    /// (undelete it in the same call), a path or a component another ticket
    /// has taken since. `act` is who asked (the tombstone goes).
    pub async fn undelete_by(
        &self,
        ids: &[String],
        _act: Act,
        may: impl FnOnce(&[String]) -> Result<Vec<(String, AdminOf)>, String> + Send,
    ) -> Result<Undeleted, StoreError> {
        let ids: Vec<String> = rules::distinct(&ids.iter().map(|i| i.trim().to_ascii_uppercase()).filter(|i| !i.is_empty()).collect::<Vec<_>>());
        if ids.is_empty() || ids.len() > UPDATE_MANY_CEILING {
            return Err(StoreError::Invalid(format!("name 1 to {UPDATE_MANY_CEILING} tickets")));
        }
        // ACCESS-11: admin, for each id, on the scope its shortcode is (or
        // one the store says covers it) — a deleted ticket has no permit.
        let proofs = may(&ids).map_err(StoreError::Invalid)?;
        let serial = self.serial().await;
        self.undelete_covered(&ids, &proofs)?;
        let db = Arc::clone(&self.db);
        let asked = ids.clone();
        let found = tokio::task::spawn_blocking(move || -> Result<Vec<(String, Option<Ticket>)>, StoreError> {
            let conn = db.lock().expect("db lock poisoned");
            let mut out = Vec::with_capacity(asked.len());
            for id in asked {
                let kept: Option<Option<String>> =
                    conn.prepare_cached("SELECT ticket FROM tombstones WHERE id = ?1")?.query_row([&id], |r| r.get(0)).optional()?;
                let ticket = kept.flatten().map(|j| serde_json::from_str::<Ticket>(&j)).transpose().map_err(|e| StoreError::Db(format!("{id}'s tombstone: {e}")))?;
                out.push((id, ticket));
            }
            Ok(out)
        })
        .await
        .map_err(|e| StoreError::Db(format!("read task failed: {e}")))??;
        let back = {
            let cache = self.tickets.read().expect("store lock poisoned");
            unplan(&cache, found)
        };
        let back = back.map_err(|refusals| StoreError::RefusedMany { refusals })?;
        self.in_one_write(serial, move |conn, cache, unders| {
            super::revisions::act(&_act);
            Self::undeleting(conn, cache, unders, back)
        })
        .await
    }

    /// The undelete's one write — the delete's, backwards.
    fn undeleting(
        conn: &mut Connection,
        cache: &mut Cache,
        unders: &lineage::UnderWatches,
        back: Vec<Back>,
    ) -> Result<(Undeleted, Vec<Change>), StoreError> {
        let txn = conn.transaction()?;
        let now = chrono::Utc::now().timestamp();
        let coming: HashSet<String> = back.iter().map(|(t, _)| t.id.to_string()).collect();
        let exists = |cache: &Cache, id: &str| cache.get(id).is_some() || coming.contains(id);
        let mut rows: BTreeMap<String, Row> = BTreeMap::new();
        let (mut moved_back, mut reblocked, mut unlocked) = (Vec::new(), Vec::new(), Vec::new());
        let mut tickets: Vec<Ticket> = Vec::with_capacity(back.len());
        for (ticket, tomb) in &back {
            for c in &tomb.reparented {
                if let Some(r) = edited(&mut rows, cache, c).filter(|r| r.parent.as_deref() == tomb.parent.as_deref()) {
                    (r.parent, r.updated_unix) = (Some(ticket.id.clone()), now);
                    moved_back.push((c.clone(), tomb.parent.clone(), ticket.id.clone()));
                }
            }
            for x in &tomb.unblocked {
                if let Some(r) = edited(&mut rows, cache, x).filter(|r| !r.blocked_by.iter().any(|b| *b == ticket.id)) {
                    r.blocked_by.push(TicketName::from(&ticket.id));
                    r.updated_unix = now;
                    reblocked.push((x.clone(), ticket.id.clone()));
                }
            }
            for b in &ticket.blocked_by {
                if let Some(r) = edited(&mut rows, cache, b).filter(|r| !r.unlocks.iter().any(|u| *u == ticket.id)) {
                    r.unlocks.push(TicketName::from(&ticket.id));
                    r.updated_unix = now;
                    unlocked.push((b.clone(), ticket.id.clone()));
                }
            }
        }
        for (ticket, _) in &back {
            let mut t = ticket.clone();
            // Its `unlocks`: who waits on it now — those that did, those
            // whose block came back and those coming back with it — each one
            // that still names it.
            let mut waits: Vec<TicketName> = t.unlocks.clone();
            waits.extend(reblocked.iter().filter(|(_, b)| *b == t.id).map(|(x, _)| TicketName::from(x)));
            waits.extend(back.iter().filter(|(o, _)| o.blocked_by.iter().any(|b| *b == t.id)).map(|(o, _)| TicketName::from(&o.id)));
            let names = |w: &str| match back.iter().find(|(o, _)| o.id == w) {
                Some((o, _)) => o.blocked_by.iter().any(|b| *b == t.id),
                None => rows.get(w).or_else(|| cache.get(w)).is_some_and(|r| r.blocked_by.iter().any(|b| *b == t.id)),
            };
            waits.retain(|w| exists(cache, w) && names(w));
            t.unlocks = rules::distinct(&waits);
            t.child_count = 0;
            db::upsert(&txn, &t)?;
            search::put(&txn, &t.id, &t.title, &t.body)?;
            txn.prepare_cached("DELETE FROM tombstones WHERE id = ?1")?.execute([&t.id])?;
            tickets.push(t);
        }
        for r in rows.values().filter(|r| !coming.contains(r.id.as_str())) {
            db::update_row(&txn, r)?;
        }
        let mut registry = Vec::new();
        for t in &tickets {
            let (uses, _) = components::read(&t.body);
            let plan = cache.plan_registry(&t.id, &uses, Some(&t.id))?;
            components::store(&txn, &plan.moves)?;
            cache.take_registry(plan.moves.clone());
            registry.extend(Change::components(plan));
        }
        // Their inputs, as the file kept them.
        let mut inputs = cache.inputs.clone();
        let kept = inputs::load(&txn)?;
        let mut input_events = Vec::new();
        for t in &tickets {
            let list = kept.of(&t.id).to_vec();
            input_events.extend(list.iter().map(|input| Change::InputChanged { shortcode: t.shortcode.clone(), input: input.clone() }));
            inputs.take(&t.id, list);
        }
        let keys: Vec<OrderKey> = {
            let mut k: Vec<OrderKey> = tickets.iter().map(|t| OrderKey { shortcode: t.shortcode.clone(), seq: t.seq }).collect();
            k.extend(reblocked.iter().filter_map(|(x, _): &(TicketId, TicketId)| cache.at.get(x.as_str()).cloned()));
            k.sort();
            k.dedup();
            k
        };
        let under = under_of(unders, cache, &keys, &[]);
        let before = Before::of(cache, keys.iter().cloned());
        let built = Cache::built(db::load_rows(&txn).map_err(|e| StoreError::Db(e.to_string()))?, inputs.clone(), cache.cells.keys());
        let built = cache.rebuilt(&txn, built)?;
        stored::write_all(&txn, &built.form())?;
        let mut out = Gathered::default();
        let ids: Vec<TicketId> = tickets.iter().map(|t| t.id.clone()).collect();
        let whole: Vec<Ticket> = ids.iter().map(|id| after(&txn, &built, id)).collect::<Result<_, _>>()?;
        out.tickets.push(Change::Undeleted { ids: ids.clone(), tickets: whole.clone() });
        for t in &whole {
            if let Some(p) = t.parent.as_deref().and_then(|p| built.get(p)) {
                out.tickets.push(Change::ChildrenChanged { id: p.id.to_string(), shortcode: p.shortcode.clone(), added: t.id.to_string(), child_count: p.child_count });
            }
        }
        for (c, from, to) in &moved_back {
            out.tickets.push(Change::ParentChanged { ticket: after(&txn, &built, c)?, previous: from.clone().map(String::from) });
            if let Some(f) = from.as_deref().and_then(|f| built.get(f)) {
                out.tickets.push(Change::ChildRemoved { id: f.id.to_string(), shortcode: f.shortcode.clone(), removed: c.to_string(), child_count: f.child_count });
            }
            if let Some(n) = built.get(to) {
                out.tickets.push(Change::ChildrenChanged { id: n.id.to_string(), shortcode: n.shortcode.clone(), added: c.to_string(), child_count: n.child_count });
            }
        }
        for (x, b) in &reblocked {
            out.tickets.push(Change::BlockersChanged { ticket: after(&txn, &built, x)?, added: vec![TicketName::from(b)], removed: Vec::new() });
        }
        for (b, added) in unlocked.into_iter().filter(|(b, _)| !coming.contains(b.as_str())) {
            out.tickets.push(Change::UnlocksChanged { ticket: after(&txn, &built, &b)?, added: TicketName::from(added) });
        }
        super::revisions::commit(txn)?;
        cache.rederive(built, inputs);
        for id in &ids {
            cache.tombstones.remove(id);
        }
        out.components = registry;
        out.inputs = input_events;
        let cells = before.moved(cache, &keys, &mut out);
        out.filtered(cells, under.and_then(|u| u.after_in(cache)));
        Ok((Undeleted { ids, tickets: whole }, out.finish()))
    }

    /// ACT-5: the bodies past [`KEEP_BODY_FOR`] at `now` purged — each
    /// tombstone keeps its id, title, who, when and why, and can no longer
    /// be undone. Returns the ids purged. What the server runs at start and
    /// every [`PURGE_EVERY`].
    ///
    /// ACCESS-11: the system's alone — the server's own upkeep.
    pub async fn purge(&self, _sys: &System, now: i64) -> Result<Vec<String>, StoreError> {
        let _serial = self.serial().await;
        let db = Arc::clone(&self.db);
        let purged = tokio::task::spawn_blocking(move || -> Result<Vec<String>, StoreError> {
            let mut conn = db.lock().expect("db lock poisoned");
            let txn = conn.transaction()?;
            let ids: Vec<String> = {
                let mut stmt = txn.prepare("SELECT id FROM tombstones WHERE ticket IS NOT NULL AND at_unix + ?1 <= ?2 ORDER BY epic, seq")?;
                let rows = stmt.query_map([KEEP_BODY_FOR, now], |r| r.get(0))?;
                rows.collect::<rusqlite::Result<_>>()?
            };
            txn.execute("UPDATE tombstones SET ticket = NULL, purged_unix = ?2 WHERE ticket IS NOT NULL AND at_unix + ?1 <= ?2", [KEEP_BODY_FOR, now])?;
            txn.commit()?;
            Ok(ids)
        })
        .await
        .map_err(|e| StoreError::Db(format!("purge task failed: {e}")))??;
        if !purged.is_empty() {
            let mut cache = self.tickets.write().expect("store lock poisoned");
            for id in &purged {
                if let Some((t, _, _)) = cache.tombstones.by_id.get_mut(id) {
                    t.restorable_until_unix = None;
                }
            }
        }
        Ok(purged)
    }
}
