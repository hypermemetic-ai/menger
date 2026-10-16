//! ACCESS-12: **what an open watch hears** — a change as its watcher's
//! reach may hear it, and what a reach that moved did to the watch.
//!
//! A watch holds its reader's reach compiled once ([`WatchSight`]: the
//! [`Seen`] every read compiles for itself), and asks the store about each
//! change ([`Store::heard`]) under one read lock: the change is
//! **classified** — about one ticket, about several, about a shortcode,
//! about a component (its owner), or the watch's own (a write's cell
//! moves) — by one `match` with no `_` arm, so a new [`Change`] does not
//! compile until it is placed (ACCESS-3 #4, #5, #13). A change about a
//! ticket the reach hides is not heard; one about several is cut to those
//! it admits; every row it carries is redacted exactly as a read's is
//! ([`Sees::ticket`], [`Sees::row`]: the hidden ids out of `blocked_by`,
//! `unlocks` and `parent`, counted in `hidden_*`), and a hidden id it names
//! beside them is dropped (a list) or blanked (one name: `""`).
//!
//! When the grants move and the reach with them ([`Store::moved_between`]),
//! the tickets that entered or left what the watch hears are answered as a
//! filtered watch's write is: `Entered` / `Left`, or past
//! [`RECOUNT_OVER`] one `Recount` per shortcode.
//!
//! The cost of a change is the reach's view compiled once against the
//! cache (as one read's is) — only when the reach is not everything and the
//! change names a ticket; nothing scans the store.

use super::cells::RECOUNT_OVER;
use super::seen::Sees;
use super::*;

/// An open watch's reach, compiled once — rebuilt only when the grants move
/// it ([`crate::access::Following`]).
#[derive(Debug, Clone)]
pub struct WatchSight {
    seen: Seen,
}

impl WatchSight {
    /// Whether it hides nothing.
    pub fn all(&self) -> bool {
        self.seen.all()
    }

    /// The reach as a lens (`None`: everything).
    pub fn lens(&self) -> Option<&Lens> {
        self.seen.lens()
    }
}

impl Store {
    /// ACCESS-12: `who`'s reach for a watch, compiled — a refusal when it
    /// reaches nothing.
    pub fn watch_sight(&self, who: &(impl Reader + ?Sized)) -> Result<WatchSight, StoreError> {
        Ok(WatchSight { seen: Seen::of(who)? })
    }

    /// ACCESS-12: `change` as a watch whose reach is `sight` may hear it —
    /// itself, the part of it the reach admits (its rows redacted), or
    /// nothing. A component's change is heard when the reach admits its
    /// owner (its references cut to the tickets and components it admits);
    /// one it does not is said as the component's removal, which the watch
    /// passes on only if it had been shown the component.
    pub fn heard(&self, sight: &WatchSight, change: Change) -> Option<Change> {
        if sight.all() {
            return Some(change);
        }
        let cache = self.tickets.read().expect("store lock poisoned");
        let sees = sight.seen.sees(&cache);
        cut(&cache, &sees, change)
    }

    /// ACCESS-12: what a watch heard through `before` that it does not
    /// through `after`, and back — each the watch's whole lens (its filter
    /// within its reach; `None` everything) as its reach moved; in `epic`
    /// alone when one is asked: `Entered` / `Left` per ticket, in the
    /// store's order, or past [`RECOUNT_OVER`] one `Recount` per shortcode
    /// moved, with what it holds now.
    pub fn moved_between(&self, before: Option<&Lens>, after: Option<&Lens>, epic: Option<&ShortcodeId>) -> Vec<Membership> {
        if before == after {
            return Vec::new();
        }
        let (before, after) = (before.cloned(), after.cloned());
        let cache = self.tickets.read().expect("store lock poisoned");
        let bitmaps = |l: &Option<Lens>| -> BTreeMap<ShortcodeId, roaring::RoaringBitmap> {
            let all = Lens::all();
            let q = Filtered { lens: l.clone().unwrap_or(all), epic: None, status: None, parent: None, keep: None };
            let view = filtered::view_lenient(&cache, &q);
            view.shortcodes_from(None)
                .iter()
                .filter(|(e, _)| epic.is_none_or(|x| x == *e))
                .map(|(e, bms)| {
                    let mut u = roaring::RoaringBitmap::new();
                    for b in bms {
                        u |= b.as_ref();
                    }
                    ((*e).clone(), u)
                })
                .collect()
        };
        let (b, a) = (bitmaps(&before), bitmaps(&after));
        let empty = roaring::RoaringBitmap::new();
        let mut shortcodes: Vec<&ShortcodeId> = b.keys().chain(a.keys()).collect();
        shortcodes.sort();
        shortcodes.dedup();
        let moved: Vec<(&ShortcodeId, roaring::RoaringBitmap, roaring::RoaringBitmap)> = shortcodes
            .into_iter()
            .map(|e| {
                let (was, is) = (b.get(e).unwrap_or(&empty), a.get(e).unwrap_or(&empty));
                (e, is - was, was - is)
            })
            .filter(|(_, entered, left)| !entered.is_empty() || !left.is_empty())
            .collect();
        let n: u64 = moved.iter().map(|(_, en, le)| en.len() + le.len()).sum();
        if n as usize > RECOUNT_OVER {
            return moved.into_iter().map(|(e, ..)| Membership::Recount { shortcode: e.clone(), held: a.get(e).map_or(0, |m| m.len()) }).collect();
        }
        let id = |e: &ShortcodeId, seq: u32| cache.order.get(&OrderKey { shortcode: e.clone(), seq }).map_or_else(|| e.key(seq), |t| t.id.to_string());
        let mut out = Vec::new();
        for (e, entered, left) in moved {
            let mut seqs: Vec<(u32, bool)> = entered.iter().map(|s| (s, true)).chain(left.iter().map(|s| (s, false))).collect();
            seqs.sort();
            for (seq, now) in seqs {
                let (id, shortcode) = (id(e, seq), e.clone());
                out.push(if now { Membership::Entered { id, shortcode, seq } } else { Membership::Left { id, shortcode, seq } });
            }
        }
        out
    }
}

/// ACCESS-12: the one classification. **No `_` arm**: a new [`Change`]
/// does not compile until it is placed here — about one ticket, several, a
/// shortcode, a component, or the watch's own — and cut to the reach as a
/// read would answer it.
fn cut(cache: &Cache, sees: &Sees<'_>, change: Change) -> Option<Change> {
    // One name the reach may hide, beside a row it shows: blanked.
    let name = |id: String| if sees.id(&id) { id } else { String::new() };
    // ACCESS-48: a blocker's name, the same way.
    let named = |id: TicketName| if sees.id(&id) { id } else { TicketName::new(String::new()) };
    let row = |mut t: Ticket| {
        sees.ticket(&mut t);
        t
    };
    match change {
        // ── About one ticket: heard when the reach admits it; its row
        // redacted, and any other ticket it names cut.
        Change::Created { ticket } => sees.id(&ticket.id).then(|| Change::Created { ticket: row(ticket) }),
        Change::StatusChanged { ticket, previous } => sees.id(&ticket.id).then(|| Change::StatusChanged { ticket: row(ticket), previous }),
        Change::KindChanged { ticket, previous } => sees.id(&ticket.id).then(|| Change::KindChanged { ticket: row(ticket), previous }),
        Change::Amended { ticket, changed } => sees.id(&ticket.id).then(|| Change::Amended { ticket: row(ticket), changed }),
        Change::CommitsLinked { ticket, added } => sees.id(&ticket.id).then(|| Change::CommitsLinked { ticket: row(ticket), added }),
        Change::CommitsUnlinked { ticket, removed } => sees.id(&ticket.id).then(|| Change::CommitsUnlinked { ticket: row(ticket), removed }),
        Change::LabelsChanged { ticket, added, removed } => sees.id(&ticket.id).then(|| Change::LabelsChanged { ticket: row(ticket), added, removed }),
        Change::SeverityChanged { ticket, previous } => sees.id(&ticket.id).then(|| Change::SeverityChanged { ticket: row(ticket), previous }),
        // The waiter it gained or lost may be hidden: its row says so in
        // `hidden_unlocks`, and the name is blanked (ACCESS-3 #13).
        Change::UnlocksChanged { ticket, added } => sees.id(&ticket.id).then(|| Change::UnlocksChanged { ticket: row(ticket), added: named(added) }),
        Change::UnlocksRemoved { ticket, removed } => sees.id(&ticket.id).then(|| Change::UnlocksRemoved { ticket: row(ticket), removed: named(removed) }),
        Change::BlockersChanged { ticket, added, removed } => {
            sees.id(&ticket.id).then(|| Change::BlockersChanged { ticket: row(ticket), added: sees.keep(added), removed: sees.keep(removed) })
        }
        // The parent it had may be out of reach: none, as a read says it.
        Change::ParentChanged { ticket, previous } => {
            sees.id(&ticket.id).then(|| Change::ParentChanged { ticket: row(ticket), previous: previous.filter(|p| sees.id(p)) })
        }
        // A parent's: the child it gained is in reach with it (a reach is
        // closed under the tree); one it lost may have moved out of it.
        Change::ChildrenChanged { id, shortcode, added, child_count } => {
            sees.id(&id).then(|| Change::ChildrenChanged { id, shortcode, added: name(added), child_count })
        }
        Change::ChildRemoved { id, shortcode, removed, child_count } => {
            sees.id(&id).then(|| Change::ChildRemoved { id, shortcode, removed: name(removed), child_count })
        }
        Change::InputChanged { shortcode, input } => sees.id(input.ticket.as_str()).then_some(Change::InputChanged { shortcode, input }),
        // MENGER-13: a transition is its ticket's; the ticket that caused
        // it may be hidden.
        Change::Transition { shortcode, mut transition } => sees.id(transition.ticket.as_str()).then(|| {
            if transition.cause_ticket.as_deref().is_some_and(|c| !sees.id(c)) {
                transition.cause_ticket = None;
            }
            Change::Transition { shortcode, transition }
        }),
        // MENGER-39: a queue write is its ticket's; its row redacted.
        Change::Queue { shortcode, change } => {
            let change = match change {
                queue::QueueChange::Queued { entry, mut ticket, action } => sees.id(&entry.ticket).then(|| {
                    sees.row(&mut ticket);
                    queue::QueueChange::Queued { entry, ticket, action }
                }),
                queue::QueueChange::Unqueued { id, entry, action, by, at_unix } => {
                    sees.id(&entry.ticket).then_some(queue::QueueChange::Unqueued { id, entry, action, by, at_unix })
                }
                queue::QueueChange::Snoozed { entry, mut ticket } => sees.id(&entry.ticket).then(|| {
                    sees.row(&mut ticket);
                    queue::QueueChange::Snoozed { entry, ticket }
                }),
            }?;
            Some(Change::Queue { shortcode, change })
        }

        // ── About several tickets: of those the reach admits.
        Change::PlacementsChanged { changes } => {
            let changes: Vec<_> = changes.into_iter().filter(|(_, c)| sees.id(&c.id)).collect();
            (!changes.is_empty()).then_some(Change::PlacementsChanged { changes })
        }
        Change::UpdatedMany { ids, changed } => {
            let ids = sees.keep(ids);
            (!ids.is_empty()).then_some(Change::UpdatedMany { ids, changed })
        }
        // ACT-5: a deleted ticket is in no reach any more — its shortcode's
        // is the reach's word (as `tombstone` answers); what its tombstone
        // names beside it, cut.
        Change::Deleted { ids, tombstones } => {
            let held = sees.shortcodes().unwrap_or_default();
            let seen = |id: &str| id.rsplit_once('-').is_some_and(|(e, _)| held.iter().any(|h| h.as_str() == e));
            let ids: Vec<TicketId> = ids.into_iter().filter(|id| seen(id)).collect();
            let tombstones = tombstones
                .into_iter()
                .filter(|t| seen(&t.id))
                .map(|mut t| {
                    t.parent = t.parent.filter(|p| sees.id(p));
                    t.reparented = sees.keep(std::mem::take(&mut t.reparented));
                    t.unblocked = sees.keep(std::mem::take(&mut t.unblocked));
                    t
                })
                .collect();
            (!ids.is_empty()).then_some(Change::Deleted { ids, tombstones })
        }
        Change::Undeleted { ids, tickets } => {
            let ids = sees.keep(ids);
            let tickets: Vec<Ticket> = tickets.into_iter().filter(|t| sees.id(&t.id)).map(row).collect();
            (!ids.is_empty()).then_some(Change::Undeleted { ids, tickets })
        }
        // ARCHIVE-2: of the tickets the reach holds.
        Change::Archived { ids, live, unqueued, mark } => {
            let ids = sees.keep(ids);
            let live = live.into_iter().filter(|l| sees.id(&l.id)).collect();
            (!ids.is_empty()).then(|| Change::Archived { ids, live, unqueued: sees.keep(unqueued), mark })
        }
        Change::Restored { ids, ancestors, still_archived, by } => {
            let ids = sees.keep(ids);
            (!ids.is_empty()).then(|| Change::Restored { ids, ancestors: sees.keep(ancestors), still_archived: sees.keep(still_archived), by })
        }
        // ACT-6: a move, of the tickets the reach holds where they are now.
        Change::Reshortcoded { moves, tickets } => {
            let moves: Vec<crate::types::ShortcodeMove> = moves.into_iter().filter(|m| sees.id(&m.to)).collect();
            let tickets: Vec<Ticket> = tickets.into_iter().filter(|t| sees.id(&t.id)).map(row).collect();
            (!moves.is_empty()).then_some(Change::Reshortcoded { moves, tickets })
        }

        // ── About a shortcode: its roll-up, heard when the reach admits
        // its root — as `groups` answers it, a host out of reach is none.
        Change::GroupChanged { mut group } => {
            if !group.root.as_deref().is_some_and(|r| sees.id(r)) {
                return None;
            }
            if group.host.as_deref().is_some_and(|h| !sees.id(h)) {
                group.host = None;
            }
            Some(Change::GroupChanged { group })
        }

        // ── About a component: its owner's — references cut to what the
        // reach admits; one it cannot see said as gone (the watch passes a
        // removal on only of a component it had shown).
        Change::ComponentChanged { mut component } => {
            if !component.owner.as_deref().is_some_and(|o| sees.id(o)) {
                return Some(Change::ComponentRemoved { name: component.name });
            }
            component.references.retain(|r| match cache.components.owner_of(r) {
                Some(owner) => owner.is_some_and(|o| sees.id(o)),
                None => sees.id(r),
            });
            Some(Change::ComponentChanged { component })
        }
        Change::ComponentRemoved { name } => Some(Change::ComponentRemoved { name }),

        // ── The watch's own: a write's cell moves, which a filtered watch's
        // watcher turns into membership under a lens that holds the reach.
        Change::CellsMoved { moves } => Some(Change::CellsMoved { moves }),
        Change::UnderMoved { moves } => Some(Change::UnderMoved { moves }),
    }
}
