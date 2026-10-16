//! LOOK-3 (APP-30's table, APP-31's mockup): the section dock — a ticket's
//! page lists the body's sections in a dock at the right, lights the one
//! being read, and gives every input a row of its own: amber while it waits
//! on the reader, green once decided.
//!
//! **Where the sections come from.** A body with sections is a framed body
//! (its `m-field`s are HTML), so the frame reports them: the SDK posts
//! `menger:sections { items }` (2.1) — each outermost `[data-field]` that is
//! not an input, and each `m-decision` / `m-input` with an `id`, in document
//! order, with where it runs — whenever they or the layout move
//! (src/frame.rs hands them to [`heard`]). The dock is the app's, not each
//! body's. The Conversation is always the last section.
//!
//! **An input's state** is the server's: the page asks `tickets.inputs.list`
//! for its ticket and follows `watch`'s `input_changed`. Open is
//! `waiting`, answered-not-closed is `answered`, closed is `decided`; a
//! removed or withdrawn input is not listed.
//!
//! **Its forms**, by the viewport's width: at ≥ 1760 (LOOK-7: the owner's
//! full-HD width less 10em, 1920 − 160) a card in its own column, sticky;
//! between 640 and 1760 a rail of ticks at the right edge
//! that opens over the body; at ≤ 640 no dock but a pill above the tray
//! (hiding while the reader scrolls down) that opens a bottom sheet, inputs
//! first. A body with fewer than three sections and no input has none.
//!
//! **Keys** (owner, 2026-10-16, LOOK-3's `dock-keys`): `}` / `{` the next
//! and previous section — `]` / `[` stay the status keys (ACT-10) — and `i`
//! the next open input, wrapping.
//!
//! What decides — the sections, the lit one, the steps, the next input, the
//! pill's rule and the form — is pure and pinned by native tests below; the
//! rest reads the DOM.

use std::cell::RefCell;
use std::collections::HashMap;

use leptos::prelude::*;
use menger_tickets_api::{InputState, TicketsEvent};
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::spawn_local;

use crate::{dom, frame, motion, session};

// ---------------------------------------------------------------------
// The model

/// One thing the frame reported: a section (`field`) or an input.
#[derive(Clone, Debug, PartialEq, serde::Deserialize)]
pub(crate) struct Item {
    pub(crate) kind: String,
    pub(crate) name: String,
    #[serde(default)]
    pub(crate) label: Option<String>,
    #[serde(default)]
    pub(crate) top: f64,
    #[serde(default)]
    pub(crate) height: f64,
}

/// Where an input stands, as the dock says it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Ask {
    /// Open: waiting on the reader.
    Waiting,
    /// Answered, not yet closed.
    Answered,
    /// Closed.
    Decided,
}

impl Ask {
    /// The server's state as the dock shows it; a removed or withdrawn input
    /// is not shown.
    pub(crate) fn of(s: InputState) -> Option<Ask> {
        match s {
            InputState::Open => Some(Ask::Waiting),
            InputState::Answered => Some(Ask::Answered),
            InputState::Closed => Some(Ask::Decided),
            InputState::Removed | InputState::Withdrawn => None,
        }
    }

    pub(crate) fn word(self) -> &'static str {
        match self {
            Ask::Waiting => "waiting",
            Ask::Answered => "answered",
            Ask::Decided => "decided",
        }
    }

    pub(crate) fn class(self) -> &'static str {
        match self {
            Ask::Waiting => "open",
            Ask::Answered => "answered",
            Ask::Decided => "decided",
        }
    }
}

/// What a row goes to: the frame's item `n`, or the Conversation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Spot {
    Item(usize),
    Conversation,
}

/// An input's row, under its section.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct AskRow {
    pub(crate) id: String,
    pub(crate) question: Option<String>,
    pub(crate) ask: Ask,
    pub(crate) at: Spot,
}

/// A section's row.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Section {
    pub(crate) label: String,
    pub(crate) at: Spot,
    pub(crate) inputs: Vec<AskRow>,
}

/// Each input the server holds: its state and question as the dock shows
/// them, or None for one it does not show (removed, withdrawn).
pub(crate) type States = HashMap<String, Option<(Ask, Option<String>)>>;

/// A field's label: its `label`, else its name in words (`decided-shape` →
/// `Decided shape`).
pub(crate) fn label_of(name: &str, label: Option<&str>) -> String {
    if let Some(l) = label.map(str::trim).filter(|l| !l.is_empty()) {
        return l.to_string();
    }
    let words = name.replace(['-', '_'], " ");
    let mut c = words.chars();
    match c.next() {
        Some(f) => f.to_uppercase().chain(c).collect(),
        None => "Section".to_string(),
    }
}

/// The dock's sections: each field in order, each input under the field
/// that precedes it (the first field, or a `Body` section, when it comes
/// before any), then the Conversation. `states` is the server's (None:
/// not asked yet — every input shown waiting); an input the server does
/// not list yet is waiting, one it lists removed is not shown.
pub(crate) fn build(items: &[Item], states: Option<&States>) -> Vec<Section> {
    let mut out: Vec<Section> = Vec::new();
    let mut early: Vec<AskRow> = Vec::new();
    for (n, it) in items.iter().enumerate() {
        if it.kind == "input" {
            let (ask, question) = match states.and_then(|s| s.get(&it.name)) {
                Some(Some((a, q))) => (*a, q.clone()),
                // Held by the server, and not shown.
                Some(None) => continue,
                None => (Ask::Waiting, None),
            };
            let row = AskRow { id: it.name.clone(), question, ask, at: Spot::Item(n) };
            match out.last_mut() {
                Some(s) => s.inputs.push(row),
                None => early.push(row),
            }
        } else {
            out.push(Section { label: label_of(&it.name, it.label.as_deref()), at: Spot::Item(n), inputs: Vec::new() });
        }
    }
    if !early.is_empty() {
        match out.first_mut() {
            Some(first) => {
                early.append(&mut first.inputs);
                first.inputs = early;
            }
            None => {
                let at = early[0].at;
                out.push(Section { label: "Body".to_string(), at, inputs: early });
            }
        }
    }
    out.push(Section { label: "Conversation".to_string(), at: Spot::Conversation, inputs: Vec::new() });
    out
}

/// Whether the page shows a dock at all: three sections or more, or an
/// input.
pub(crate) fn shows(sections: &[Section]) -> bool {
    sections.len() >= 3 || sections.iter().any(|s| !s.inputs.is_empty())
}

/// How many inputs wait on the reader.
pub(crate) fn waiting(sections: &[Section]) -> usize {
    sections.iter().flat_map(|s| &s.inputs).filter(|i| i.ask == Ask::Waiting).count()
}

/// The scroll-spy's line: a quarter of the way down the view.
pub(crate) fn spy_line(view_top: f64, view_height: f64) -> f64 {
    view_top + view_height * 0.25
}

/// The lit section: the last whose top has passed `line` (the first when
/// none has), and the last at the page's end.
pub(crate) fn lit(tops: &[f64], line: f64, at_end: bool) -> usize {
    if tops.is_empty() {
        return 0;
    }
    if at_end {
        return tops.len() - 1;
    }
    tops.iter().rposition(|t| *t <= line).unwrap_or(0)
}

/// `}` / `{` from section `at` of `len`: one on or back, held at the ends.
pub(crate) fn stepped(at: usize, len: usize, forward: bool) -> usize {
    if len == 0 {
        0
    } else if forward {
        (at + 1).min(len - 1)
    } else {
        at.saturating_sub(1)
    }
}

/// `i`: of the open inputs' tops, the first below `mid` (the view's middle),
/// else the first (wrapping); None with none open.
pub(crate) fn next_open(tops: &[f64], mid: f64) -> Option<usize> {
    if tops.is_empty() {
        return None;
    }
    Some(tops.iter().position(|t| *t > mid + 1.0).unwrap_or(0))
}

/// The pill's rule (≤ 640): it hides while the reader scrolls down, comes
/// back on the way up, at the top and at the page's end.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Pill {
    pub(crate) last: f64,
    pub(crate) away: bool,
}

/// A scroll smaller than this moves nothing.
const PILL_SLACK: f64 = 6.0;

impl Pill {
    /// The scroller is at `y` now (`at_end`: at the page's end); whether the
    /// pill is away after it.
    pub(crate) fn scrolled(&mut self, y: f64, at_end: bool) -> bool {
        let dy = y - self.last;
        if at_end || y <= 0.0 || dy < -PILL_SLACK {
            self.away = false;
        } else if dy > PILL_SLACK {
            self.away = true;
        }
        self.last = y;
        self.away
    }
}

/// The dock's form at a viewport `width`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Form {
    Column,
    Rail,
    Pill,
}

pub(crate) fn form(width: f64) -> Form {
    if width >= 1760.0 {
        Form::Column
    } else if width > 640.0 {
        Form::Rail
    } else {
        Form::Pill
    }
}

// ---------------------------------------------------------------------
// What the frames reported

thread_local! {
    /// Each body frame's last report, by its channel.
    static REPORTS: RefCell<HashMap<String, Vec<Item>>> = RefCell::new(HashMap::new());
    static VERSION: ArcRwSignal<u64> = ArcRwSignal::new(0);
    /// The dock on screen, for the keys.
    static ACTIVE: RefCell<Option<Handle>> = const { RefCell::new(None) };
}

/// A body frame's report (src/frame.rs) — kept by its channel; a frame no
/// longer in the document is forgotten.
pub(crate) fn heard(channel: &str, items: Vec<Item>) {
    let live: Vec<String> = dom::window()
        .document()
        .and_then(|d| d.query_selector_all("iframe[data-channel]").ok())
        .map(|l| (0..l.length()).filter_map(|i| l.get(i)).filter_map(|n| n.dyn_into::<web_sys::Element>().ok()).filter_map(|e| e.get_attribute("data-channel")).collect())
        .unwrap_or_default();
    REPORTS.with(|r| {
        let mut r = r.borrow_mut();
        r.retain(|k, _| live.contains(k));
        r.insert(channel.to_string(), items);
    });
    VERSION.with(|v| v.update(|n| *n = n.wrapping_add(1)));
}

fn report_of(channel: &str) -> Vec<Item> {
    REPORTS.with(|r| r.borrow().get(channel).cloned().unwrap_or_default())
}

// ---------------------------------------------------------------------
// The keys

/// A key the dock answers on the ticket page.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Key {
    Next,
    Prev,
    Input,
}

#[derive(Clone, Copy)]
struct Handle {
    sections: Memo<Vec<Section>>,
    lit: RwSignal<usize>,
    ticket: StoredValue<String>,
}

/// The ticket page's `}` `{` `i` — true when a dock took it.
pub(crate) fn key(k: Key) -> bool {
    let Some(h) = ACTIVE.with(|a| *a.borrow()) else { return false };
    let Some(sections) = h.sections.try_get_untracked() else { return false };
    if !shows(&sections) {
        return false;
    }
    let ticket = h.ticket.get_value();
    match k {
        Key::Next | Key::Prev => {
            let at = stepped(h.lit.get_untracked(), sections.len(), k == Key::Next);
            go(&ticket, sections[at].at, false);
        }
        Key::Input => next_input(&ticket, &sections),
    }
    true
}

// ---------------------------------------------------------------------
// The DOM half

/// The scrolling column the page lives in.
fn scroller() -> Option<web_sys::Element> {
    dom::window().document()?.query_selector("#app .content").ok().flatten()
}

fn conversation() -> Option<web_sys::Element> {
    dom::window().document()?.get_element_by_id("conversation")
}

/// Where `at` runs now, in the viewport: its top and height.
fn rect_of(ticket: &str, at: Spot, items: &[Item]) -> Option<(f64, f64)> {
    match at {
        Spot::Conversation => conversation().map(|c| {
            let r = c.get_bounding_client_rect();
            (r.top(), r.height())
        }),
        Spot::Item(n) => {
            let f = frame::body_frame(ticket)?;
            let it = items.get(n)?;
            Some((f.get_bounding_client_rect().top() + it.top, it.height))
        }
    }
}

fn current_items(ticket: &str) -> Vec<Item> {
    frame::body_frame(ticket).and_then(|f| f.get_attribute("data-channel")).map(|c| report_of(&c)).unwrap_or_default()
}

/// Scroll `at` in — an input centred, a section to the top — and flash it
/// once; under Motion none it jumps.
fn go(ticket: &str, at: Spot, centre: bool) {
    let Some(content) = scroller() else { return };
    let items = current_items(ticket);
    let Some((y, h)) = rect_of(ticket, at, &items) else { return };
    let view = content.get_bounding_client_rect();
    // A section's top lands just above the spy's line, so it is the one lit.
    let delta = if centre { y + h / 2.0 - (view.top() + view.height() / 2.0) } else { y - spy_line(view.top(), view.height()) + 8.0 };
    let top = (content.scroll_top() as f64 + delta).max(0.0);
    let smooth = motion::current() != motion::Motion::None;
    let opts = js_sys::Object::new();
    let _ = js_sys::Reflect::set(&opts, &"top".into(), &JsValue::from_f64(top));
    let _ = js_sys::Reflect::set(&opts, &"behavior".into(), &JsValue::from_str(if smooth { "smooth" } else { "auto" }));
    if let Ok(f) = js_sys::Reflect::get(&content, &"scrollTo".into()).and_then(|f| f.dyn_into::<js_sys::Function>()) {
        let _ = f.call1(&content, &opts);
    } else {
        content.set_scroll_top(top as i32);
    }
    match at {
        Spot::Item(n) => {
            if let Some(f) = frame::body_frame(ticket) {
                frame::post_flash(&f, n);
            }
        }
        Spot::Conversation => {
            if let Some(c) = conversation() {
                motion::replay(&c, "dock-flash", 1800);
            }
        }
    }
}

fn next_input(ticket: &str, sections: &[Section]) {
    let open: Vec<Spot> = sections.iter().flat_map(|s| &s.inputs).filter(|i| i.ask == Ask::Waiting).map(|i| i.at).collect();
    let items = current_items(ticket);
    let tops: Vec<f64> = open.iter().map(|at| rect_of(ticket, *at, &items).map_or(f64::MAX, |r| r.0)).collect();
    let mid = scroller().map_or(0.0, |c| {
        let r = c.get_bounding_client_rect();
        r.top() + r.height() / 2.0
    });
    if let Some(k) = next_open(&tops, mid) {
        go(ticket, open[k], true);
    }
}

/// The dock for `ticket`'s page: the column, the rail, the pill and its
/// sheet — none of them when the body has too little to list.
#[component]
pub(crate) fn Dock(ticket: String) -> impl IntoView {
    let tid = StoredValue::new(ticket.clone());
    let version = VERSION.with(Clone::clone);
    // The frame's report for this page's body, read again whenever any
    // frame reports (a re-rendered body is a new frame, a new channel).
    let items = Memo::new(move |_| {
        version.track();
        current_items(&tid.get_value())
    });
    let states = RwSignal::new(None::<States>);
    let sections = Memo::new(move |_| items.with(|it| states.with(|s| build(it, s.as_ref()))));
    let shown = Memo::new(move |_| sections.with(|s| shows(s)));
    let lit = RwSignal::new(0usize);
    let rail_open = RwSignal::new(false);
    let sheet = RwSignal::new(false);
    let pill = RwSignal::new(Pill::default());

    // The server's inputs for this ticket, then each `input_changed`.
    {
        let id = ticket.clone();
        spawn_local(async move {
            let Some(client) = session::client() else { return };
            let Ok(t) = crate::typed::typed(&id) else { return };
            let Ok(events) = client.inputs().list_all(Some(t), None, None, None, session::zone()).await else { return };
            let mut map = States::new();
            for ev in events {
                if let TicketsEvent::Input { input } = ev {
                    map.insert(input.id.to_string(), Ask::of(input.state).map(|a| (a, input.question.clone())));
                }
            }
            states.try_set(Some(map));
        });
    }
    {
        let id = ticket.clone();
        session::listen(move |ev| {
            if let TicketsEvent::InputChanged { input } = ev {
                if !input.ticket.to_string().eq_ignore_ascii_case(&id) {
                    return;
                }
                let row = Ask::of(input.state).map(|a| (a, input.question.clone()));
                states.try_update(|s| {
                    s.get_or_insert_with(States::new).insert(input.id.to_string(), row);
                });
            }
        });
    }

    // The spy: on every scroll of the column, a resize, and every report.
    let tick = move || {
        let Some(content) = scroller() else { return };
        let Some(secs) = sections.try_get_untracked() else { return };
        let id = tid.get_value();
        let items = current_items(&id);
        let view = content.get_bounding_client_rect();
        let tops: Vec<f64> = secs.iter().map(|s| rect_of(&id, s.at, &items).map_or(f64::MAX, |r| r.0)).collect();
        let y = content.scroll_top() as f64;
        let at_end = y + content.client_height() as f64 >= content.scroll_height() as f64 - 4.0;
        let n = self::lit(&tops, spy_line(view.top(), view.height()), at_end);
        if lit.try_get_untracked() != Some(n) {
            lit.try_set(n);
        }
        pill.try_update(|p| {
            p.scrolled(y, at_end);
        });
    };
    Effect::new(move |_| {
        sections.track();
        tick();
    });
    let on_scroll = Closure::<dyn Fn()>::new(tick);
    if let Some(c) = scroller() {
        let opts = web_sys::AddEventListenerOptions::new();
        opts.set_passive(true);
        let _ = c.add_event_listener_with_callback_and_add_event_listener_options("scroll", on_scroll.as_ref().unchecked_ref(), &opts);
    }
    let _ = dom::window().add_event_listener_with_callback("resize", on_scroll.as_ref().unchecked_ref());
    // A tap outside the opened rail closes it; Esc closes it and the sheet.
    let on_click = Closure::<dyn Fn(web_sys::MouseEvent)>::new(move |ev: web_sys::MouseEvent| {
        let inside = ev.target().and_then(|t| t.dyn_into::<web_sys::Element>().ok()).and_then(|e| e.closest("#tdock").ok().flatten()).is_some();
        if !inside {
            rail_open.try_set(false);
        }
    });
    let on_key = Closure::<dyn Fn(web_sys::KeyboardEvent)>::new(move |ev: web_sys::KeyboardEvent| {
        if crate::bindings::is(&ev, crate::bindings::Context::Dialog, "dialog.close") && (sheet.get_untracked() || rail_open.get_untracked()) {
            sheet.set(false);
            rail_open.set(false);
            ev.stop_propagation();
        }
    });
    if let Some(d) = dom::window().document() {
        let _ = d.add_event_listener_with_callback("click", on_click.as_ref().unchecked_ref());
        let _ = d.add_event_listener_with_callback_and_bool("keydown", on_key.as_ref().unchecked_ref(), true);
    }
    let held = StoredValue::new_local(Some((on_scroll, on_click, on_key)));
    ACTIVE.with(|a| *a.borrow_mut() = Some(Handle { sections, lit, ticket: tid }));
    on_cleanup(move || {
        ACTIVE.with(|a| {
            let mut a = a.borrow_mut();
            // Only this dock's handle: a newer page's may already be there.
            if a.as_ref().is_some_and(|h| h.lit == lit) {
                *a = None;
            }
        });
        if let Some(Some((s, c, k))) = held.try_update_value(Option::take) {
            if let Some(el) = scroller() {
                let _ = el.remove_event_listener_with_callback("scroll", s.as_ref().unchecked_ref());
            }
            let _ = dom::window().remove_event_listener_with_callback("resize", s.as_ref().unchecked_ref());
            if let Some(d) = dom::window().document() {
                let _ = d.remove_event_listener_with_callback("click", c.as_ref().unchecked_ref());
                let _ = d.remove_event_listener_with_callback_and_bool("keydown", k.as_ref().unchecked_ref(), true);
            }
        }
    });

    let go_to = move |at: Spot, centre: bool| {
        sheet.set(false);
        // The rail's dock, opened by a tap, closes behind the choice.
        if form(dom::window().inner_width().ok().and_then(|w| w.as_f64()).unwrap_or(0.0)) == Form::Rail {
            rail_open.set(false);
        }
        go(&tid.get_value(), at, centre);
    };
    let label = format!("Sections of {ticket}");
    let sheet_label = label.clone();
    let pos = move || sections.with(|s| format!("{} / {}", lit.get().min(s.len().saturating_sub(1)) + 1, s.len()));
    let n_waiting = Memo::new(move |_| sections.with(|s| waiting(s)));

    view! {
        <Show when=move || shown.get()>
            <aside id="tdock" aria-label=label.clone() class:open=move || rail_open.get()>
                <div class="rail" aria-hidden="true" on:click=move |ev| { ev.stop_propagation(); rail_open.set(true); }>
                    {move || sections.with(|secs| {
                        let at = lit.get();
                        secs.iter().enumerate().map(|(k, s)| {
                            let dots = s.inputs.iter().map(|i| {
                                let c = match i.ask { Ask::Waiting => "ask", Ask::Answered => "ask answered", Ask::Decided => "dec" };
                                view! { <i class=c></i> }
                            }).collect_view();
                            view! { <i class="tick" class:on=k == at></i>{dots} }
                        }).collect_view()
                    })}
                </div>
                <div class="dhead"><h2>"On this ticket"</h2><span class="pos">{pos}</span></div>
                {move || (n_waiting.get() > 0).then(|| view! {
                    <button type="button" class="waiting" on:click=move |_| { let s = sections.get_untracked(); next_input(&tid.get_value(), &s); }>
                        {format!("● {} waiting on you", n_waiting.get())}<kbd>"i"</kbd>
                    </button>
                })}
                <ol class="dlist">
                    {move || sections.with(|secs| {
                        let at = lit.get();
                        secs.iter().enumerate().map(|(k, s)| {
                            let spot = s.at;
                            let open = s.inputs.iter().filter(|i| i.ask == Ask::Waiting).count();
                            let rows = s.inputs.iter().map(|i| {
                                let spot = i.at;
                                let title = i.question.clone().unwrap_or_default();
                                view! {
                                    <li class=format!("i {}", i.ask.class())>
                                        <button type="button" title=title on:click=move |_| go_to(spot, true)>
                                            <span class="dot"></span><span class="l">{i.id.clone()}</span><span class="mini">{i.ask.word()}</span>
                                        </button>
                                    </li>
                                }
                            }).collect_view();
                            view! {
                                <li class="s" class:on=k == at class:seen=k < at>
                                    <button type="button" on:click=move |_| go_to(spot, false)>
                                        <span class="n">{format!("{:02}", k + 1)}</span>
                                        <span class="l">{s.label.clone()}</span>
                                        {(open > 0).then(|| view! { <span class="mini">{format!("{open} open")}</span> })}
                                    </button>
                                </li>
                                {rows}
                            }
                        }).collect_view()
                    })}
                </ol>
                <div class="dfoot"><span><kbd>"{"</kbd>" "<kbd>"}"</kbd>" sections"</span><span><kbd>"i"</kbd>" next input"</span></div>
            </aside>
            <button
                type="button"
                id="tdock-pill"
                class:away=move || pill.get().away
                aria-haspopup="dialog"
                on:click=move |_| sheet.set(true)
            >
                <span class="pos">{move || sections.with(|s| format!("{}/{}", lit.get().min(s.len().saturating_sub(1)) + 1, s.len()))}</span>
                <span class="l">{move || sections.with(|s| s.get(lit.get()).map(|x| x.label.clone()).unwrap_or_default())}</span>
                {move || (n_waiting.get() > 0).then(|| view! { <span class="ask"><i></i>{n_waiting.get()}</span> })}
            </button>
            <div id="tdock-scrim" class:on=move || sheet.get() on:click=move |_| sheet.set(false)></div>
            <div id="tdock-sheet" role="dialog" aria-label=sheet_label.clone() class:on=move || sheet.get()>
                <div class="grab"></div>
                <div class="shead"><h2>"On this ticket"</h2><span class="id">{tid.get_value()}</span><button type="button" on:click=move |_| sheet.set(false)>"Close"</button></div>
                <div class="sscroll">
                    {move || sections.with(|secs| {
                        let mut ins: Vec<&AskRow> = secs.iter().flat_map(|s| &s.inputs).collect();
                        ins.sort_by_key(|i| i.ask != Ask::Waiting);
                        (!ins.is_empty()).then(|| {
                            let w = waiting(secs);
                            let rows = ins.into_iter().map(|i| {
                                let spot = i.at;
                                let q = i.question.clone().unwrap_or_else(|| i.id.clone());
                                view! {
                                    <button type="button" class=format!("in {}", i.ask.class()) on:click=move |_| go_to(spot, true)>
                                        <span class="dot"></span><span class="q">{q}</span><small>{i.ask.word()}</small>
                                    </button>
                                }
                            }).collect_view();
                            view! { <h3>{format!("Inputs · {w} waiting on you")}</h3><div class="ins">{rows}</div> }
                        })
                    })}
                    <h3>"Sections"</h3>
                    <ol class="slist">
                        {move || sections.with(|secs| {
                            let at = lit.get();
                            secs.iter().enumerate().map(|(k, s)| {
                                let spot = s.at;
                                let open = s.inputs.iter().filter(|i| i.ask == Ask::Waiting).count();
                                view! {
                                    <li class:on=k == at>
                                        <button type="button" on:click=move |_| go_to(spot, false)>
                                            <span class="n">{format!("{:02}", k + 1)}</span>{s.label.clone()}
                                            {(open > 0).then(|| view! { <span class="mini">{format!("● {open}")}</span> })}
                                        </button>
                                    </li>
                                }
                            }).collect_view()
                        })}
                    </ol>
                </div>
            </div>
        </Show>
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn field(name: &str, label: Option<&str>, top: f64) -> Item {
        Item { kind: "field".into(), name: name.into(), label: label.map(Into::into), top, height: 40.0 }
    }
    fn input(id: &str, top: f64) -> Item {
        Item { kind: "input".into(), name: id.into(), label: None, top, height: 120.0 }
    }

    #[test]
    fn sections_are_the_fields_in_order_then_the_conversation() {
        let items = [field("standing", None, 0.0), field("required-behavior", Some("Requirements"), 300.0), field("decided-shape", Some("  "), 600.0)];
        let s = build(&items, None);
        let labels: Vec<&str> = s.iter().map(|x| x.label.as_str()).collect();
        assert_eq!(labels, ["Standing", "Requirements", "Decided shape", "Conversation"]);
        assert_eq!(s[1].at, Spot::Item(1));
        assert_eq!(s[3].at, Spot::Conversation);
        assert!(shows(&s));
    }

    #[test]
    fn an_input_is_a_row_under_its_section_with_the_servers_state() {
        let items = [field("context", None, 0.0), input("sensor", 50.0), field("open-questions", None, 400.0), input("cut-off", 450.0), input("notes", 700.0), input("gone", 900.0)];
        let mut st = States::new();
        st.insert("sensor".into(), Some((Ask::Decided, Some("Which thermocouple?".into()))));
        st.insert("cut-off".into(), Some((Ask::Waiting, None)));
        st.insert("notes".into(), Some((Ask::Answered, None)));
        let s = build(&items, Some(&st));
        assert_eq!(s[0].inputs.len(), 1);
        assert_eq!(s[0].inputs[0].ask, Ask::Decided);
        assert_eq!(s[0].inputs[0].question.as_deref(), Some("Which thermocouple?"));
        assert_eq!(s[0].inputs[0].at, Spot::Item(1));
        let ids: Vec<&str> = s[1].inputs.iter().map(|i| i.id.as_str()).collect();
        assert_eq!(ids, ["cut-off", "notes", "gone"], "an input the server has not listed shows waiting until it says");
        assert_eq!(waiting(&s), 2);
    }

    #[test]
    fn a_removed_input_is_not_listed_and_states_map_from_the_server() {
        assert_eq!(Ask::of(InputState::Open), Some(Ask::Waiting));
        assert_eq!(Ask::of(InputState::Answered), Some(Ask::Answered));
        assert_eq!(Ask::of(InputState::Closed), Some(Ask::Decided));
        assert_eq!(Ask::of(InputState::Removed), None);
        assert_eq!(Ask::of(InputState::Withdrawn), None);
        let mut st = States::new();
        st.insert("x".into(), Some((Ask::Waiting, None)));
        st.insert("y".into(), None);
        // "y" is held by the server and removed: not listed.
        let items = [field("a", None, 0.0), input("x", 1.0), input("y", 2.0)];
        let s = build(&items, Some(&st));
        assert_eq!(s[0].inputs.iter().map(|i| i.id.as_str()).collect::<Vec<_>>(), ["x"]);
    }

    #[test]
    fn inputs_before_any_field_belong_to_the_first() {
        let s = build(&[input("q", 0.0), field("problem", None, 100.0)], None);
        assert_eq!(s[0].label, "Problem");
        assert_eq!(s[0].inputs[0].id, "q");
        let s = build(&[input("q", 0.0)], None);
        assert_eq!(s.iter().map(|x| x.label.as_str()).collect::<Vec<_>>(), ["Body", "Conversation"]);
        assert!(shows(&s), "an input shows the dock");
    }

    #[test]
    fn too_little_to_list_shows_no_dock() {
        assert!(!shows(&build(&[], None)), "the Conversation alone");
        assert!(!shows(&build(&[field("standing", None, 0.0)], None)), "two sections, no input");
        assert!(shows(&build(&[field("a", None, 0.0), field("b", None, 1.0)], None)), "three");
    }

    #[test]
    fn the_spy_lights_the_last_section_past_the_quarter_line() {
        let line = spy_line(100.0, 800.0);
        assert_eq!(line, 300.0);
        let tops = [-500.0, 120.0, 299.0, 301.0, 900.0];
        assert_eq!(lit(&tops, line, false), 2);
        assert_eq!(lit(&[400.0, 900.0], line, false), 0, "none passed: the first");
        assert_eq!(lit(&tops, line, true), 4, "the page's end lights the last");
        assert_eq!(lit(&[], line, false), 0);
    }

    #[test]
    fn the_keys_step_and_hold_at_the_ends_and_i_wraps() {
        assert_eq!(stepped(0, 5, true), 1);
        assert_eq!(stepped(4, 5, true), 4);
        assert_eq!(stepped(0, 5, false), 0);
        assert_eq!(stepped(3, 5, false), 2);
        assert_eq!(stepped(0, 0, true), 0);
        let mid = 500.0;
        assert_eq!(next_open(&[100.0, 499.0, 520.0, 900.0], mid), Some(2));
        assert_eq!(next_open(&[100.0, 300.0], mid), Some(0), "wraps to the first");
        assert_eq!(next_open(&[], mid), None);
        // A centred input (its top a little above the middle) is passed.
        assert_eq!(next_open(&[440.0, 800.0], mid), Some(1));
    }

    #[test]
    fn the_pill_hides_reading_down_and_returns_going_up_and_at_the_ends() {
        let mut p = Pill::default();
        assert!(!p.scrolled(3.0, false), "a nudge moves nothing");
        assert!(p.scrolled(200.0, false), "down: away");
        assert!(p.scrolled(204.0, false), "a nudge keeps it away");
        assert!(!p.scrolled(150.0, false), "up: back");
        assert!(p.scrolled(800.0, false));
        assert!(!p.scrolled(1200.0, true), "the page's end: back");
        assert!(p.scrolled(1300.0, false));
        assert!(!p.scrolled(0.0, false), "the top: back");
    }

    #[test]
    fn the_form_follows_the_width() {
        assert_eq!(form(2560.0), Form::Column);
        assert_eq!(form(1920.0), Form::Column);
        assert_eq!(form(1760.0), Form::Column);
        assert_eq!(form(1759.0), Form::Rail);
        assert_eq!(form(1440.0), Form::Rail);
        assert_eq!(form(768.0), Form::Rail);
        assert_eq!(form(641.0), Form::Rail);
        assert_eq!(form(640.0), Form::Pill);
        assert_eq!(form(390.0), Form::Pill);
    }

    #[test]
    fn a_frames_report_reads_as_items() {
        let v: Vec<Item> = serde_json::from_str(
            r#"[{"kind":"field","name":"standing","label":null,"top":12,"height":80},{"kind":"input","name":"cut-off","label":null,"top":900.5,"height":200}]"#,
        )
        .unwrap();
        assert_eq!(v[0], field("standing", None, 12.0).clone_with_height(80.0));
        assert_eq!(v[1].kind, "input");
        assert_eq!(v[1].top, 900.5);
    }

    impl Item {
        fn clone_with_height(mut self, h: f64) -> Self {
            self.height = h;
            self
        }
    }
}
