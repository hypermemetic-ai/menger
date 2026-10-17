//! APP-50 (APP-37's direction A, the owner's pick): **the phone nav as a
//! bar along the bottom edge.**
//!
//! - **Where**: a viewport ≤ 820 px wide ([`PHONE_MAX`]: phones, and a
//!   tablet held upright) takes the phone shape — no nav column and no
//!   header at the top. From 821 px the side nav returns as it was.
//! - **The top** is one 44 px line, [`Head`]: the wordmark, the shortcode ›
//!   the page, and the live mark.
//! - **The bar**, [`Bar`]: 62 px (58 from 641, each label beside its icon),
//!   fixed, safe-area aware, four places ([`places`]) — **the tree**
//!   (labelled with the shortcode you are in), **the filter** (the global
//!   filter, the header's, with its count of groups) or, on a ticket page,
//!   **the sections** (LOOK-3's dock: `2/7 the ask`, open inputs a badge),
//!   **find** (the palette), and **the queue** (APP-38's control: its count,
//!   the loudest urgency's chip, its colours, nudge and arrival flash).
//! - **A place tapped** raises its sheet above the bar and lights; tapping it
//!   again, Esc, the scrim or *close* lowers it; another place swaps it
//!   ([`tapped`]). One sheet at a time: each sheet stays its owner's (the
//!   tree is the nav, the filter the header and the lens bar, find the
//!   palette, the sections the dock's sheet, the queue the tray's) and the
//!   bar asks it up or down ([`open`]); one raised another way lowers the
//!   bar's own.
//! - **Motion** (LOOK-8): sheets rise in .32 s eased out and leave in .2 s,
//!   the lit place fades in .12 s; calm stops the queue place's nudge, none
//!   moves nothing (index.html's rules).
//!
//! What decides — the breakpoint, the places per page, which sheet is up,
//! what a tap does, the words and the queue place's face — is pure and
//! pinned by native tests below; the rest is the DOM.

use std::cell::Cell;

use leptos::prelude::*;
use menger_tickets_api::Urgency;

use crate::keys::{self, Overlay};
use crate::lens::{self, Level};
use crate::queue::Control;
use crate::{inputs, nav, needs, queue, sections, signin, Route};

/// The phone shape's widest viewport, in CSS px.
pub(crate) const PHONE_MAX: f64 = 820.0;

/// Whether a viewport `width` px wide takes the phone shape.
pub(crate) fn is_phone(width: f64) -> bool {
    width <= PHONE_MAX
}

/// A place on the bar, and the sheet it raises.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Place {
    Tree,
    Filter,
    Sections,
    Find,
    Queue,
}

impl Place {
    pub(crate) fn name(self) -> &'static str {
        match self {
            Place::Tree => "tree",
            Place::Filter => "filter",
            Place::Sections => "sections",
            Place::Find => "find",
            Place::Queue => "queue",
        }
    }
}

/// The bar's four places: on a ticket page the sections take the filter's.
pub(crate) fn places(on_ticket: bool) -> [Place; 4] {
    [Place::Tree, if on_ticket { Place::Sections } else { Place::Filter }, Place::Find, Place::Queue]
}

/// The sheet up after `tap` with `up` up: the same place lowers it, any
/// other raises its own in its place — one at a time.
pub(crate) fn tapped(up: Option<Place>, tap: Place) -> Option<Place> {
    (up != Some(tap)).then_some(tap)
}

/// Which sheet is up, from its owners: the palette, the tray, the dock's
/// sheet, then the bar's own (the tree or the filter).
pub(crate) fn up_of(palette: bool, queue: bool, sections: bool, own: Option<Place>) -> Option<Place> {
    if palette {
        Some(Place::Find)
    } else if queue {
        Some(Place::Queue)
    } else if sections {
        Some(Place::Sections)
    } else {
        own
    }
}

/// A ticket's shortcode: `APP-50` → `APP` (an address's last part).
pub(crate) fn shortcode_of(id: &str) -> String {
    let last = id.rsplit('/').next().unwrap_or(id);
    last.rsplit_once('-').map_or(last, |(code, _)| code).to_ascii_uppercase()
}

/// The tree place's label: the open ticket's shortcode, else the last one
/// opened, else `tree`.
pub(crate) fn tree_label(ticket: Option<&str>, last: Option<&str>) -> String {
    ticket.map(shortcode_of).or_else(|| last.map(str::to_string)).unwrap_or_else(|| "tree".into())
}

/// The head's page words.
pub(crate) fn page_words(r: &Route) -> String {
    match r {
        Route::Board => "the board".into(),
        Route::Ticket(id) => id.clone(),
        Route::Waves(s) if s.is_empty() => "waves".into(),
        Route::Waves(s) => format!("waves · {s}"),
        Route::WavesOf(_) => "waves".into(),
        Route::Components(_) => "components".into(),
        Route::Inputs => "inputs".into(),
        Route::Settings => "settings".into(),
        Route::Comments => "comments".into(),
        Route::Members => "members".into(),
        Route::Deployments(_) => "deployments".into(),
    }
}

/// What the queue place shows: a badge (the count), a label (the loudest
/// urgency's chip, `3 blocking`), its classes, and what a screen reader
/// hears — APP-38's [`Control`], carried whole.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct QueueFace {
    pub(crate) badge: Option<usize>,
    pub(crate) label: String,
    pub(crate) class: String,
    pub(crate) words: String,
}

/// The queue place for the tray's control `ctl` (None: nothing queued),
/// `open` its sheet, `fresh` the arrival it blinks for.
pub(crate) fn queue_face(ctl: Option<&Control>, open: bool, fresh: Option<Urgency>) -> QueueFace {
    let Some(c) = ctl else {
        let mut class = "pl pq empty".to_string();
        if open {
            class.push_str(" on");
        }
        return QueueFace { badge: None, label: "queue".into(), class, words: "the queue: nothing queued".into() };
    };
    let mut class = format!("pl pq u-{}", c.loudest.as_str());
    for (on, word) in [(c.loudest.loud(), "loud"), (c.nudge, "nudge"), (open, "on"), (c.new > 0, "has-new")] {
        if on {
            class.push(' ');
            class.push_str(word);
        }
    }
    if let Some(u) = fresh.filter(|_| !open) {
        class.push_str(" fresh f-");
        class.push_str(u.as_str());
    }
    QueueFace { badge: Some(c.count), label: c.chip(), class, words: format!("the queue: {}", c.words()) }
}

/// The sections place's label and badge (the inputs waiting), or none
/// when the page has no dock.
pub(crate) fn sections_face(face: Option<&sections::Face>) -> (String, Option<usize>) {
    match face {
        Some(f) => (f.words(), (f.waiting > 0).then_some(f.waiting)),
        None => ("sections".into(), None),
    }
}

// ---------------------------------------------------------------------
// The state

/// What the bar asks of the sheets' owners.
#[derive(Clone, Copy)]
struct Hub {
    overlay: RwSignal<Overlay>,
    lens: lens::Lens,
    tree: nav::Tree,
}

thread_local! {
    static HUB: Cell<Option<Hub>> = const { Cell::new(None) };
    /// The bar's own sheet: the tree or the filter.
    static OWN: ArcRwSignal<Option<Place>> = ArcRwSignal::new(None);
    /// The last shortcode a ticket page was in.
    static LAST: ArcRwSignal<Option<String>> = ArcRwSignal::new(None);
}

/// The bar's own sheet (the tree's or the filter's), as `#app[data-sheet]`
/// says it.
pub(crate) fn own() -> ArcRwSignal<Option<Place>> {
    OWN.with(Clone::clone)
}

/// Raise `to`'s sheet (None: lower every one) — each owner asked.
pub(crate) fn open(to: Option<Place>) {
    let Some(h) = HUB.with(Cell::get) else { return };
    let own = OWN.with(Clone::clone);
    let was = own.get_untracked();
    let mine = to.filter(|p| matches!(p, Place::Tree | Place::Filter));
    if was != mine {
        own.set(mine);
    }
    let pop = h.lens.pop(Level::Global);
    if to == Some(Place::Filter) {
        if !pop.is_open() {
            h.lens.open_panel(Level::Global, None);
        }
    } else if was == Some(Place::Filter) && pop.is_open() {
        pop.close(false);
    }
    match (to == Some(Place::Find), h.overlay.get_untracked() == Overlay::Palette) {
        (true, false) => keys::open_overlay(h.overlay, Overlay::Palette),
        (false, true) => keys::close_overlay(h.overlay),
        _ => {}
    }
    let s = sections::sheet();
    if s.get_untracked() != (to == Some(Place::Sections)) {
        s.set(to == Some(Place::Sections));
    }
    let q = queue::face().with_untracked(|f| f.open);
    if q != (to == Some(Place::Queue)) {
        queue::want(to == Some(Place::Queue));
    }
    if to == Some(Place::Tree) {
        // The tree measures the sheet it now fills.
        let tree = h.tree;
        request_animation_frame(move || tree.viewport.notify());
    }
}

/// Which sheet is up now.
fn up_now(overlay: RwSignal<Overlay>) -> Option<Place> {
    up_of(
        overlay.get_untracked() == Overlay::Palette,
        queue::face().with_untracked(|f| f.open),
        sections::sheet().get_untracked(),
        OWN.with(|o| o.get_untracked()),
    )
}

// ---------------------------------------------------------------------
// The view

/// The icons, the mockup's strokes on `currentColor`.
fn icon(p: Place) -> impl IntoView {
    let paths: &[&str] = match p {
        Place::Tree => &["M5 4v16M5 8h6M5 14h6M5 20h6", "M12 5.5h7v5h-7zM12 11.5h7v5h-7z"],
        Place::Filter => &["M4 5h16l-6 7.5V19l-4 1.5v-8z"],
        Place::Sections => &["M9 6h11M9 12h11M9 18h11", "M4.5 5a1 1 0 1 0 0 2 1 1 0 1 0 0-2zM4.5 11a1 1 0 1 0 0 2 1 1 0 1 0 0-2zM4.5 17a1 1 0 1 0 0 2 1 1 0 1 0 0-2z"],
        Place::Find => &["M10.5 4.5a6 6 0 1 0 0 12 6 6 0 1 0 0-12zM15 15l5 5"],
        Place::Queue => &["M3 14h5l1.5 3h5L16 14h5v5a1 1 0 0 1-1 1H4a1 1 0 0 1-1-1z", "M6 10l6-6 6 6"],
    };
    view! {
        <svg class="pic" viewBox="0 0 24 24" aria-hidden="true">
            {paths.iter().map(|d| view! { <path d=*d /> }).collect_view()}
        </svg>
    }
}

/// The top line at phone widths: the wordmark, where you are, live.
#[component]
pub(crate) fn Head(route: RwSignal<Route>, #[prop(into)] conn_class: Signal<String>, #[prop(into)] conn_words: Signal<String>) -> impl IntoView {
    let last = LAST.with(Clone::clone);
    let here = move || {
        let r = route.get();
        let ticket = match &r { Route::Ticket(id) => Some(id.clone()), _ => None };
        let code = tree_label(ticket.as_deref(), last.get().as_deref());
        let code = (code != "tree").then_some(code);
        (code, page_words(&r))
    };
    view! {
        <div id="phead">
            <a href="#/" class="wordmark" aria-label="menger — the board"><crate::carpet::Carpet class="mark" /></a>
            <span class="here">
                {move || {
                    let (code, page) = here();
                    view! {
                        {code.map(|c| view! { <span class="id">{c}</span>" › " })}
                        <span class="pg">{page}</span>
                    }
                }}
            </span>
            <span class=move || format!("pconn {}", conn_class.get())><i class="cell" aria-hidden="true"></i>{move || conn_words.get()}</span>
        </div>
    }
}

/// The bar along the bottom edge.
#[component]
pub(crate) fn Bar(route: RwSignal<Route>, overlay: RwSignal<Overlay>, tree: nav::Tree) -> impl IntoView {
    let lens = lens::use_lens();
    HUB.with(|h| h.set(Some(Hub { overlay, lens, tree })));
    let own = RwSignal::from(OWN.with(Clone::clone));
    let qface = RwSignal::from(queue::face());
    let sface = RwSignal::from(sections::face());
    let ssheet = RwSignal::from(sections::sheet());
    let last = RwSignal::from(LAST.with(Clone::clone));
    let path = Memo::new(move |_| route.get());
    let on_ticket = Memo::new(move |_| matches!(path.get(), Route::Ticket(_)));
    let up = Memo::new(move |_| up_of(overlay.get() == Overlay::Palette, qface.with(|f| f.open), ssheet.get(), own.get()));
    // The last shortcode a ticket page was in; a new page lowers the bar's
    // own sheet (a row picked in the tree has done its work).
    Effect::new(move |prev: Option<Route>| {
        let r = path.get();
        if let Route::Ticket(id) = &r {
            let code = shortcode_of(id);
            if last.get_untracked().as_deref() != Some(code.as_str()) {
                last.set(Some(code));
            }
        }
        if prev.is_some_and(|p| p != r) && own.get_untracked().is_some() {
            open(None);
        }
        r
    });
    // A sheet raised another way (`/`, the dock's pill) lowers the bar's own.
    Effect::new(move |prev: Option<(bool, bool, bool)>| {
        let now = (overlay.get() == Overlay::Palette, qface.with(|f| f.open), ssheet.get());
        if let Some(p) = prev {
            let rose = (now.0 && !p.0) || (now.1 && !p.1) || (now.2 && !p.2);
            if rose && own.get_untracked().is_some() {
                let to = up_of(now.0, now.1, now.2, None);
                open(to);
            }
        }
        now
    });
    // Esc lowers the bar's own sheet — after a menu inside it has had it.
    let esc = window_event_listener(leptos::ev::keydown, move |ev: web_sys::KeyboardEvent| {
        if ev.default_prevented() || own.get_untracked().is_none() {
            return;
        }
        if crate::bindings::is(&ev, crate::bindings::Context::Dialog, "dialog.close") {
            ev.prevent_default();
            open(None);
        }
    });
    on_cleanup(move || esc.remove());
    let tap = move |p: Place| open(tapped(up_now(overlay), p));
    let lit = move |p: Place| up.get() == Some(p);

    let tree_place = move || {
        let label = move || {
            let ticket = match path.get() { Route::Ticket(id) => Some(id), _ => None };
            tree_label(ticket.as_deref(), last.get().as_deref())
        };
        view! {
            <button type="button" class="pl pt" class:on=move || lit(Place::Tree) data-place="tree"
                aria-expanded=move || lit(Place::Tree).to_string() aria-label=move || format!("the tree — {}", label())
                on:click=move |_| tap(Place::Tree)>
                {icon(Place::Tree)}
                <span class="sub">{label}</span>
            </button>
        }
    };
    let filter_place = move || {
        let n = move || lens.group_count(Level::Global);
        view! {
            <button type="button" class="pl pf" class:on=move || lit(Place::Filter) data-place="filter"
                aria-expanded=move || lit(Place::Filter).to_string()
                aria-label=move || match n() { 0 => "the filter".to_string(), n => format!("the filter — {n} group{}", if n == 1 { "" } else { "s" }) }
                on:click=move |_| tap(Place::Filter)>
                {icon(Place::Filter)}
                {move || (n() > 0).then(|| view! { <span class="bdg">{n()}</span> })}
                <span class="sub">"filter"</span>
            </button>
        }
    };
    let sections_place = move || {
        let face = Memo::new(move |_| sface.with(|f| sections_face(f.as_ref())));
        view! {
            <button type="button" class="pl ps" class:on=move || lit(Place::Sections) data-place="sections"
                disabled=move || sface.with(Option::is_none)
                aria-expanded=move || lit(Place::Sections).to_string()
                aria-label=move || format!("the sections — {}", face.with(|f| f.0.clone()))
                on:click=move |_| tap(Place::Sections)>
                {icon(Place::Sections)}
                {move || face.with(|f| f.1).map(|n| view! { <span class="bdg ask">{n}</span> })}
                <span class="sub">{move || face.with(|f| f.0.clone())}</span>
            </button>
        }
    };
    let qview = Memo::new(move |_| qface.with(|f| queue_face(f.ctl.as_ref(), f.open, f.fresh)));
    view! {
        <nav id="pbar" aria-label="the phone nav">
            {tree_place}
            {move || if on_ticket.get() { sections_place().into_any() } else { filter_place().into_any() }}
            <button type="button" class="pl pn" class:on=move || lit(Place::Find) data-place="find"
                aria-expanded=move || lit(Place::Find).to_string() aria-label="find — jump to a ticket"
                on:click=move |_| tap(Place::Find)>
                {icon(Place::Find)}
                <span class="sub">"find"</span>
            </button>
            <button type="button" class=move || qview.with(|q| q.class.clone()) data-place="queue"
                disabled=move || qface.with(|f| f.ctl.is_none() && !f.open)
                aria-expanded=move || lit(Place::Queue).to_string()
                aria-controls="qsheet"
                aria-label=move || qview.with(|q| q.words.clone())
                on:click=move |_| tap(Place::Queue)>
                {icon(Place::Queue)}
                {move || qview.with(|q| q.badge).map(|n| view! { <span class="bdg">{n}</span> })}
                <span class="sub">{move || qview.with(|q| q.label.clone())}</span>
            </button>
        </nav>
        <div id="pscrim" class:on=move || own.get() == Some(Place::Tree) aria-hidden="true" on:click=move |_| open(None)></div>
    }
}

/// The tree's sheet's head (inside the nav, shown at phone widths): where
/// you are, *close*, and the views as a row of pills.
#[component]
pub(crate) fn TreeHead(route: RwSignal<Route>) -> impl IntoView {
    let last = LAST.with(Clone::clone);
    let title = move || {
        let ticket = match route.get() { Route::Ticket(id) => Some(id), _ => None };
        let code = tree_label(ticket.as_deref(), last.get().as_deref());
        if code == "tree" { "Tickets".to_string() } else { code }
    };
    let open_inputs = {
        let held = inputs::signal();
        Memo::new(move |_| held.with(|h| h.as_ref().map(inputs::Inputs::open_count)))
    };
    let needs = needs::use_needs();
    let admin = {
        let account = signin::account();
        Memo::new(move |_| account.with(|a| a.is_admin()))
    };
    let cur = move |want: fn(&Route) -> bool| move || want(&route.get()).then_some("page");
    view! {
        <div class="psheet-head">
            <div class="grab" aria-hidden="true"></div>
            <div class="sh">
                <h3>{title}</h3>
                <span class="m">"the tree"</span>
                <button type="button" class="pclose" on:click=move |_| open(None)>"close ▾"</button>
            </div>
            <div class="pviews" role="navigation" aria-label="the views">
                <a href="#/" aria-current=cur(|r| matches!(r, Route::Board | Route::Ticket(_)))>"board"</a>
                <a href="#/waves" aria-current=cur(|r| matches!(r, Route::Waves(_) | Route::WavesOf(_)))>"waves"</a>
                <a href="#/inputs" aria-current=cur(|r| matches!(r, Route::Inputs))>
                    "inputs"{move || open_inputs.get().filter(|n| *n > 0).map(|n| view! { <b>{n}</b> })}
                </a>
                <button type="button" on:click=move |_| needs::go()>
                    "needs you"{move || needs.count.n().filter(|n| *n > 0).map(|n| view! { <b>{n}</b> })}
                </button>
                <a href="#/comments" aria-current=cur(|r| matches!(r, Route::Comments))>"comments"</a>
                <a href="#/deployments" aria-current=cur(|r| matches!(r, Route::Deployments(_)))>"deployments"</a>
                <a href="#/components" aria-current=cur(|r| matches!(r, Route::Components(_)))>"components"</a>
                {move || admin.get().then(|| view! { <a href="#/members" aria-current=cur(|r| matches!(r, Route::Members))>"members"</a> })}
                <a href="#/settings" aria-current=cur(|r| matches!(r, Route::Settings))>"settings"</a>
            </div>
        </div>
    }
}

/// The tree's sheet's foot, under the thumb: *go to a ticket…* (find).
#[component]
pub(crate) fn TreeFoot() -> impl IntoView {
    view! {
        <div class="psheet-foot">
            <button type="button" class="pgo" on:click=move |_| open(Some(Place::Find))>
                {icon(Place::Find)}"go to a ticket…"
            </button>
        </div>
    }
}

/// The filter's sheet's head (the header and the lens bar are its body at
/// phone widths): *Filter*, close.
#[component]
pub(crate) fn FilterHead() -> impl IntoView {
    view! {
        <div class="psheet-head pfilter-head">
            <div class="grab" aria-hidden="true"></div>
            <div class="sh">
                <h3>"Filter"</h3>
                <span class="m">"every view"</span>
                <button type="button" class="pclose" on:click=move |_| open(None)>"close ▾"</button>
            </div>
        </div>
    }
}

/// The filter's sheet's scrim — inside the page's column, as the sheet is.
#[component]
pub(crate) fn FilterScrim() -> impl IntoView {
    let own = RwSignal::from(OWN.with(Clone::clone));
    view! {
        <div class="pscrim-in" class:on=move || own.get() == Some(Place::Filter) aria-hidden="true" on:click=move |_| open(None)></div>
    }
}

/// The filter's sheet's foot: clear, and done.
#[component]
pub(crate) fn FilterFoot() -> impl IntoView {
    let lens = lens::use_lens();
    view! {
        <span class="pfilter-foot">
            <button type="button" class="pbtn" disabled=move || !lens.differs(Level::Global) on:click=move |_| lens.reset(Level::Global)>"clear"</button>
            <button type="button" class="pbtn fill" on:click=move |_| open(None)>"done"</button>
        </span>
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    /// The phone shape runs to 820 px — a tablet held upright too — and the
    /// side nav returns from 821.
    #[test]
    fn the_phone_shape_stops_at_820() {
        assert!(is_phone(390.0));
        assert!(is_phone(768.0));
        assert!(is_phone(820.0));
        assert!(!is_phone(821.0));
        assert!(!is_phone(1440.0));
        // index.html's rules say the same widths.
        let css = crate::tokens_lint::app_css();
        assert!(css.contains("@media (max-width: 820px)"), "the phone shape's rules");
        assert!(css.contains("@media (min-width: 821px)"), "the side nav's return");
        assert!(!css.contains("grid-template-rows: minmax(0, 38vh)"), "LOOK-2's nav band is gone");
    }

    /// The board: tree · filter · find · queue; a ticket: tree · sections ·
    /// find · queue.
    #[test]
    fn the_places_per_page() {
        assert_eq!(places(false), [Place::Tree, Place::Filter, Place::Find, Place::Queue]);
        assert_eq!(places(true), [Place::Tree, Place::Sections, Place::Find, Place::Queue]);
        assert_eq!(places(true).map(Place::name), ["tree", "sections", "find", "queue"]);
    }

    /// A tap raises its place's sheet; the same place lowers it; another
    /// swaps it — one sheet at a time.
    #[test]
    fn one_sheet_at_a_time() {
        assert_eq!(tapped(None, Place::Tree), Some(Place::Tree));
        assert_eq!(tapped(Some(Place::Tree), Place::Tree), None);
        assert_eq!(tapped(Some(Place::Tree), Place::Filter), Some(Place::Filter));
        assert_eq!(tapped(Some(Place::Queue), Place::Find), Some(Place::Find));
        assert_eq!(tapped(Some(Place::Find), Place::Find), None);
        // Which is up, whoever raised it: one answer.
        assert_eq!(up_of(false, false, false, None), None);
        assert_eq!(up_of(false, false, false, Some(Place::Tree)), Some(Place::Tree));
        assert_eq!(up_of(true, false, false, Some(Place::Tree)), Some(Place::Find));
        assert_eq!(up_of(false, true, false, None), Some(Place::Queue));
        assert_eq!(up_of(false, false, true, None), Some(Place::Sections));
    }

    fn ctl(count: usize, loudest: Urgency, sharing: usize, new: usize, nudge: bool) -> Control {
        Control { count, loudest, sharing, new, nudge }
    }

    /// The queue place carries APP-38's control: the count, the loudest's
    /// chip, its colours, its nudge, an arrival's flash, `N new`.
    #[test]
    fn the_queue_place_is_the_control() {
        let c = ctl(40, Urgency::Blocking, 3, 0, true);
        let f = queue_face(Some(&c), false, None);
        assert_eq!(f.badge, Some(40));
        assert_eq!(f.label, "3 blocking");
        assert_eq!(f.class, "pl pq u-blocking loud nudge");
        assert_eq!(f.words, "the queue: 40 queued, 3 blocking");
        // Open: lit, and no flash while the sheet is up.
        assert_eq!(queue_face(Some(&c), true, Some(Urgency::Now)).class, "pl pq u-blocking loud nudge on");
        // An arrival while down: the flash, in the arrival's colour.
        let f = queue_face(Some(&ctl(9, Urgency::Today, 3, 1, false)), false, Some(Urgency::Today));
        assert_eq!(f.class, "pl pq u-today has-new fresh f-today");
        assert_eq!(f.words, "the queue: 9 queued, 3 today, 1 new");
        // One ticket: its urgency alone.
        assert_eq!(queue_face(Some(&ctl(1, Urgency::Whenever, 1, 0, false)), false, None).label, "whenever");
        // Nothing queued: a quiet place.
        let none = queue_face(None, false, None);
        assert_eq!((none.badge, none.label.as_str(), none.class.as_str()), (None, "queue", "pl pq empty"));
        // The same model the tray's control is drawn from.
        let rows: Vec<queue::Row> = Vec::new();
        assert_eq!(queue::control(&rows, &HashSet::new(), &HashSet::new()), None);
    }

    /// The tree place says the shortcode you are in; the head where you are.
    #[test]
    fn the_words() {
        assert_eq!(shortcode_of("APP-50"), "APP");
        assert_eq!(shortcode_of("MENGER/APP-50"), "APP");
        assert_eq!(shortcode_of("kiln-4"), "KILN");
        assert_eq!(tree_label(Some("KILN-4"), Some("APP")), "KILN");
        assert_eq!(tree_label(None, Some("APP")), "APP");
        assert_eq!(tree_label(None, None), "tree");
        assert_eq!(page_words(&Route::Board), "the board");
        assert_eq!(page_words(&Route::Ticket("KILN-4".into())), "KILN-4");
        assert_eq!(page_words(&Route::Waves(String::new())), "waves");
        assert_eq!(page_words(&Route::Inputs), "inputs");
    }

    /// The sections place: the lit one of how many and its name, the open
    /// inputs a badge; nothing to list, a quiet place.
    #[test]
    fn the_sections_place() {
        let s = |label: &str| sections::Section { label: label.into(), at: sections::Spot::Conversation, inputs: Vec::new() };
        let mut secs = vec![s("Problem"), s("The ask"), s("Acceptance"), s("Conversation")];
        secs[1].inputs.push(sections::AskRow { id: "landing".into(), question: None, ask: sections::Ask::Waiting, at: sections::Spot::Conversation });
        let f = sections::Face::of(&secs, 1).unwrap();
        assert_eq!(sections_face(Some(&f)), ("2/4 The ask".to_string(), Some(1)));
        let f = sections::Face::of(&secs, 99).unwrap();
        assert_eq!(f.words(), "4/4 Conversation");
        assert_eq!(sections::Face::of(&[], 0), None);
        assert_eq!(sections_face(None), ("sections".to_string(), None));
    }
}
