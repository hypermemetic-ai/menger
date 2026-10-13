//! FILTER-12: the server's counts in the app — what every server-backed
//! menu counts, and the header's `n of N`.
//!
//! - **A menu's counts** are Linear's facets (FILTER-9 decision 4): for
//!   each value of each category, how many tickets a click would give —
//!   the level's own clause for that category set aside, never the levels
//!   above it. The server works them out over its cells (`facets`,
//!   FILTER-4): the global menu's are `facets { filter: global }`, a
//!   surface's `facets { base: global, filter: own }` ([`facets_ask`]).
//!   They are asked when the level's menu opens, and again whenever the
//!   level's filters change while it is open ([`menu_counts`]) — never at
//!   startup, never for a menu nobody opened. [`provider`] hands them to
//!   FILTER-11's bar as its [`CountsProvider`]; the nav (FILTER-14) and the
//!   inputs page (FILTER-16) take the same, the inputs page merging its own
//!   categories' counts over them.
//! - **A live count** ([`LiveCounts`]) is `counts { filter }` — each
//!   shortcode's matches, the board's arithmetic ([`Matches`]) — kept live
//!   by a membership watch ([`session::Membership`]): `entered` and `left`
//!   move a shortcode's count, `recount` sets it, and a `lagged` or a new
//!   connection asks again.
//! - **The header's count** ([`GlobalCount`]): beside the `Filter` button
//!   while the global filter is non-empty, `n of N` — n the sum of `counts
//!   { filter: global }`, live; N the store's ticket count (the extents'
//!   sum, which `created` keeps).
//!
//! What decides — the two filters a menu is counted with, `facets`' answer
//! as a level's counts — is pure, pinned by native tests.

use std::collections::HashMap;

use leptos::prelude::*;
use menger_tickets_api::filter::Filter;
use menger_tickets_api::TicketsEvent;
use wasm_bindgen_futures::spawn_local;

use crate::board::{Board, Matches};
use crate::filter::{Cat, Counts, CountsProvider, ScTree};
use crate::lens::{use_lens, Level};
use crate::session::{self, Member, Membership};

/// A filter's groups as the wire's text — its display part is the app's,
/// not the server's.
pub(crate) fn wire_text(f: &Filter) -> String {
    Filter { groups: f.groups.clone(), display: Default::default() }.format()
}

/// A filter as a request's parameter: its groups' text, or none at all for
/// a filter with no group (every ticket).
fn param(f: &Filter) -> Option<String> {
    (!f.groups.is_empty()).then(|| wire_text(f))
}

/// The two filters a level's menu is counted with — `(filter, base)`: the
/// global menu's own filter alone; a surface's own filter over the global
/// one, which is never set aside.
pub(crate) fn facets_ask(level: Level, global: &Filter, own: &Filter) -> (Option<String>, Option<String>) {
    match level {
        Level::Global => (param(global), None),
        _ => (param(own), param(global)),
    }
}

/// The category that draws a field of `facets`' answer — `category` has
/// none.
pub(crate) fn cat_of_field(field: &str) -> Option<Cat> {
    Some(match field {
        "in" => Cat::Shortcode,
        "status" => Cat::Status,
        "kind" => Cat::Kind,
        "severity" => Cat::Severity,
        "ready" => Cat::Ready,
        "blocked" => Cat::Blocked,
        "has commits" => Cat::Commits,
        "menger doc" => Cat::MengerDocs,
        "asking" => Cat::Asking,
        "label" => Cat::Label,
        _ => return None,
    })
}

/// `facets`' answer — `(field, value, held)` — as a level's counts: every
/// value a server-backed category offers counts 0 unless the answer says
/// otherwise (it names only values above 0), Shortcode's each known
/// shortcode; a value as the category's token (FILTER-11's spelling: the
/// server's own canonical one).
pub(crate) fn counts_of(facets: impl IntoIterator<Item = (String, String, u64)>, codes: impl IntoIterator<Item = String>) -> Counts {
    let mut out: Counts = HashMap::new();
    // FILTER-27: When is no facet: its clauses go uncounted.
    for c in Cat::SERVER.into_iter().filter(|c| *c != Cat::When) {
        let m = out.entry(c).or_default();
        for v in c.fixed() {
            m.insert(v.to_string(), 0);
        }
    }
    out.entry(Cat::Shortcode).or_default().extend(codes.into_iter().map(|c| (c, 0)));
    for (field, value, held) in facets {
        let Some(c) = cat_of_field(&field) else { continue };
        let Some(token) = c.token(&value) else { continue };
        out.entry(c).or_default().insert(token, held as usize);
    }
    out
}

/// One `facets` ask: its counts, or `None` (a refusal, or no connection —
/// no counts show).
async fn ask_facets(filter: Option<String>, base: Option<String>, tree: Option<ScTree>) -> Option<Counts> {
    let client = session::client()?;
    let events = client.facets_all(filter, base, crate::session::tz()).await.ok()?;
    let mut facets = Vec::new();
    for ev in events {
        match ev {
            TicketsEvent::Facet { field, value, held } => facets.push((field, value, held)),
            TicketsEvent::Error { message } => {
                leptos::logging::log!("facets: {message}");
                return None;
            }
            _ => {}
        }
    }
    let codes = tree.map(|t| t.known().iter().cloned().collect::<Vec<_>>()).unwrap_or_default();
    Some(counts_of(facets, codes))
}

/// `level`'s menu counts, from `facets`: asked when its menu opens, and
/// again whenever its filters change while it is open — the latest answer
/// wins. Empty (no counts show) until one is in. For the current owner's
/// lifetime (the view drawing the level's bar).
pub(crate) fn menu_counts(level: Level) -> Signal<Counts> {
    let lens = use_lens();
    let pop = lens.pop(level);
    let counts = RwSignal::new(Counts::new());
    let asked = StoredValue::new(0u64);
    Effect::new(move |_| {
        if !pop.open_tracked() {
            return;
        }
        let (filter, base) = lens.filter(Level::Global).with(|g| lens.filter(level).with(|own| facets_ask(level, g, own)));
        // A reconnect while the menu is open asks again.
        session::link().track();
        let tree = lens.sctree().get_untracked();
        let n = asked.get_value().wrapping_add(1);
        asked.set_value(n);
        spawn_local(async move {
            let answer = ask_facets(filter, base, tree).await;
            if asked.try_get_value() == Some(n) {
                if let Some(c) = answer {
                    counts.try_set(c);
                }
            }
        });
    });
    counts.into()
}

/// The counts provider of a server-backed level — FILTER-11's interface,
/// given `facets`' answer ([`menu_counts`]).
pub(crate) fn provider(level: Level) -> CountsProvider {
    CountsProvider::of(menu_counts(level))
}

// ---------------------------------------------------------------------
// Live counts

/// `counts { filter }`, kept live by a membership watch — for the current
/// owner's lifetime. Every handle `Copy`.
#[derive(Clone, Copy)]
pub(crate) struct LiveCounts {
    matches: RwSignal<Option<Matches>>,
    /// The filter counted (canonical text) — `None`: nothing.
    text: StoredValue<Option<String>>,
    /// Moves with every ask and every new filter: an older answer is not
    /// applied.
    asked: StoredValue<u64>,
    member: StoredValue<Option<Membership>>,
}

impl LiveCounts {
    /// Counts for `filter` (canonical text; `None`: nothing is asked and
    /// nothing is counted): its membership watch opened when it says one,
    /// replaced (the new first) when it says another; the counts asked
    /// whenever the watch is open on a connection, and after a `lagged`.
    pub(crate) fn new(filter: Signal<Option<String>>) -> Self {
        let lc = LiveCounts {
            matches: RwSignal::new(None),
            text: StoredValue::new(None),
            asked: StoredValue::new(0),
            member: StoredValue::new(None),
        };
        Effect::new(move |_| {
            let next = filter.get();
            if lc.text.with_value(|t| *t == next) {
                return;
            }
            lc.text.set_value(next.clone());
            lc.asked.update_value(|n| *n = n.wrapping_add(1));
            lc.matches.set(None);
            match (lc.member.get_value(), next) {
                (Some(m), Some(f)) => m.replace(f),
                (Some(m), None) => {
                    m.close();
                    lc.member.set_value(None);
                }
                (None, Some(f)) => lc.member.set_value(Some(Membership::open(f, move |ev| lc.heard(ev)))),
                (None, None) => {}
            }
        });
        on_cleanup(move || {
            if let Some(Some(m)) = lc.member.try_get_value() {
                m.close();
            }
        });
        lc
    }

    fn heard(self, ev: &Member) {
        let moved = |f: &dyn Fn(&mut Matches)| {
            self.matches.try_update(|m| {
                if let Some(m) = m {
                    f(m);
                }
            });
        };
        match ev {
            Member::Entered { epic, .. } => moved(&|m| m.entered(epic)),
            Member::Left { epic, .. } => moved(&|m| m.left(epic)),
            Member::Recount { epic, held } => moved(&|m| m.recount(epic, *held)),
            Member::Opened | Member::Lagged => self.ask(),
            Member::Refused(message) => {
                leptos::logging::log!("counts: {message}");
                self.matches.try_set(Some(Matches::default()));
            }
        }
    }

    /// `counts { filter }`, asked now.
    fn ask(self) {
        let Some(Some(text)) = self.text.try_get_value() else { return };
        let Some(client) = session::client() else { return };
        let n = self.asked.get_value().wrapping_add(1);
        self.asked.set_value(n);
        spawn_local(async move {
            let Ok(events) = client.counts_all(Some(text), None, crate::session::tz()).await else { return };
            let mut runs = Vec::new();
            for ev in events {
                match ev {
                    TicketsEvent::Count { epic, held } => runs.push((epic, held)),
                    TicketsEvent::Error { message } => {
                        leptos::logging::log!("counts: {message}");
                        return;
                    }
                    _ => {}
                }
            }
            if self.asked.try_get_value() == Some(n) {
                self.matches.try_set(Some(Matches::new(runs)));
            }
        });
    }

    /// Every match — `None` until counted (tracked).
    pub(crate) fn n(self) -> Option<usize> {
        self.matches.with(|m| m.as_ref().map(Matches::n))
    }

    /// Each shortcode's matches — `None` until counted (tracked).
    #[allow(dead_code)] // FILTER-14
    pub(crate) fn get(self) -> Option<Matches> {
        self.matches.get()
    }
}

/// The header's count, beside the global `Filter` button while the global
/// filter is non-empty: `n of N` — n the tickets it admits, live; N the
/// store's. A global filter that admits nothing is `0 of N`, asked of no
/// one.
#[component]
pub(crate) fn GlobalCount(board: Board) -> impl IntoView {
    let lens = use_lens();
    let global = lens.filter(Level::Global);
    let shown = Memo::new(move |_| global.with(|g| (!g.groups.is_empty()).then(|| g.admits_nothing().is_some())));
    let text = Memo::new(move |_| global.with(|g| (!g.groups.is_empty() && g.admits_nothing().is_none()).then(|| wire_text(g))));
    let live = LiveCounts::new(text.into());
    move || {
        shown.get().map(|nothing| {
            let n = move || if nothing { Some(0) } else { live.n() };
            let all = move || board.store_n();
            view! {
                <span class="gcount" title="tickets the global filter admits, of every ticket" aria-live="polite">
                    <span class="gcount-n">{move || n().map_or_else(|| "…".to_string(), |n| n.to_string())}</span>
                    " of "
                    <span class="gcount-all">{move || all().map_or_else(|| "…".to_string(), |n| n.to_string())}</span>
                </span>
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn f(text: &str) -> Filter {
        Filter::parse(text).expect("parses")
    }

    /// FILTER-12: the global menu is counted over the global filter alone
    /// (its own groups set aside by the server, field by field); a
    /// surface's over its own filter with the global one as the base,
    /// never set aside. An empty filter is no parameter (every ticket); a
    /// display part is not the server's.
    #[test]
    fn a_menu_is_counted_over_its_own_filter_and_the_levels_above() {
        let g = f("status Ready and not kind doc");
        let own = f("in MNG");
        assert_eq!(facets_ask(Level::Global, &g, &own), (Some("status Ready and not kind doc".into()), None));
        assert_eq!(facets_ask(Level::Board, &g, &own), (Some("in MNG".into()), Some("status Ready and not kind doc".into())));
        assert_eq!(facets_ask(Level::Nav, &g, &own), (Some("in MNG".into()), Some("status Ready and not kind doc".into())));
        assert_eq!(facets_ask(Level::Inputs, &Filter::default(), &own), (Some("in MNG".into()), None));
        assert_eq!(facets_ask(Level::Board, &g, &Filter::default()), (None, Some("status Ready and not kind doc".into())));
        assert_eq!(facets_ask(Level::Global, &Filter::default(), &own), (None, None));
        assert_eq!(facets_ask(Level::Global, &f("ready · tree"), &own), (Some("ready".into()), None));
        assert_eq!(wire_text(&f("status in (Ready, Coding) · flat")), "status in (Ready, Coding)");
    }

    /// FILTER-12: `facets`' answer as the bar's counts — each field as the
    /// category drawing it, each value as its token; every offered value
    /// counts 0 unless named; `category` drawn by none; a value no category
    /// knows dropped.
    #[test]
    fn facets_are_the_bars_counts() {
        let facets = [
            ("status", "Ready", 7),
            ("status", "In Review", 2),
            ("category", "ready", 7),
            ("kind", "build", 5),
            ("severity", "high", 1),
            ("severity", "none", 8),
            ("blocked", "yes", 3),
            ("ready", "no", 2),
            ("has commits", "yes", 4),
            ("menger doc", "yes", 2),
            ("menger doc", "no", 11),
            ("asking", "yes", 5),
            ("label", "needs-you", 3),
            ("label", "Bad Label", 1),
            ("in", "MNG", 6),
            ("in", "FILTER", 3),
            ("kind", "bogus", 1),
        ]
        .map(|(f, v, n)| (f.to_string(), v.to_string(), n));
        let c = counts_of(facets, ["MNG".to_string(), "PLEXUS".to_string()]);
        let get = |cat: Cat, v: &str| c.get(&cat).and_then(|m| m.get(v)).copied();
        assert_eq!(get(Cat::Status, "Ready"), Some(7));
        assert_eq!(get(Cat::Status, "In Review"), Some(2));
        assert_eq!(get(Cat::Status, "Coding"), Some(0), "an offered value not named counts 0");
        assert_eq!(get(Cat::Kind, "build"), Some(5));
        assert_eq!(get(Cat::Kind, "doc"), Some(0));
        assert_eq!(get(Cat::Severity, "high"), Some(1));
        assert_eq!(get(Cat::Severity, "none"), Some(8));
        assert_eq!(get(Cat::Blocked, "yes"), Some(3));
        assert_eq!(get(Cat::Blocked, "no"), Some(0));
        assert_eq!(get(Cat::Ready, "no"), Some(2));
        assert_eq!(get(Cat::Commits, "yes"), Some(4));
        assert_eq!(get(Cat::Commits, "no"), Some(0));
        // FILTER-21: Menger docs, a flag category with both counts.
        assert_eq!((get(Cat::MengerDocs, "yes"), get(Cat::MengerDocs, "no")), (Some(2), Some(11)));
        assert_eq!(cat_of_field("menger doc"), Some(Cat::MengerDocs));
        // MENGER-12: Asking a flag with both counts; Label each label held.
        assert_eq!((get(Cat::Asking, "yes"), get(Cat::Asking, "no")), (Some(5), Some(0)));
        assert_eq!(get(Cat::Label, "needs-you"), Some(3));
        assert_eq!(get(Cat::Label, "Bad Label"), None, "not a label: not counted");
        assert_eq!(get(Cat::Shortcode, "MNG"), Some(6));
        assert_eq!(get(Cat::Shortcode, "PLEXUS"), Some(0), "a known shortcode with none");
        assert_eq!(get(Cat::Shortcode, "FILTER"), Some(3), "named, though the tree did not know it");
        assert_eq!(get(Cat::Kind, "bogus"), None);
        assert!(!c.contains_key(&Cat::Wave), "no local category");
        assert_eq!(cat_of_field("category"), None);
    }
}
