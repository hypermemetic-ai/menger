//! ACCESS-10: the store's public reads of tickets, each as a [`Reader`] may
//! read it ([`super::seen`]). Each is its unscoped form (private to the
//! store) under the reader's reach: a hidden ticket is not found, a cursor
//! naming one is a cursor into nothing, a shortcode the reach admits nothing
//! of is never named, and every row it does admit is redacted. For the
//! [`System`] (and a reader of everything) each answers exactly what its
//! unscoped form does.
//!
//! The reads of a ticket's neighbourhood answer only what is in reach:
//! `blockers` drops a blocker it hides (and, to a restricted reader, one
//! that is no ticket: the two are alike to it — ACCESS-3 #1); `waves`
//! places only tickets in reach, a hidden blocker still holding its waiter
//! back (its wave is the store's) but never named in `waiting_on` (#6);
//! `components` are those owned by a ticket in reach, their references cut
//! to what is in reach (#4); `groups` are the shortcodes it sees anything
//! of, a hidden root or host none (#13).

use super::seen::{Seen, Sees};
use super::*;
use crate::comments::Comment;
use std::collections::BTreeSet;

impl Store {
    /// MNG-10: every shortcode's roll-up ([`Store::groups_unscoped`]) — of
    /// the shortcodes `who` sees a ticket of. A root it may not see is no
    /// root (`root`, `root_row` none; the counts those of the shortcode's
    /// tickets it sees: ACCESS-1 decided 10 draws them at the shortcode's
    /// top), a host it may not see no host (the shortcode at the top). A
    /// reach that reaches nothing sees no group.
    pub fn groups(&self, who: &(impl Reader + ?Sized)) -> Vec<Group> {
        let Ok(seen) = Seen::of(who) else { return Vec::new() };
        if seen.all() {
            return self.groups_unscoped();
        }
        let cache = self.tickets.read().expect("store lock poisoned");
        let sees = seen.sees(&cache);
        let held = sees.shortcodes().unwrap_or_default();
        let mut out = Vec::new();
        for g in cache.rollups.groups() {
            let mut g = cache.dress(g);
            if !held.contains(&g.shortcode) {
                continue;
            }
            if g.host.as_deref().is_some_and(|h| !sees.id(h)) {
                g.host = None;
            }
            if g.root.as_deref().is_some_and(|r| !sees.id(r)) {
                (g.root, g.root_row) = (None, None);
                let (mut total, mut done, mut docs) = (0, 0, 0);
                for seq in sees.admitted(&g.shortcode).unwrap_or_default() {
                    let Some(t) = cache.order.get(&OrderKey { shortcode: g.shortcode.clone(), seq }) else { continue };
                    match t.kind.has_lifecycle() {
                        true => (total, done) = (total + 1, done + u64::from(t.status == Status::Complete)),
                        false => docs += 1,
                    }
                }
                (g.total, g.done, g.docs) = (total, done, docs);
            }
            out.push(g);
        }
        out
    }

    /// SCALE-16: every shortcode's highest sequence ([`Store::extents_unscoped`])
    /// — of the shortcodes `who` sees a ticket of, each its highest it sees.
    pub fn extents(&self, who: &(impl Reader + ?Sized)) -> Vec<(ShortcodeId, u32)> {
        let Ok(seen) = Seen::of(who) else { return Vec::new() };
        if seen.all() {
            return self.extents_unscoped();
        }
        let cache = self.tickets.read().expect("store lock poisoned");
        let sees = seen.sees(&cache);
        let held = sees.shortcodes().unwrap_or_default();
        cache
            .ids
            .last_seqs()
            .filter(|(sc, _)| held.contains(*sc))
            .filter_map(|(sc, _)| Some((sc.clone(), sees.admitted(sc)?.max()?)))
            .collect()
    }

    /// ACT-5: `extents` with holes ([`Store::extents_holed_unscoped`]) — of
    /// the shortcodes `who` sees a ticket of. One it sees whole is answered
    /// as it is; of one it sees part of, the highest sequence it sees and
    /// the deleted ones below it.
    pub fn extents_holed(&self, who: &(impl Reader + ?Sized)) -> Vec<(ShortcodeId, crate::address::Code, u32, Vec<u32>)> {
        let Ok(seen) = Seen::of(who) else { return Vec::new() };
        let all = self.extents_holed_unscoped();
        if seen.all() {
            return all;
        }
        let cache = self.tickets.read().expect("store lock poisoned");
        let sees = seen.sees(&cache);
        let held = sees.shortcodes().unwrap_or_default();
        all.into_iter()
            .filter(|(sc, ..)| held.contains(sc))
            .filter_map(|(sc, code, last, holes)| {
                let admitted = sees.admitted(&sc)?;
                if admitted.len() == cache.ids.held(&sc) {
                    return Some((sc, code, last, holes));
                }
                let last = admitted.max()?;
                let holes = cache.holes(&sc, last);
                Some((sc, code, last, holes))
            })
            .collect()
    }

    /// WAVE-4: every ticket `who` sees of the shortcodes `epics` (as `in`
    /// reads letters; one it sees nothing of contributes nothing, as one
    /// with no tickets) whose kind is in `kinds` — never a doc — placed in
    /// the order of work, with what it waits on, and the tally
    /// ([`waves::Waves`]). Each ticket's place is the store's (a blocker
    /// out of reach holds it back as any does); what it waits on names only
    /// tickets in reach. Rows only, under the read lock.
    pub fn waves(&self, who: &(impl Reader + ?Sized), epics: &[String], kinds: &[Kind]) -> waves::Waves {
        let Ok(seen) = Seen::of(who) else { return waves::Waves::default() };
        let cache = self.tickets.read().expect("store lock poisoned");
        let sees = seen.sees(&cache);
        let epics: BTreeSet<ShortcodeId> = epics.iter().flat_map(|e| cache.shortcodes.in_set(e)).collect();
        let tickets = epics.iter().flat_map(|epic| {
            let first = OrderKey { shortcode: epic.clone(), seq: 0 };
            let last = OrderKey { shortcode: epic.clone(), seq: u32::MAX };
            cache.order.range(first..=last).map(|(_, t)| t)
        });
        let mut w = waves::of(cache.rows(), tickets, kinds, &|t| sees.sees_row(t));
        redact_waves(&sees, &mut w);
        w
    }

    /// MENGER-31: the wave board of a ticket's subtree — `id` and every
    /// ticket under it (the kept tree, FILTER-13's `under`) — placed as
    /// [`Store::waves`] places a shortcode's, of the kinds `kinds`, never a
    /// doc, of the tickets `who` sees. [`StoreError::NotFound`] when `id`
    /// names no ticket it sees.
    pub fn waves_under(&self, who: &(impl Reader + ?Sized), id: &str, kinds: &[Kind]) -> Result<waves::Waves, StoreError> {
        let seen = Seen::of(who)?;
        let cache = self.tickets.read().expect("store lock poisoned");
        let sees = seen.sees(&cache);
        let id = sees.resolve(id)?;
        let row = cache.get(&id).ok_or_else(|| StoreError::NotFound(id.to_string()))?;
        let sub = lineage::subtree(&cache, &row.id, Some(row));
        let tickets = sub
            .iter()
            .flat_map(|(epic, seqs)| seqs.iter().filter_map(|seq| cache.order.get(&OrderKey { shortcode: epic.clone(), seq })));
        let mut w = waves::of(cache.rows(), tickets, kinds, &|t| sees.sees_row(t));
        redact_waves(&sees, &mut w);
        Ok(w)
    }

    /// MENGER-31: the wave board whose final state is `id` ([`waves::to`]):
    /// what must happen first, each where the store keeps it, the ticket
    /// alone in the last wave — of the tickets `who` sees (what it may not
    /// see still holds the ticket back, never named), the cycles among them
    /// those it sees. [`StoreError::NotFound`] when `id` names no ticket it
    /// sees.
    pub fn waves_to(&self, who: &(impl Reader + ?Sized), id: &str, kinds: &[Kind]) -> Result<waves::WavesTo, StoreError> {
        let seen = Seen::of(who)?;
        let cache = self.tickets.read().expect("store lock poisoned");
        let sees = seen.sees(&cache);
        let id = sees.resolve(id)?;
        let row = cache.get(&id).ok_or_else(|| StoreError::NotFound(id.to_string()))?;
        if row.kind == Kind::Doc {
            return Err(StoreError::DocHasNoLifecycle { id: row.id.to_string() });
        }
        let sub = lineage::subtree(&cache, &row.id, None);
        let descendants = sub
            .iter()
            .flat_map(|(epic, seqs)| seqs.iter().filter_map(|seq| cache.order.get(&OrderKey { shortcode: epic.clone(), seq })))
            .filter(|t| t.id != row.id);
        let mut to = waves::to(cache.rows(), row, descendants, kinds, &|t| sees.sees_row(t));
        redact_waves(&sees, &mut to.waves);
        if !sees.all() {
            to.cycles = to.cycles.into_iter().map(|c| sees.keep(c)).filter(|c| !c.is_empty()).collect();
        }
        Ok(to)
    }

    /// One ticket, whole ([`Store::get_unscoped`]) — when `who` sees it: one
    /// it may not see is [`StoreError::NotFound`], as one that is not there
    /// (IDENTITY-37), and an ambiguous name's candidates are those in reach.
    /// Redacted ([`Sees::ticket`]); ACT-6's `moved_from` keeps the old ids
    /// of shortcodes `who` sees a ticket of.
    pub async fn get(&self, who: &(impl Reader + ?Sized), id: &str) -> Result<Ticket, StoreError> {
        let seen = Seen::of(who)?;
        let (row, written, moved_from) = {
            let cache = self.tickets.read().expect("store lock poisoned");
            let key = seen.sees(&cache).resolve(id)?;
            let row = cache.get(&key).cloned().ok_or_else(|| StoreError::NotFound(id.trim().to_string()))?;
            let moved_from = cache.shortcodes.moved_from(&row.id);
            (row, cache.written, moved_from)
        };
        let mut ticket = self.full_one(row, written).await?;
        ticket.moved_from = moved_from;
        if !seen.all() {
            let cache = self.tickets.read().expect("store lock poisoned");
            let sees = seen.sees(&cache);
            sees.ticket(&mut ticket);
            let held = sees.shortcodes().unwrap_or_default();
            ticket.moved_from.retain(|old| old.rsplit_once('-').is_some_and(|(sc, _)| held.contains(&ShortcodeId::new(sc))));
        }
        Ok(ticket)
    }

    /// ACT-5: the tombstone of the deleted ticket `id` names — when it was
    /// in a shortcode `who` sees a ticket of (a deleted ticket is in no
    /// reach of its own: its shortcode's is the word, as `watch` says it).
    pub fn tombstone(&self, who: &(impl Reader + ?Sized), id: &str) -> Option<crate::types::Tombstone> {
        let seen = Seen::of(who).ok()?;
        let t = self.tombstone_unscoped(id)?;
        if seen.all() {
            return Some(t);
        }
        let cache = self.tickets.read().expect("store lock poisoned");
        let held = seen.sees(&cache).shortcodes().unwrap_or_default();
        let (sc, _) = cache.tombstones.place_of(&t.id)?;
        held.contains(sc).then_some(t)
    }

    /// Tickets ordered by (epic, seq), optionally filtered, whole — the
    /// unpaged shape: [`Store::walk`] of `list`'s selection, collected.
    pub async fn list(&self, who: &(impl Reader + ?Sized), epic: Option<&str>, status: Option<Status>, parent: Option<&str>) -> Result<Vec<Ticket>, StoreError> {
        let select = Select::List { epic: epic.map(String::from), status, parent: parent.map(String::from) };
        self.collect(self.walk(who, select, None, None)).await
    }

    /// Tickets that can start now ([`ready::Ready`]'s rule), whole, in the
    /// store's order — of those `who` sees: [`Store::walk`] of the ready
    /// set, collected.
    pub async fn ready_set(&self, who: &(impl Reader + ?Sized), epic: Option<&str>) -> Result<Vec<Ticket>, StoreError> {
        self.collect(self.walk(who, Select::Ready { epic: epic.map(String::from) }, None, None)).await
    }

    /// MNG-9: one page of the store's order ([`Store::page_unscoped`]) — of
    /// the tickets `who` sees: a restricted reader's page is the filtered
    /// page of its reach (with `first`, as IDENTITY-21 answered it), rows
    /// redacted, a cursor it may not see a cursor into nothing.
    pub fn page(&self, who: &(impl Reader + ?Sized), q: PageQuery<'_>) -> Result<Page, StoreError> {
        let seen = Seen::of(who)?;
        if seen.all() {
            return self.page_unscoped(q);
        }
        let select = Select::List { epic: q.epic.map(str::to_string), status: q.status, parent: q.parent.map(str::to_string) };
        self.seen_page(&seen, select, q.after, q.limit, None)
    }

    /// MNG-9: one page of the ready set ([`Store::ready_page_unscoped`]) — of
    /// the tickets `who` sees.
    pub fn ready_page(&self, who: &(impl Reader + ?Sized), epic: Option<&str>, after: Option<&str>, limit: Option<u32>) -> Result<Page, StoreError> {
        let seen = Seen::of(who)?;
        if seen.all() {
            return self.ready_page_unscoped(epic, after, limit);
        }
        let page = self.seen_page(&seen, Select::Ready { epic: epic.map(str::to_string) }, after, limit, None)?;
        Ok(Page { first: None, ..page })
    }

    /// SCALE-6: one page of the tickets whose id or title holds `query`
    /// ([`Store::find_page_unscoped`]) — of those `who` sees: a restricted
    /// reader's is `find`'s filtered page of its reach.
    pub fn find_page(&self, who: &(impl Reader + ?Sized), query: &str, after: Option<&str>, limit: Option<u32>) -> Result<Page, StoreError> {
        let seen = Seen::of(who)?;
        match seen.lens() {
            None => self.find_page_unscoped(query, after, limit),
            Some(reach) => self.find_seen(&seen, query, reach, after, limit),
        }
    }

    /// FILTER-15: `find { query, filter }` ([`Store::find_filtered_unscoped`])
    /// — `lens` within `who`'s reach.
    pub fn find_filtered(&self, who: &(impl Reader + ?Sized), query: &str, lens: &Lens, after: Option<&str>, limit: Option<u32>) -> Result<Page, StoreError> {
        let seen = Seen::of(who)?;
        let lens = seen.within(lens)?;
        self.find_seen(&seen, query, &lens, after, limit)
    }

    /// `find` under `lens` (already within the reach): its cursor checked,
    /// its rows redacted.
    fn find_seen(&self, seen: &Seen, query: &str, lens: &Lens, after: Option<&str>, limit: Option<u32>) -> Result<Page, StoreError> {
        self.cursor_seen(seen, after)?;
        let mut page = self.find_filtered_unscoped(query, lens, after, limit)?;
        self.redact_rows(seen, &mut page.tickets);
        Ok(page)
    }

    /// FILTER-4: a filtered page ([`Store::filtered_page_unscoped`]) — `q`
    /// within `who`'s reach ([`Seen::filtered`]: its `keep` only when in
    /// reach), the cursor one it sees, the rows redacted.
    pub fn filtered_page(&self, who: &(impl Reader + ?Sized), q: &Filtered, after: Option<&str>, at: Option<u64>, limit: Option<u32>) -> Result<Page, StoreError> {
        let seen = Seen::of(who)?;
        if seen.all() {
            return self.filtered_page_unscoped(q, after, at, limit);
        }
        self.seen_page(&seen, Select::Filtered(q.clone()), after, limit, at)
    }

    /// FILTER-15: a page of `ready_set { epic, filter }` — `q` from
    /// [`ready_and`] — as [`Store::filtered_page`], with no `first`.
    pub fn ready_filtered_page(&self, who: &(impl Reader + ?Sized), q: &Filtered, after: Option<&str>, limit: Option<u32>) -> Result<Page, StoreError> {
        let page = self.filtered_page(who, q, after, None, limit)?;
        Ok(Page { first: None, ..page })
    }

    /// A restricted reader's page of `select` (as a walk takes it, within
    /// the reach), cut once under the read lock: the cursor one it sees,
    /// the rows redacted.
    fn seen_page(&self, seen: &Seen, select: Select, after: Option<&str>, limit: Option<u32>, at: Option<u64>) -> Result<Page, StoreError> {
        let after = after.map(str::to_ascii_uppercase);
        let cache = self.tickets.read().expect("store lock poisoned");
        let sees = seen.sees(&cache);
        sees.cursor(after.as_deref())?;
        let Select::Filtered(q) = seen.select(&cache, select)? else { unreachable!("a restricted reader's selection is filtered") };
        let mut page = Self::cut_filtered(&cache, &q, after.as_deref(), at, limit, Rest::Count)?;
        sees.rows(&mut page.tickets);
        Ok(page)
    }

    /// A read's `after` as `seen` may name it: a cursor it may not see is a
    /// cursor into nothing (ACCESS-3 #8).
    fn cursor_seen(&self, seen: &Seen, after: Option<&str>) -> Result<(), StoreError> {
        if seen.all() {
            return Ok(());
        }
        let cache = self.tickets.read().expect("store lock poisoned");
        seen.sees(&cache).cursor(after.map(str::to_ascii_uppercase).as_deref())
    }

    /// `rows` redacted as `seen` reads them.
    fn redact_rows(&self, seen: &Seen, rows: &mut [Row]) {
        if seen.all() {
            return;
        }
        let cache = self.tickets.read().expect("store lock poisoned");
        seen.sees(&cache).rows(rows);
    }

    /// One ticket's blockers — each resolved whole, or (to a reader of
    /// everything) dangling — of `id`, when `who` sees it
    /// ([`Store::blockers_unscoped`]). A blocker it may not see is not
    /// answered, nor, to a restricted reader, one that is no ticket: both
    /// are a blocker it is not shown (the ticket's `hidden_blocked_by`
    /// counts them). ACCESS-3 #1: a hidden blocker's body is never read.
    pub async fn blockers(&self, who: &(impl Reader + ?Sized), id: &str) -> Result<Vec<BlockerRef>, StoreError> {
        let seen = Seen::of(who)?;
        let key = {
            let cache = self.tickets.read().expect("store lock poisoned");
            let key = seen.sees(&cache).resolve(id)?;
            if cache.get(&key).is_none() {
                return Err(StoreError::NotFound(id.to_string()));
            }
            key
        };
        if seen.all() {
            return self.blockers_unscoped(&key).await;
        }
        let (rows, written) = {
            let cache = self.tickets.read().expect("store lock poisoned");
            let sees = seen.sees(&cache);
            let ticket = cache.get(&key).ok_or_else(|| StoreError::NotFound(id.to_string()))?;
            let rows: Vec<Row> = ticket.blocked_by.iter().filter(|b| sees.id(b)).filter_map(|b| cache.get(b).cloned()).collect();
            (rows, cache.written)
        };
        let mut tickets = self.full(rows, written).await?;
        let cache = self.tickets.read().expect("store lock poisoned");
        let sees = seen.sees(&cache);
        tickets.iter_mut().for_each(|t| sees.ticket(t));
        Ok(tickets.into_iter().map(BlockerRef::Resolved).collect())
    }

    /// How many tickets `who` sees.
    pub fn count(&self, who: &(impl Reader + ?Sized)) -> usize {
        let Ok(seen) = Seen::of(who) else { return 0 };
        match seen.lens() {
            None => self.count_unscoped(),
            Some(reach) => self.counts_unscoped(reach).into_iter().map(|(_, n)| n as usize).sum(),
        }
    }

    /// COMP-3: the component registry ([`Store::components_unscoped`]) — to
    /// a restricted reader, the components owned by a ticket it sees (an
    /// orphan is no one's to show), each with the references it sees: the
    /// tickets in reach, and the components it is shown (ACCESS-3 #4).
    pub fn components(&self, who: &(impl Reader + ?Sized)) -> Vec<Component> {
        let Ok(seen) = Seen::of(who) else { return Vec::new() };
        let components = self.components_unscoped();
        if seen.all() {
            return components;
        }
        let cache = self.tickets.read().expect("store lock poisoned");
        let sees = seen.sees(&cache);
        let shown = |owner: &Option<String>| owner.as_deref().is_some_and(|o| sees.id(o));
        let owners: HashMap<String, bool> = components.iter().map(|c| (c.name.clone(), shown(&c.owner))).collect();
        components
            .into_iter()
            .filter(|c| owners.get(&c.name).copied().unwrap_or(false))
            .map(|mut c| {
                c.references.retain(|r| match owners.get(r) {
                    Some(seen) => *seen,
                    None => sees.id(r),
                });
                c
            })
            .collect()
    }

    /// INPUT-3: every input of `ticket` (every ticket's, without one) in the
    /// shortcode `epic` and in `state` — of the tickets `who` sees. A
    /// `ticket` naming none it sees is [`StoreError::NotFound`]. From the
    /// cache alone.
    pub fn inputs(&self, who: &(impl Reader + ?Sized), ticket: Option<&str>, epic: Option<&str>, state: Option<InputState>) -> Result<Vec<Input>, StoreError> {
        let seen = Seen::of(who)?;
        let cache = self.tickets.read().expect("store lock poisoned");
        let sees = seen.sees(&cache);
        // ORG-3: the ticket by any address; the shortcode by its letters or
        // address, one naming several refused.
        let ticket = ticket.map(|t| sees.resolve(t)).transpose()?;
        let epic = epic.map(|e| span_shortcode(&cache, Some(e))).transpose()?.flatten();
        Ok(match sees.all() {
            true => cache.inputs.query(ticket.as_deref(), epic.as_ref(), state),
            false => cache.inputs.query_where(ticket.as_deref(), epic.as_ref(), state, |id| sees.id(id)),
        })
    }

    /// FILTER-15: `inputs { …, filter }` ([`Store::inputs_filtered_unscoped`])
    /// — `lens` within `who`'s reach, a `ticket` it may not see not found.
    pub fn inputs_filtered(&self, who: &(impl Reader + ?Sized), ticket: Option<&str>, epic: Option<&str>, state: Option<InputState>, lens: &Lens) -> Result<Vec<Input>, StoreError> {
        let seen = Seen::of(who)?;
        let ticket = match ticket {
            Some(t) if !seen.all() => {
                let cache = self.tickets.read().expect("store lock poisoned");
                Some(seen.sees(&cache).resolve(t)?)
            }
            t => t.map(str::to_string),
        };
        self.inputs_filtered_unscoped(ticket.as_deref(), epic, state, &seen.within(lens)?)
    }

    /// COMMENT-2: one page of the comments `q` admits, newest first
    /// ([`comments::page`]) — of those `who` sees ([`Sees::comment`]),
    /// judged as the page is cut: a short page never stands for a hidden
    /// comment, and a cursor naming one is a cursor into nothing.
    pub fn comments(&self, who: &(impl Reader + ?Sized), q: &CommentQuery, after: Option<u64>, limit: Option<u32>) -> Result<CommentPage, StoreError> {
        let seen = Seen::of(who)?;
        let cache = self.tickets.read().expect("store lock poisoned");
        let sees = seen.sees(&cache);
        comments::page(&cache, q, after, limit, &|c: &Comment| sees.comment(c))
    }

    /// COMMENT-2: one comment as the cache holds it — when `who` sees it;
    /// else [`StoreError::CommentNotFound`], the newest comment it sees
    /// beside it, as for an id that names none (ACCESS-3 #7).
    pub fn comment(&self, who: &(impl Reader + ?Sized), id: u64) -> Result<Comment, StoreError> {
        let seen = Seen::of(who)?;
        let cache = self.tickets.read().expect("store lock poisoned");
        let sees = seen.sees(&cache);
        match cache.comments.get(id) {
            Some(c) if sees.comment(c) => Ok(c.clone()),
            _ => {
                let newest = cache.comments.newest_where(|c| sees.comment(c));
                Err(StoreError::CommentNotFound { id, newest })
            }
        }
    }

    /// IDENTITY-21: a principal's stored grants — the access layer's own
    /// read, as the system.
    pub fn grants_of(&self, _: &System, principal: &str) -> Vec<crate::members::Grant> {
        self.grants_of_unscoped(principal)
    }

    /// IDENTITY-21: every grant, by principal then scope — the access
    /// layer's own read, as the system.
    pub fn grants(&self, _: &System) -> Vec<crate::members::Grant> {
        self.grants_unscoped()
    }

    /// MENGER-13: `id`'s history ([`Store::history_unscoped`]) — when `who`
    /// sees it (a deleted ticket's when it saw its shortcode, as its
    /// tombstone): the transitions it caused and the descendants a restore
    /// puts back, those `who` sees.
    pub fn history(&self, who: &(impl Reader + ?Sized), id: &str) -> Result<cascade::HistoryAnswer, StoreError> {
        let seen = Seen::of(who)?;
        if !seen.all() {
            let cache = self.tickets.read().expect("store lock poisoned");
            let sees = seen.sees(&cache);
            // As the unscoped read names it: a key, or the id as written.
            let key = match cache.find_ticket(id, shortcodes::OrgReach::All) {
                shortcodes::Found::One(key) => key,
                _ => crate::address::folded(id),
            };
            let seen_it = match cache.get(&key) {
                Some(_) => sees.id(&key),
                None => cache.tombstones.place_of(&key).is_some_and(|(sc, _)| sees.shortcodes().unwrap_or_default().contains(sc)),
            };
            if !seen_it {
                return Err(StoreError::NotFound(key));
            }
        }
        let mut h = self.history_unscoped(id)?;
        if !seen.all() {
            let cache = self.tickets.read().expect("store lock poisoned");
            let sees = seen.sees(&cache);
            h.caused.retain(|t| sees.id(&t.ticket));
            h.restorable.retain(|t| sees.id(t));
        }
        Ok(h)
    }

    /// FILTER-15: `member { ids, filter }` — for each of `ids`, whether
    /// `lens` admits it within `who`'s reach (an id it may not see is not
    /// admitted, as one naming no ticket). A reach that reaches nothing
    /// admits none.
    pub fn member(&self, who: &(impl Reader + ?Sized), ids: &[String], lens: &Lens) -> Vec<bool> {
        match Seen::of(who).and_then(|seen| seen.within(lens)) {
            Ok(lens) => self.member_unscoped(ids, &lens),
            Err(_) => vec![false; ids.len()],
        }
    }

    /// FILTER-4: `counts { filter }` ([`Store::counts_unscoped`]) — `lens`
    /// within `who`'s reach. A reach that reaches nothing counts nothing.
    pub fn counts(&self, who: &(impl Reader + ?Sized), lens: &Lens) -> Vec<(ShortcodeId, u64)> {
        self.counts_keeping(who, lens, None)
    }

    /// FILTER-24: [`Store::counts`] with `keep` — counted only when `who`
    /// sees it.
    pub fn counts_keeping(&self, who: &(impl Reader + ?Sized), lens: &Lens, keep: Option<&str>) -> Vec<(ShortcodeId, u64)> {
        let Ok(seen) = Seen::of(who) else { return Vec::new() };
        let Ok(lens) = seen.within(lens) else { return Vec::new() };
        let keep = self.kept_seen(&seen, keep);
        self.counts_keeping_unscoped(&lens, keep.as_deref())
    }

    /// FILTER-24: [`Store::counts_keeping`], a filter `under` names refused
    /// ([`Store::counts_read_unscoped`]).
    pub fn counts_read(&self, who: &(impl Reader + ?Sized), lens: &Lens, keep: Option<&str>) -> Result<Vec<(ShortcodeId, u64)>, StoreError> {
        let seen = Seen::of(who)?;
        let lens = seen.within(lens)?;
        let keep = self.kept_seen(&seen, keep);
        self.counts_read_unscoped(&lens, keep.as_deref())
    }

    /// A `keep` (FILTER-24) as `seen` may keep it: a ticket it sees, by its
    /// key; else none.
    fn kept_seen(&self, seen: &Seen, keep: Option<&str>) -> Option<String> {
        let keep = keep?;
        if seen.all() {
            return Some(keep.to_string());
        }
        let cache = self.tickets.read().expect("store lock poisoned");
        let t = filtered::keep_row(&cache, keep)?;
        seen.sees(&cache).sees_row(t).then(|| t.id.to_string())
    }

    /// FILTER-4: `facets { filter, base }` ([`Store::facets_at_unscoped`]) —
    /// both within `who`'s reach.
    pub fn facets_at(&self, who: &(impl Reader + ?Sized), filter: &crate::filter::Filter, base: &crate::filter::Filter, clock: &history::Clock) -> Result<Vec<(String, String, u64)>, StoreError> {
        let seen = Seen::of(who)?;
        match seen.filter() {
            None => self.facets_at_unscoped(filter, base, clock),
            Some(reach) => self.facets_at_unscoped(&filter.and(reach), &base.and(reach), clock),
        }
    }

    /// [`Store::facets_at`] at the moment, in UTC.
    pub fn facets(&self, who: &(impl Reader + ?Sized), filter: &crate::filter::Filter, base: &crate::filter::Filter) -> Result<Vec<(String, String, u64)>, StoreError> {
        self.facets_at(who, filter, base, &Default::default())
    }

    /// SEARCH-2: `search { query, filter, limit, offset }`
    /// ([`Store::search_unscoped`]) — of the tickets `who` sees: `lens`
    /// within its reach (its reach alone without one), the hits redacted.
    pub async fn search(&self, who: &(impl Reader + ?Sized), query: &str, lens: Option<Lens>, limit: Option<u32>, offset: Option<u32>) -> Result<SearchPage, StoreError> {
        let seen = Seen::of(who)?;
        let lens = match (lens, seen.lens()) {
            (l, None) => l,
            (None, Some(reach)) => Some(reach.clone()),
            (Some(l), Some(_)) => Some(seen.within(&l)?),
        };
        let mut page = self.search_unscoped(query, lens, limit, offset).await?;
        if !seen.all() {
            let cache = self.tickets.read().expect("store lock poisoned");
            let sees = seen.sees(&cache);
            page.hits.iter_mut().for_each(|h| sees.row(&mut h.summary));
        }
        Ok(page)
    }
}

/// A wave answer as `sees` may read it: each placed row redacted, and what
/// it waits on cut to the tickets in reach (a hidden blocker still holds
/// its place: the ticket's `hidden_blocked_by` counts it).
fn redact_waves(sees: &Sees<'_>, w: &mut waves::Waves) {
    if sees.all() {
        return;
    }
    for p in &mut w.placed {
        sees.row(&mut p.summary);
        p.waiting_on.retain(|id| sees.id(id));
    }
}
