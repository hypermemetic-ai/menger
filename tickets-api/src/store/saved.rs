//! FILTER-36: what `saved "Name"` reads — and every filter a read, a count or
//! a watch is asked, resolved through it before it is compiled to cells.
//!
//! **What a name finds** is the saved filter of FILTER-46 ([`super::filters`],
//! the `saved_filters` table): the row whose slug is the name's (`Needs you`,
//! `needs you` and `needs-you` are one name); with none, `Needs you` still
//! means what the app means by it (MENGER-43's default,
//! [`crate::saved_filters::NEEDS_YOU`]) and every other name is refused.
//!
//! **What is resolved** ([`Store::lens`], [`Store::narrowing`],
//! [`Store::resolved`]): the text parsed by the language, every name
//! expanded by [`crate::filter::Filter::resolve`] from the table under
//! the read lock — a name no row has, a cycle, a saved text the language
//! refuses, one holding `text`: the read's in-band `error`, naming it —
//! then compiled to cells as FILTER-4 always did. A filter naming no name
//! is compiled exactly as before.
//!
//! **A watch** keeps the text it was asked ([`Watcher::naming`]): at each
//! change it hears, a watch whose filter names saved filters resolves it
//! again, and where the resolved filter moved — a filter saved, renamed or
//! deleted — says what that did to its membership
//! ([`Watcher::resaved`]): `entered` / `left` for each ticket it moved, or
//! a `recount` per shortcode past [`super::cells::RECOUNT_OVER`], as a cell
//! change would; a name that stops resolving ends the watch with the
//! in-band `error`.

use super::cells::RECOUNT_OVER;
use super::filtered::{Lens, Membership, Watcher};
use super::lineage::Compiled;
use super::*;
use crate::address::ShortcodeId;
use crate::filter::Filter;

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
        self.lens_of(&filter)
    }

    /// FILTER-36, ACCESS-9: [`Store::lens`] of a filter already read.
    pub fn lens_of(&self, filter: &Filter) -> Result<Lens, StoreError> {
        Lens::of(&self.resolve_saved(filter)?)
    }

    /// FILTER-36: [`Store::lens_of`] of a filter that may be absent — every
    /// ticket.
    pub fn lens_or_all(&self, filter: Option<&Filter>) -> Result<Lens, StoreError> {
        filter.map_or_else(|| Ok(Lens::all()), |f| self.lens_of(f))
    }

    /// FILTER-36: FILTER-15's [`super::narrowing_of`], its names resolved.
    pub fn narrowing(&self, filter: Option<&Filter>) -> Result<Option<Filter>, StoreError> {
        let Some(filter) = filter else { return Ok(None) };
        super::narrowing_of(Some(&self.resolve_saved(filter)?))
    }

    /// FILTER-36: FILTER-15's [`super::narrowing_lens`], its names resolved.
    pub fn narrowing_lens(&self, filter: Option<&Filter>) -> Result<Option<Lens>, StoreError> {
        self.narrowing(filter)?.map(|f| Lens::of(&f)).transpose()
    }

    /// FILTER-36: a filter resolved — the empty filter when there is none
    /// (`facets`' `filter` and `base`).
    pub fn resolved(&self, filter: Option<&Filter>) -> Result<Filter, String> {
        let Some(filter) = filter else { return Ok(Filter::default()) };
        self.resolve_saved(filter).map_err(|e| e.to_string())
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
                let id = cache.order.get(&OrderKey { shortcode: shortcode.clone(), seq }).map_or_else(|| shortcode.key(seq), |t| t.id.to_string());
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
    pub fn naming(mut self, filter: Option<&Filter>) -> Self {
        self.source = filter.filter(|f| !f.saved_names().is_empty()).map(Filter::format);
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
