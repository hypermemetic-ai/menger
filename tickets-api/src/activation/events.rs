//! MENGER-29: the tickets activation's events — how a store answer or a
//! change becomes the wire's [`TicketsEvent`]s. Moved out of
//! `activation.rs` as they were, so that file holds the methods alone.

use super::*;

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
                    yield TicketsEvent::Error { message: e.to_string() };
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
        Change::CellsMoved { .. } => return None,
        // FILTER-13: each `watch { filter }` asking `under` takes its own.
        Change::UnderMoved { .. } => return None,
    })
}

/// FILTER-13: one page of `tree` as its events — a `node` per row, then one
/// `page` (no `first`); a refusal is the one in-band `error`, and nothing
/// else.
pub(super) fn tree_events(page: Result<TreePage, StoreError>) -> Vec<TicketsEvent> {
    match page {
        Err(e) => vec![TicketsEvent::Error { message: e.to_string() }],
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
pub(super) fn membership_event(m: Membership) -> TicketsEvent {
    match m {
        Membership::Entered { id, epic, seq } => TicketsEvent::Entered { id, epic, seq },
        Membership::Left { id, epic, seq } => TicketsEvent::Left { id, epic, seq },
        Membership::Recount { epic, held } => TicketsEvent::Recount { epic, held },
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
        Err(StoreError::NotFound(_)) => TicketsEvent::NotFound { id: ticket },
        Err(e) => TicketsEvent::Error { message: e.to_string() },
    }
}
