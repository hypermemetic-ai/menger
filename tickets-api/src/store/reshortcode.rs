//! ACT-6: **`reshortcode`** — tickets moved into another shortcode,
//! renumbered there, and every old id kept naming the ticket it was. Owner,
//! 2026-10-13: *"I want to be able to delete, reshortcode, etc."*
//!
//! **Where they go** ([`plan`]). `to` names the shortcode, by any short form:
//!
//! - one there is — each moved ticket takes its next number, in the order
//!   given ([`Cache::next_seq`]: past every number it ever gave), and hangs
//!   under its root, or `parent` (which must be in it);
//! - with `new_scope`, one minted first, its root a new scope of that title
//!   under `parent` (required): `TO-1`, and the moved tickets `TO-2`, … —
//!   minted through [`Cache::namespaces`], so a code its namespace holds is
//!   refused naming the one there;
//! - none, and the first ticket a scope or a doc: that ticket becomes the
//!   new shortcode's root, `TO-1`, hosted where it hangs (or under
//!   `parent`) — a scope moved out to root a shortcode of its own (ORG-5);
//! - a shortcode's root moves only with `with_children`, alone, and the whole
//!   shortcode with it: a rename — every ticket keeps its number, `to` a
//!   code its namespace does not hold.
//!
//! A ticket whose parent moves with it keeps that parent. `with_children`
//! moves each ticket's descendants in its own shortcode with it, numbered
//! after it in their order; without it, a child of its shortcode stays there
//! and hangs under the nearest ancestor that stays (a non-root's parent is
//! inside its shortcode, MNG-14); a child that roots another shortcode stays
//! hosted under the moved ticket, whose namespace it follows.
//!
//! **What moves with a ticket.** Its key is its id: the old one is rewritten
//! to the new in one transaction wherever a row names it — the tickets' own
//! `parent`, `blocked_by` and `unlocks`, the search index, inputs and
//! answers, comments (and a route that is the ticket's page), the
//! transition log, grants, notes, themes and their references, components,
//! tombstones, the queue and its log, deployments — and the old id is
//! **pinned** to the new key (ORG-3's `pins`, ACT-6's alias table): `get`,
//! filters, blockers, a URL and a chip in a body all resolve it, and `get`
//! says it in `moved_from`. Bodies are not rewritten: an old id in one is an
//! alias. A shortcode's numbers are never given out again — past a moved
//! ticket's too.
//!
//! **One write, the cache read back.** All or nothing (ACT-2): every
//! refusal named, per ticket, nothing written. The derived state is built
//! from the rows the transaction leaves ([`Cache::built`], the cell ids
//! kept) and stored; after the commit the cache is the file's, read as an
//! open reads it ([`Store::load_cache`]) — every record beside the tickets
//! rekeyed with it — so `--verify-derived` holds it by construction. A scan
//! and a read per move: moves are rare.
//!
//! **Its events**: [`Change::Reshortcoded`] first (each move, every moved
//! ticket whole), then each child that stayed and was moved's
//! `ParentChanged`, the groups and placements that moved, and for
//! `watch { filter }` the cells (an old key leaves, a new one enters) and
//! the lineage.

use super::cascade::Gathered;
use super::delete::{after, under_of, Before};
use super::history::Act;
use super::shortcodes::Target;
use super::update_many::UPDATE_MANY_CEILING;
use super::*;
use crate::address::{Address, Shortcode, ShortcodeId};
use crate::types::ShortcodeMove;
use std::collections::BTreeSet;

/// ACT-6: what a `reshortcode` is told.
#[derive(Debug, Clone, Default)]
pub struct Reshortcode {
    /// The tickets, by any short form, in the order they are numbered.
    pub ids: Vec<String>,
    /// The shortcode they move into (the module doc).
    pub to: String,
    /// Mint `to` first, its root a scope of this title under `parent`.
    pub new_scope: Option<String>,
    /// Where the moved tickets hang (`new_scope`: where the new scope does).
    pub parent: Option<String>,
    /// Each one's descendants in its own shortcode move with it.
    pub with_children: bool,
    /// ORG-4: the organizations the caller holds a role in — where `ids`,
    /// `to` and `parent` are read (a short form fitting two refused, as
    /// ORG-4's resolver does); `None`: every one.
    pub orgs: Option<Vec<crate::address::OrgId>>,
    /// ORG-23: answer the moves it would make — each old key and the key it
    /// would take, in order — and write nothing (the app's preview).
    pub dry_run: bool,
}

/// ACT-6: a `reshortcode`'s answer — each move in the order numbered, and
/// every moved ticket (a scope `new_scope` made first) as the write left it.
#[derive(Debug, Clone, Default)]
pub struct Reshortcoded {
    pub moves: Vec<ShortcodeMove>,
    pub tickets: Vec<Ticket>,
}

/// One ticket the move renumbers.
#[derive(Debug, Clone)]
struct Step {
    row: Row,
    key: String,
    shortcode: ShortcodeId,
    seq: u32,
    /// Its parent after, by its new key where that moves too.
    parent: Option<String>,
}

/// A reshortcode worked out against the cache as it stands.
#[derive(Debug)]
pub(super) struct Plan {
    steps: Vec<Step>,
    /// A shortcode the write mints.
    minted: Option<Shortcode>,
    /// `new_scope`'s root: its key, title and parent (ORG-13: none, at the
    /// top of the epic `to` names).
    born: Option<(String, String, Option<String>)>,
    /// Each child that stays, and the ticket it hangs under after.
    stays: Vec<(Row, Option<String>)>,
    /// A rename: the shortcode that was, and the one it is.
    renamed: Option<(ShortcodeId, ShortcodeId)>,
    /// Every ticket the caller's grant is asked of — the moved ones and
    /// where they go.
    asked: Vec<String>,
}

impl Plan {
    /// Each old key with its new one.
    fn map(&self) -> HashMap<String, String> {
        self.steps.iter().map(|s| (s.row.id.to_string(), s.key.clone())).collect()
    }
}

fn refuse(why: String) -> StoreError {
    StoreError::Invalid(why)
}

/// What `watch { epic }` passes on of a move: the moves into or out of
/// `epic` and its tickets there, when any is.
pub(super) fn in_epic(moves: Vec<ShortcodeMove>, tickets: Vec<Ticket>, epic: &ShortcodeId) -> Option<Change> {
    let of = |id: &str| id.rsplit_once('-').is_some_and(|(e, _)| e == epic.as_str());
    let moves: Vec<ShortcodeMove> = moves.into_iter().filter(|m| of(&m.from) || of(&m.to)).collect();
    let tickets = tickets.into_iter().filter(|t| t.shortcode == *epic).collect();
    (!moves.is_empty()).then_some(Change::Reshortcoded { moves, tickets })
}

/// `row`'s descendants in its own shortcode, in the store's order of each
/// level — what `with_children` moves with it.
fn descendants(cache: &Cache, row: &Row) -> Vec<Row> {
    let mut out = Vec::new();
    let mut stack = vec![row.id.clone()];
    let mut seen = HashSet::from([row.id.clone()]);
    while let Some(id) = stack.pop() {
        for (sc, seq) in cache.children.of(&id) {
            if sc != row.shortcode {
                continue;
            }
            if let Some(k) = cache.order.get(&OrderKey { shortcode: sc, seq }) {
                if seen.insert(k.id.clone()) {
                    stack.push(k.id.clone());
                    out.push(k.clone());
                }
            }
        }
    }
    out.sort_by_key(OrderKey::of);
    out
}

/// ACT-6: the move `ask` describes — every refusal, or the plan (the module
/// doc). Pure: the cache as it stands.
pub(super) fn plan(cache: &Cache, ask: &Reshortcode) -> Result<Plan, StoreError> {
    let reach = match &ask.orgs {
        Some(orgs) => shortcodes::OrgReach::Only(orgs),
        None => shortcodes::OrgReach::All,
    };
    // The tickets, each by any short form, once each, in the order given.
    let mut refusals: Vec<(String, Vec<StoreError>)> = Vec::new();
    let mut given: Vec<Row> = Vec::new();
    for text in ask.ids.iter().map(|i| i.trim()).filter(|i| !i.is_empty()) {
        match cache.resolve_in(text, reach) {
            Ok(key) => match cache.get(&key) {
                Some(r) if !given.iter().any(|g| g.id == r.id) => given.push(r.clone()),
                Some(_) => {}
                None => refusals.push((text.to_string(), vec![refuse(format!("{key} is deleted — undelete it first"))])),
            },
            Err(e) => refusals.push((text.to_string(), vec![e])),
        }
    }
    if !refusals.is_empty() {
        return Err(StoreError::RefusedMany { refusals });
    }
    if given.is_empty() || given.len() > UPDATE_MANY_CEILING {
        return Err(refuse(format!("name 1 to {UPDATE_MANY_CEILING} tickets to move — nothing written")));
    }
    let to = Address::parse(&ask.to).map_err(|e| refuse(format!("to: {e} — nothing written")))?;
    if to.names_ticket() {
        return Err(refuse(format!("to: {} names a ticket; name the shortcode they move into — nothing written", ask.to.trim())));
    }
    let root_of = |sc: &ShortcodeId| cache.rollups.root(sc).map(str::to_string);
    // ORG-13: the epic `to` names (`org:epic:CODE`, or `org:CODE` for the
    // org's first) — a move into another epic hangs at its top, or under a
    // `parent` in it.
    let to_era = to_era(cache, &to)?;
    let era_of_row = |r: &Row| cache.shortcodes.era_of(&r.shortcode);
    // A shortcode's root: a rename, or refused.
    let roots: Vec<&Row> = given.iter().filter(|g| root_of(&g.shortcode).as_deref() == Some(g.id.as_str())).collect();
    if !roots.is_empty() {
        if !ask.with_children || given.len() > 1 {
            let why = roots
                .iter()
                .map(|r| {
                    (r.id.to_string(), vec![refuse(format!(
                        "{} roots the shortcode {}: it moves only alone, with --with-children and the whole shortcode — a rename",
                        r.id, r.epic
                    ))])
                })
                .collect();
            return Err(StoreError::RefusedMany { refusals: why });
        }
        return rename(cache, &given[0], &to, to_era, ask);
    }
    // Every ticket that moves, in the order it is numbered.
    let mut moving: Vec<Row> = Vec::new();
    for g in &given {
        if !moving.iter().any(|m| m.id == g.id) {
            moving.push(g.clone());
        }
        if ask.with_children {
            for d in descendants(cache, g) {
                if !moving.iter().any(|m| m.id == d.id) {
                    moving.push(d);
                }
            }
        }
    }
    let moves: HashSet<&str> = moving.iter().map(|m| m.id.as_str()).collect();
    let parent_row = match ask.parent.as_deref().map(str::trim).filter(|p| !p.is_empty()) {
        None => None,
        Some(p) => {
            let key = cache.resolve_in(p, reach).map_err(|e| match e {
                StoreError::NotFound(_) => refuse(format!("parent {p}: no such ticket — nothing written")),
                e => e,
            })?;
            if moves.contains(key.as_str()) {
                return Err(refuse(format!("parent {key} moves too: name a ticket that stays — nothing written")));
            }
            Some(cache.get(&key).cloned().ok_or_else(|| StoreError::NotFound(key))?)
        }
    };
    // Where they go: the shortcode, its first number, and where a moved
    // ticket whose parent does not move hangs.
    let (target, minted, born, mut next, home): (ShortcodeId, Option<Shortcode>, Option<(String, String, Option<String>)>, u32, Option<String>) =
        if let Some(title) = ask.new_scope.as_deref() {
            let title = title.trim();
            if title.is_empty() {
                return Err(refuse("new_scope: the new scope's title — nothing written".into()));
            }
            // ORG-13: into an epic the address names, it may root there, at
            // the top.
            if parent_row.is_none() && to_era.is_none() {
                return Err(refuse("new_scope makes a scope under --parent: name where it hangs (or an epic, --to org:epic:CODE) — nothing written".into()));
            }
            if let Some(p) = parent_row.as_ref() {
                in_era(to_era, era_of_row(p), &p.id)?;
                // TICKETS-6: no new shortcode hosted under a frozen scope.
                rules::frozen_refusal(cache, Some(&p.id), None)?;
            }
            match cache.target(&to, parent_row.as_ref(), true)? {
                Target::Mint(sc) => {
                    let root = sc.id.key(1);
                    (sc.id.clone(), Some(sc), Some((root.clone(), title.to_string(), parent_row.as_ref().map(|p| p.id.to_string()))), 2, Some(root))
                }
                Target::In(there) => {
                    return Err(refuse(format!(
                        "{} is a shortcode there already ({}) — drop --new-scope to move into it — nothing written",
                        to,
                        cache.shortcodes.address(&there)
                    )))
                }
            }
        } else {
            let named = match cache.shortcodes.find_shortcode(&ask.to, reach) {
                shortcodes::Found::One(sc) => Some(sc),
                shortcodes::Found::Many(candidates) => return Err(StoreError::Ambiguous { text: ask.to.trim().to_string(), candidates }),
                shortcodes::Found::None => None,
            };
            match named {
                Some(sc) => {
                    let home = match &parent_row {
                        Some(p) if p.shortcode != sc => {
                            return Err(refuse(format!("parent {} is not in {}: a moved ticket hangs inside its shortcode — nothing written", p.id, to)))
                        }
                        Some(p) => Some(p.id.to_string()),
                        None => root_of(&sc),
                    };
                    // TICKETS-6: a frozen shortcode takes no new numbers.
                    rules::frozen_refusal(cache, home.as_deref(), Some(&sc))?;
                    (sc.clone(), None, None, cache.next_seq(&sc), home)
                }
                None => {
                    // A scope (or a doc) moved out to root a shortcode of its own.
                    let first = &moving[0];
                    if !first.kind.roots_a_shortcode() {
                        return Err(refuse(format!(
                            "no shortcode {to}: pass --new-scope TITLE --parent P to make one, or move a scope (it becomes the new \
                             shortcode's root) — nothing written"
                        )));
                    }
                    let hangs = parent_row.clone().or_else(|| first.parent.as_deref().and_then(|p| cache.get(p)).cloned());
                    // ORG-13: into another epic, at its top — or under a
                    // `parent` there.
                    if let Some(p) = parent_row.as_ref() {
                        in_era(to_era, era_of_row(p), &p.id)?;
                    }
                    let hangs = hangs.filter(|h| to_era.is_none_or(|e| era_of_row(h) == e));
                    rules::frozen_refusal(cache, hangs.as_ref().map(|h| h.id.as_str()), None)?;
                    match cache.target(&to, hangs.as_ref(), true)? {
                        Target::Mint(sc) => (sc.id.clone(), Some(sc.clone()), None, 1, Some(sc.id.key(1))),
                        Target::In(there) => {
                            return Err(refuse(format!("{to} is a shortcode there already ({}) — nothing written", cache.shortcodes.address(&there))))
                        }
                    }
                }
            }
        };
    let mut refusals: Vec<(String, Vec<StoreError>)> = Vec::new();
    // ORG-4: a move keeps every ticket in its organization.
    let target_org = minted.as_ref().map(|m| m.ns.org.clone()).or_else(|| cache.shortcodes.org(&target).cloned());
    for m in &moving {
        let org = cache.shortcodes.org(&m.shortcode).cloned();
        if target_org.is_some() && org.is_some() && org != target_org {
            refusals.push((m.id.to_string(), vec![refuse(format!(
                "{} is in the organization {}, and {to} in {}: a move keeps a ticket in its organization",
                m.id,
                org.expect("some"),
                target_org.as_ref().expect("some")
            ))]));
        }
    }
    if !refusals.is_empty() {
        return Err(StoreError::RefusedMany { refusals });
    }
    // Each moved ticket's new key, in the order given.
    let mut map: HashMap<String, String> = HashMap::new();
    let mut steps: Vec<Step> = Vec::with_capacity(moving.len());
    for m in &moving {
        if m.shortcode == target {
            refusals.push((m.id.to_string(), vec![refuse(format!("{} is in {} already", m.id, to))]));
            continue;
        }
        let seq = next;
        next += 1;
        map.insert(m.id.to_string(), target.key(seq));
        steps.push(Step { row: m.clone(), key: target.key(seq), shortcode: target.clone(), seq, parent: None });
    }
    let as_root = minted.is_some() && born.is_none();
    // ORG-5: a scope roots its shortcode — it moves only to root a new one.
    for (i, s) in steps.iter().enumerate() {
        if s.row.kind == Kind::Scope && !(as_root && i == 0) {
            refusals.push((s.row.id.to_string(), vec![refuse(format!(
                "{} is a scope, and a scope roots its own shortcode (ORG-5): move it alone into a new code, which it roots                  (tickets reshortcode --ids {} --to NEW --with-children true)",
                s.row.id, s.row.id
            ))]));
        }
    }
    for (i, s) in steps.iter_mut().enumerate() {
        s.parent = match s.row.parent.as_deref().and_then(|p| map.get(p)) {
            Some(p) => Some(p.clone()),
            // The new root keeps where it hangs, or hangs under --parent.
            // ORG-13: but not across epics — at the new one's top.
            None if as_root && i == 0 => parent_row.as_ref().map(|p| p.id.to_string()).or_else(|| {
                s.row.parent.clone().map(String::from).filter(|p| to_era.is_none_or(|e| cache.get(p).is_some_and(|r| era_of_row(r) == e)))
            }),
            None => home.clone(),
        };
        // WAVE-10: a path is unique in its shortcode.
        if let Some(path) = &s.row.path {
            if let Some(holder) = cache.paths.get(&(target.clone(), path.clone())) {
                refusals.push((s.row.id.to_string(), vec![StoreError::PathTaken { epic: to.code.to_string(), path: path.clone(), holder: holder.clone() }]));
            }
        }
    }
    if !refusals.is_empty() {
        return Err(StoreError::RefusedMany { refusals });
    }
    // The children that stay: in their shortcode, under the nearest
    // ancestor that stays; a hosted root, under the moved ticket.
    let mut stays: Vec<(Row, Option<String>)> = Vec::new();
    for s in &steps {
        for (sc, seq) in cache.children.of(&s.row.id) {
            let Some(c) = cache.order.get(&OrderKey { shortcode: sc, seq }) else { continue };
            if map.contains_key(c.id.as_str()) {
                continue;
            }
            let roots_its_own = root_of(&c.shortcode).as_deref() == Some(c.id.as_str());
            let under = if roots_its_own || c.shortcode != s.row.shortcode {
                Some(s.key.clone())
            } else {
                let mut up = s.row.parent.clone();
                let mut seen = HashSet::new();
                while let Some(p) = up.clone().filter(|p| map.contains_key(p.as_str())) {
                    if !seen.insert(p.clone()) {
                        break;
                    }
                    up = cache.get(&p).and_then(|r| r.parent.clone());
                }
                up.filter(|p| cache.get(p).is_some_and(|r| r.shortcode == c.shortcode)).map(String::from).or_else(|| root_of(&c.shortcode))
            };
            stays.push((c.clone(), under));
        }
    }
    // No cycle: every moved ticket's chain after the move reaches the top.
    let mut after: HashMap<String, Option<String>> = steps.iter().map(|s| (s.key.clone(), s.parent.clone())).collect();
    for (c, under) in &stays {
        after.insert(c.id.to_string(), under.clone());
    }
    let back: HashMap<&str, &str> = map.iter().map(|(o, n)| (n.as_str(), o.as_str())).collect();
    let parent_after = |id: &str| -> Option<String> {
        if let Some(p) = after.get(id) {
            return p.clone();
        }
        let p = cache.get(back.get(id).copied().unwrap_or(id))?.parent.clone()?;
        Some(map.get(p.as_str()).cloned().unwrap_or(String::from(p)))
    };
    for s in &steps {
        let mut path = vec![s.key.clone()];
        let mut up = parent_after(&s.key);
        while let Some(p) = up {
            if path.contains(&p) {
                path.push(p);
                return Err(StoreError::ParentCycle { id: s.row.id.to_string(), path });
            }
            path.push(p.clone());
            if path.len() > cache.len() + 2 {
                break;
            }
            up = parent_after(&p);
        }
    }
    let mut asked: Vec<String> = steps.iter().map(|s| s.row.id.to_string()).collect();
    asked.extend(parent_row.as_ref().map(|p| p.id.to_string()).or_else(|| home.filter(|h| cache.get(h).is_some())));
    Ok(Plan { steps, minted, born, stays, renamed: None, asked })
}

/// A shortcode's root moved with every ticket of it: the shortcode renamed
/// `to`, in its namespace, every number kept.
fn rename(cache: &Cache, root: &Row, to: &Address, to_era: Option<i64>, ask: &Reshortcode) -> Result<Plan, StoreError> {
    if ask.new_scope.is_some() || ask.parent.as_deref().is_some_and(|p| !p.trim().is_empty()) {
        return Err(refuse(format!(
            "{} roots its shortcode: moving it renames the shortcode where it is — drop --new-scope and --parent (set_parent moves \
             a root) — nothing written",
            root.id
        )));
    }
    if !to.hosts.is_empty() {
        return Err(refuse(format!("to: a rename names the new code alone (or org:epic:CODE, ORG-13), as {} — nothing written", to.code)));
    }
    let old = root.shortcode.clone();
    let mut ns = cache.shortcodes.get(&old).map(|s| s.ns.clone()).ok_or_else(|| StoreError::NoShortcode(old.to_string()))?;
    // ORG-13: a whole shortcode moved into another epic — a root at its top,
    // every number kept (they start at 1 there already).
    let cross = to_era.is_some_and(|e| e != ns.era);
    if cross {
        (ns.era, ns.host) = (to_era.unwrap_or(ns.era), None);
    }
    if cache.shortcodes.code(&old) == to.code && !cross {
        return Err(refuse(format!("{} is already {} — nothing written", root.epic, to.code)));
    }
    let sc = cache.shortcodes.mint(ns, to.code.clone())?;
    let mut all = vec![root.clone()];
    all.extend(descendants(cache, root));
    let held = cache.ids.held(&old);
    if all.len() as u64 != held {
        return Err(refuse(format!(
            "{} holds {} tickets that don't hang under its root {}: move them first (set_parent), then rename it — nothing written",
            root.epic,
            held - all.len() as u64,
            root.id
        )));
    }
    let map: HashMap<String, String> = all.iter().map(|t| (t.id.to_string(), sc.id.key(t.seq))).collect();
    let steps: Vec<Step> = all
        .iter()
        .map(|t| Step {
            row: t.clone(),
            key: sc.id.key(t.seq),
            shortcode: sc.id.clone(),
            seq: t.seq,
            parent: match cross && t.id == root.id {
                true => None,
                false => t.parent.as_deref().map(|p| map.get(p).cloned().unwrap_or_else(|| p.to_string())),
            },
        })
        .collect();
    // A shortcode one of them hosts stays hosted under it, by its new key
    // (its root's parent rewritten), and follows its namespace.
    let mut stays: Vec<(Row, Option<String>)> = Vec::new();
    for t in &all {
        for (csc, seq) in cache.children.of(&t.id) {
            let Some(c) = cache.order.get(&OrderKey { shortcode: csc, seq }) else { continue };
            if !map.contains_key(c.id.as_str()) {
                stays.push((c.clone(), map.get(t.id.as_str()).cloned()));
            }
        }
    }
    let mut asked: Vec<String> = all.iter().map(|t| t.id.to_string()).collect();
    asked.extend(root.parent.clone().map(String::from).filter(|_| !cross));
    Ok(Plan { steps, renamed: Some((old, sc.id.clone())), minted: Some(sc), born: None, stays, asked })
}

/// TICKETS-26: what a `rename` is told.
#[derive(Debug, Clone, Default)]
pub struct Rename {
    /// The ticket, by any short form.
    pub id: String,
    /// The id it takes: its parent's, `.`, a number — `TICKETS-10.1`, or
    /// any short form of the parent's address with `.N` after it.
    pub to: String,
    /// ORG-4: the organizations the caller holds a role in.
    pub orgs: Option<Vec<crate::address::OrgId>>,
    /// Answer the move it would make and write nothing.
    pub dry_run: bool,
}

/// TICKETS-26: the rule for a subtask id, said in every refusal of one.
fn subtask_rule(why: String) -> StoreError {
    refuse(format!(
        "{why} — a subtask id is `<parent>.N`, only for a ticket whose parent is `<parent>` (in its own shortcode), N a number from 1 — nothing written"
    ))
}

/// TICKETS-26: a ticket renamed into `<parent>.N` — every refusal, or the
/// plan: one step that keeps the shortcode and the number and takes the new
/// key, the ticket's children hanging under it by it. Pure: the cache as it
/// stands.
pub(super) fn plan_rename(cache: &Cache, ask: &Rename) -> Result<Plan, StoreError> {
    let reach = match &ask.orgs {
        Some(orgs) => shortcodes::OrgReach::Only(orgs),
        None => shortcodes::OrgReach::All,
    };
    let key = cache.resolve_in(ask.id.trim(), reach)?;
    let row = cache.get(&key).ok_or_else(|| refuse(format!("{key} is deleted — undelete it first")))?;
    let to = ask.to.trim();
    let Some(parent) = row.parent.as_deref().and_then(|p| cache.get(p)) else {
        return Err(subtask_rule(format!("{} has no parent, so `{to}` cannot name it", row.id)));
    };
    if parent.shortcode != row.shortcode {
        return Err(subtask_rule(format!("{}'s parent {} is in another shortcode, so `{to}` cannot name it", row.id, parent.id)));
    }
    let n = match to.rsplit_once('.') {
        Some((stem, n)) if !stem.is_empty() && !n.starts_with('0') && n.bytes().all(|b| b.is_ascii_digit()) => n.parse::<u32>().ok().map(|n| (stem, n)),
        _ => None,
    };
    let Some((stem, n)) = n else {
        return Err(subtask_rule(format!("`{to}` is not {}.N", parent.id)));
    };
    // The stem is the parent, by any name it answers to.
    match cache.resolve_in(stem, reach) {
        Ok(k) if k == parent.id.as_str() => {}
        _ => return Err(subtask_rule(format!("`{to}` is not {}.N: {} is {}'s parent", parent.id, parent.id, row.id))),
    }
    let new = format!("{}.{n}", parent.id);
    if new == row.id.as_str() {
        return Err(refuse(format!("{} is {new} already — nothing written", row.id)));
    }
    if cache.at.contains_key(&new) || cache.tombstones.get(&new).is_some() || cache.shortcodes.pin(&new).is_some() {
        return Err(refuse(format!("{new} is taken — nothing written")));
    }
    let mut stays: Vec<(Row, Option<String>)> = Vec::new();
    for (sc, seq) in cache.children.of(&row.id) {
        if let Some(c) = cache.order.get(&OrderKey { shortcode: sc, seq }) {
            stays.push((c.clone(), Some(new.clone())));
        }
    }
    let step = Step { row: row.clone(), key: new, shortcode: row.shortcode.clone(), seq: row.seq, parent: Some(parent.id.to_string()) };
    Ok(Plan { asked: vec![row.id.to_string()], steps: vec![step], minted: None, born: None, stays, renamed: None })
}

/// ORG-13: the epic a move's `to` names — `org:epic:CODE` that one (refused
/// when the org has none of that name), `org:CODE` the org's first; a short
/// form names none (the move stays where its shortcode is).
fn to_era(cache: &Cache, to: &Address) -> Result<Option<i64>, StoreError> {
    let r = &cache.shortcodes;
    let Some(org) = &to.org else { return Ok(None) };
    let row = r.org_row(org).ok_or_else(|| StoreError::NoOrg(org.to_string()))?;
    match &to.era {
        Some(name) => {
            let era = r.era_named(row, name).ok_or_else(|| refuse(format!("to: no epic `{name}` in {org} — nothing written")))?;
            // ORG-23: nothing moves into an archived epic.
            r.live_era(era, "moving into it")?;
            Ok(Some(era))
        }
        None => Ok(r.first_era(row)),
    }
}

/// ORG-13: a `parent` in the epic the move goes to, or the refusal.
fn in_era(to_era: Option<i64>, parent_era: i64, parent: &str) -> Result<(), StoreError> {
    match to_era.is_some_and(|e| e != parent_era) {
        true => Err(refuse(format!("parent {parent} is in another epic than --to names: a moved ticket hangs in its epic — nothing written"))),
        false => Ok(()),
    }
}

/// `ids` (a JSON list) with every moved id renamed.
fn renamed_list(list: &[TicketName], map: &HashMap<String, String>) -> Vec<TicketName> {
    list.iter().map(|i| map.get(i.as_str()).map_or_else(|| i.clone(), TicketName::new)).collect()
}

/// [`renamed_list`] for a column's text — a list as another table stores it.
fn renamed_texts(list: &[String], map: &HashMap<String, String>) -> Vec<String> {
    list.iter().map(|i| map.get(i).cloned().unwrap_or_else(|| i.clone())).collect()
}

/// Every row beside the tickets that names a moved ticket, rekeyed — in the
/// write's transaction. `renamed`: a whole shortcode's id, where a column
/// names a shortcode.
fn rekey(txn: &rusqlite::Transaction<'_>, map: &HashMap<String, String>, renamed: Option<&(ShortcodeId, ShortcodeId)>) -> Result<(), StoreError> {
    // A column that names a ticket by its key. V2-4: the tables of the
    // services that went stay in a file that has them, unread — and
    // unrekeyed: a later service reads them through the old id's pin.
    const COLUMNS: &[(&str, &str)] = &[
        ("search_rows", "id"),
        ("inputs", "ticket"),
        ("answers", "ticket"),
        ("queue", "ticket"),
        ("queue_log", "ticket"),
        ("transitions", "ticket"),
        ("tombstones", "parent"),
        ("members", "scope"),
        ("pins", "ticket"),
    ];
    let mut pairs: Vec<(&str, &str)> = map.iter().map(|(o, n)| (o.as_str(), n.as_str())).collect();
    pairs.sort();
    for (table, column) in COLUMNS {
        let sql = format!("UPDATE {table} SET {column} = ?2 WHERE {column} = ?1");
        let mut stmt = txn.prepare(&sql)?;
        for (old, new) in &pairs {
            stmt.execute([old, new])?;
        }
    }
    for (old, new) in &pairs {
        // The old id names the new key from now on (ACT-6's alias).
        txn.prepare_cached("INSERT INTO pins (text, ticket) VALUES (?1, ?2) ON CONFLICT (text) DO UPDATE SET ticket = excluded.ticket")?
            .execute([old, new])?;
    }
    // A whole shortcode renamed: what names it by its id.
    if let Some((was, is)) = renamed {
        txn.execute("UPDATE members SET scope = ?2 WHERE scope = ?1", [was.as_str(), is.as_str()])?;
    }
    // Lists in a column: JSON lists.
    let json_lists: &[(&str, &str, &str)] = &[("tombstones", "id", "reparented"), ("tombstones", "id", "unblocked")];
    for (table, key, column) in json_lists {
        let rows: Vec<(String, String)> = {
            let mut stmt = txn.prepare(&format!("SELECT {key}, {column} FROM {table}"))?;
            let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
            rows.collect::<rusqlite::Result<_>>()?
        };
        for (k, text) in rows {
            let Ok(list) = serde_json::from_str::<Vec<String>>(&text) else { continue };
            if list.iter().any(|i| map.contains_key(i)) {
                let list = serde_json::to_string(&renamed_texts(&list, map)).expect("serialize ids");
                txn.execute(&format!("UPDATE {table} SET {column} = ?2 WHERE {key} = ?1"), [k, list])?;
            }
        }
    }
    Ok(())
}

impl Store {
    /// [`Store::reshortcode_by`] as nobody signed in, through the API, with
    /// every ticket allowed.
    pub async fn reshortcode(&self, sys: &System, ask: Reshortcode) -> Result<Reshortcoded, StoreError> {
        self.reshortcode_by(ask, Act::default(), |ids| Ok(ids.iter().map(|id| sys.permit(id)).collect())).await
    }

    /// ACT-6: the tickets `ask` names moved into its shortcode — all of them
    /// or none (the module doc). `may` is asked of every ticket that moves
    /// and of where they go (the parent, or the shortcode's root) before
    /// anything is written; its refusal is the write's.
    pub async fn reshortcode_by(
        &self,
        ask: Reshortcode,
        act: Act,
        may: impl FnOnce(&[String]) -> Result<Vec<Permit<Editor>>, String> + Send,
    ) -> Result<Reshortcoded, StoreError> {
        if let Some(title) = &ask.new_scope {
            self.check_title(None, title)?;
        }
        let serial = self.serial().await;
        let planned = {
            let cache = self.tickets.read().expect("store lock poisoned");
            plan(&cache, &ask)
        };
        self.moving(serial, planned?, ask.dry_run, act, may).await
    }

    /// [`Store::rename_by`] as nobody signed in, with every ticket allowed.
    pub async fn rename(&self, sys: &System, ask: Rename) -> Result<Reshortcoded, StoreError> {
        self.rename_by(ask, Act::default(), |ids| Ok(ids.iter().map(|id| sys.permit(id)).collect())).await
    }

    /// TICKETS-26: a ticket renamed into `<parent>.N` under its own parent —
    /// the same write as a move (the module doc): its key rewritten wherever
    /// a row names it, the old id pinned, nothing renumbered. All or none;
    /// every refusal names the rule.
    pub async fn rename_by(
        &self,
        ask: Rename,
        act: Act,
        may: impl FnOnce(&[String]) -> Result<Vec<Permit<Editor>>, String> + Send,
    ) -> Result<Reshortcoded, StoreError> {
        let serial = self.serial().await;
        let planned = {
            let cache = self.tickets.read().expect("store lock poisoned");
            plan_rename(&cache, &ask)
        };
        self.moving(serial, planned?, ask.dry_run, act, may).await
    }

    /// A planned move, checked against the caller's permits and written.
    async fn moving(
        &self,
        serial: OwnedMutexGuard<()>,
        p: Plan,
        dry_run: bool,
        act: Act,
        may: impl FnOnce(&[String]) -> Result<Vec<Permit<Editor>>, String> + Send,
    ) -> Result<Reshortcoded, StoreError> {
        // ACCESS-11: an editor's permit for every ticket the move names.
        let permits = may(&p.asked).map_err(StoreError::Invalid)?;
        writs::covers_exactly(&self.tickets.read().expect("store lock poisoned"), &permits, &p.asked)?;
        if dry_run {
            let moves = p.steps.iter().map(|s| ShortcodeMove { from: s.row.id.to_string(), to: s.key.clone() }).collect();
            return Ok(Reshortcoded { moves, tickets: Vec::new() });
        }
        self.in_one_write(serial, move |conn, cache, unders| {
            super::revisions::act(&act);
            Self::reshortcoding(conn, cache, unders, p, act)
        })
        .await
    }

    /// The move's one write — see the module doc.
    fn reshortcoding(
        conn: &mut Connection,
        cache: &mut Cache,
        unders: &lineage::UnderWatches,
        p: Plan,
        act: Act,
    ) -> Result<(Reshortcoded, Vec<Change>), StoreError> {
        let now = chrono::Utc::now().timestamp();
        let map = p.map();
        // What the open watches saw before: the old keys and the moved
        // subtrees, and the cells of every key the move names.
        let mut touched: Vec<OrderKey> = p.steps.iter().map(|s| OrderKey::of(&s.row)).collect();
        touched.extend(p.stays.iter().map(|(c, _)| OrderKey::of(c)));
        let roots: Vec<&Row> = p.steps.iter().map(|s| &s.row).collect();
        let under = under_of(unders, cache, &touched, &roots);
        touched.extend(p.steps.iter().map(|s| OrderKey { shortcode: s.shortcode.clone(), seq: s.seq }));
        if let (Some(_), Some(sc)) = (&p.born, &p.minted) {
            touched.push(OrderKey { shortcode: sc.id.clone(), seq: 1 });
        }
        touched.sort();
        touched.dedup();
        let before = Before::of(cache, touched.iter().cloned());
        let txn = conn.transaction()?;
        // `new_scope`'s root, born first: a scope, its birth in the log.
        let mut born_ticket = None;
        if let (Some((key, title, parent)), Some(sc)) = (&p.born, &p.minted) {
            let t = Ticket {
                // TICKETS-35: a scope born here takes a uid of its own.
                uid: cache.mint_uid(&[]),
                uid_of: Default::default(),
                id: crate::address::TicketId::new(key.clone()),
                epic: sc.code.clone(),
                shortcode: sc.id.clone(),
                address: String::new(),
                seq: 1,
                title: title.clone(),
                status: Status::at_birth(Kind::Scope),
                kind: Kind::Scope,
                severity: None,
                blocked_by: Vec::new(),
                unlocks: Vec::new(),
                hidden_blocked_by: 0,
                hidden_unlocks: 0,
                parent_hidden: false,
                archived: None,
                frozen: false,
                hidden: false,
                parent: parent.clone().map(crate::address::TicketId::new),
                child_count: 0,
                commits: Vec::new(),
                path: None,
                placement: None,
                labels: Vec::new(),
                body: String::new(),
                created_unix: now,
                updated_unix: now,
                moved_from: Vec::new(),
                epic_archived: None,
            };
            db::upsert(&txn, &t)?;
            search::put(&txn, &t.id, &t.title, &t.body)?;
            history::record(
                &txn,
                &crate::types::Transition {
                    id: cache.log.next_id(),
                    ticket: t.id.clone(),
                    field: crate::types::TransitionField::Status,
                    from: None,
                    to: t.status.as_str().to_string(),
                    at_unix: now,
                    actor: act.actor.clone(),
                    name: act.name.clone(),
                    source: act.source,
                    message: act.message.clone(),
                    cause: None,
                    cause_ticket: None,
                },
            )?;
            born_ticket = Some(t.id.clone());
        }
        // The keys first, then every row that names one.
        for s in &p.steps {
            txn.prepare_cached("UPDATE tickets SET id = ?2, epic = ?3, seq = ?4 WHERE id = ?1")?
                .execute(rusqlite::params![s.row.id, s.key, s.shortcode, s.seq])?;
        }
        // TICKETS-7: the revisions move with their tickets, before any is touched.
        super::revisions::rekey(&txn, &map)?;
        let mut rows: BTreeMap<String, Row> = BTreeMap::new();
        let stay_under: HashMap<&str, &Option<String>> = p.stays.iter().map(|(c, u)| (c.id.as_str(), u)).collect();
        let mut named: BTreeSet<String> = BTreeSet::new();
        for s in &p.steps {
            named.extend(s.row.blocked_by.iter().chain(&s.row.unlocks).map(TicketName::to_string));
        }
        named.extend(p.stays.iter().map(|(c, _)| c.id.to_string()));
        for s in &p.steps {
            let mut r = s.row.clone();
            (r.id, r.shortcode, r.seq, r.parent, r.updated_unix) = (crate::address::TicketId::new(s.key.clone()), s.shortcode.clone(), s.seq, s.parent.clone().map(crate::address::TicketId::new), now);
            r.blocked_by = renamed_list(&r.blocked_by, &map);
            r.unlocks = renamed_list(&r.unlocks, &map);
            rows.insert(r.id.to_string(), r);
        }
        for id in named.iter().filter(|i| !map.contains_key(*i)) {
            let Some(mut r) = cache.get(id).cloned() else { continue };
            r.blocked_by = renamed_list(&r.blocked_by, &map);
            r.unlocks = renamed_list(&r.unlocks, &map);
            if let Some(under) = stay_under.get(id.as_str()) {
                (r.parent, r.updated_unix) = ((*under).clone().map(crate::address::TicketId::new), now);
            }
            rows.insert(r.id.to_string(), r);
        }
        for r in rows.values() {
            db::update_row(&txn, r)?;
        }
        rekey(&txn, &map, p.renamed.as_ref())?;
        // The derived state, built from the rows as the write leaves them;
        // the shortcodes with the one it mints, every namespace a moved root
        // changes checked.
        let inputs = inputs::load(&txn)?;
        let mut built = Cache::built(db::load_rows(&txn).map_err(|e| StoreError::Db(e.to_string()))?, inputs, cache.cells.keys());
        built.shortcodes = cache.shortcodes.clone();
        let first = p.steps.first().map(|s| s.row.id.to_string());
        if let Some(m) = cache.namespaces(built.rows(), &built.rollups, p.minted.as_ref(), first.as_deref())? {
            m.write(&txn)?;
            built.shortcodes = m.after;
        }
        built.readdress_all();
        stored::write_all(&txn, &built.form())?;
        let mut tickets: Vec<Ticket> = Vec::with_capacity(p.steps.len() + 1);
        for id in born_ticket.iter().map(|t| t.as_str()).chain(p.steps.iter().map(|s| s.key.as_str())) {
            tickets.push(after(&txn, &built, id)?);
        }
        let mut out = Gathered::default();
        for (c, _) in p.stays.iter().filter(|(c, under)| c.parent.as_deref() != under.as_deref()) {
            out.tickets.push(Change::ParentChanged { ticket: after(&txn, &built, &c.id)?, previous: c.parent.clone().map(String::from) });
        }
        super::revisions::commit(txn)?;
        // The cache, as an open of the file reads it: every record beside
        // the tickets rekeyed with them. The count of writes stays.
        let mut fresh = Self::load_cache(conn).map_err(|e| StoreError::Db(format!("reading the moved file back: {e}")))?;
        fresh.written = cache.written;
        *cache = fresh;
        let moves: Vec<ShortcodeMove> = p.steps.iter().map(|s| ShortcodeMove { from: s.row.id.to_string(), to: s.key.clone() }).collect();
        // Each moved ticket as the reader sees it now — its address dressed.
        let tickets: Vec<Ticket> = tickets
            .into_iter()
            .map(|mut t| {
                if let Some(r) = cache.get(&t.id) {
                    (t.epic, t.address, t.child_count, t.placement) = (r.epic.clone(), r.address.clone(), r.child_count, r.placement);
                }
                t
            })
            .collect();
        out.tickets.insert(0, Change::Reshortcoded { moves: moves.clone(), tickets: tickets.clone() });
        let cells = before.moved(cache, &touched, &mut out);
        out.filtered(cells, under.and_then(|u| u.after_in(cache)));
        Ok((Reshortcoded { moves, tickets }, out.finish()))
    }
}
