//! The reads as their streams, beside the activation so its file keeps
//! only its methods (MENGER-29, ACCESS-13): `get`, `list`, `ready_set`,
//! `find`, `search`, `member`, `inputs`, `tree`, the overview's plain
//! listings, `blockers`, and `history` — MENGER-13's, with MENGER-39's
//! queue records after its transitions. Each takes the caller the frame's
//! resolver verified.

use super::*;

/// `get` — one ticket by any address, in the caller's reach: `ticket` (ACT-6:
/// with `moved_from` when an old id named it), a deleted one's tombstone
/// (ACT-5), or `not_found`.
pub(super) fn get_stream(store: Arc<Store>, caller: VerifiedUser, id: String) -> impl Stream<Item = TicketsEvent> + Send + 'static {
    stream! {
        // ORG-4: a short form read in the caller's organizations.
        let named = named_in(&store, &caller, id.clone());
        // ACCESS-10: a ticket out of reach is not found, as one not there.
        match store.get(&caller, &named).await {
            Ok(ticket) => yield TicketsEvent::Ticket { ticket },
            // ACT-5: a deleted ticket answers its tombstone.
            Err(StoreError::NotFound(_)) => yield store.tombstone(&caller, &id).map_or(TicketsEvent::NotFound { id: Asked::stored(id.as_str()) }, |t| TicketsEvent::Deleted { ids: vec![t.id.clone()], tombstones: vec![t] }),
            Err(e) => yield veiled(&caller, e),
        }
    }
}

/// `blockers` — the ticket's blockers in its reach, each resolved or
/// dangling.
pub(super) fn blockers_stream(store: Arc<Store>, caller: VerifiedUser, id: String) -> impl Stream<Item = TicketsEvent> + Send + 'static {
    stream! {
        // ACCESS-10: the blockers in reach, a hidden one not named.
        match store.blockers(&caller, &id).await {
            Ok(refs) => {
                for r in refs {
                    yield match r {
                        BlockerRef::Resolved(ticket) => TicketsEvent::Blocker { ticket },
                        BlockerRef::Dangling(id) => TicketsEvent::DanglingBlocker { id },
                    };
                }
            }
            Err(StoreError::NotFound(_)) => yield TicketsEvent::NotFound { id: Asked::stored(id.as_str()) },
            Err(e) => yield veiled(&caller, e),
        }
    }
}

/// MENGER-13: `history` — the ticket's transitions, those they caused where
/// the caller can see, MENGER-39: its queue history, then `history_end`.
pub(super) fn history_stream(store: Arc<Store>, caller: VerifiedUser, id: String) -> impl Stream<Item = TicketsEvent> + Send + 'static {
    stream! {
        // ACCESS-10: what it caused and what a restore puts back, in reach.
        match store.history(&caller, &id) {
            Ok(h) => {
                for transition in h.own {
                    yield TicketsEvent::Transition { transition };
                }
                for transition in h.caused {
                    yield TicketsEvent::Transition { transition };
                }
                // MENGER-39: who queued it, why, at what urgency.
                for record in store.queue_history(&caller, &id) {
                    yield TicketsEvent::QueueRecord { record };
                }
                let (status_since_unix, status_since_exact) = (h.since.map(|s| s.0), h.since.map(|s| s.1));
                yield TicketsEvent::HistoryEnd { began_unix: h.began, restorable: h.restorable, status_since_unix, status_since_exact, deletions: h.deletions };
            }
            Err(StoreError::NotFound(_)) => yield TicketsEvent::NotFound { id: Asked::stored(id.as_str()) },
            Err(e) => yield veiled(&caller, e),
        }
    }
}

/// What a `list` asks (MNG-9, FILTER-4, FILTER-24).
pub(super) struct ListAsk {
    pub(super) epic: Option<ShortcodeRef>,
    pub(super) status: Option<Status>,
    pub(super) parent: Option<TicketRef>,
    pub(super) after: Option<Cursor>,
    pub(super) limit: Option<u32>,
    pub(super) summary: Option<bool>,
    pub(super) filter: Option<FilterText>,
    pub(super) at: Option<u64>,
    pub(super) keep: Option<TicketRef>,
    pub(super) tz: Option<Zone>,
}

/// `list` — a page of the caller's tickets: by epic, status and parent, or
/// (FILTER-4) from the cells under a filter or at a position.
pub(super) fn list_stream(store: Arc<Store>, caller: VerifiedUser, ask: ListAsk) -> impl Stream<Item = TicketsEvent> + Send + 'static {
    let ListAsk { epic, status, parent, after, limit, summary, filter, at, keep, tz } = ask;
    let (epic, parent) = (epic.map(String::from), parent.map(String::from));
    let limit = Some(crate::limits::page(limit, crate::limits::LIST_PAGE_DEFAULT, crate::limits::LIST_PAGE_MAX));
    stream! {
        // ACCESS-10: the caller's own filter; the store answers it within
        // the caller's reach.
        let filter = match asked(filter.as_deref()) {
            Ok(v) => v,
            Err(message) => {
                yield TicketsEvent::Error { message };
                return;
            }
        };
        if filter.is_some() || at.is_some() {
            // FILTER-4: from the cells.
            if after.is_some() && at.is_some() {
                yield TicketsEvent::Error { message: StoreError::AtAndAfter.to_string() };
                return;
            }
            let lens = match store.lens_or_all(filter.as_ref()) {
                Ok(lens) => lens,
                Err(e) => {
                    yield veiled(&caller, e);
                    return;
                }
            };
            let lens = match clock_of(&caller, tz.as_deref()) {
                Ok(clock) => lens.with_clock(clock),
                Err(message) => {
                    yield TicketsEvent::Error { message };
                    return;
                }
            };
            let keep = filter.is_some().then(|| keep.map(String::from)).flatten();
            let q = Filtered { lens, epic, status, parent, keep };
            if summary.unwrap_or(false) {
                for ev in page_events(store.filtered_page(&caller, &q, after.as_deref(), at, limit)) {
                    yield ev;
                }
            } else {
                let walk = store.walk(&caller, Select::Filtered(q), after.as_deref(), limit).at(at);
                let mut events = Box::pin(walk_events(store, walk));
                while let Some(ev) = events.next().await {
                    yield ev;
                }
            }
            return;
        }
        if summary.unwrap_or(false) {
            let page = store.page(&caller, PageQuery {
                epic: epic.as_deref(),
                status,
                parent: parent.as_deref(),
                after: after.as_deref(),
                limit,
            });
            for ev in page_events(page) {
                yield ev;
            }
        } else {
            let walk = store.walk(&caller, Select::List { epic, status, parent }, after.as_deref(), limit);
            let mut events = Box::pin(walk_events(store, walk));
            while let Some(ev) = events.next().await {
                yield ev;
            }
        }
    }
}

/// What a `ready_set` asks (MNG-9, FILTER-15).
pub(super) struct ReadyAsk {
    pub(super) epic: Option<ShortcodeRef>,
    pub(super) after: Option<Cursor>,
    pub(super) limit: Option<u32>,
    pub(super) summary: Option<bool>,
    pub(super) filter: Option<FilterText>,
    pub(super) tz: Option<Zone>,
}

/// `ready_set` — what can start now, in the caller's reach; under a
/// filter, `ready and (filter)` from the cells.
pub(super) fn ready_stream(store: Arc<Store>, caller: VerifiedUser, ask: ReadyAsk) -> impl Stream<Item = TicketsEvent> + Send + 'static {
    let ReadyAsk { epic, after, limit, summary, filter, tz } = ask;
    let epic = epic.map(String::from);
    stream! {
        let filter = match asked(filter.as_deref()) {
            Ok(v) => v,
            Err(message) => {
                yield TicketsEvent::Error { message };
                return;
            }
        };
        let clock = match clock_of(&caller, tz.as_deref()) {
            Ok(c) => c,
            Err(message) => {
                yield TicketsEvent::Error { message };
                return;
            }
        };
        // FILTER-15: from the cells.
        match store.narrowing(filter.as_ref()).and_then(|f| f.map(|f| ready_and(&f, epic.clone())).transpose()) {
            Err(e) => {
                yield veiled(&caller, e);
                return;
            }
            Ok(Some(q)) => {
                let q = Filtered { lens: q.lens.with_clock(clock), ..q };
                if summary.unwrap_or(false) {
                    for ev in page_events(store.ready_filtered_page(&caller, &q, after.as_deref(), limit)) {
                        yield ev;
                    }
                } else {
                    let walk = store.walk(&caller, Select::Filtered(q), after.as_deref(), limit);
                    let mut events = Box::pin(walk_events(store, walk));
                    while let Some(ev) = events.next().await {
                        yield without_first(ev);
                    }
                }
                return;
            }
            Ok(None) => {}
        }
        if summary.unwrap_or(false) {
            for ev in page_events(store.ready_page(&caller, epic.as_deref(), after.as_deref(), limit)) {
                yield ev;
            }
        } else {
            // ACCESS-10: a restricted reader's is the filtered walk of its
            // reach — its page, as every ready set's, without `first`.
            let walk = store.walk(&caller, Select::Ready { epic }, after.as_deref(), limit);
            let mut events = Box::pin(walk_events(store, walk));
            while let Some(ev) = events.next().await {
                yield without_first(ev);
            }
        }
    }
}

/// `find` — the jump palette's question (SCALE-6), FILTER-15: of the
/// candidates a filter's cells admit.
pub(super) fn find_stream(
    store: Arc<Store>,
    caller: VerifiedUser,
    query: Query,
    after: Option<Cursor>,
    limit: Option<u32>,
    filter: Option<FilterText>,
    tz: Option<Zone>,
) -> impl Stream<Item = TicketsEvent> + Send + 'static {
    let limit = Some(limit.unwrap_or(FIND_LIMIT));
    let page = asked(filter.as_deref()).map_err(StoreError::Filter).and_then(|filter| match zoned_lens(&store, &caller, filter.as_ref(), tz.as_deref())? {
        // FILTER-15: the candidates the cells admit.
        Some(lens) => store.find_filtered(&caller, &query, &lens, after.as_deref(), limit),
        None => store.find_page(&caller, &query, after.as_deref(), limit),
    });
    futures::stream::iter(page_events(page))
}

/// `search` — SEARCH-2's hits in the caller's reach, then `searched`.
pub(super) fn search_stream(
    store: Arc<Store>,
    caller: VerifiedUser,
    query: Query,
    filter: Option<FilterText>,
    limit: Option<u32>,
    offset: Option<u32>,
    tz: Option<Zone>,
) -> impl Stream<Item = TicketsEvent> + Send + 'static {
    stream! {
        // IDENTITY-21: only what this caller can see — its filter ANDed
        // with the shortcodes it holds a role on, as `find`'s is.
        let filter = match asked(filter.as_deref()) {
            Ok(v) => v,
            Err(message) => {
                yield TicketsEvent::Error { message };
                return;
            }
        };
        let lens = match zoned_lens(&store, &caller, filter.as_ref(), tz.as_deref()) {
            Ok(lens) => lens,
            Err(e) => {
                yield veiled(&caller, e);
                return;
            }
        };
        let limit = Some(limit.unwrap_or(SEARCH_LIMIT).min(SEARCH_MAX));
        match store.search(&caller, &query, lens, limit, offset).await {
            Ok(SearchPage { hits, matched, next, words }) => {
                for hit in hits {
                    yield TicketsEvent::Hit { summary: hit.summary, title: hit.title, snippet: hit.snippet, corrected: hit.corrected };
                }
                yield TicketsEvent::Searched { matched, next, words };
            }
            Err(e) => yield veiled(&caller, e),
        }
    }
}

/// `member` — which of `ids` a filter admits (FILTER-15), one lookup each.
pub(super) fn member_events(store: &Store, caller: &VerifiedUser, named: Vec<TicketRef>, filter: FilterText, tz: Option<Zone>) -> Vec<TicketsEvent> {
    // ACCESS-48: each echoed as asked, a `TicketRef`.
    let ids = texts(named.clone());
    let filter = match asked(Some(&filter)) {
        Ok(v) => v.unwrap_or_default(),
        Err(message) => return vec![TicketsEvent::Error { message }],
    };
    if ids.len() > MEMBER_MAX {
        return vec![TicketsEvent::Error { message: format!("member: at most {MEMBER_MAX} ids a call — {} were asked; ask in parts", ids.len()) }];
    }
    let lens = match zoned_lens(store, caller, Some(&filter), tz.as_deref()) {
        Ok(lens) => lens.unwrap_or_else(Lens::all),
        Err(e) => return vec![veiled(caller, e)],
    };
    let matched = store.member(caller, &ids, &lens);
    named.into_iter().zip(matched).map(|(id, matched)| TicketsEvent::Member { id, matched }).collect()
}

/// `inputs` — INPUT-3's inputs in the caller's reach; FILTER-15: of the
/// tickets a filter admits, by the cells.
pub(super) fn inputs_events(
    store: &Store,
    caller: &VerifiedUser,
    ticket: Option<TicketRef>,
    epic: Option<ShortcodeRef>,
    state: Option<InputState>,
    filter: Option<FilterText>,
    tz: Option<Zone>,
) -> Vec<TicketsEvent> {
    let filter = match asked(filter.as_deref()) {
        Ok(v) => v,
        Err(message) => return vec![TicketsEvent::Error { message }],
    };
    let answer = match zoned_lens(store, caller, filter.as_ref(), tz.as_deref()) {
        Err(e) => Err(e),
        Ok(Some(lens)) => store.inputs_filtered(caller, ticket.as_deref(), epic.as_deref(), state, &lens),
        Ok(None) => store.inputs(caller, ticket.as_deref(), epic.as_deref(), state),
    };
    match answer {
        Ok(inputs) => inputs.into_iter().map(|input| TicketsEvent::Input { input }).collect(),
        // ACCESS-10: a ticket out of reach is not found, as one not there.
        Err(StoreError::NotFound(id)) => vec![TicketsEvent::NotFound { id: ticket.map_or_else(|| Asked::stored(id), Asked::from) }],
        Err(e) => vec![veiled(caller, e)],
    }
}

/// What a `tree` asks (FILTER-13, FILTER-23, FILTER-24).
pub(super) struct TreeAsk {
    pub(super) parent: Option<TicketRef>,
    pub(super) epic: Option<ShortcodeRef>,
    pub(super) filter: FilterText,
    pub(super) after: Option<Cursor>,
    pub(super) limit: Option<u32>,
    pub(super) matches_only: Option<bool>,
    pub(super) keep: Option<TicketRef>,
    pub(super) tz: Option<Zone>,
}

/// `tree` — the nav under a filter, from the kept tree.
pub(super) fn tree_events(store: &Store, caller: &VerifiedUser, ask: TreeAsk) -> Vec<TicketsEvent> {
    let TreeAsk { parent, epic, filter, after, limit, matches_only, keep, tz } = ask;
    let filter = match asked(Some(&filter)) {
        Ok(v) => v.unwrap_or_default(),
        Err(message) => return vec![TicketsEvent::Error { message }],
    };
    let lens = match store.lens_of(&filter) {
        Ok(lens) => lens,
        Err(e) => return vec![veiled(caller, e)],
    };
    let lens = match clock_of(caller, tz.as_deref()) {
        Ok(clock) => lens.with_clock(clock),
        Err(message) => return vec![TicketsEvent::Error { message }],
    };
    // ACCESS-10: kept only when in the caller's reach (the store's).
    let q = TreeQuery { parent: parent.map(String::from), epic: epic.map(String::from), lens, matches_only: matches_only.unwrap_or(false), keep: keep.map(String::from) };
    tree_page_events(store.tree_page(caller, &q, after.as_deref(), limit))
}

/// A listing the overview answers whole (`groups`, `extents`,
/// `components`): `f`'s events, once the caller's reach is one a read can
/// be made of — else its refusal. Answered at the call, from the cache.
pub(super) fn listed(store: &Store, caller: &VerifiedUser, f: impl FnOnce(&Store, &VerifiedUser) -> Vec<TicketsEvent>) -> impl Stream<Item = TicketsEvent> + Send + 'static {
    let events = match caller.read_filter() {
        Err(message) => vec![TicketsEvent::Error { message }],
        Ok(_) => f(store, caller),
    };
    futures::stream::iter(events)
}
