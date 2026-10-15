//! MENGER-29: who a call is and what it sees — a filtered watch's watcher,
//! a read's clock, a write's act, IDENTITY-37's [`Sight`] and the
//! per-caller reach every method narrows its answer by. Moved out of
//! `activation.rs` as they were, so that file holds the methods alone.

use super::*;
use crate::access::{Closed, Followed, Following};
use crate::filter::Filter;
use crate::store::WatchSight;

/// FILTER-4: a filtered watch's watcher — `filter` (already ANDed with the
/// caller's reach) compiled, in `epic` alone when one is asked. MENGER-13:
/// its history read by `clock`.
/// FILTER-36: its saved names resolved, and the filter kept to resolve again.
pub(super) fn watcher_for(store: &Store, filter: Option<&Filter>, epic: Option<&crate::address::ShortcodeId>, clock: &Clock) -> Result<crate::store::Watcher, String> {
    let lens = store.lens_or_all(filter).map_err(|e| e.to_string())?;
    Ok(store.watcher_in(lens.with_clock(clock.clone()), epic).naming(filter))
}

/// ACCESS-10: a read's own filter text, read — ACCESS-29: held to its size
/// first; `None` with none, an empty text the empty filter. The store ANDs
/// the reader's reach into it (never this text formatted with it).
pub(super) fn asked(text: Option<&str>) -> Result<Option<Filter>, String> {
    text.map(crate::limits::check_filter).transpose()?;
    match text.map(str::trim) {
        None => Ok(None),
        Some("") => Ok(Some(Filter::default())),
        Some(t) => Filter::parse(t).map(Some).map_err(|e| e.to_string()),
    }
}

/// MENGER-13: how a read's filter reads time — in `tz` (an IANA name, UTC,
/// or an offset; none: UTC), now, with `me` the caller's principal
/// (`anonymous` with login off). A zone the server cannot read is refused.
pub(super) fn clock_of(caller: &VerifiedUser, tz: Option<&str>) -> Result<Clock, String> {
    let me = caller.principal_text().unwrap_or_else(|| crate::types::ANONYMOUS_ACTOR.to_string());
    Clock { me, ..Clock::default() }.zoned(tz)
}

/// ACCESS-8: typed values as the text the store still takes (ACCESS-10
/// and ACCESS-11 type the store's own signatures).
pub(super) fn texts<T: Into<String>>(values: Vec<T>) -> Vec<String> {
    values.into_iter().map(Into::into).collect()
}

/// FILTER-15's [`narrowing_lens`], MENGER-13: its history read by the
/// caller's clock in `tz`. FILTER-36: its saved names resolved by `store`.
pub(super) fn zoned_lens(store: &Store, caller: &VerifiedUser, filter: Option<&Filter>, tz: Option<&str>) -> Result<Option<Lens>, StoreError> {
    let clock = clock_of(caller, tz).map_err(StoreError::Filter)?;
    Ok(store.narrowing_lens(filter)?.map(|l| l.with_clock(clock)))
}

/// MENGER-13: who a write acts as — the caller's principal and name
/// (`anonymous` with login off), from `source` (`app`, `api` — the default
/// —, `pipeline`; ACCESS-8: `cascade`, the store's own, is no
/// [`CallerSource`]), saying `message` (blank: none).
pub(super) fn act_of(caller: &VerifiedUser, source: Option<CallerSource>, message: Option<Message>) -> Result<Act, String> {
    Ok(Act {
        actor: caller.principal_text().unwrap_or_else(|| crate::types::ANONYMOUS_ACTOR.to_string()),
        name: caller.name().map(str::to_string),
        source: TransitionSource::from(source.unwrap_or_default()),
        message: message.and_then(Message::given),
        cause: None,
    })
}

/// IDENTITY-37: what an open `watch` relays, as the caller's grants stand
/// now — ACCESS-12: the caller followed ([`Following`]: verified again only
/// when the grants generation moves, a sign-out or the expiry comes), its
/// reach compiled once ([`WatchSight`]) and every change cut to it by the
/// store ([`Store::heard`]), and the components it has been shown.
pub(super) struct Sight {
    following: Following,
    sight: WatchSight,
    /// The components this caller can see now (by name) — with a reach
    /// that hides something only.
    shown: std::collections::HashSet<String>,
}

/// What a moved reach did: the reach the watch heard through until now,
/// and the components it no longer sees.
pub(super) struct Moved {
    pub(super) was: WatchSight,
    pub(super) gone: Vec<String>,
}

impl Sight {
    /// The watch's caller, verified now; `Err` (in words) when it is
    /// refused or sees nothing.
    pub(super) fn open(store: &Arc<Store>, access: &Access, who: Option<&AuthContext>, reach: &Reach) -> Result<Self, String> {
        let following = Following::open(access, store, who, reach)?;
        let sight = store.watch_sight(following.user()).map_err(|e| e.to_string())?;
        let shown = Self::shown(store, &following, &sight);
        Ok(Self { following, sight, shown })
    }

    fn shown(store: &Store, following: &Following, sight: &WatchSight) -> std::collections::HashSet<String> {
        match sight.all() {
            true => Default::default(),
            false => store.components(following.user()).into_iter().map(|c| c.name).collect(),
        }
    }

    /// The caller, as it was last verified.
    pub(super) fn caller(&self) -> &VerifiedUser {
        self.following.user()
    }

    /// The reach it hears through now.
    pub(super) fn sight(&self) -> &WatchSight {
        &self.sight
    }

    /// Until something that could change the caller moves (cancel-safe).
    pub(super) async fn changed(&mut self) {
        self.following.changed().await
    }

    /// The caller as it stands: `Ok(None)` when what it hears is as it was;
    /// `Ok(Some(moved))` when its reach moved — the sight rebuilt, `moved`
    /// the reach it had and the components it no longer sees; `Err` when it
    /// is refused, or now sees nothing (the watch ends, saying why).
    pub(super) fn again(&mut self, store: &Store) -> Result<Option<Moved>, Closed> {
        match self.following.again()? {
            Followed::Same => Ok(None),
            Followed::Moved { was } => {
                let sight = store.watch_sight(self.following.user()).map_err(Closed::revoked)?;
                let shown = Self::shown(store, &self.following, &sight);
                let mut gone: Vec<String> = match (self.sight.all(), sight.all()) {
                    (_, true) => Vec::new(),
                    (true, false) => store.components(&was).into_iter().map(|c| c.name).filter(|n| !shown.contains(n)).collect(),
                    (false, false) => self.shown.difference(&shown).cloned().collect(),
                };
                gone.sort();
                let was = std::mem::replace(&mut self.sight, sight);
                self.shown = shown;
                Ok(Some(Moved { was, gone }))
            }
        }
    }

    /// A change as this caller may hear it, as its one wire event, or
    /// nothing — cut to the reach by the store ([`Store::heard`]: every
    /// change classified, rows redacted). A component's change reaches it
    /// when it can view the component's owner; one it had been shown and
    /// can no longer see is said as its removal, one it never saw not at all.
    pub(super) fn relay(&mut self, store: &Store, change: Change) -> Option<TicketsEvent> {
        let change = store.heard(&self.sight, change)?;
        if !self.sight.all() {
            match &change {
                Change::ComponentChanged { component } => {
                    self.shown.insert(component.name.clone());
                }
                Change::ComponentRemoved { name } => {
                    if !self.shown.remove(name) {
                        return None;
                    }
                }
                _ => {}
            }
        }
        wire_event(change)
    }
}

/// Whether `who` sees the ticket `id` — ACCESS-10: the store's answer,
/// one lookup in its reach.
pub(super) fn sees_id(store: &Store, who: &(impl crate::store::Reader + ?Sized), id: &str) -> bool {
    store.member(who, &[id.to_string()], &Lens::all()).first().copied().unwrap_or(false)
}

/// MENGER-15: a body past the cap, as the typed refusal (`body_too_large`)
/// — any other error as the in-band `error` it always was.
pub(super) fn too_large(e: StoreError) -> TicketsEvent {
    let message = e.to_string();
    match e {
        StoreError::BodyTooLarge { id, bytes, max } => TicketsEvent::BodyTooLarge { id, bytes: bytes as u64, max: max as u64, message },
        _ => TicketsEvent::Error { message },
    }
}

/// IDENTITY-37: a refused `create`'s reason, naming only what the caller
/// may see — a ticket out of its reach that a cycle passes through, or that
/// already waits on a doc's id, is said as such, not by its id.
pub(super) fn redacted(e: StoreError, sees: impl Fn(&str) -> bool) -> String {
    redact(e, &sees)
}

/// [`redacted`], over a reference — MENGER-30: an update's refusals are
/// each redacted the same way.
fn redact(e: StoreError, sees: &dyn Fn(&str) -> bool) -> String {
    const HIDDEN: &str = "(a ticket out of your reach)";
    let one = |i: String| if sees(&i) { i } else { HIDDEN.to_string() };
    let veil = |ids: Vec<String>| -> Vec<String> { ids.into_iter().map(one).collect() };
    // ACCESS-48: blockers' names, the same way.
    let veiled = |ids: Vec<crate::TicketName>| -> Vec<crate::TicketName> {
        ids.into_iter().map(|i| if sees(&i) { i } else { crate::TicketName::new(HIDDEN) }).collect()
    };
    match e {
        StoreError::Cycle { id, path } => StoreError::Cycle { id, path: veiled(path) }.to_string(),
        // ACCESS-11 (ACCESS-3 #8): every write's refusal names only what
        // the caller may see — a doc's place in the graph, a parent chain,
        // a component's owner, a shortcode's root, a path's holder, and the
        // id a birth would have taken (the next number of a shortcode).
        StoreError::DocInGraph { id, blocked_by, unlocks } => StoreError::DocInGraph { id, blocked_by: veiled(blocked_by), unlocks: veiled(unlocks) }.to_string(),
        StoreError::ParentCycle { id, path } => StoreError::ParentCycle { id, path: veil(path) }.to_string(),
        StoreError::ParentOutsideShortcode { id, parent } => StoreError::ParentOutsideShortcode { id: one(id), parent: one(parent) }.to_string(),
        StoreError::ComponentOwned { id, name, owner } => StoreError::ComponentOwned { id, name, owner: one(owner) }.to_string(),
        StoreError::ScopeNotRoot { id, root, parent } => StoreError::ScopeNotRoot { id: one(id), root: one(root), parent: one(parent) }.to_string(),
        StoreError::PathTaken { epic, path, holder } => StoreError::PathTaken { epic, path, holder: one(holder) }.to_string(),
        StoreError::PathParent { path, parent, root } => StoreError::PathParent { path, parent: one(parent), root: one(root) }.to_string(),
        StoreError::DocAwaited { id, by } => StoreError::DocAwaited { id, by: veiled(by) }.to_string(),
        // MENGER-30: `update`'s cycle, and each of its refusals.
        StoreError::BlockCycle { id, path } => StoreError::BlockCycle { id, path: veiled(path) }.to_string(),
        StoreError::Refused { id, refusals } => {
            let each: Vec<String> = refusals.into_iter().map(|r| redact(r, sees)).collect();
            format!("update of '{id}' refused, nothing written — {}", each.join("; "))
        }
        // ACT-2: `update_many`'s, each ticket's each refusal.
        StoreError::RefusedMany { refusals } => {
            crate::store::refused_many(refusals.into_iter().map(|(id, w)| (id, w.into_iter().map(|e| redact(e, sees)).collect())).collect())
        }
        e => e.to_string(),
    }
}


/// ORG-3: the shortcode `watch { epic }` names, by any short form of its
/// address — one naming several refuses the watch (the second, why); one
/// naming none is a shortcode with no tickets, which nothing is said of.
pub(super) fn watched_shortcode(store: &Store, epic: Option<&str>) -> (Option<crate::address::ShortcodeId>, Option<String>) {
    let Some(e) = epic else { return (None, None) };
    match store.shortcode(e) {
        Ok(Some(sc)) => (Some(sc.id), None),
        Ok(None) => (Some(crate::address::ShortcodeId::new(&e.trim().to_ascii_uppercase())), None),
        Err(why) => (None, Some(why.to_string())),
    }
}
