//! MENGER-29: the counts behind the board and the menus — `counts` and
//! `facets` as their streams, moved out of the activation's file so it keeps
//! its methods (ACT-5 made the room for `delete` and `undelete`).

use super::*;

/// FILTER-4: `counts` — each shortcode's matches of a filter (the activation's doc).
pub(super) fn counts_stream(store: Arc<Store>, admitted: Result<Caller, String>, filter: Option<String>, keep: Option<String>, tz: Option<String>) -> impl Stream<Item = TicketsEvent> + Send + 'static {
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
        match filter.as_deref().map(Lens::parse).unwrap_or_else(|| Ok(Lens::all())) {
            Ok(lens) => {
                let lens = lens.with_clock(clock);
                let keep = kept(&store, &caller, keep.as_deref());
                for (epic, held) in store.counts_keeping(&lens, keep.as_deref()) {
                    yield TicketsEvent::Count { epic, held };
                }
            }
            Err(e) => yield TicketsEvent::Error { message: e.to_string() },
        }
    }
}

/// FILTER-4: `facets` — the menus' counts (the activation's doc).
pub(super) fn facets_stream(store: Arc<Store>, admitted: Result<Caller, String>, filter: Option<String>, base: Option<String>, tz: Option<String>) -> impl Stream<Item = TicketsEvent> + Send + 'static {
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
        let read = |text: Option<String>| -> Result<crate::filter::Filter, String> {
            text.as_deref().map_or_else(|| Ok(Default::default()), |t| crate::filter::Filter::parse(t).map_err(|e| e.to_string()))
        };
        let answer = clock_of(&caller, tz.as_deref()).and_then(|clock| {
            read(filter)
                .and_then(|f| Ok((f, read(base)?)))
                .and_then(|(f, b)| store.facets_at(&f, &b, &clock).map_err(|e| e.to_string()))
        });
        match answer {
            Ok(facets) => {
                for (field, value, held) in facets {
                    yield TicketsEvent::Facet { field, value, held };
                }
            }
            Err(message) => yield TicketsEvent::Error { message },
        }
    }
}
