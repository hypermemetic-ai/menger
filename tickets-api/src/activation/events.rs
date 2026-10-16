//! MENGER-29: the tickets activation's events — how a store answer or a
//! change becomes the wire's [`TicketsEvent`]s. Moved out of
//! `activation.rs` as they were, so that file holds the methods alone.

use super::*;
use crate::store::QueueChange;

/// MNG-9: one page of the store, as the events that carry it — a row per
/// ticket and then exactly one `page` event, which is what tells a client
/// whether to ask for more. A failed cursor is the one in-band `error`
/// instead, with no page event: nothing was walked, so there is nowhere to
/// resume from. SCALE-14: the `summary` shape — a page of the cache's rows,
/// as they are; whole tickets come from [`walk_events`].
pub(super) fn page_events(page: Result<Page, StoreError>) -> Box<dyn Iterator<Item = TicketsEvent> + Send> {
    let page = match page {
        Ok(page) => page,
        Err(e) => {
            return Box::new(std::iter::once(TicketsEvent::Error {
                message: e.to_string(),
            }))
        }
    };
    Box::new(
        page.tickets
            .into_iter()
            .map(|summary| TicketsEvent::Summary { summary })
            .chain(std::iter::once(TicketsEvent::Page {
                next: page.next,
                remaining: page.remaining,
                first: page.first,
            })),
    )
}

/// SCALE-14: the same events with whole tickets — a `ticket` row each,
/// then the one `page` event — for `list` and `ready_set` without
/// `summary`. Streamed from a [`Walk`], a page of bodies at a time, so a
/// `list {}` of the whole store holds one page of bodies, not every one. A
/// failed cursor is still the one in-band `error` and nothing else (the
/// walk's first step); a read that fails part way ends the rows with an
/// `error` in place of the page event.
pub(super) fn walk_events(store: Arc<Store>, mut walk: Walk) -> impl Stream<Item = TicketsEvent> + Send {
    stream! {
        loop {
            match walk.next(&store).await {
                Ok(Step::Tickets(tickets)) => {
                    for ticket in tickets {
                        yield TicketsEvent::Ticket { ticket };
                    }
                }
                Ok(Step::End { next, remaining, first }) => {
                    yield TicketsEvent::Page { next, remaining, first };
                    break;
                }
                Err(e) => {
                    yield refusal(e);
                    break;
                }
            }
        }
    }
}

/// MENGER-13, FILTER-27: a `watch` whose filter moves with the clock
/// (`within 1h`, `today`, `for over 2w`, `before 3d ago`) sleeps until the
/// next moment time moves it ([`crate::store::Watcher::wait`]: a ticket
/// crossing a length, a transition leaving a window, a new day) — one timer
/// per watch — and reads it again then; at most this long, when no moment
/// is in sight.
pub(super) const AGE_AT_MOST: std::time::Duration = std::time::Duration::from_secs(3600);

/// SCALE-6: how many rows `find` answers when no `limit` is asked for —
/// the 50 the jump palette shows (AGS-18), so a keystroke's answer is a
/// page, not the store.
pub(super) const FIND_LIMIT: u32 = 50;

/// FILTER-15: a `ready_set { filter }` page event as every `ready_set` page
/// is — without `first`, which only `list` gives (FILTER-4).
pub(super) fn without_first(ev: TicketsEvent) -> TicketsEvent {
    match ev {
        TicketsEvent::Page { next, remaining, .. } => TicketsEvent::Page { next, remaining, first: None },
        ev => ev,
    }
}

/// A change as its one wire event — `None` for FILTER-4's cell moves, which
/// are never sent as themselves: `watch { filter }` turns them into
/// `entered`, `left` and `recount` for its filter.
pub(super) fn wire_event(change: Change) -> Option<TicketsEvent> {
    Some(match change {
        Change::Created { ticket } => TicketsEvent::Created { ticket },
        Change::StatusChanged { ticket, previous } => {
            TicketsEvent::StatusChanged { ticket, previous }
        }
        Change::KindChanged { ticket, previous } => TicketsEvent::KindChanged { ticket, previous },
        Change::Amended { ticket, changed } => TicketsEvent::Amended { ticket, changed },
        Change::CommitsLinked { ticket, added } => TicketsEvent::CommitsLinked { ticket, added },
        Change::CommitsUnlinked { ticket, removed } => {
            TicketsEvent::CommitsUnlinked { ticket, removed }
        }
        Change::LabelsChanged { ticket, added, removed } => TicketsEvent::LabelsChanged { ticket, added, removed },
        Change::UnlocksChanged { ticket, added } => TicketsEvent::UnlocksChanged { ticket, added },
        // MENGER-30: `update`'s.
        Change::UnlocksRemoved { ticket, removed } => TicketsEvent::UnlocksRemoved { ticket, removed },
        Change::BlockersChanged { ticket, added, removed } => TicketsEvent::BlockersChanged { ticket, added, removed },
        Change::SeverityChanged { ticket, previous } => TicketsEvent::SeverityChanged { ticket, previous },
        // ACT-2: an `update_many`'s write, after its tickets' own events.
        Change::UpdatedMany { ids, changed } => TicketsEvent::UpdatedMany { ids, changed, tickets: Vec::new(), unchanged: Vec::new() },
        // ACT-5: a delete's and an undelete's, as their answers are.
        Change::Deleted { ids, tombstones } => TicketsEvent::Deleted { ids, tombstones },
        Change::Undeleted { ids, tickets } => TicketsEvent::Undeleted { ids, tickets },
        Change::Archived { ids, live, unqueued, mark } => TicketsEvent::Archived { ids, live, unqueued, mark, dry_run: false },
        Change::Restored { ids, ancestors, still_archived, by } => TicketsEvent::Restored { ids, ancestors, still_archived, by, dry_run: false },
        // ACT-6: a reshortcode's, as its answer is.
        Change::Reshortcoded { moves, tickets } => TicketsEvent::Reshortcoded { moves, tickets },
        // SCALE-4: the parent's shortcode filtered `watch { epic }` on the
        // way here; the wire carries the id and the count.
        Change::ChildrenChanged {
            id,
            added,
            child_count,
            ..
        } => TicketsEvent::ChildrenChanged {
            id,
            added,
            child_count,
        },
        Change::ParentChanged { ticket, previous } => {
            TicketsEvent::ParentChanged { ticket, previous }
        }
        Change::ChildRemoved {
            id,
            removed,
            child_count,
            ..
        } => TicketsEvent::ChildRemoved {
            id,
            removed,
            child_count,
        },
        Change::GroupChanged { group } => TicketsEvent::GroupChanged { group },
        // WAVE-6: each change's shortcode filtered `watch { epic }` on the
        // way here; the wire carries the id and the placement.
        Change::PlacementsChanged { changes } => TicketsEvent::PlacementsChanged {
            changes: changes.into_iter().map(|(_, change)| change).collect(),
        },
        // COMP-3: past any `epic` filter — a component is in no shortcode.
        Change::ComponentChanged { component } => TicketsEvent::ComponentChanged { component },
        Change::ComponentRemoved { name } => TicketsEvent::ComponentRemoved { name },
        // INPUT-3: its ticket's shortcode filtered `watch { epic }` on the
        // way here; the wire carries the input.
        Change::InputChanged { input, .. } => TicketsEvent::InputChanged { input },
        // MENGER-13: its ticket's shortcode filtered `watch { epic }` on the
        // way here; the wire carries the transition.
        Change::Transition { transition, .. } => TicketsEvent::Transition { transition },
        // MENGER-39: its ticket's shortcode filtered `watch { epic }` on the
        // way here; the wire carries the queue write.
        Change::Queue { change, .. } => match change {
            QueueChange::Queued { entry, ticket, action } => TicketsEvent::Queued { entry, ticket, action },
            QueueChange::Unqueued { id, entry, action, by, at_unix } => TicketsEvent::Unqueued { id, entry, action, by, at_unix },
            QueueChange::Snoozed { entry, ticket } => TicketsEvent::Snoozed { entry, ticket },
        },
        Change::CellsMoved { .. } => return None,
        // FILTER-13: each `watch { filter }` asking `under` takes its own.
        Change::UnderMoved { .. } => return None,
    })
}

/// FILTER-13: one page of `tree` as its events — a `node` per row, then one
/// `page` (no `first`); a refusal is the one in-band `error`, and nothing
/// else.
pub(super) fn tree_page_events(page: Result<TreePage, StoreError>) -> Vec<TicketsEvent> {
    match page {
        Err(e) => vec![refusal(e)],
        Ok(page) => page
            .nodes
            .into_iter()
            .map(|n| TicketsEvent::Node {
                summary: n.summary,
                matched: n.matched,
                below: n.below,
            })
            .chain(std::iter::once(TicketsEvent::Page {
                next: page.next,
                remaining: page.remaining,
                first: None,
            }))
            .collect(),
    }
}

/// FILTER-4 / FILTER-13: a `watch { filter }`'s membership events as the
/// wire carries them.
///
/// ORG-3: each with its shortcode's identity, and its letters beside it.
pub(super) fn membership_event(store: &Store, m: Membership) -> TicketsEvent {
    let code = |sc: &crate::address::ShortcodeId| store.shortcode_named_by(sc).0;
    match m {
        Membership::Entered { id, shortcode, seq } => TicketsEvent::Entered { id, epic: code(&shortcode), shortcode, seq },
        Membership::Left { id, shortcode, seq } => TicketsEvent::Left { id, epic: code(&shortcode), shortcode, seq },
        Membership::Recount { shortcode, held } => TicketsEvent::Recount { epic: code(&shortcode), shortcode, held },
    }
}

/// ORG-3: a refusal as its one event — `ambiguous { id, candidates }` for a
/// name that names more than one in reach (each by its full address), the
/// in-band `error` for every other.
pub(super) fn refusal(e: StoreError) -> TicketsEvent {
    match e {
        StoreError::Ambiguous { text, candidates } => TicketsEvent::Ambiguous { id: crate::vocab::Asked::stored(text), candidates },
        e => TicketsEvent::Error { message: e.to_string() },
    }
}

/// A write's own answer: the change it made, which is always one that
/// crosses the wire.
pub(super) fn change_event(change: Change) -> TicketsEvent {
    wire_event(change).expect("a write answers with a change the wire carries")
}

/// INPUT-3: a write to one input, as its one event — `input_changed` for a
/// change, `input` for one that changed nothing, `not_found` for a ticket
/// that is none, an in-band `error` for every refusal.
pub(super) fn input_event(written: Result<InputWrite, StoreError>, ticket: String) -> TicketsEvent {
    match written {
        Ok(InputWrite::Changed(input)) => TicketsEvent::InputChanged { input },
        Ok(InputWrite::Unchanged(input)) => TicketsEvent::Input { input },
        Err(StoreError::NotFound(_)) => TicketsEvent::NotFound { id: Asked::stored(ticket) },
        Err(e) => refusal(e),
    }
}

/// ACCESS-13: why a call answers other than with its event — the one shape
/// every refusal beside the activation takes, said by [`answer`].
pub(super) enum Failed {
    /// Already its one event (a permit's `not_found`, an ambiguity veiled,
    /// a refusal the store said for a name that is no ticket).
    Said(TicketsEvent),
    /// Refused in words — a proof not minted, a filter not read, a time
    /// zone not known.
    Refused(String),
    /// The store's refusal.
    Store(StoreError),
}

impl From<TicketsEvent> for Failed {
    fn from(ev: TicketsEvent) -> Self {
        Self::Said(ev)
    }
}

impl From<String> for Failed {
    fn from(message: String) -> Self {
        Self::Refused(message)
    }
}

impl From<StoreError> for Failed {
    fn from(e: StoreError) -> Self {
        Self::Store(e)
    }
}

impl From<crate::access::Refusal> for Failed {
    fn from(r: crate::access::Refusal) -> Self {
        Self::Refused(r.to_string())
    }
}

/// ACCESS-13: **the one helper** — a call's answer as its one event, every
/// failure said as `caller` may hear it: `not_found` for a ticket the store
/// does not have (named as `asked`, the id the call gave — a call that
/// names none hears the store's words), a body past the cap typed (`body_too_large`), an ambiguity veiled
/// to the caller's organizations, and every other refusal with the tickets
/// it names out of the caller's reach said as such ([`said`]).
pub(super) fn answer(store: &Store, caller: &VerifiedUser, asked: Option<&str>, r: Result<TicketsEvent, Failed>) -> TicketsEvent {
    match r {
        Ok(ev) | Err(Failed::Said(ev)) => ev,
        Err(Failed::Refused(message)) => TicketsEvent::Error { message },
        Err(Failed::Store(StoreError::NotFound(_))) if asked.is_some() => TicketsEvent::NotFound { id: Asked::stored(asked.unwrap_or_default()) },
        Err(Failed::Store(e @ StoreError::BodyTooLarge { .. })) => too_large(e),
        Err(Failed::Store(e @ StoreError::Ambiguous { .. })) => veiled(caller, e),
        Err(Failed::Store(e)) => said(store, caller, e),
    }
}

/// ACCESS-13: a method's body as its one answer — `answered!(store =
/// <Arc<Store>>, user, <asked: Option<String>> => <statements ending in
/// Ok(event)>)`: the statements run as one `async` block (each `?` turns a
/// refusal into a [`Failed`]), and what they end in is the stream's one
/// event, through [`answer`].
macro_rules! answered {
    ($store:ident = $at:expr, $user:ident, $asked:expr => $($body:tt)*) => {{
        let $store = $at;
        let __asked: Option<String> = $asked;
        async_stream::stream! {
            let __r: Result<TicketsEvent, Failed> = async { $($body)* }.await;
            yield answer(&$store, &$user, __asked.as_deref(), __r);
        }
    }};
}
pub(super) use answered;

/// ARCHIVE-2: [`answered!`] for a write that may bring an archived chain
/// back after it (a create or a move under an archived parent, ARCHIVE-1
/// `child-of-archived`): the body sets `$then` to the chain's `restored`,
/// yielded after the write's own event.
macro_rules! answered_then {
    ($store:ident = $at:expr, $user:ident, $asked:expr, $then:ident => $($body:tt)*) => {{
        let $store = $at;
        let __asked: Option<String> = $asked;
        async_stream::stream! {
            let mut $then: Option<TicketsEvent> = None;
            let __r: Result<TicketsEvent, Failed> = async { $($body)* }.await;
            yield answer(&$store, &$user, __asked.as_deref(), __r);
            if let Some(ev) = $then {
                yield ev;
            }
        }
    }};
}
pub(super) use answered_then;
