//! FILTER-46: the **saved filters** — a table of their own, `saved_filters`
//! (an organization, a name unique within it, the filter as canonical text,
//! who saved it and when), kept in the cache like everything stored and
//! written through. They are not tickets.
//!
//! **Behind the cache** (AGENTS.md): every row is read when the store opens
//! ([`load`]); `list`, `get` and the `saved "Name"` cell
//! ([`super::Cache::saved_text`]) answer from it, under the read lock. A
//! write — `save`, `rename`, `delete` — is the write path every write takes
//! ([`Store::write`], MNG-20): checked under the write-serial lock against
//! the cache, committed in one transaction, put into the cache, and said as
//! a [`Change::Filters`] (`filter_saved` / `filter_deleted` on a watch).
//!
//! **What `save` refuses**, nothing kept: a name with no letter or digit or
//! longer than [`NAME_MAX`]; a filter the language cannot read (the error
//! names the chunk and where it stands); one that names no predicate or
//! holds `text`; and one whose `saved "…"` names do not resolve once it is
//! saved — an unknown name, or a cycle (`"A" names "B" names "A"`).
//! A name already there is replaced, keeping who first saved it.
//!
//! **A name is matched by its slug** ([`crate::saved_filters::slug`]):
//! `Needs you`, `needs you` and `needs-you` are one name. With no row,
//! `Needs you` still means MENGER-43's default.

use super::*;
use crate::access::{Editor, On};
use crate::address::OrgId;
use crate::filter::Filter;
use crate::saved_filters::{slug, SavedFilter, NAME_MAX};
use rusqlite::{params, Connection};

/// The table. One row per (organization, slug); the name keeps its case.
pub(super) const TABLE: &str = "
        CREATE TABLE IF NOT EXISTS saved_filters (
            org        TEXT NOT NULL,
            slug       TEXT NOT NULL,
            name       TEXT NOT NULL,
            filter     TEXT NOT NULL,
            saved_by   TEXT NOT NULL,
            saved_at   INTEGER NOT NULL,
            updated_at INTEGER NOT NULL,
            PRIMARY KEY (org, slug)
        );
    ";

/// A file without the table gains it, empty, on open — a no-op once it is
/// there. Returns whether it was made.
pub(crate) fn migrate(conn: &mut Connection) -> anyhow::Result<bool> {
    let had: bool =
        conn.query_row("SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name = 'saved_filters'", [], |r| r.get::<_, i64>(0).map(|n| n > 0))?;
    if had {
        return Ok(false);
    }
    conn.execute_batch(TABLE)?;
    tracing::warn!("FILTER-46: the file gained the saved_filters table, empty, durably");
    Ok(true)
}

/// What the cache says of saved filters happening: for `watch`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum FilterChange {
    /// A filter saved, replaced or renamed (`renamed_from`: the name it had).
    Saved { filter: SavedFilter, #[serde(default, skip_serializing_if = "Option::is_none")] renamed_from: Option<String> },
    /// A filter deleted.
    Deleted { name: String },
}

/// Every saved filter, by (organization, slug).
#[derive(Debug, Default, Clone)]
pub(super) struct SavedFilters {
    by_key: BTreeMap<(String, String), SavedFilter>,
}

impl SavedFilters {
    fn put(&mut self, org: &str, f: SavedFilter) {
        self.by_key.insert((org.to_string(), slug(&f.name)), f);
    }

    fn get(&self, org: &str, name: &str) -> Option<&SavedFilter> {
        self.by_key.get(&(org.to_string(), slug(name)))
    }

    #[cfg(test)]
    pub(super) fn len(&self) -> usize {
        self.by_key.len()
    }
}

/// Every row as stored — what a start reads.
pub(super) fn load(conn: &Connection) -> rusqlite::Result<SavedFilters> {
    let mut stmt = conn.prepare("SELECT org, name, filter, saved_by, saved_at, updated_at FROM saved_filters")?;
    let mut out = SavedFilters::default();
    for row in stmt.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            SavedFilter { name: r.get(1)?, filter: r.get(2)?, count: None, saved_by: r.get(3)?, saved_at: r.get(4)?, updated_at: r.get(5)? },
        ))
    })? {
        let (org, f) = row?;
        out.put(&org, f);
    }
    Ok(out)
}

fn upsert(conn: &Connection, org: &str, f: &SavedFilter) -> rusqlite::Result<()> {
    conn.prepare_cached(
        "INSERT INTO saved_filters (org, slug, name, filter, saved_by, saved_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
         ON CONFLICT (org, slug) DO UPDATE SET name = excluded.name, filter = excluded.filter, updated_at = excluded.updated_at",
    )?
    .execute(params![org, slug(&f.name), f.name, f.filter, f.saved_by, f.saved_at, f.updated_at])?;
    Ok(())
}

fn refused(message: impl Into<String>) -> StoreError {
    StoreError::Filter(message.into())
}

fn not_found(name: &str) -> StoreError {
    refused(format!("no saved filter named \"{name}\""))
}

/// A name as kept: trimmed, one with a letter or digit, at most [`NAME_MAX`].
fn clean_name(raw: &str) -> Result<String, StoreError> {
    let name = raw.trim();
    match (slug(name).is_empty(), name.chars().count() > NAME_MAX) {
        (true, _) => Err(refused("a saved filter's name needs a letter or a digit")),
        (_, true) => Err(refused(format!("a saved filter's name is at most {NAME_MAX} characters"))),
        _ => Ok(name.to_string()),
    }
}

impl Cache {
    /// FILTER-36 / FILTER-46: the text of the saved filter `name` finds — the
    /// one whose slug is the name's (the first organization's, in order),
    /// else `Needs you`'s default; `None` when it finds none.
    pub(super) fn saved_text(&self, name: &str) -> Option<String> {
        let key = slug(name);
        let found = self.saved.by_key.iter().find(|((_, s), _)| *s == key).map(|(_, f)| f.filter.clone());
        found.or_else(|| crate::saved_filters::default_text(name).map(str::to_string))
    }
}

impl Store {
    /// FILTER-46: `org`'s saved filters, by name (`Needs you`'s default
    /// among them while none is saved under it).
    pub fn saved_filters(&self, org: &OrgId) -> Vec<SavedFilter> {
        let cache = self.tickets.read().expect("store lock poisoned");
        let mut all: Vec<SavedFilter> = cache.saved.by_key.iter().filter(|((o, _), _)| o == org.as_str()).map(|(_, f)| f.clone()).collect();
        if cache.saved.get(org.as_str(), crate::saved_filters::NEEDS_YOU.0).is_none() {
            all.push(default_row());
        }
        all.sort_by_key(|f| slug(&f.name));
        all
    }

    /// FILTER-46: the saved filter `name` finds in `org`.
    pub fn saved_filter(&self, org: &OrgId, name: &str) -> Result<SavedFilter, StoreError> {
        let cache = self.tickets.read().expect("store lock poisoned");
        cache
            .saved
            .get(org.as_str(), name)
            .cloned()
            .or_else(|| (slug(name) == slug(crate::saved_filters::NEEDS_YOU.0)).then(default_row))
            .ok_or_else(|| not_found(name))
    }

    /// FILTER-46: whether `text` could be saved as `name` — read by the
    /// language, resolving every name it holds with `name` meaning `text` —
    /// the refusal naming what is wrong. Writes nothing.
    pub fn check_filter(&self, name: Option<&str>, text: &str) -> Result<Filter, StoreError> {
        let parsed = Filter::parse(text).map_err(|e| {
            let at = e.chunks.iter().map(|c| format!("byte {}", c.at)).collect::<Vec<_>>().join(", ");
            refused(format!("{e} (at {at})"))
        })?;
        if parsed.groups.is_empty() {
            return Err(refused("the filter names no predicate"));
        }
        if parsed.leaves().iter().any(|p| matches!(p.field, crate::filter::Field::Text(_))) {
            return Err(refused("a saved filter cannot hold `text`"));
        }
        let cache = self.tickets.read().expect("store lock poisoned");
        let me = name.map(|n| (slug(n), parsed.format()));
        let mut lookup = |n: &str| match &me {
            Some((k, t)) if *k == slug(n) => Some(t.clone()),
            _ => cache.saved_text(n),
        };
        let resolved = parsed.resolve(&mut lookup).map_err(|e| refused(e.to_string()))?;
        drop(cache);
        Lens::of(&resolved)?;
        Ok(parsed)
    }

    /// FILTER-46: saves `text` as `name` in `org` — a new filter, or the
    /// one of that name replaced (who first saved it kept). Answers the row
    /// and whether it replaced one. See the module doc for the refusals.
    pub async fn save_filter(&self, on: On<Editor>, org: &OrgId, name: &str, text: &str, by: String) -> Result<(SavedFilter, bool), StoreError> {
        Self::in_org(&on, org)?;
        let (name, parsed) = (clean_name(name)?, self.check_filter(Some(name.trim()), text)?);
        let serial = self.serial().await;
        let before = self.tickets.read().expect("store lock poisoned").saved.get(org.as_str(), &name).cloned();
        let now = chrono::Utc::now().timestamp();
        let row = SavedFilter {
            name: name.clone(),
            filter: parsed.format(),
            count: None,
            saved_by: before.as_ref().map_or(by, |b| b.saved_by.clone()),
            saved_at: before.as_ref().map_or(now, |b| b.saved_at),
            updated_at: now,
        };
        let replaced = before.is_some();
        let (o, kept, stored) = (org.to_string(), row.clone(), row.clone());
        self.write(
            serial,
            move |tx| Ok(upsert(tx, &o, &stored)?),
            {
                let o = org.to_string();
                move |cache, _| cache.saved.put(&o, kept)
            },
            move |(), (), tx| {
                tx.send(Change::Filters(FilterChange::Saved { filter: row.clone(), renamed_from: None }));
                (row, replaced)
            },
        )
        .await
    }

    /// FILTER-46: `name` in `org` renamed `to` — its filter, author and
    /// first time kept. `to` may differ from `name` in case alone; another
    /// filter's name is refused. Said as `Saved { renamed_from }`, then
    /// `Deleted { name: <old> }` (SURFACE-6) when the name moved.
    pub async fn rename_filter(&self, on: On<Editor>, org: &OrgId, name: &str, to: &str) -> Result<SavedFilter, StoreError> {
        Self::in_org(&on, org)?;
        let to = clean_name(to)?;
        let serial = self.serial().await;
        let before = self.tickets.read().expect("store lock poisoned").saved.get(org.as_str(), name).cloned().ok_or_else(|| not_found(name))?;
        if slug(&to) != slug(&before.name) && self.tickets.read().expect("store lock poisoned").saved.get(org.as_str(), &to).is_some() {
            return Err(refused(format!("a saved filter is already named \"{to}\"")));
        }
        let row = SavedFilter { name: to, updated_at: chrono::Utc::now().timestamp(), ..before.clone() };
        let (o, old, kept, stored) = (org.to_string(), slug(&before.name), row.clone(), row.clone());
        self.write(
            serial,
            {
                let (o, old) = (o.clone(), old.clone());
                move |tx| {
                    tx.execute("DELETE FROM saved_filters WHERE org = ?1 AND slug = ?2", params![o, old])?;
                    Ok(upsert(tx, &o, &stored)?)
                }
            },
            move |cache, _| {
                cache.saved.by_key.remove(&(o.clone(), old));
                cache.saved.put(&o, kept)
            },
            move |(), (), tx| {
                // SURFACE-6: and the old name said gone, right after — a
                // reader keyed by name (the app's live list) takes the old
                // row out by `filter_deleted` alone. A case-only rename says
                // it too: the key is the name as written.
                let old = (before.name != row.name).then(|| before.name.clone());
                tx.send(Change::Filters(FilterChange::Saved { filter: row.clone(), renamed_from: Some(before.name) }));
                if let Some(name) = old {
                    tx.send(Change::Filters(FilterChange::Deleted { name }));
                }
                row
            },
        )
        .await
    }

    /// FILTER-46: deletes `name` from `org`. A filter that names it stops
    /// resolving — a read of it says so.
    pub async fn delete_filter(&self, on: On<Editor>, org: &OrgId, name: &str) -> Result<String, StoreError> {
        Self::in_org(&on, org)?;
        let serial = self.serial().await;
        let before = self.tickets.read().expect("store lock poisoned").saved.get(org.as_str(), name).cloned().ok_or_else(|| not_found(name))?;
        let (o, k) = (org.to_string(), slug(&before.name));
        self.write(
            serial,
            {
                let (o, k) = (o.clone(), k.clone());
                move |tx| Ok(tx.execute("DELETE FROM saved_filters WHERE org = ?1 AND slug = ?2", params![o, k]).map(|_| ())?)
            },
            move |cache, _| {
                cache.saved.by_key.remove(&(o, k));
            },
            move |(), (), tx| {
                tx.send(Change::Filters(FilterChange::Deleted { name: before.name.clone() }));
                before.name
            },
        )
        .await
    }

    /// The proof is for the organization written.
    fn in_org(on: &On<Editor>, org: &OrgId) -> Result<(), StoreError> {
        match on.scope() {
            crate::members::Scope::Org(o) if o == org => Ok(()),
            s => Err(refused(format!("the proof is for {s}, not {org}"))),
        }
    }
}

/// `Needs you` as every reader has it before saving it.
fn default_row() -> SavedFilter {
    let (name, filter) = crate::saved_filters::NEEDS_YOU;
    SavedFilter { name: name.into(), filter: filter.into(), count: None, saved_by: "menger".into(), saved_at: 0, updated_at: 0 }
}
