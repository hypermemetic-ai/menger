//! THEME-3: the **theme registry** — a theme is a ticket's ` ```theme `
//! block ([`crate::theme_block`]), owned and kept as COMP-3 keeps a
//! component.
//!
//! - **One owner.** The ticket whose body holds the block owns the theme; a
//!   block naming a theme another ticket owns refuses the write, naming the
//!   owner. A theme `themes.define` made (no owner) is **adopted** by the
//!   first block that names it, and so is an orphan.
//! - **Orphans.** A block gone from its owner's body leaves its theme an
//!   orphan: kept, read-only, while something uses it — a ticket whose
//!   frontmatter names it (`theme:`, `frame-theme:`), a setting (`themes.set`),
//!   or a live theme that `extends` it; the write that leaves it unused
//!   removes it (an orphan only an orphan extends goes with it).
//! - **Validated on write.** A block that is not a theme is refused before
//!   anything is read ([`super::components::read`] reports it, naming the
//!   block's line); so is an `extends` cycle, under the lock. Nothing is
//!   written.
//!
//! A body write's change rides with the component registry's
//! ([`super::components::Plan`]'s `moves.themes`): worked out under the
//! write-serial lock ([`Cache::plan_registry`]), stored in the write's
//! transaction ([`store`]), taken by the cache after it commits
//! ([`Cache::take_registry`]) — which says each theme it moved or removed on
//! the themes channel (`theme_changed`, `theme_removed`).
//!
//! **Stored**: the `themes` table's THEME-3 columns (`owner`, `orphan`,
//! `extends`, `rules`, `imports`) and `theme_refs` (a row per ticket and
//! theme its frontmatter names). A file without them gains them on open,
//! the refs and every block's theme read from the bodies once ([`migrate`]).

use super::components::{self, Plan};
use super::themes::{store_theme, ThemeChange, Themes};
use super::{Cache, StoreError};
use crate::theme_block::{self, BlockError, ThemeDef};
use crate::themes::{self, Theme, DEFAULT_THEME};
use rusqlite::{params, Connection};
use std::collections::{BTreeMap, BTreeSet};

/// What one body says about themes.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct BodyThemes {
    /// Each ` ```theme ` block, read.
    pub(crate) defs: Vec<ThemeDef>,
    /// Each theme its frontmatter names.
    pub(crate) refs: BTreeSet<String>,
}

/// A body's themes, or its first block that is not one (`name`, why).
pub(crate) fn read(body: &str) -> Result<BodyThemes, (String, BlockError)> {
    let defs = theme_block::read(body)?;
    let d = themes::declared(body);
    let refs = [d.ticket.theme, d.frame.theme].into_iter().flatten().filter(|t| t != DEFAULT_THEME).collect();
    Ok(BodyThemes { defs, refs })
}

/// One write's change to the theme registry.
#[derive(Debug, Default, Clone)]
pub(crate) struct Moves {
    /// Each theme that moves (`None`: it goes), in name order.
    pub(crate) themes: Vec<(String, Option<Theme>)>,
    /// The ticket's frontmatter references, when they move.
    pub(crate) refs: Option<(String, BTreeSet<String>)>,
}

/// The themes `uses` of `themes`' refs are kept by, after `ticket`'s refs
/// become `refs` — every theme a ticket names.
fn named_by_tickets(t: &Themes, ticket: &str, refs: &BTreeSet<String>) -> BTreeSet<String> {
    let mut out: BTreeSet<String> = t.refs.iter().filter(|(_, by)| by.iter().any(|b| b != ticket)).map(|(n, _)| n.clone()).collect();
    out.extend(refs.iter().cloned());
    out
}

/// The orphans nothing keeps, given the registry as it will be: an orphan
/// is alive while a ticket or a setting names it, or a live theme — owned,
/// defined, or an alive orphan — extends it.
pub(crate) fn unused(by_name: &BTreeMap<String, Theme>, named: &BTreeSet<String>, settings: &BTreeSet<String>) -> Vec<String> {
    let orphans: Vec<&String> = by_name.iter().filter(|(_, t)| t.orphan).map(|(n, _)| n).collect();
    let mut alive: BTreeSet<&str> = BTreeSet::new();
    loop {
        let mut grew = false;
        for o in &orphans {
            if alive.contains(o.as_str()) {
                continue;
            }
            let kept = named.contains(*o)
                || settings.contains(*o)
                || by_name.values().any(|t| t.extends.as_deref() == Some(o.as_str()) && (!t.orphan || alive.contains(t.name.as_str())));
            if kept {
                alive.insert(o);
                grew = true;
            }
        }
        if !grew {
            break;
        }
    }
    orphans.into_iter().filter(|o| !alive.contains(o.as_str())).cloned().collect()
}

/// The themes the site and every shortcode name.
pub(crate) fn set_names(t: &Themes) -> BTreeSet<String> {
    t.settings.values().filter_map(|s| s.theme.clone()).collect()
}

/// An `extends` cycle through `by_name`, if one passes through `start`.
fn cycle(by_name: &BTreeMap<String, Theme>, start: &str) -> Option<Vec<String>> {
    let mut path = vec![start.to_string()];
    let mut at = by_name.get(start)?.extends.clone();
    while let Some(n) = at {
        if n == start {
            path.push(n);
            return Some(path);
        }
        if path.contains(&n) || n == DEFAULT_THEME {
            return None;
        }
        path.push(n.clone());
        at = by_name.get(&n).and_then(|t| t.extends.clone());
    }
    None
}

/// What `ticket`'s write of a body saying `body` does to the themes — see
/// the module doc. Nothing changes here.
fn plan(t: &Themes, ticket: &str, body: &BodyThemes, now: i64) -> Result<Moves, StoreError> {
    for d in &body.defs {
        if let Some(owner) = t.by_name.get(&d.name).and_then(|x| x.owner.as_deref()).filter(|o| *o != ticket) {
            return Err(StoreError::Invalid(format!(
                "the theme `{}` is defined by {owner}'s block: a theme has one owner — change it in {owner}, or give this one \
                 another name (nothing was written)",
                d.name
            )));
        }
    }
    let mut next = t.by_name.clone();
    let mut moved: BTreeSet<String> = BTreeSet::new();
    for (name, th) in next.iter_mut() {
        if th.owner.as_deref() == Some(ticket) && !body.defs.iter().any(|d| &d.name == name) {
            th.owner = None;
            th.orphan = true;
            th.updated_unix = now;
            moved.insert(name.clone());
        }
    }
    for d in &body.defs {
        let was = next.get(&d.name);
        let mut th = Theme {
            name: d.name.clone(),
            light: d.light.clone(),
            dark: d.dark.clone(),
            updated_unix: now,
            extends: d.extends.clone(),
            owner: Some(ticket.to_string()),
            orphan: false,
            rules: d.rules.clone(),
            imports: d.imports.clone(),
        };
        if let Some(w) = was.filter(|w| Theme { updated_unix: w.updated_unix, ..th.clone() } == **w) {
            th.updated_unix = w.updated_unix;
        } else {
            moved.insert(d.name.clone());
        }
        next.insert(d.name.clone(), th);
    }
    for d in &body.defs {
        if let Some(path) = cycle(&next, &d.name) {
            return Err(StoreError::Invalid(format!(
                "```theme {}: its extends go round — {} (a theme extends towards default; nothing was written)",
                d.name,
                path.join(" → ")
            )));
        }
    }
    let had: BTreeSet<String> = t.refs.iter().filter(|(_, by)| by.contains(ticket)).map(|(n, _)| n.clone()).collect();
    let refs = (had != body.refs).then(|| (ticket.to_string(), body.refs.clone()));
    let named = named_by_tickets(t, ticket, &body.refs);
    let gone = unused(&next, &named, &set_names(t));
    let mut themes: Vec<(String, Option<Theme>)> = moved
        .iter()
        .filter(|n| !gone.contains(n))
        .map(|n| (n.clone(), next.get(n).cloned()))
        .collect();
    themes.extend(gone.into_iter().map(|n| (n, None)));
    themes.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(Moves { themes, refs })
}

impl Cache {
    /// COMP-3's plan for `ticket`'s body, and THEME-3's beside it: a
    /// component or a theme another ticket owns, an `extends` cycle — each
    /// refused, naming it (`id`: the ticket, for the message; `None` for a
    /// create).
    pub(super) fn plan_registry(&self, ticket: &str, uses: &components::Uses, id: Option<&str>) -> Result<Plan, StoreError> {
        let mut plan = self.components.plan(ticket, uses).map_err(|o| StoreError::ComponentOwned {
            id: id.map(str::to_string),
            name: o.name,
            owner: o.owner,
        })?;
        plan.moves.themes = plan_themes(&self.themes, ticket, &uses.themes)?;
        Ok(plan)
    }

    /// A committed plan, taken: the components, then the themes — each theme
    /// moved or removed said on the themes channel.
    pub(super) fn take_registry(&mut self, moves: components::Moves) {
        self.components.take(moves.components);
        take(&mut self.themes, moves.themes);
    }
}

/// [`plan`], over the clock.
fn plan_themes(t: &Themes, ticket: &str, body: &BodyThemes) -> Result<Moves, StoreError> {
    let had = t.by_name.values().any(|x| x.owner.as_deref() == Some(ticket)) || t.refs.values().any(|by| by.contains(ticket));
    if !had && body.defs.is_empty() && body.refs.is_empty() {
        return Ok(Moves::default());
    }
    plan(t, ticket, body, chrono::Utc::now().timestamp())
}

/// The cache's half of committed theme moves.
pub(crate) fn take(t: &mut Themes, moves: Moves) {
    if let Some((ticket, refs)) = moves.refs {
        for by in t.refs.values_mut() {
            by.remove(&ticket);
        }
        for r in refs {
            t.refs.entry(r).or_default().insert(ticket.clone());
        }
        t.refs.retain(|_, by| !by.is_empty());
    }
    for (name, theme) in moves.themes {
        match theme {
            Some(th) => {
                t.by_name.insert(name, th.clone());
                let _ = t.tx.send(ThemeChange::Changed(th));
            }
            None => {
                t.by_name.remove(&name);
                let _ = t.tx.send(ThemeChange::Removed(name));
            }
        }
    }
}

/// Committed theme moves, stored in the write's transaction.
pub(crate) fn store(conn: &Connection, moves: &Moves) -> rusqlite::Result<()> {
    for (name, theme) in &moves.themes {
        match theme {
            Some(th) => store_theme(conn, th)?,
            None => {
                conn.prepare_cached("DELETE FROM themes WHERE name = ?1")?.execute([name])?;
            }
        }
    }
    if let Some((ticket, refs)) = &moves.refs {
        conn.prepare_cached("DELETE FROM theme_refs WHERE ticket = ?1")?.execute([ticket])?;
        for r in refs {
            conn.prepare_cached("INSERT INTO theme_refs (theme, ticket) VALUES (?1, ?2)")?.execute(params![r, ticket])?;
        }
    }
    Ok(())
}

/// Every ticket's frontmatter references, as stored.
pub(crate) fn load_refs(conn: &Connection, into: &mut Themes) -> rusqlite::Result<()> {
    let mut stmt = conn.prepare("SELECT theme, ticket FROM theme_refs")?;
    let mut rows = stmt.query([])?;
    while let Some(r) = rows.next()? {
        into.refs.entry(r.get(0)?).or_default().insert(r.get(1)?);
    }
    Ok(())
}

/// A file from before THEME-3 gains the `themes` table's columns and
/// `theme_refs`, once, durably — every body read for its blocks (a name two
/// bodies define is the first's, in the store's order; a block that is not
/// a theme is logged and skipped) and its frontmatter's references.
pub(crate) fn migrate(conn: &mut Connection) -> anyhow::Result<()> {
    let has_refs: i64 = conn.query_row("SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name = 'theme_refs'", [], |r| r.get(0))?;
    if has_refs > 0 {
        return Ok(());
    }
    let tx = conn.transaction()?;
    for (col, def) in [
        ("owner", "TEXT"),
        ("orphan", "INTEGER NOT NULL DEFAULT 0"),
        ("extends", "TEXT"),
        ("rules", "TEXT NOT NULL DEFAULT '[]'"),
        ("imports", "TEXT NOT NULL DEFAULT '[]'"),
    ] {
        if !super::db::has_column(&tx, "themes", col)? {
            tx.execute(&format!("ALTER TABLE themes ADD COLUMN {col} {def}"), [])?;
        }
    }
    tx.execute_batch("CREATE TABLE theme_refs (theme TEXT NOT NULL, ticket TEXT NOT NULL, PRIMARY KEY (theme, ticket)) WITHOUT ROWID;")?;
    let (mut blocks, mut refs) = (0usize, 0usize);
    {
        let mut stmt = tx.prepare("SELECT id, body FROM tickets WHERE instr(body, 'theme') > 0 ORDER BY epic, seq")?;
        let mut rows = stmt.query([])?;
        let mut owned: BTreeMap<String, String> = BTreeMap::new();
        let now = chrono::Utc::now().timestamp();
        while let Some(r) = rows.next()? {
            let id: String = r.get(0)?;
            let body: String = r.get(1)?;
            let read = match read(&body) {
                Ok(b) => b,
                Err((name, e)) => {
                    tracing::warn!("THEME-3 migration: {id}'s ```theme {name} is not a theme ({e}) — not registered");
                    continue;
                }
            };
            for d in read.defs {
                if let Some(first) = owned.get(&d.name) {
                    tracing::warn!("THEME-3 migration: {id} defines the theme `{}`, which {first} defined first — not registered", d.name);
                    continue;
                }
                owned.insert(d.name.clone(), id.clone());
                let th = Theme { name: d.name, light: d.light, dark: d.dark, updated_unix: now, extends: d.extends, owner: Some(id.clone()), orphan: false, rules: d.rules, imports: d.imports };
                store_theme(&tx, &th)?;
                blocks += 1;
            }
            for t in read.refs {
                tx.execute("INSERT OR IGNORE INTO theme_refs (theme, ticket) VALUES (?1, ?2)", params![t, id])?;
                refs += 1;
            }
        }
    }
    tx.commit()?;
    tracing::warn!("THEME-3: the file gained theme owners and theme_refs, durably — {blocks} block(s) registered, {refs} frontmatter reference(s)");
    Ok(())
}

#[cfg(test)]
#[path = "tests_theme_registry.rs"]
mod tests;
