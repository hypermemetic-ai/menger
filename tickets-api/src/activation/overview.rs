//! MENGER-29: the counts behind the board and the menus — `counts` and
//! `facets` as their streams, moved out of the activation's file so it keeps
//! its methods (ACT-5 made the room for `delete` and `undelete`).

use super::*;

/// FILTER-4: `counts` — each shortcode's matches of a filter (the activation's doc).
pub(super) fn counts_stream(store: Arc<Store>, admitted: Result<VerifiedUser, String>, filter: Option<FilterText>, keep: Option<TicketRef>, tz: Option<Zone>) -> impl Stream<Item = TicketsEvent> + Send + 'static {
    stream! {
        let caller = match admitted {
            Ok(caller) => caller,
            Err(message) => {
                yield TicketsEvent::Error { message };
                return;
            }
        };
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
        match store.lens_or_all(filter.as_ref()) {
            Ok(lens) => {
                let lens = lens.with_clock(clock);
                // ORG-3: a count per shortcode, by its identity — its
                // letters beside it; an `under` naming several is refused.
                // ACCESS-10: within the caller's reach, `keep` only in it.
                match store.counts_read(&caller, &lens, keep.as_deref()) {
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
pub(super) fn facets_stream(store: Arc<Store>, admitted: Result<VerifiedUser, String>, filter: Option<FilterText>, base: Option<FilterText>, tz: Option<Zone>) -> impl Stream<Item = TicketsEvent> + Send + 'static {
    stream! {
        let caller = match admitted {
            Ok(caller) => caller,
            Err(message) => {
                yield TicketsEvent::Error { message };
                return;
            }
        };
        let filter = match asked(filter.as_deref()) {
            Ok(v) => v,
            Err(message) => {
                yield TicketsEvent::Error { message };
                return;
            }
        };
        let base = match asked(base.as_deref()) {
            Ok(v) => v,
            Err(message) => {
                yield TicketsEvent::Error { message };
                return;
            }
        };
        // FILTER-36: each read with its saved names resolved; ACCESS-10:
        // the store answers it within the caller's reach.
        let answer = clock_of(&caller, tz.as_deref()).and_then(|clock| {
            let f = store.resolved(filter.as_ref())?;
            let b = store.resolved(base.as_ref())?;
            store.facets_at(&caller, &f, &b, &clock).map_err(|e| e.to_string())
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
