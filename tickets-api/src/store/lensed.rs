//! FILTER-15: every read takes the lens — `find { query, filter }`,
//! `inputs { …, filter }`, `ready_set { …, filter }` and `member { ids,
//! filter }`, each answered through FILTER-4's cells and never by walking
//! the store:
//!
//! - **membership** (`member`, `inputs`) is one lookup a candidate: its
//!   position (`at`), then its shortcode's admitted bitmaps
//!   ([`View::contains`]);
//! - **`find`** tests its needle only on the candidates the filter's cells
//!   admit — each shortcode's admitted bitmaps merged, each candidate's row
//!   looked up by its position, or, where the candidates are most of a
//!   shortcode, its run read in step with the bitmap — so a filter
//!   admitting 1 % of the store reads about 1 % of its rows (FILTER-3
//!   §6.9);
//! - **`ready_set`** is a filtered page of `ready and (filter)`: `ready` is
//!   the ready set's rule as a cell predicate (FILTER-4), so the page, its
//!   `next` and its exact `remaining` are FILTER-4's sums and merges.
//!
//! A filter that narrows nothing — absent, empty, or a display part alone —
//! is no filter at all: the read takes today's path, byte for byte
//! ([`narrowing`]). One the language does not take, or that asks `under`
//! or `text`, is FILTER-4's in-band refusal.

use super::cells::View;
use super::filtered::{view, view_lenient};
use super::*;
use crate::filter::{Field, Filter, Group, Predicate};
use roaring::{MultiOps, RoaringBitmap};
use std::borrow::Cow;

/// FILTER-15: the most ids one `member` call takes; more is an in-band
/// error and nothing else.
pub const MEMBER_MAX: usize = 1_000;

/// FILTER-15: what a read's `filter` asks — `None` when it narrows nothing
/// (absent, empty, or a display part alone: every ticket), so the read
/// answers exactly as it does without one; else the filter, read by the
/// language (FILTER-10). A text the language does not take, or one asking
/// `under` or `text`, is FILTER-4's refusal ([`Lens::parse`]'s message).
pub fn narrowing(text: Option<&str>) -> Result<Option<Filter>, StoreError> {
    let Some(text) = text else { return Ok(None) };
    let filter = Filter::parse(text).map_err(|e| StoreError::Filter(e.to_string()))?;
    narrowing_of(Some(&filter))
}

/// ACCESS-9: [`narrowing`] of a filter already read — what a read's filter
/// ANDed with the caller's reach is (never formatted and read back).
pub fn narrowing_of(filter: Option<&Filter>) -> Result<Option<Filter>, StoreError> {
    let Some(filter) = filter else { return Ok(None) };
    super::filtered::refuse(filter)?;
    Ok((!filter.groups.is_empty()).then(|| filter.clone()))
}

/// FILTER-15: [`narrowing_of`], as the lens a read is answered under.
pub fn narrowing_lens(filter: Option<&Filter>) -> Result<Option<Lens>, StoreError> {
    narrowing_of(filter)?.map(|f| Lens::of(&f)).transpose()
}

/// FILTER-15: `ready_set { epic, filter }`'s selection — the ready set ∧ the
/// filter, as a filtered list of `ready and (filter)` in `epic` (exactly as
/// named, as `ready_set { epic }` always took it): `ready` is the ready
/// set's rule, category `ready` and not `blocked` (FILTER-4), so its cells
/// hold exactly the ready set.
pub fn ready_and(filter: &Filter, epic: Option<String>) -> Result<Filtered, StoreError> {
    let ready = Filter {
        groups: vec![Group::one(Predicate::is(Field::Ready))],
        display: Default::default(),
    };
    Ok(Filtered {
        lens: Lens::of(&ready.and(filter))?,
        epic,
        status: None,
        parent: None,
        keep: None,
    })
}

/// The whole store under `lens`: no shortcode, status or parent beside it.
fn everywhere(lens: &Lens) -> Filtered {
    Filtered {
        lens: lens.clone(),
        epic: None,
        status: None,
        parent: None,
        keep: None,
    }
}

/// Where `id` sits in the store's order — ORG-3: any address the resolver
/// takes to one ticket (a short form, a full address, a path address,
/// today's pinned ids); else an id as given, else ASCII upper-cased (a hand
/// edit may write a lower-case one, which `get` names as it is). One that
/// names several sits nowhere.
fn locate<'c>(cache: &'c Cache, id: &str) -> Option<&'c OrderKey> {
    if let shortcodes::Found::One(key) = cache.find_ticket(id, shortcodes::OrgReach::All) {
        return cache.at.get(&key);
    }
    let id = id.trim();
    cache.at.get(id).or_else(|| cache.at.get(&id.to_ascii_uppercase()))
}

/// A shortcode whose admitted tickets are at least one in this many of its
/// tickets is read along its run, in step with the bitmap, rather than a
/// lookup a candidate. At 10⁶ a lookup costs about four steps along the run
/// (perf/reads-probe.mjs: a filter admitting every ticket, by lookups, was
/// 2.2× an unfiltered `find`; by the run, 1.05×) — so past one in four, the
/// run is the cheaper read of the same candidates.
const DENSE: u64 = 4;

/// Every candidate the view admits strictly after `after` (from the first
/// without it), in the store's order, each row handed to `visit`: per
/// shortcode, its admitted bitmaps merged into one; each member's row
/// looked up by its position — or, where the members are most of the
/// shortcode ([`DENSE`]), its run read in order, each row kept when the
/// bitmap holds it. A shortcode the filter admits nothing of is never read,
/// and no row is read twice.
fn candidates<'c>(cache: &'c Cache, view: &View<'c>, after: Option<&OrderKey>, mut visit: impl FnMut(&'c Row)) {
    let mut key = OrderKey {
        shortcode: ShortcodeId::default(),
        seq: 0,
    };
    for (epic, bms) in view.shortcodes_from(after.map(|k| &k.shortcode)) {
        let lo = match after {
            Some(k) if k.shortcode == **epic => match k.seq.checked_add(1) {
                Some(seq) => seq,
                None => continue,
            },
            _ => 0,
        };
        let merged: Cow<'_, RoaringBitmap> = match bms.as_slice() {
            [one] => Cow::Borrowed(one.as_ref()),
            many => Cow::Owned(many.iter().map(|b| b.as_ref()).union()),
        };
        key.shortcode = (*epic).clone();
        if merged.len().saturating_mul(DENSE) >= cache.ids.held(epic) {
            key.seq = lo;
            let end = OrderKey {
                shortcode: key.shortcode.clone(),
                seq: u32::MAX,
            };
            // The run and the members, both ascending: a merge, which stops
            // at the last member.
            let mut members = merged.range(lo..).peekable();
            for (_, t) in cache.order.range(&key..=&end) {
                while members.next_if(|s| *s < t.seq).is_some() {}
                match members.peek() {
                    None => break,
                    Some(s) if *s == t.seq => {
                        members.next();
                        visit(t);
                    }
                    Some(_) => {}
                }
            }
            continue;
        }
        for seq in merged.range(lo..) {
            key.seq = seq;
            if let Some(t) = cache.order.get(&key) {
                visit(t);
            }
        }
    }
}

impl Store {
    /// FILTER-15: `find { query, filter }` — one page of the tickets whose
    /// id or title contains `query` (the palette's rule, [`Needle`]) and
    /// which `lens` admits, in the store's order, cut as [`Store::find_page`]
    /// cuts one: strictly after `after` (a position; one naming no ticket is
    /// [`StoreError::CursorNotFound`]), at most `limit`, `remaining` the
    /// matches past the page, exactly; no `first`. The needle is tested only
    /// on the candidates the filter's cells admit (FILTER-3 §6.9). A blank
    /// query is every ticket the filter admits — FILTER-4's filtered page,
    /// by position.
    pub fn find_filtered(&self, query: &str, lens: &Lens, after: Option<&str>, limit: Option<u32>) -> Result<Page, StoreError> {
        let after = after.map(str::to_ascii_uppercase);
        let cache = self.tickets.read().expect("store lock poisoned");
        let q = everywhere(lens);
        let Some(needle) = Needle::of(query) else {
            let page = Self::cut_filtered(&cache, &q, after.as_deref(), None, limit, Rest::Count)?;
            return Ok(Page { first: None, ..page });
        };
        let cursor = match after.as_deref() {
            Some(id) => Some(cache.at.get(id).cloned().ok_or_else(|| StoreError::CursorNotFound(id.to_string()))?),
            None => None,
        };
        let view = view(&cache, &q)?;
        let take = limit.map_or(usize::MAX, |n| n as usize);
        let mut tickets: Vec<Row> = Vec::with_capacity(take.min(1024));
        let mut remaining = 0u64;
        candidates(&cache, &view, cursor.as_ref(), |t| {
            if !needle.matches(t) {
                return;
            }
            if tickets.len() < take {
                tickets.push(t.clone());
            } else {
                remaining += 1;
            }
        });
        // As every page of the store's order: the last id, or — for an
        // empty page with matches past it (`limit: 0`) — the cursor.
        let next = match remaining {
            0 => None,
            _ => tickets.last().map(|t| t.id.to_string()).or(after),
        };
        Ok(Page {
            tickets,
            next,
            remaining,
            first: None,
        })
    }

    /// FILTER-15: a page of `ready_set { epic, filter }` — `q` from
    /// [`ready_and`] — cut as `ready_set` cuts one: `after` a position,
    /// `limit`, an exact `remaining`, no `first`.
    pub fn ready_filtered_page(&self, q: &Filtered, after: Option<&str>, limit: Option<u32>) -> Result<Page, StoreError> {
        let page = self.filtered_page(q, after, None, limit)?;
        Ok(Page { first: None, ..page })
    }

    /// FILTER-15: `inputs { ticket, epic, state, filter }` — what
    /// [`Store::inputs`] answers, of the tickets `lens` admits: each ticket
    /// that declares an input is one lookup in the admitted bitmaps. The
    /// same `NotFound` for a `ticket` naming no ticket.
    pub fn inputs_filtered(
        &self,
        ticket: Option<&str>,
        epic: Option<&str>,
        state: Option<InputState>,
        lens: &Lens,
    ) -> Result<Vec<Input>, StoreError> {
        let cache = self.tickets.read().expect("store lock poisoned");
        // ORG-3: as [`Store::inputs`] resolves them.
        let ticket = ticket.map(|t| cache.resolve(t)).transpose()?;
        let epic = epic.map(|e| span_shortcode(&cache, Some(e))).transpose()?.flatten();
        let view = view(&cache, &everywhere(lens))?;
        let admitted = |id: &str| cache.at.get(id).is_some_and(|k| view.contains(&k.shortcode, k.seq));
        Ok(cache.inputs.query_where(ticket.as_deref(), epic.as_ref(), state, admitted))
    }

    /// FILTER-15: `member { ids, filter }` — for each of `ids`, in the order
    /// given, whether `lens` admits it: its position, then a lookup in its
    /// shortcode's admitted bitmaps. An id that names no ticket is not
    /// admitted. A path address (`SHORTCODE/path`) names its ticket as `get`
    /// takes it. How many ids one call may ask ([`MEMBER_MAX`]) is the
    /// wire's to hold.
    pub fn member(&self, ids: &[String], lens: &Lens) -> Vec<bool> {
        let cache = self.tickets.read().expect("store lock poisoned");
        let view = view_lenient(&cache, &everywhere(lens));
        ids.iter()
            .map(|id| locate(&cache, id).is_some_and(|k| view.contains(&k.shortcode, k.seq)))
            .collect()
    }

    /// FILTER-24: the id of the ticket `id` names, as `member` takes it (a
    /// path address, or an id in any case) — `None` when it names none.
    pub fn ticket_id(&self, id: &str) -> Option<String> {
        let cache = self.tickets.read().expect("store lock poisoned");
        let key = locate(&cache, id)?;
        cache.order.get(key).map(|t| t.id.clone()).map(String::from)
    }
}
