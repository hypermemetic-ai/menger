//! MENGER-29: the counts behind the board and the menus — `counts` and
//! `facets` as their streams, moved out of the activation's file so it keeps
//! its methods (ACT-5 made the room for `delete` and `undelete`).

use super::*;

/// FILTER-4: `counts` — each shortcode's matches of a filter (the activation's doc).
pub(super) fn counts_stream(store: Arc<Store>, admitted: Result<Caller, String>, filter: Option<FilterText>, keep: Option<TicketRef>, tz: Option<Zone>) -> impl Stream<Item = TicketsEvent> + Send + 'static {
    stream! {
        let caller = match admitted {
            Ok(caller) => caller,
            Err(message) => {
                yield TicketsEvent::Error { message };
                return;
            }
        };
        let filter = match caller.scoped(filter.as_deref()) {
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
        match store.lens_or_all(filter.as_deref()) {
            Ok(lens) => {
                let lens = lens.with_clock(clock);
                let keep = kept(&store, &caller, keep.as_deref());
                // ORG-3: a count per shortcode, by its identity — its
                // letters beside it; an `under` naming several is refused.
                match store.counts_read(&lens, keep.as_deref()) {
                    Ok(counts) => {
                        for (shortcode, held) in counts {
                            let epic = store.shortcode_named_by(&shortcode).0;
                            yield TicketsEvent::Count { epic, shortcode, held };
                        }
                    }
                    Err(e) => yield veiled(&caller, e),
                }
            }
            Err(e) => yield veiled(&caller, e),
        }
    }
}

/// FILTER-4: `facets` — the menus' counts (the activation's doc).
pub(super) fn facets_stream(store: Arc<Store>, admitted: Result<Caller, String>, filter: Option<FilterText>, base: Option<FilterText>, tz: Option<Zone>) -> impl Stream<Item = TicketsEvent> + Send + 'static {
    stream! {
        let caller = match admitted {
            Ok(caller) => caller,
            Err(message) => {
                yield TicketsEvent::Error { message };
                return;
            }
        };
        let filter = match caller.scoped(filter.as_deref()) {
            Ok(v) => v,
            Err(message) => {
                yield TicketsEvent::Error { message };
                return;
            }
        };
        let base = match caller.scoped(base.as_deref()) {
            Ok(v) => v,
            Err(message) => {
                yield TicketsEvent::Error { message };
                return;
            }
        };
        // FILTER-36: each read with its saved names resolved.
        let read = |text: Option<String>| -> Result<crate::filter::Filter, String> { store.resolved(text.as_deref()) };
        let answer = clock_of(&caller, tz.as_deref()).and_then(|clock| {
            read(filter)
                .and_then(|f| Ok((f, read(base)?)))
                .and_then(|(f, b)| store.facets_at(&f, &b, &clock).map_err(|e| e.to_string()))
        });
        match answer {
            Ok(facets) => {
                for (field, value, held) in facets {
                    // ACCESS-8: a field of the store's own list, always.
                    if let Ok(field) = field.parse() {
                        yield TicketsEvent::Facet { field, value, held };
                    }
                }
            }
            Err(message) => yield TicketsEvent::Error { message },
        }
    }
}
