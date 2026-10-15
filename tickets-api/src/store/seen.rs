//! ACCESS-10: **who a read answers** — every public read of the store takes
//! a [`Reader`]: a [`crate::access::VerifiedUser`] (its reach, as the call's
//! grant is held), one reading through another of its grants
//! ([`crate::access::ReadingAs`]), or the [`System`] (the code that opened
//! the store, acting for no user: everything). The trait is sealed: nothing
//! else reads, so a read that forgets its caller does not compile.
//!
//! A read answers only what the reach admits ([`Seen`]): its lens is the
//! caller's AND the reach's ([`Lens::and`]), and one ticket is looked up in
//! the reach's admitted bitmaps ([`Sees`]). What the reach does not admit is
//! **hidden** — answered exactly as a ticket that is not there
//! ([`StoreError::NotFound`], [`StoreError::CursorNotFound`]) — and never
//! named in a row it does admit: a visible row's `blocked_by`, `unlocks`
//! and `parent` lose the ids it hides, counted instead
//! (`hidden_blocked_by`, `hidden_unlocks`, `parent_hidden`), so readiness
//! stays honest ([`Sees::row`]).
//!
//! A reach is closed under the kept tree: every scope it compiles from
//! (`*`, an organization, a shortcode and those hosted under it, `under
//! ID`) holds a ticket's descendants with it, and so does their AND. So a
//! visible ticket's children are all visible — `child_count` needs no
//! redaction — and what can be hidden from a visible row is its parent, its
//! blockers and what it unlocks.
//!
//! ```compile_fail,E0277
//! // A read without a reader: an id is not one, nor anything but the three.
//! async fn read(store: &menger_tickets_api::store::Store) {
//!     let _ = store.get(&"anyone", "MENGER-1").await;
//! }
//! ```
//!
//! ```compile_fail,E0624
//! // An unscoped read is the store's own: a reader outside it names a Reader.
//! let store = menger_tickets_api::store::Store::new();
//! let _ = store.groups_unscoped();
//! ```
//!
//! ```compile_fail,E0277
//! // Nothing outside the store is a Reader: the trait is sealed.
//! struct Anyone;
//! impl menger_tickets_api::store::Reader for Anyone {
//!     fn reach(&self) -> Result<Option<menger_tickets_api::filter::Filter>, menger_tickets_api::store::StoreError> { Ok(None) }
//! }
//! ```

use super::cells::View;
use super::*;
use crate::access::{ReadingAs, VerifiedUser};
use crate::filter::Filter;

mod sealed {
    pub trait Sealed {}
    impl Sealed for super::System {}
    impl Sealed for crate::access::VerifiedUser {}
    impl Sealed for crate::access::ReadingAs<'_> {}
}

/// What a store read is asked by: a verified user, a user reading through
/// one of its grants, or the [`System`]. Sealed — these three alone.
pub trait Reader: sealed::Sealed + Sync {
    /// The filter `under` what it may read: `None` for everything (the
    /// system, login off, a grant on `*`); a refusal when it reaches
    /// nothing (held nowhere, or a reach no read can narrow to).
    fn reach(&self) -> Result<Option<Filter>, StoreError>;

    /// ACCESS-19: which comments on the app (a comment naming no ticket)
    /// it sees — every one, or only those its principal left.
    fn app_comments(&self) -> AppSight;
}

/// ACCESS-19 (ACCESS-1's first question, as proposed): a comment on the app
/// names no ticket, so no reach decides it — it is its author's and the
/// admins' of `*`, and nobody else's.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppSight {
    /// The system, login off, an admin of `*`: every one.
    All,
    /// A signed-in principal: the ones it left.
    By(String),
}

impl AppSight {
    /// Whether it sees the app comment left by `principal` (`None`: left
    /// with login off, by no one in particular).
    fn admits(&self, principal: Option<&str>) -> bool {
        match self {
            AppSight::All => true,
            AppSight::By(p) => principal == Some(p.as_str()),
        }
    }
}

impl Reader for System {
    fn reach(&self) -> Result<Option<Filter>, StoreError> {
        Ok(None)
    }

    fn app_comments(&self) -> AppSight {
        AppSight::All
    }
}

impl Reader for VerifiedUser {
    fn reach(&self) -> Result<Option<Filter>, StoreError> {
        self.read_filter().map_err(StoreError::Filter)
    }

    fn app_comments(&self) -> AppSight {
        // Login off, `administers` is everywhere; a principal-less user
        // past it is none — it sees the ones left by no one.
        match (self.administers("*"), self.principal_text()) {
            (true, _) => AppSight::All,
            (false, p) => AppSight::By(p.unwrap_or_default()),
        }
    }
}

impl Reader for ReadingAs<'_> {
    fn reach(&self) -> Result<Option<Filter>, StoreError> {
        self.read_filter().map_err(StoreError::Filter)
    }

    fn app_comments(&self) -> AppSight {
        self.user().app_comments()
    }
}

/// A reader's reach, compiled once for a read: `None` everything; else the
/// filter and its lens.
#[derive(Clone)]
pub(super) struct Seen {
    reach: Option<(Filter, Lens)>,
    /// ACCESS-19: the comments on the app it sees.
    app: AppSight,
}

impl std::fmt::Debug for Seen {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Seen({:?})", self.lens())
    }
}

impl Seen {
    /// `who`'s reach for this read.
    pub(super) fn of(who: &(impl Reader + ?Sized)) -> Result<Self, StoreError> {
        let reach = match who.reach()? {
            None => None,
            Some(f) => {
                let lens = Lens::of(&f)?;
                Some((f, lens))
            }
        };
        Ok(Self { reach, app: who.app_comments() })
    }

    /// The system's sight: everything. What a walk whose reach could not
    /// be read holds beside its refusal (it reads nothing).
    pub(super) fn everything() -> Self {
        Self { reach: None, app: AppSight::All }
    }

    /// Everything: nothing is hidden.
    pub(super) fn all(&self) -> bool {
        self.reach.is_none()
    }

    /// A walk's selection within the reach: `list`'s and the ready set's as
    /// the filtered selections they are, the reach their lens (IDENTITY-21
    /// answered a restricted reader so: the reach is a filter); a filtered
    /// one ANDed ([`Seen::filtered`]).
    pub(super) fn select(&self, cache: &Cache, select: Select) -> Result<Select, StoreError> {
        let Some((filter, lens)) = &self.reach else { return Ok(select) };
        Ok(Select::Filtered(match select {
            Select::List { epic, status, parent } => Filtered { lens: lens.clone(), epic, status, parent, keep: None },
            Select::Ready { epic } => lensed::ready_and(filter, epic)?,
            Select::Filtered(q) => self.filtered(cache, &q)?,
        }))
    }

    /// The reach as a filter (`None`: everything).
    pub(super) fn filter(&self) -> Option<&Filter> {
        self.reach.as_ref().map(|(f, _)| f)
    }

    /// The reach as a lens (`None`: everything).
    pub(super) fn lens(&self) -> Option<&Lens> {
        self.reach.as_ref().map(|(_, l)| l)
    }

    /// `lens` within the reach — what a filtered read answers under.
    pub(super) fn within(&self, lens: &Lens) -> Result<Lens, StoreError> {
        match self.lens() {
            None => Ok(lens.clone()),
            Some(reach) => lens.and(reach),
        }
    }

    /// A filtered read's query within the reach — its lens ANDed, and its
    /// `keep` (FILTER-24) only when the reach admits it: keeping never
    /// answers a ticket the reader could not read.
    pub(super) fn filtered(&self, cache: &Cache, q: &Filtered) -> Result<Filtered, StoreError> {
        if self.all() {
            return Ok(q.clone());
        }
        let keep = q.keep.as_deref().and_then(|k| filtered::keep_row(cache, k)).filter(|t| self.sees(cache).key(&OrderKey { shortcode: t.shortcode.clone(), seq: t.seq }));
        Ok(Filtered { lens: self.within(&q.lens)?, keep: keep.map(|t| t.id.to_string()), ..q.clone() })
    }

    /// The reach's admitted bitmaps, under the read lock the caller holds.
    pub(super) fn sees<'c>(&self, cache: &'c Cache) -> Sees<'c> {
        let view = self.lens().map(|lens| {
            let q = Filtered { lens: lens.clone(), epic: None, status: None, parent: None, keep: None };
            filtered::view_lenient(cache, &q)
        });
        Sees { cache, view, app: self.app.clone() }
    }
}

/// [`Seen`] compiled against the cache: which tickets the reach admits.
pub(super) struct Sees<'c> {
    cache: &'c Cache,
    /// `None`: everything.
    view: Option<View<'c>>,
    /// ACCESS-19: the comments on the app it sees.
    app: AppSight,
}

impl<'c> Sees<'c> {
    /// Everything: nothing is hidden.
    pub(super) fn all(&self) -> bool {
        self.view.is_none()
    }

    /// Whether the reach admits the ticket `seq` of `sc`.
    pub(super) fn has(&self, sc: &ShortcodeId, seq: u32) -> bool {
        self.view.as_ref().is_none_or(|v| v.contains(sc, seq))
    }

    /// Whether the reach admits the row `t`.
    pub(super) fn sees_row(&self, t: &Row) -> bool {
        self.has(&t.shortcode, t.seq)
    }

    /// The sequences of `sc` the reach admits (`None`: every one).
    pub(super) fn admitted(&self, sc: &ShortcodeId) -> Option<roaring::RoaringBitmap> {
        let v = self.view.as_ref()?;
        let mut out = roaring::RoaringBitmap::new();
        if let Some((_, bms)) = v.shortcodes_from(Some(sc)).first().filter(|(e, _)| *e == sc) {
            for b in bms {
                out |= b.as_ref();
            }
        }
        Some(out)
    }

    /// Whether the reach admits a comment (IDENTITY-37): every ticket it is
    /// on — its anchor's, and the one whose page it was left on. ACCESS-19:
    /// a comment on the app alone (naming no ticket), only its author and
    /// the admins of `*` ([`AppSight`]) — no longer any reach at all.
    pub(super) fn comment(&self, c: &crate::comments::Comment) -> bool {
        let route = c.anchor.route.trim().strip_prefix("#/ticket/").map(|r| r.split(['?', '#']).next().unwrap_or(r).trim()).filter(|r| !r.is_empty());
        let mut on = c.anchor.ticket.iter().map(String::as_str).chain(route).peekable();
        if on.peek().is_none() {
            return self.app.admits(c.principal.as_deref());
        }
        self.all() || on.all(|t| self.cache.resolve(t).is_ok_and(|k| self.id(&k)))
    }

    /// Whether the reach admits the ticket at `k`.
    pub(super) fn key(&self, k: &OrderKey) -> bool {
        self.view.as_ref().is_none_or(|v| v.contains(&k.shortcode, k.seq))
    }

    /// Whether the reach admits the ticket whose key is `id` (as the store
    /// keeps it — a row's `blocked_by`, `parent`). An id naming no ticket
    /// is not admitted: to a reader, a hidden ticket and none are alike.
    pub(super) fn id(&self, id: &str) -> bool {
        match &self.view {
            None => true,
            Some(v) => self.cache.at.get(id).is_some_and(|k| v.contains(&k.shortcode, k.seq)),
        }
    }

    /// The key `address` names, when the reach admits it — ORG-3's resolver
    /// ([`Cache::resolve`]), a hidden ticket answered as none
    /// ([`StoreError::NotFound`], the address as the resolver spells it),
    /// an ambiguous name's candidates cut to those in reach (one left is
    /// the answer; none, not found).
    pub(super) fn resolve(&self, address: &str) -> Result<String, StoreError> {
        let resolved = self.cache.resolve(address);
        if self.all() {
            return resolved;
        }
        // As the resolver says "none" (`resolve_in`): an address as given,
        // an id as the store spells it.
        let t = address.trim();
        let none = || StoreError::NotFound(if t.contains('/') || t.contains(':') { t.to_string() } else { crate::address::folded(t) });
        match resolved {
            Ok(key) if self.id(&key) => Ok(key),
            // A deleted ticket's key (a tombstone's) is the tombstone's to
            // answer, by its shortcode.
            Ok(key) if !self.cache.at.contains_key(&key) => Ok(key),
            Ok(_) => Err(none()),
            Err(StoreError::Ambiguous { text, candidates }) => {
                let kept: Vec<String> =
                    candidates.into_iter().filter(|c| self.cache.resolve(c).is_ok_and(|k| self.id(&k))).collect();
                match kept.len() {
                    0 => Err(none()),
                    1 => self.cache.resolve(&kept[0]),
                    _ => Err(StoreError::Ambiguous { text, candidates: kept }),
                }
            }
            Err(e) => Err(e),
        }
    }

    /// A cursor (`after`, upper case) as a read takes one: a ticket the
    /// reach hides is a cursor into nothing ([`StoreError::CursorNotFound`]),
    /// as one naming no ticket is (ACCESS-3 #8).
    pub(super) fn cursor(&self, after: Option<&str>) -> Result<(), StoreError> {
        match after {
            Some(id) if !self.all() && self.cache.at.contains_key(id) && !self.id(id) => Err(StoreError::CursorNotFound(id.to_string())),
            _ => Ok(()),
        }
    }

    /// A visible row as the reader may read it: the ids the reach hides
    /// dropped from `blocked_by` and `unlocks`, counted instead, and a
    /// hidden parent dropped, `parent_hidden` said (ACCESS-3 #13).
    pub(super) fn row(&self, r: &mut Row) {
        if self.all() {
            return;
        }
        r.hidden_blocked_by = self.cut(&mut r.blocked_by);
        r.hidden_unlocks = self.cut(&mut r.unlocks);
        if r.parent.as_deref().is_some_and(|p| !self.id(p)) {
            r.parent = None;
            r.parent_hidden = true;
        }
    }

    /// [`Sees::row`] for each of `rows`.
    pub(super) fn rows(&self, rows: &mut [Row]) {
        if self.all() {
            return;
        }
        for r in rows {
            self.row(r);
        }
    }

    /// [`Sees::row`] for a whole ticket.
    pub(super) fn ticket(&self, t: &mut Ticket) {
        if self.all() {
            return;
        }
        t.hidden_blocked_by = self.cut(&mut t.blocked_by);
        t.hidden_unlocks = self.cut(&mut t.unlocks);
        if t.parent.as_deref().is_some_and(|p| !self.id(p)) {
            t.parent = None;
            t.parent_hidden = true;
        }
    }

    /// `ids` without those the reach hides; how many went.
    fn cut(&self, ids: &mut Vec<TicketName>) -> u32 {
        let before = ids.len();
        ids.retain(|id| self.id(id));
        (before - ids.len()) as u32
    }

    /// Of `ids`, those the reach admits, in order.
    pub(super) fn keep<T: std::ops::Deref<Target = str>>(&self, ids: impl IntoIterator<Item = T>) -> Vec<T> {
        ids.into_iter().filter(|id| self.id(id)).collect()
    }

    /// The shortcodes the reach admits any ticket of — `None`: every one. A
    /// shortcode the reader sees nothing of is one it is never told of (a
    /// group, an extent, a deleted ticket's).
    pub(super) fn shortcodes(&self) -> Option<HashSet<ShortcodeId>> {
        self.view.as_ref().map(|v| v.counts().filter(|(_, n)| *n > 0).map(|(e, _)| e.clone()).collect())
    }
}
