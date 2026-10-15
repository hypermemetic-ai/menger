//! MENGER-29: who a call is and what it sees — a filtered watch's watcher,
//! a read's clock, a write's act, IDENTITY-37's [`Sight`] and the
//! per-caller reach every method narrows its answer by. Moved out of
//! `activation.rs` as they were, so that file holds the methods alone.

use super::*;

/// FILTER-4: a filtered watch's watcher — `text` (already ANDed with the
/// caller's reach) compiled, in `epic` alone when one is asked. MENGER-13:
/// its history read by `clock`.
pub(super) fn watcher_for(store: &Store, text: Option<&str>, epic: Option<&crate::address::ShortcodeId>, clock: &Clock) -> Result<crate::store::Watcher, String> {
    let lens = text.map(Lens::parse).unwrap_or_else(|| Ok(Lens::all())).map_err(|e| e.to_string())?;
    Ok(store.watcher_in(lens.with_clock(clock.clone()), epic))
}

/// MENGER-13: how a read's filter reads time — in `tz` (an IANA name, UTC,
/// or an offset; none: UTC), now, with `me` the caller's principal
/// (`anonymous` with login off). A zone the server cannot read is refused.
pub(super) fn clock_of(caller: &Caller, tz: Option<&str>) -> Result<Clock, String> {
    let me = caller.principal.clone().unwrap_or_else(|| crate::types::ANONYMOUS_ACTOR.to_string());
    Clock { me, ..Clock::default() }.zoned(tz)
}

/// MENGER-13: an answer's or a clear's `source`, as a caller may say it —
/// `app` or `api`; `system` is the store's own (a withdrawal, a restore).
pub(super) fn caller_source(s: AnswerSource) -> Result<AnswerSource, String> {
    match s {
        AnswerSource::System => Err("unknown answer source 'system' (expected app or api: system is the store's own)".into()),
        s => Ok(s),
    }
}

/// FILTER-15's [`narrowing_lens`], MENGER-13: its history read by the
/// caller's clock in `tz`.
pub(super) fn zoned_lens(caller: &Caller, filter: Option<&str>, tz: Option<&str>) -> Result<Option<Lens>, StoreError> {
    let clock = clock_of(caller, tz).map_err(StoreError::Filter)?;
    Ok(narrowing_lens(filter)?.map(|l| l.with_clock(clock)))
}

/// MENGER-13: who a write acts as — the caller's principal and name
/// (`anonymous` with login off), from `source` (`app`, `api` — the default
/// —, `pipeline`; never `cascade`, the store's own), saying `message`.
pub(super) fn act_of(caller: &Caller, source: Option<&str>, message: Option<String>) -> Result<Act, String> {
    let source = match source.map(str::parse::<TransitionSource>) {
        None => TransitionSource::Api,
        Some(Ok(TransitionSource::Cascade)) => {
            return Err("source: `cascade` is the store's own — a call is app, api or pipeline".into());
        }
        Some(Ok(s)) => s,
        Some(Err(e)) => return Err(e),
    };
    Ok(Act {
        actor: caller.principal.clone().unwrap_or_else(|| crate::types::ANONYMOUS_ACTOR.to_string()),
        name: caller.name.clone(),
        source,
        message: message.map(|m| m.trim().to_string()).filter(|m| !m.is_empty()),
        cause: None,
    })
}

/// IDENTITY-37: what an open `watch` relays, as the caller's grants stand
/// now — its reach, the lens of it, and the components it has been shown.
/// Asked again at every change ([`Sight::again`]): rebuilt only when the
/// grants moved, which is a comparison of the caller's few grants.
pub(super) struct Sight {
    /// AUTH-7: the reach's filter, as text — what moves when the grants do.
    reach: Option<String>,
    /// `None`: everything.
    lens: Option<Lens>,
    /// The components this caller can see now (by name) — with a lens only.
    shown: std::collections::HashSet<String>,
}

impl Sight {
    /// `caller`'s sight; `Err` when it sees nothing.
    pub(super) fn of(store: &Store, caller: &Caller) -> Result<Self, String> {
        let lens = view_lens(caller)?;
        let shown = match &lens {
            None => Default::default(),
            Some(l) => visible_components(store, Some(l), store.components()).into_iter().map(|c| c.name).collect(),
        };
        Ok(Self { reach: caller.reach_key()?, lens, shown })
    }

    /// The caller admitted again: `Ok(None)` when its reach is as it was;
    /// `Ok(Some((caller, gone)))` when it moved — the sight rebuilt, and the
    /// components it no longer sees; `Err` when it is refused, or now sees
    /// nothing (the watch ends).
    pub(super) fn again(&mut self, access: &Access, who: Option<&AuthContext>, reach: &Reach, store: &Store) -> Result<Option<(Caller, Vec<String>)>, String> {
        // AUTH-7: the gate's live reach first — held nowhere any more, the
        // watch ends.
        let reach = reach.current().ok_or_else(|| "your access changed: you hold this nowhere any more".to_string())?;
        let caller = access.admit(who, &reach)?;
        let key = caller.reach_key().map_err(|why| format!("your access changed: {why}"))?;
        if key == self.reach {
            return Ok(None);
        }
        let now = Self::of(store, &caller).map_err(|why| format!("your access changed: {why}"))?;
        let gone: Vec<String> = match (&self.lens, &now.lens) {
            (_, None) => Vec::new(),
            (None, Some(_)) => store.components().into_iter().map(|c| c.name).filter(|n| !now.shown.contains(n)).collect(),
            (Some(_), Some(_)) => {
                let mut g: Vec<String> = self.shown.difference(&now.shown).cloned().collect();
                g.sort();
                g
            }
        };
        *self = now;
        Ok(Some((caller, gone)))
    }

    /// A change as this caller may hear it, or nothing. A component's change
    /// reaches it when it can view the component's owner (its references cut
    /// to what it can view); one it could see and no longer can is said as
    /// its removal.
    pub(super) fn relay(&mut self, store: &Store, ev: TicketsEvent) -> Option<TicketsEvent> {
        let Some(lens) = &self.lens else { return Some(ev) };
        match ev {
            TicketsEvent::ComponentChanged { component } => {
                let name = component.name.clone();
                match visible_components(store, Some(lens), vec![component]).pop() {
                    Some(component) => {
                        self.shown.insert(name);
                        Some(TicketsEvent::ComponentChanged { component })
                    }
                    None => self.shown.remove(&name).then_some(TicketsEvent::ComponentRemoved { name }),
                }
            }
            TicketsEvent::ComponentRemoved { name } => self.shown.remove(&name).then_some(TicketsEvent::ComponentRemoved { name }),
            ev => within(store, Some(lens), ev),
        }
    }
}

/// IDENTITY-21: a watched event as a restricted caller may hear it: itself
/// when every ticket it names is within `reach`, the part of it that is
/// (a placements change), or nothing. IDENTITY-37: every variant is named —
/// a new one does not compile until it is placed — and those that name no
/// ticket and no shortcode pass; a component's are [`Sight::relay`]'s, and
/// here nothing.
pub(super) fn within(store: &Store, reach: Option<&Lens>, ev: TicketsEvent) -> Option<TicketsEvent> {
    let Some(reach) = reach else { return Some(ev) };
    let sees = |id: &str| store.member(&[id.to_string()], reach).first().copied().unwrap_or(false);
    let id: Option<String> = match &ev {
        TicketsEvent::Created { ticket }
        | TicketsEvent::Ticket { ticket }
        | TicketsEvent::StatusChanged { ticket, .. }
        | TicketsEvent::KindChanged { ticket, .. }
        | TicketsEvent::Amended { ticket, .. }
        | TicketsEvent::CommitsLinked { ticket, .. }
        | TicketsEvent::CommitsUnlinked { ticket, .. }
        | TicketsEvent::LabelsChanged { ticket, .. }
        | TicketsEvent::UnlocksChanged { ticket, .. }
        | TicketsEvent::UnlocksRemoved { ticket, .. }
        | TicketsEvent::BlockersChanged { ticket, .. }
        | TicketsEvent::SeverityChanged { ticket, .. }
        | TicketsEvent::Updated { ticket, .. }
        | TicketsEvent::ParentChanged { ticket, .. }
        | TicketsEvent::Blocker { ticket } => Some(ticket.id.clone()),
        TicketsEvent::Summary { summary } | TicketsEvent::Placed { summary, .. } | TicketsEvent::Node { summary, .. } => {
            Some(summary.id.clone())
        }
        TicketsEvent::ChildrenChanged { id, .. }
        | TicketsEvent::ChildRemoved { id, .. }
        | TicketsEvent::Entered { id, .. }
        | TicketsEvent::Left { id, .. } => Some(id.clone()),
        TicketsEvent::Group { group } | TicketsEvent::GroupChanged { group } => {
            if !sees(group.root.as_deref().unwrap_or_default()) {
                return None;
            }
            // As `groups` answers it: a host out of reach is no host.
            let mut group = group.clone();
            if group.host.as_deref().is_some_and(|h| !sees(h)) {
                group.host = None;
            }
            return Some(match ev {
                TicketsEvent::Group { .. } => TicketsEvent::Group { group },
                _ => TicketsEvent::GroupChanged { group },
            });
        }
        TicketsEvent::InputChanged { input } | TicketsEvent::Input { input } => Some(input.ticket.clone()),
        // MENGER-13: a transition is its ticket's.
        TicketsEvent::Transition { transition } => Some(transition.ticket.clone()),
        TicketsEvent::PlacementsChanged { changes } => {
            let kept: Vec<_> = changes.iter().filter(|c| sees(&c.id)).cloned().collect();
            return (!kept.is_empty()).then_some(TicketsEvent::PlacementsChanged { changes: kept });
        }
        // MENGER-31: a cycle, of the tickets on it the reach holds.
        TicketsEvent::WaveCycle { ids } => {
            let kept: Vec<String> = ids.iter().filter(|id| sees(id)).cloned().collect();
            return (!kept.is_empty()).then_some(TicketsEvent::WaveCycle { ids: kept });
        }
        // ACT-2: a write over many tickets, of those the reach holds.
        TicketsEvent::UpdatedMany { ids, changed, tickets, unchanged } => {
            let kept: Vec<String> = ids.iter().filter(|id| sees(id)).cloned().collect();
            let tickets: Vec<crate::types::Ticket> = tickets.iter().filter(|t| sees(&t.id)).cloned().collect();
            let unchanged: Vec<String> = unchanged.iter().filter(|id| sees(id)).cloned().collect();
            return (!kept.is_empty() || !tickets.is_empty())
                .then(|| TicketsEvent::UpdatedMany { ids: kept, changed: changed.clone(), tickets, unchanged });
        }
        // ACT-5: a deleted ticket is no member of any reach any more — its
        // shortcode's is the reach's word; a ticket brought back, its own.
        TicketsEvent::Deleted { ids, tombstones } => {
            // ORG-3: a deleted ticket's shortcode is the one its key was
            // minted in (a deleted ticket never moves).
            let held: Vec<crate::address::ShortcodeId> = store.counts(reach).into_iter().filter(|(_, n)| *n > 0).map(|(e, _)| e).collect();
            let seen = |id: &str| id.rsplit_once('-').is_some_and(|(e, _)| held.iter().any(|h| h.as_str() == e));
            let ids: Vec<String> = ids.iter().filter(|id| seen(id)).cloned().collect();
            let tombstones = tombstones.iter().filter(|t| seen(&t.id)).cloned().collect();
            return (!ids.is_empty()).then_some(TicketsEvent::Deleted { ids, tombstones });
        }
        TicketsEvent::Undeleted { ids, tickets } => {
            let ids: Vec<String> = ids.iter().filter(|id| sees(id)).cloned().collect();
            let tickets = tickets.iter().filter(|t| sees(&t.id)).cloned().collect();
            return (!ids.is_empty()).then_some(TicketsEvent::Undeleted { ids, tickets });
        }
        TicketsEvent::Hit { summary, .. } => Some(summary.id.clone()),
        // MENGER-39: a queue write, a queue row and a history line are their
        // ticket's.
        TicketsEvent::Queued { entry, .. } | TicketsEvent::Snoozed { entry, .. } | TicketsEvent::QueueRow { entry, .. } => Some(entry.ticket.clone()),
        TicketsEvent::Unqueued { id, .. } | TicketsEvent::NotQueued { id } => Some(id.clone()),
        TicketsEvent::QueueRecord { record } => Some(record.ticket.clone()),
        TicketsEvent::DanglingBlocker { id } | TicketsEvent::Member { id, .. } | TicketsEvent::NotFound { id } => Some(id.clone()),
        TicketsEvent::Extent { shortcode, .. } | TicketsEvent::Count { shortcode, .. } | TicketsEvent::Recount { shortcode, .. } => {
            let seen = store.counts(reach).into_iter().any(|(e, n)| e == *shortcode && n > 0);
            return seen.then_some(ev);
        }
        // Without the owner in hand, nothing (Sight::relay places them).
        TicketsEvent::Component { .. } | TicketsEvent::ComponentChanged { .. } | TicketsEvent::ComponentRemoved { .. } => return None,
        // Name no ticket and no shortcode: answers worked out under the reach.
        TicketsEvent::Health { .. }
        | TicketsEvent::Page { .. }
        | TicketsEvent::Lagged { .. }
        | TicketsEvent::Facet { .. }
        | TicketsEvent::WaveCounts { .. }
        | TicketsEvent::Error { .. }
        // ORG-3: a refusal, its candidates the store's (ORG-4 gives the
        // resolver the caller's reach).
        | TicketsEvent::Ambiguous { .. }
        | TicketsEvent::BodyTooLarge { .. }
        | TicketsEvent::Searched { .. }
        | TicketsEvent::HistoryEnd { .. }
        | TicketsEvent::QueueEnd { .. } => None,
    };
    match id {
        Some(id) if !sees(&id) => None,
        _ => Some(ev),
    }
}

/// IDENTITY-21: the lens a caller sees tickets through in this call —
/// `under` where it holds the call's grant (AUTH-7) — `None`: all; `Err`:
/// nothing. IDENTITY-37: a reach that does not compile is refused, never
/// read as "all".
pub(super) fn view_lens(caller: &Caller) -> Result<Option<Lens>, String> {
    caller.reach_filter()?.map(|f| Lens::of(&f).map_err(|e| e.to_string())).transpose()
}

/// AUTH-7: the lens a write sees the tickets it names through — where the
/// caller holds `tickets.get` ([`crate::access::SEES`]).
pub(super) fn sight_lens(caller: &Caller) -> Result<Option<Lens>, String> {
    caller.sight_filter()?.map(|f| Lens::of(&f).map_err(|e| e.to_string())).transpose()
}

/// FILTER-24: a read's `keep` — the id of the ticket it names, when the
/// caller's reach covers it: keeping never answers a ticket the caller
/// could not read. Blank, naming no ticket, or outside the reach: `None`
/// (the read answers as without it).
pub(super) fn kept(store: &Store, caller: &Caller, keep: Option<&str>) -> Option<String> {
    let id = store.ticket_id(keep.map(str::trim).filter(|k| !k.is_empty())?)?;
    match view_lens(caller) {
        Ok(None) => Some(id),
        Ok(Some(reach)) => store.member(std::slice::from_ref(&id), &reach).first().copied().unwrap_or(false).then_some(id),
        Err(_) => None,
    }
}

/// Whether `reach` admits the ticket `id` (`None`: everything does).
pub(super) fn sees_id(store: &Store, reach: Option<&Lens>, id: &str) -> bool {
    match reach {
        None => true,
        Some(lens) => store.member(&[id.to_string()], lens).first().copied().unwrap_or(false),
    }
}

/// IDENTITY-37: of `components`, those `reach` may see — each whose owner
/// it admits (an orphan has none: it is no one's to show) — with only the
/// references it may see: the tickets it admits, and the components it may
/// see. Owners and references are asked of the cells in one batch.
pub(super) fn visible_components(store: &Store, reach: Option<&Lens>, components: Vec<crate::types::Component>) -> Vec<crate::types::Component> {
    let Some(lens) = reach else { return components };
    // Every component's owner, to place a reference to one by its owner.
    let owners: std::collections::HashMap<String, Option<String>> = store.components().into_iter().map(|c| (c.name, c.owner)).collect();
    let mut ids: Vec<String> = owners.values().flatten().cloned().collect();
    ids.extend(components.iter().filter_map(|c| c.owner.clone()));
    ids.extend(components.iter().flat_map(|c| c.references.iter()).filter(|r| !owners.contains_key(*r)).cloned());
    ids.sort();
    ids.dedup();
    let admitted: std::collections::HashSet<String> =
        ids.iter().zip(store.member(&ids, lens)).filter(|(_, ok)| *ok).map(|(id, _)| id.clone()).collect();
    let owned_in_reach = |o: &Option<String>| o.as_ref().is_some_and(|o| admitted.contains(o));
    components
        .into_iter()
        .filter(|c| owned_in_reach(&c.owner))
        .map(|mut c| {
            c.references.retain(|r| match owners.get(r) {
                Some(owner) => owned_in_reach(owner),
                None => admitted.contains(r),
            });
            c
        })
        .collect()
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
    let veil = |ids: Vec<String>| -> Vec<String> { ids.into_iter().map(|i| if sees(&i) { i } else { HIDDEN.to_string() }).collect() };
    match e {
        StoreError::Cycle { id, path } => StoreError::Cycle { id, path: veil(path) }.to_string(),
        StoreError::DocAwaited { id, by } => StoreError::DocAwaited { id, by: veil(by) }.to_string(),
        // MENGER-30: `update`'s cycle, and each of its refusals.
        StoreError::BlockCycle { id, path } => StoreError::BlockCycle { id, path: veil(path) }.to_string(),
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

/// IDENTITY-21: the shortcodes a caller can see — `None`: all of them. A
/// grant covers whole shortcodes (`under`), so a shortcode is visible
/// exactly when the caller's reach admits any of its tickets.
///
/// ORG-3: by the shortcodes' identities.
pub(super) fn visible_epics(store: &Store, caller: &Caller) -> Result<Option<std::collections::HashSet<crate::address::ShortcodeId>>, String> {
    epics_under(store, caller.reach_filter()?)
}

/// AUTH-7: [`visible_epics`] by where `tickets.get` is held — what a write
/// may name.
pub(super) fn sight_epics(store: &Store, caller: &Caller) -> Result<Option<std::collections::HashSet<crate::address::ShortcodeId>>, String> {
    epics_under(store, caller.sight_filter()?)
}

pub(super) fn epics_under(store: &Store, reach: Option<crate::filter::Filter>) -> Result<Option<std::collections::HashSet<crate::address::ShortcodeId>>, String> {
    let Some(reach) = reach else { return Ok(None) };
    let lens = Lens::of(&reach).map_err(|e| e.to_string())?;
    Ok(Some(store.counts(&lens).into_iter().filter(|(_, n)| *n > 0).map(|(e, _)| e).collect()))
}

/// IDENTITY-21: how many tickets a caller can see.
pub(super) fn visible_count(store: &Store, caller: &Caller) -> u64 {
    match caller.reach_filter() {
        Ok(None) => store.count() as u64,
        Ok(Some(reach)) => Lens::of(&reach).map(|lens| store.counts(&lens).into_iter().map(|(_, n)| n).sum()).unwrap_or(0),
        Err(_) => 0,
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
