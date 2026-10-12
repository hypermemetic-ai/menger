//! SCALE-24: the row cache — one signal per ticket, shared by every view
//! that shows it, held while a view shows it.
//!
//! Until SCALE-24 a row lived in exactly one place, the board's list
//! (`main.rs`'s `Vec<Row>`: every ticket the walk had found so far), and
//! every view found its ticket there — the nav planned its forest over the
//! whole list, the ticket page scanned it for its own id, the menu too. That
//! is why each of them needed the walk to have finished before it was
//! right. The nav now shows tickets the walk may not have reached (a page of
//! a root's 163,530 children, a deep link's ancestors at 10^6), fetched for
//! what it shows; so do the ticket page and the menu. They keep those rows
//! here, and so does the board's list, so a ticket on screen in three
//! places is one signal and one `watch` event updates all three.
//!
//! The rules:
//!
//! - **One signal per id.** [`hold`] inserts a row, or updates the one
//!   there, and hands every caller the same `ArcRwSignal`. A view takes a
//!   Copy `RwSignal` handle on it in its own owner (`RwSignal::from`), which
//!   goes when the view does.
//! - **Held while a view shows it.** Every hold is counted and [`release`]
//!   gives one back; an entry leaves the cache when its count reaches zero.
//!   Who holds: the board's list, one hold per row it has (every row the
//!   walk found, until SCALE-26 ends the walk); each open nav list, for the
//!   children near its window; each ticket page, for its ticket, blockers
//!   and children; the nav's open shortcode rows, for their roots. A view
//!   holds through a [`Holder`], which gives everything back when the view's
//!   owner is cleaned up.
//! - **A signal outlives the cache's interest, never the other way round.**
//!   The cache keeps `ArcRwSignal`s — reference-counted, not arena-allocated
//!   — so a view still drawing a row the cache has let go of reads a live
//!   value, never a disposed one; it only stops hearing about changes. No
//!   eviction can turn into a use-after-dispose panic in the wasm.
//! - **`watch` updates, never inserts.** [`put`] applies an event's ticket
//!   to the row the cache holds and otherwise drops it: what the cache holds
//!   is what some view shows. (The board's list still inserts every ticket
//!   it hears of, through [`hold`]; SCALE-26 changes that.)
//! - **Arrivals are tracked.** A view waiting for a row it cannot draw yet —
//!   a ticket page whose `get` is in flight, a blocker created later — looks
//!   it up with [`find`], which re-runs when new rows arrive. Rows already
//!   held never notify it, so a page of the walk landing re-runs those few
//!   lookups once, not per row.
//!
//! SCALE-26: the walk is gone, and with it the one view that held every
//! ticket. The board holds its window and a margin either side, through its
//! own [`Holder`] (src/board.rs), and gives back what scrolls far away; a
//! `created` is held only where the board holds the rows around it, or by a
//! ticket page that asked for it. So the cache now holds what is shown —
//! about 2,400 rows at most for the board, however far it has been scrolled,
//! plus what the nav and an open page show — and [`put`] is the whole of
//! `watch`'s reach: an event for a row nobody holds is dropped. How many it
//! holds is a signal ([`held_signal`]), which the page carries as
//! `#app[data-rows-held]` for the harnesses.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use leptos::prelude::*;
use menger_tickets_api::{Placement, TicketSummary};

/// A row as the cache keeps it — and as a view reads it, through a handle.
pub(crate) type RowSignal = ArcRwSignal<TicketSummary>;

struct Entry {
    sig: RowSignal,
    holds: u32,
}

thread_local! {
    /// Every row some view holds, by id. A `thread_local` for the reason
    /// the session's transport is one: wasm is single-threaded, and every
    /// view that shows a ticket reaches it.
    static ROWS: RefCell<HashMap<String, Entry>> = RefCell::new(HashMap::new());

    /// Notified when an id the cache did not hold arrives — what [`find`]
    /// re-runs on.
    static ARRIVALS: ArcTrigger = ArcTrigger::new();

    /// SCALE-26: how many rows are held, as a signal — set after the map
    /// changes size, never while it is borrowed.
    static HELD: ArcRwSignal<usize> = ArcRwSignal::new(0);
}

/// The held count, set to what the map holds now if it moved.
fn count() {
    let n = ROWS.with(|rows| rows.borrow().len());
    HELD.with(|h| {
        if h.get_untracked() != n {
            h.set(n);
        }
    });
}

/// `t` onto `sig`, if it differs — an unchanged row notifies nobody, so a
/// re-fetch of rows already shown repaints nothing.
fn write(sig: &RowSignal, t: TicketSummary) {
    if sig.with_untracked(|old| *old != t) {
        sig.set(t);
    }
}

/// One hold on `t`'s row, without announcing an arrival: the row's signal,
/// and whether the id is new to the cache. Never writes a signal while the
/// map is borrowed — a write can run code that looks a row up.
fn hold_quiet(t: TicketSummary) -> (RowSignal, bool) {
    let held = ROWS.with(|rows| {
        rows.borrow_mut().get_mut(&t.id).map(|e| {
            e.holds += 1;
            e.sig.clone()
        })
    });
    match held {
        Some(sig) => {
            write(&sig, t);
            (sig, false)
        }
        None => {
            let id = t.id.clone();
            let sig = ArcRwSignal::new(t);
            ROWS.with(|rows| rows.borrow_mut().insert(id, Entry { sig: sig.clone(), holds: 1 }));
            (sig, true)
        }
    }
}

/// One hold on `t`'s row — inserted, or updated in place — and its signal.
/// Every hold is given back with [`release`] (a [`Holder`] does it for a
/// view).
pub(crate) fn hold(t: TicketSummary) -> RowSignal {
    let (sig, fresh) = hold_quiet(t);
    if fresh {
        count();
        ARRIVALS.with(ArcTrigger::notify);
    }
    sig
}

/// [`hold`] for a page of rows, announcing their arrival once: a page of
/// the walk is 500 rows, and every view waiting on [`find`] re-runs per
/// announcement, not per row.
pub(crate) fn hold_page(page: impl IntoIterator<Item = TicketSummary>) -> Vec<RowSignal> {
    let mut fresh = false;
    let sigs = page
        .into_iter()
        .map(|t| {
            let (sig, new) = hold_quiet(t);
            fresh |= new;
            sig
        })
        .collect();
    if fresh {
        count();
        ARRIVALS.with(ArcTrigger::notify);
    }
    sigs
}

/// One more hold on a row the cache already has; `None` (and no hold) when
/// it has none.
pub(crate) fn hold_id(id: &str) -> Option<RowSignal> {
    ROWS.with(|rows| {
        rows.borrow_mut().get_mut(id).map(|e| {
            e.holds += 1;
            e.sig.clone()
        })
    })
}

/// Give back one hold; the last one takes the row out of the cache.
pub(crate) fn release(id: &str) {
    let gone = ROWS.with(|rows| {
        let mut rows = rows.borrow_mut();
        match rows.get_mut(id) {
            Some(e) => {
                e.holds = e.holds.saturating_sub(1);
                e.holds == 0 && rows.remove(id).is_some()
            }
            None => false,
        }
    });
    if gone {
        count();
    }
}

/// A `watch` event's ticket: applied to the row if the cache holds it,
/// dropped otherwise. True when the cache holds it.
pub(crate) fn put(t: TicketSummary) -> bool {
    match get(&t.id) {
        Some(sig) => {
            write(&sig, t);
            true
        }
        None => false,
    }
}

/// SCALE-4: a parent's count moved (`children_changed`, `child_removed`) —
/// set on its row, if held, in place.
pub(crate) fn set_child_count(id: &str, count: u64) {
    if let Some(sig) = get(id) {
        if sig.with_untracked(|t| t.child_count != count) {
            sig.update(|t| t.child_count = count);
        }
    }
}

/// WAVE-6: a ticket's kept placement moved (`placements_changed`) — set
/// on its row, if held, in place.
pub(crate) fn set_placement(id: &str, placement: Placement) {
    if let Some(sig) = get(id) {
        if sig.with_untracked(|t| t.placement != Some(placement)) {
            sig.update(|t| t.placement = Some(placement));
        }
    }
}

/// The row for `id`, if held — untracked: a caller that must hear of its
/// arrival uses [`find`].
pub(crate) fn get(id: &str) -> Option<RowSignal> {
    ROWS.with(|rows| rows.borrow().get(id).map(|e| e.sig.clone()))
}

/// The row for `id`, if held — and, inside a reactive scope, a dependency
/// on arrivals, so a view waiting for a row draws it when it comes.
pub(crate) fn find(id: &str) -> Option<RowSignal> {
    ARRIVALS.with(|a| a.track());
    get(id)
}

/// How many rows are held — the number SCALE-26's budget is about.
#[cfg(test)]
pub(crate) fn held() -> usize {
    ROWS.with(|rows| rows.borrow().len())
}

/// SCALE-26: [`held`] as a signal — what `#app[data-rows-held]` shows.
pub(crate) fn held_signal() -> ArcRwSignal<usize> {
    HELD.with(Clone::clone)
}

/// A view's holds, given back together.
///
/// A view that fetches rows to show holds them through one of these; when
/// the view's owner is cleaned up ([`Holder::new`]) every hold goes back,
/// and an answer that lands after that holds nothing — it only updates a
/// row someone else still holds. A holder holds an id at most once, so a
/// re-fetch of what it shows is an update, not a second hold. `Arc<Mutex>`
/// only because a cleanup must be `Send + Sync`; nothing here is shared
/// across threads, and the lock is never held across a call into the cache.
#[derive(Clone)]
pub(crate) struct Holder(Arc<Mutex<Option<HashSet<String>>>>);

impl Holder {
    /// A holder that gives its holds back when the current reactive owner —
    /// the view creating it — is cleaned up.
    pub(crate) fn new() -> Self {
        let h = Self(Arc::new(Mutex::new(Some(HashSet::new()))));
        let mine = h.clone();
        on_cleanup(move || mine.close());
        h
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Option<HashSet<String>>> {
        self.0.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Hold `t`'s row for this view (or update it, if held already): its
    /// signal, or `None` once the view is gone.
    pub(crate) fn hold(&self, t: TicketSummary) -> Option<RowSignal> {
        let first = {
            let mut g = self.lock();
            match g.as_mut() {
                None => None,
                Some(ids) => Some(ids.insert(t.id.clone())),
            }
        };
        match first {
            None => {
                put(t);
                None
            }
            Some(true) => Some(hold(t)),
            Some(false) => {
                let id = t.id.clone();
                put(t);
                get(&id)
            }
        }
    }

    /// [`Holder::hold`] for a page, announcing arrivals once. False once the
    /// view is gone.
    pub(crate) fn hold_page(&self, page: Vec<TicketSummary>) -> bool {
        let fresh: Option<Vec<bool>> = {
            let mut g = self.lock();
            g.as_mut().map(|ids| page.iter().map(|t| ids.insert(t.id.clone())).collect())
        };
        let Some(fresh) = fresh else {
            for t in page {
                put(t);
            }
            return false;
        };
        let (new, old): (Vec<_>, Vec<_>) = page.into_iter().zip(fresh).partition(|(_, f)| *f);
        hold_page(new.into_iter().map(|(t, _)| t));
        for (t, _) in old {
            put(t);
        }
        true
    }

    /// Hold a row the cache already has (a `watch` event put it there): true
    /// if this view holds it now.
    pub(crate) fn hold_id(&self, id: &str) -> bool {
        let first = {
            let mut g = self.lock();
            match g.as_mut() {
                None => return false,
                Some(ids) => !ids.contains(id),
            }
        };
        if !first {
            return true;
        }
        if hold_id(id).is_none() {
            return false;
        }
        if let Some(ids) = self.lock().as_mut() {
            ids.insert(id.to_string());
        }
        true
    }

    /// Whether this view holds `id`.
    pub(crate) fn holds(&self, id: &str) -> bool {
        self.lock().as_ref().is_some_and(|ids| ids.contains(id))
    }

    /// Give back the hold on `id`, if this view has one.
    pub(crate) fn release(&self, id: &str) {
        let had = self.lock().as_mut().is_some_and(|ids| ids.remove(id));
        if had {
            release(id);
        }
    }

    /// Give back every hold `keep` says no to — a nav list letting go of the
    /// children far from its window.
    pub(crate) fn retain(&self, keep: impl Fn(&str) -> bool) {
        let gone: Vec<String> = {
            let mut g = self.lock();
            let Some(ids) = g.as_mut() else { return };
            let gone: Vec<String> = ids.iter().filter(|id| !keep(id)).cloned().collect();
            for id in &gone {
                ids.remove(id);
            }
            gone
        };
        for id in gone {
            release(&id);
        }
    }

    /// Give back everything; later holds hold nothing. The view's cleanup.
    pub(crate) fn close(&self) {
        let ids = self.lock().take();
        for id in ids.into_iter().flatten() {
            release(&id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{find, get, held, hold, hold_id, put, release, set_child_count, Holder};
    use leptos::prelude::*;
    use menger_tickets_api::{Kind, Status, TicketSummary};

    fn row(id: &str) -> TicketSummary {
        let (epic, seq) = id.rsplit_once('-').expect("EPIC-N");
        TicketSummary {
            labels: Vec::new(),
            id: id.to_string(),
            epic: epic.to_string(),
            seq: seq.parse().expect("seq"),
            title: format!("the title of {id}"),
            status: Status::Pending,
            kind: Kind::Build,
            severity: None,
            parent: None,
            child_count: 0,
            commits: Vec::new(),
            path: None,
            placement: None,
            blocked_by: Vec::new(),
            unlocks: Vec::new(),
            created_unix: 0,
            updated_unix: 0,
        }
    }

    /// One signal per id: a second hold is the same signal, updated in place;
    /// a `watch` event reaches it; the row leaves with its last hold, and an
    /// event for a row nobody holds inserts nothing.
    #[test]
    fn one_signal_per_id_held_while_shown() {
        let a = hold(row("CAC-1"));
        let mut done = row("CAC-1");
        done.status = Status::Complete;
        let b = hold(done);
        assert_eq!(a, b, "the same signal");
        assert_eq!(a.with_untracked(|t| t.status), Status::Complete, "updated in place");
        let mut amended = row("CAC-1");
        amended.title = "amended".into();
        assert!(put(amended));
        assert_eq!(b.with_untracked(|t| t.title.clone()), "amended");
        set_child_count("CAC-1", 7);
        assert_eq!(a.with_untracked(|t| t.child_count), 7);

        release("CAC-1");
        assert!(get("CAC-1").is_some(), "one hold left");
        release("CAC-1");
        assert!(get("CAC-1").is_none(), "the last hold took it out");
        assert_eq!(a.with_untracked(|t| t.title.clone()), "amended", "a handle still reads a live value");
        assert!(!put(row("CAC-1")), "an event for a row nobody holds is dropped");
        assert!(find("CAC-1").is_none());
        assert!(hold_id("CAC-1").is_none());
    }

    /// A holder holds an id once however often it is fetched, gives back
    /// what it is told to let go of, and everything when closed — after
    /// which it holds nothing.
    #[test]
    fn a_holder_gives_its_holds_back() {
        let before = held();
        let h = Holder(std::sync::Arc::new(std::sync::Mutex::new(Some(Default::default()))));
        assert!(h.hold(row("HLD-1")).is_some());
        assert!(h.hold(row("HLD-1")).is_some(), "a re-fetch");
        assert!(h.hold_page(vec![row("HLD-2"), row("HLD-3"), row("HLD-1")]));
        assert_eq!(held(), before + 3);
        let other = hold(row("HLD-2"));
        h.retain(|id| id != "HLD-2" && id != "HLD-3");
        assert!(get("HLD-2").is_some(), "held elsewhere too");
        assert!(get("HLD-3").is_none());
        assert!(h.holds("HLD-1") && !h.holds("HLD-2"));
        assert!(h.hold_id("HLD-2"));
        h.close();
        assert!(get("HLD-1").is_none());
        assert!(get("HLD-2").is_some(), "the other hold");
        assert!(h.hold(row("HLD-9")).is_none(), "closed: holds nothing");
        assert!(get("HLD-9").is_none());
        drop(other);
        release("HLD-2");
        assert_eq!(held(), before);
    }
}
