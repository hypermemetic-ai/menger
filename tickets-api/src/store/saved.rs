//! FILTER-36: the **saved-filter registry** — what `saved "Name"` reads —
//! and every filter a read, a count or a watch is asked, resolved through it
//! before it is compiled to cells.
//!
//! **What is kept** ([`SavedFilters`]): for each ticket whose body holds a
//! `filter:` line ([`crate::saved_filters::filter_line`]), that line's text,
//! by the ticket's id — nothing else of the body (SCALE-14: bodies stay out
//! of the cache). It is read from the bodies of every doc at a view path at
//! open ([`load`]), and moved by every write of a body with the component
//! registry, in the same critical section as the ticket's row
//! ([`super::Cache::take_registry`]): `create`, `amend`, `update`, `delete`
//! (its text goes) and `undelete` (its text comes back). A file read back
//! after a batch or a re-shortcode is read again whole. Derived from the
//! bodies, it is not stored, and `--verify-derived` has nothing to compare.
//!
//! **What a name finds** ([`super::Cache::saved_text`]): a doc in the
//! shortcode `VIEW` at a path `_MENGER/view/<slug>` — a saved filter as the
//! app writes one (FILTER-6) — whose title's slug is the name's (`Needs
//! you`, `needs you` and `needs-you` are one name); several so titled, the
//! one whose path is that slug, else the first in the store's order. No
//! title so: the doc whose path is the name's slug — the old name of a
//! renamed filter keeps naming it until another filter takes the name. Its
//! title and path are read from its row, so a rename (a title written
//! alone) moves what a name finds without a body written. A doc the
//! collection no longer lists (`remove` in `Saved ▾` keeps the doc) still
//! resolves: a filter naming a filter taken off the menu keeps working
//! while the doc exists. A deleted doc resolves nothing. No doc at all:
//! `Needs you` still means what the app means by it until one is saved
//! (MENGER-43's default, [`crate::saved_filters::NEEDS_YOU`]); every other
//! name is refused.
//!
//! **What is resolved** ([`Store::lens`], [`Store::narrowing`],
//! [`Store::resolved`]): the text parsed by the language, every name
//! expanded by [`crate::filter::Filter::resolve`] from the registry under
//! the read lock — a name no doc has, a cycle, a saved text the language
//! refuses, one holding `text`: the read's in-band `error`, naming it —
//! then compiled to cells as FILTER-4 always did. A filter naming no name
//! is compiled exactly as before.
//!
//! **A watch** keeps the text it was asked ([`Watcher::naming`]): at each
//! change it hears, a watch whose filter names saved filters resolves it
//! again, and where the resolved filter moved — a doc's text amended, a
//! rename, a delete — says what that did to its membership
//! ([`Watcher::resaved`]): `entered` / `left` for each ticket it moved, or
//! a `recount` per shortcode past [`super::cells::RECOUNT_OVER`], as a cell
//! change would; a name that stops resolving ends the watch with the
//! in-band `error`.

use super::cells::RECOUNT_OVER;
use super::filtered::{refuse, Lens, Membership, Watcher};
use super::lineage::Compiled;
use super::*;
use crate::address::ShortcodeId;
use crate::filter::Filter;
use crate::saved_filters::{path_slug, slug, EPIC};

/// FILTER-36: every saved filter's text, by its doc's id.
#[derive(Debug, Default, Clone)]
pub(super) struct SavedFilters {
    texts: HashMap<String, String>,
}

impl SavedFilters {
    /// `ticket`'s body written: its `filter:` line now, or none.
    pub(super) fn take(&mut self, ticket: String, text: Option<String>) {
        match text {
            Some(t) => {
                self.texts.insert(ticket, t);
            }
            None => {
                self.texts.remove(&ticket);
            }
        }
    }

    #[cfg(test)]
    pub(super) fn len(&self) -> usize {
        self.texts.len()
    }
}

/// Whether `row` is a saved filter's doc: in `VIEW`, at a view path.
pub(super) fn is_saved_doc(row: &Row) -> bool {
    row.epic.as_str().eq_ignore_ascii_case(EPIC) && row.path.as_deref().and_then(path_slug).is_some()
}

/// The registry from the file: the `filter:` line of every doc at a view
/// path.
pub(super) fn load(conn: &rusqlite::Connection) -> rusqlite::Result<SavedFilters> {
    let mut stmt = conn.prepare("SELECT id, body FROM tickets WHERE path LIKE '\\_MENGER/view/%' ESCAPE '\\'")?;
    let mut texts = HashMap::new();
    for row in stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))? {
        let (id, body) = row?;
        if let Some(text) = crate::saved_filters::filter_line(&body) {
            texts.insert(id, text.to_string());
        }
    }
    Ok(SavedFilters { texts })
}

impl Cache {
    /// FILTER-36: the text of the saved filter `name` finds (the module
    /// doc's rule), `None` when it finds none.
    pub(super) fn saved_text(&self, name: &str) -> Option<String> {
        let key = slug(name);
        let mut titled: Option<(bool, &OrderKey, &String)> = None;
        let mut pathed: Option<(&OrderKey, &String)> = None;
        for (id, text) in &self.saved.texts {
            let Some(k) = self.at.get(id) else { continue };
            let Some(row) = self.order.get(k).filter(|r| is_saved_doc(r)) else { continue };
            let at_path = row.path.as_deref().and_then(path_slug) == Some(key.as_str());
            if slug(&row.title) == key {
                let better = titled.is_none_or(|(p, o, _)| (!at_path, k) < (!p, o));
                if better {
                    titled = Some((at_path, k, text));
                }
            } else if at_path && pathed.is_none_or(|(o, _)| k < o) {
                pathed = Some((k, text));
            }
        }
        // MENGER-43: and a name every reader has before saving it.
        titled.map(|(_, _, t)| t).or(pathed.map(|(_, t)| t)).cloned().or_else(|| crate::saved_filters::default_text(name).map(str::to_string))
    }
}

impl Store {
    /// FILTER-36: `filter` with every saved name in it resolved from the
    /// registry — the read's in-band refusal naming what does not resolve.
    pub fn resolve_saved(&self, filter: &Filter) -> Result<Filter, StoreError> {
        if filter.saved_names().is_empty() {
            return Ok(filter.clone());
        }
        let cache = self.tickets.read().expect("store lock poisoned");
        filter.resolve(&mut |name| cache.saved_text(name)).map_err(|e| StoreError::Filter(e.to_string()))
    }

    /// FILTER-36: a filter's text as [`Lens::parse`] reads it — its names
    /// resolved first.
    pub fn lens(&self, text: &str) -> Result<Lens, StoreError> {
        let filter = Filter::parse(text).map_err(|e| StoreError::Filter(e.to_string()))?;
        Lens::of(&self.resolve_saved(&filter)?)
    }

    /// FILTER-36: [`Store::lens`] of a filter that may be absent — every
    /// ticket.
    pub fn lens_or_all(&self, text: Option<&str>) -> Result<Lens, StoreError> {
        text.map_or_else(|| Ok(Lens::all()), |t| self.lens(t))
    }

    /// FILTER-36: FILTER-15's [`super::narrowing`], its names resolved.
    pub fn narrowing(&self, text: Option<&str>) -> Result<Option<Filter>, StoreError> {
        let Some(text) = text else { return Ok(None) };
        let filter = Filter::parse(text).map_err(|e| StoreError::Filter(e.to_string()))?;
        let filter = self.resolve_saved(&filter)?;
        refuse(&filter)?;
        Ok((!filter.groups.is_empty()).then_some(filter))
    }

    /// FILTER-36: FILTER-15's [`super::narrowing_lens`], its names resolved.
    pub fn narrowing_lens(&self, text: Option<&str>) -> Result<Option<Lens>, StoreError> {
        self.narrowing(text)?.map(|f| Lens::of(&f)).transpose()
    }

    /// FILTER-36: a filter's text read and resolved — the empty filter when
    /// there is none (`facets`' `filter` and `base`).
    pub fn resolved(&self, text: Option<&str>) -> Result<Filter, String> {
        let Some(text) = text else { return Ok(Filter::default()) };
        let filter = Filter::parse(text).map_err(|e| e.to_string())?;
        self.resolve_saved(&filter).map_err(|e| e.to_string())
    }

    /// FILTER-36: what a filter changing from `before` to `after` did to a
    /// watch of it — each ticket one admits and the other does not, as
    /// `Entered` / `Left` in the store's order (in `epic` alone, when one is
    /// asked); past [`RECOUNT_OVER`], one `Recount` per shortcode moved,
    /// with `after`'s count there.
    pub(super) fn refiltered(&self, before: &Lens, after: &Lens, epic: Option<&ShortcodeId>) -> Vec<Membership> {
        let cache = self.tickets.read().expect("store lock poisoned");
        let (b, a) = (Compiled::new(&cache, before, None), Compiled::new(&cache, after, None));
        let mut shortcodes: Vec<&ShortcodeId> = cache.cells.shortcodes().map(|(e, _)| e).filter(|e| epic.is_none_or(|x| x == *e)).collect();
        shortcodes.sort();
        let mut moved: Vec<(ShortcodeId, u32, bool)> = Vec::new();
        let mut held: Vec<(ShortcodeId, u64)> = Vec::new();
        for e in shortcodes {
            let (was, is) = (b.matches_in(e), a.matches_in(e));
            if was == is {
                continue;
            }
            held.push((e.clone(), is.len()));
            let mut here: Vec<(u32, bool)> = (&is - &was).iter().map(|s| (s, true)).chain((&was - &is).iter().map(|s| (s, false))).collect();
            here.sort();
            moved.extend(here.into_iter().map(|(s, now)| (e.clone(), s, now)));
        }
        if moved.len() > RECOUNT_OVER {
            return held.into_iter().map(|(shortcode, held)| Membership::Recount { shortcode, held }).collect();
        }
        moved
            .into_iter()
            .map(|(shortcode, seq, now)| {
                let id = cache.order.get(&OrderKey { shortcode: shortcode.clone(), seq }).map_or_else(|| shortcode.key(seq), |t| t.id.clone());
                match now {
                    true => Membership::Entered { id, shortcode, seq },
                    false => Membership::Left { id, shortcode, seq },
                }
            })
            .collect()
    }
}

impl Watcher {
    /// FILTER-36: this watch asked as `text` — kept when it names saved
    /// filters, for [`Watcher::resaved`] to read again.
    pub fn naming(mut self, text: Option<&str>) -> Self {
        self.source = text.filter(|t| Filter::parse(t).is_ok_and(|f| !f.saved_names().is_empty())).map(str::to_string);
        self
    }

    /// FILTER-36: whether its filter names saved filters.
    pub fn names_saved(&self) -> bool {
        self.source.is_some()
    }

    /// FILTER-36: the filter it was asked, resolved again as the saved
    /// filters stand — when that moved what it is, the watch is compiled
    /// again and what it now admits differently is said
    /// ([`Store::refiltered`]); a name that no longer resolves is the
    /// watch's in-band refusal.
    pub fn resaved(&mut self, store: &Store, epic: Option<&ShortcodeId>) -> Result<Vec<Membership>, String> {
        let Some(source) = self.source.clone() else { return Ok(Vec::new()) };
        let lens = store.lens(&source).map_err(|e| e.to_string())?.with_clock(self.lens.clock().clone());
        if lens.text() == self.lens.text() {
            return Ok(Vec::new());
        }
        let out = store.refiltered(&self.lens, &lens, epic);
        *self = store.watcher_in(lens, epic);
        self.source = Some(source);
        Ok(out)
    }
}
