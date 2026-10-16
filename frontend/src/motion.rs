//! DESIGN-39: the app's chrome in motion — DESIGN-35's moves as DESIGN-34's
//! first screen plays them.
//!
//! - **The Motion setting** (`#/settings` → Motion): full, calm or none —
//!   or, by default, the system's: `prefers-reduced-motion: reduce` is
//!   `none`, anything else `full`; an explicit choice wins either way
//!   ([`resolve`]). What it resolves to is stamped on `<html>` as
//!   `data-motion="full|calm|none"` — the contract the stylesheet and
//!   DESIGN-40's house components key on (absent reads as `full`). `none`
//!   stops every animation and transition the app adds (one rule block in
//!   index.html, plus the script-side checks here); `calm` keeps the state
//!   changes (sweeps, flashes, ticks, a ticket opening) and drops the boot,
//!   the pip's breathe, the live mark's pulse and the counters.
//! - **The boot** ([`boot_begin`], [`boot_live`]): a sheet over the app, the
//!   carpet's eight cells lighting one by one while the count runs to the
//!   store's total from the first `health`, then a wipe — never again on a
//!   reconnect, ≤ 1.3 s from `health`, `pointer-events: none`, removed by
//!   its own timer (and by a fallback when no `health` comes).
//! - **A change** ([`changed`], [`replay`]): a status `watch` moved — its
//!   board cell sweeps, its row flashes, on the DOM nodes that already
//!   draw it; a header count that moved ticks ([`tick_on_change`]).
//! - **Counters** ([`count_text`], [`Count`]): every integer in a text runs
//!   up from zero (cubic ease-out, requestAnimationFrame) — the header's
//!   ticket count, a ticket page's done/total.
//! - **A ticket opening** ([`note_origin`], [`open_page`]): the id moves
//!   from the row it was opened from to the page's head (a FLIP), the title
//!   rises out of its mask and the body develops beneath.
//!
//! What decides — the setting's resolution, the pip's proportions, a
//! counter's frame — is pure and pinned by native tests.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use leptos::prelude::*;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::spawn_local;

use crate::dom;

// ---------------------------------------------------------------------
// The setting

/// What the reader chose in `#/settings` → Motion. `System` (the default)
/// follows `prefers-reduced-motion`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Pref {
    #[default]
    System,
    Full,
    Calm,
    None,
}

impl Pref {
    pub(crate) const ALL: [Pref; 4] = [Pref::System, Pref::Full, Pref::Calm, Pref::None];

    /// The stored word (`None` for the default, which is not stored).
    pub(crate) fn word(self) -> Option<&'static str> {
        match self {
            Pref::System => None,
            Pref::Full => Some("full"),
            Pref::Calm => Some("calm"),
            Pref::None => Some("none"),
        }
    }

    /// A stored word, read back; anything else is the default.
    pub(crate) fn parse(raw: Option<&str>) -> Self {
        match raw {
            Some("full") => Pref::Full,
            Some("calm") => Pref::Calm,
            Some("none") => Pref::None,
            _ => Pref::System,
        }
    }

    /// The radio's value and label.
    pub(crate) fn value(self) -> &'static str {
        self.word().unwrap_or("system")
    }
}

/// What the app plays, the setting resolved against the system.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Motion {
    Full,
    Calm,
    None,
}

impl Motion {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Motion::Full => "full",
            Motion::Calm => "calm",
            Motion::None => "none",
        }
    }
}

/// The setting against the system: an explicit choice wins; the default is
/// `none` under `prefers-reduced-motion: reduce`, else `full`.
pub(crate) fn resolve(pref: Pref, reduce: bool) -> Motion {
    match pref {
        Pref::Full => Motion::Full,
        Pref::Calm => Motion::Calm,
        Pref::None => Motion::None,
        Pref::System if reduce => Motion::None,
        Pref::System => Motion::Full,
    }
}

thread_local! {
    static PREF: Cell<Pref> = const { Cell::new(Pref::System) };
    static REDUCE: Cell<bool> = const { Cell::new(false) };
    static BOOT: Cell<Boot> = const { Cell::new(Boot::Never) };
    static ORIGIN: RefCell<Option<Origin>> = const { RefCell::new(None) };
    static COUNTING: Cell<u32> = const { Cell::new(0) };
}

/// What the app plays now.
pub(crate) fn current() -> Motion {
    resolve(PREF.with(Cell::get), REDUCE.with(Cell::get))
}

fn root() -> Option<web_sys::Element> {
    dom::window().document().and_then(|d| d.document_element())
}

fn apply() {
    if let Some(r) = root() {
        let _ = r.set_attribute("data-motion", current().as_str());
    }
    if current() != Motion::Full {
        boot_end();
    }
}

/// The reader's choice, from `#/settings`: kept by the caller, applied here.
pub(crate) fn set_pref(p: Pref) {
    PREF.with(|c| c.set(p));
    apply();
}

pub(crate) fn pref() -> Pref {
    PREF.with(Cell::get)
}

// ---------------------------------------------------------------------
// DESIGN-43: Typed text

/// What the reader chose in `#/settings` → Motion → Typed text: how fast a
/// framed code block types itself in (ART-9's `m-code`), or `Off` — it
/// appears at once. Stamped on `<html>` as `data-typing` beside
/// `data-motion` and relayed into every frame (src/frame.rs, SDK 1.19's
/// `menger.typing`). It alone decides whether code types: Motion `calm`
/// no longer stops it, though Motion `none` (or reduced motion) still
/// stops every move, typing with it — the components read that from
/// `data-motion`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Typing {
    Off,
    Slow,
    #[default]
    Normal,
    Fast,
}

impl Typing {
    pub(crate) const ALL: [Typing; 4] = [Typing::Off, Typing::Slow, Typing::Normal, Typing::Fast];

    /// The attribute's value and the radio's.
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Typing::Off => "off",
            Typing::Slow => "slow",
            Typing::Normal => "normal",
            Typing::Fast => "fast",
        }
    }

    /// The stored word (`None` for the default, which is not stored).
    pub(crate) fn word(self) -> Option<&'static str> {
        (self != Typing::Normal).then(|| self.as_str())
    }

    /// A stored word or an attribute's value, read back; anything else (or
    /// none) is the default, `Normal`.
    pub(crate) fn parse(raw: Option<&str>) -> Self {
        match raw.map(str::trim) {
            Some("off") => Typing::Off,
            Some("slow") => Typing::Slow,
            Some("fast") => Typing::Fast,
            _ => Typing::Normal,
        }
    }
}

thread_local! {
    static TYPING: Cell<Typing> = const { Cell::new(Typing::Normal) };
}

fn apply_typing() {
    if let Some(r) = root() {
        let _ = r.set_attribute("data-typing", TYPING.with(Cell::get).as_str());
    }
}

/// The reader's Typed text choice, from `#/settings`: kept by the caller,
/// stamped here.
pub(crate) fn set_typing(t: Typing) {
    TYPING.with(|c| c.set(t));
    apply_typing();
}

pub(crate) fn typing() -> Typing {
    TYPING.with(Cell::get)
}

/// Once, first thing in the app: the stored choice and the system's
/// preference (followed live), stamped on `<html>`; the clicks a ticket
/// opens from, noted.
pub(crate) fn install() {
    let stored = crate::settings::load();
    PREF.with(|c| c.set(stored.motion));
    // DESIGN-43: and the Typed text speed, stamped beside it.
    TYPING.with(|c| c.set(stored.typing));
    apply_typing();
    if let Some(mql) = dom::window().match_media("(prefers-reduced-motion: reduce)").ok().flatten() {
        REDUCE.with(|c| c.set(mql.matches()));
        let cb = Closure::<dyn FnMut(web_sys::MediaQueryList)>::new(move |m: web_sys::MediaQueryList| {
            REDUCE.with(|c| c.set(m.matches()));
            apply();
        });
        let _ = mql.add_event_listener_with_callback("change", cb.as_ref().unchecked_ref());
        cb.forget(); // lives as long as the app
    }
    apply();
    // A row clicked: where its id stood, for the page it opens.
    let click = Closure::<dyn FnMut(web_sys::MouseEvent)>::new(move |ev: web_sys::MouseEvent| {
        let Some(el) = ev.target().and_then(|t| t.dyn_into::<web_sys::Element>().ok()) else { return };
        let Ok(Some(row)) = el.closest("tbody#board tr[data-ticket-id], #nav .row[data-ticket-id]") else { return };
        if let Some(id) = row.get_attribute("data-ticket-id") {
            let at = row.query_selector("td.id a, .code").ok().flatten().unwrap_or(row);
            note(&id, &at);
        }
    });
    if let Some(d) = dom::window().document() {
        let _ = d.add_event_listener_with_callback_and_bool("click", click.as_ref().unchecked_ref(), true);
    }
    click.forget();
}

// ---------------------------------------------------------------------
// One-shot classes

/// `class` on `el` again from its start: removed, a reflow, added — and
/// taken off when its animation ends (or after `ms`, should none run), so
/// the next change replays it.
pub(crate) fn replay(el: &web_sys::Element, class: &str, ms: i32) {
    let list = el.class_list();
    let _ = list.remove_1(class);
    if let Some(h) = el.dyn_ref::<web_sys::HtmlElement>() {
        let _ = h.offset_width();
    } else {
        let _ = el.get_bounding_client_rect();
    }
    let _ = list.add_1(class);
    let gen = el.get_attribute(&format!("data-m-{class}")).and_then(|g| g.parse::<u32>().ok()).unwrap_or(0) + 1;
    let _ = el.set_attribute(&format!("data-m-{class}"), &gen.to_string());
    let (el, class) = (el.clone(), class.to_string());
    let done = Rc::new({
        let (el, class) = (el.clone(), class.clone());
        move || {
            if el.get_attribute(&format!("data-m-{class}")).as_deref() == Some(gen.to_string().as_str()) {
                let _ = el.class_list().remove_1(&class);
                let _ = el.remove_attribute(&format!("data-m-{class}"));
            }
        }
    });
    let end = done.clone();
    let target = el.clone();
    let cb = Closure::once_into_js(move |ev: web_sys::Event| {
        let mine = ev.target().and_then(|t| t.dyn_into::<web_sys::Element>().ok()).is_some_and(|t| t == target);
        if mine {
            end();
        }
    });
    let opts = web_sys::AddEventListenerOptions::new();
    opts.set_once(true);
    let _ = el.add_event_listener_with_callback_and_add_event_listener_options("animationend", cb.unchecked_ref(), &opts);
    spawn_local(async move {
        dom::sleep_ms(ms).await;
        done();
    });
}

/// A status `watch` moved `id`: once the views have drawn it, its board
/// cell sweeps and its row flashes; its nav row too. Calm keeps it; none
/// does not.
pub(crate) fn changed(id: String) {
    if current() == Motion::None {
        return;
    }
    spawn_local(async move {
        // After the views' effects have put the new status on the nodes.
        dom::sleep_ms(0).await;
        let Some(doc) = dom::window().document() else { return };
        let q = css_escape(&id);
        if let Ok(Some(tr)) = doc.query_selector(&format!("tbody#board tr[data-ticket-id=\"{q}\"]")) {
            if let Ok(Some(cell)) = tr.query_selector("td.status .status") {
                replay(&cell, "swap", 900);
            }
            replay(&tr, "flash", 1900);
        }
        if let Ok(rows) = doc.query_selector_all(&format!("#nav .row[data-ticket-id=\"{q}\"]")) {
            for i in 0..rows.length() {
                let Some(row) = rows.item(i).and_then(|n| n.dyn_into::<web_sys::Element>().ok()) else { continue };
                if let Ok(Some(tag)) = row.query_selector(":scope > .tag:not(.roll)") {
                    replay(&tag, "swap", 900);
                }
                replay(&row, "flash", 1900);
            }
        }
    });
}

/// An id inside a double-quoted attribute selector.
fn css_escape(id: &str) -> String {
    id.replace('\\', "\\\\").replace('"', "\\\"")
}

/// The header's count at `selector` ticks whenever `value` moves from one
/// number to another (not when it first appears). Calm keeps it.
pub(crate) fn tick_on_change(selector: &'static str, value: impl Fn() -> Option<usize> + 'static) {
    Effect::new(move |prev: Option<Option<usize>>| {
        let now = value();
        if let (Some(Some(a)), Some(b)) = (prev, now) {
            if a != b && current() != Motion::None {
                spawn_local(async move {
                    dom::sleep_ms(0).await;
                    if let Some(el) = dom::window().document().and_then(|d| d.query_selector(selector).ok().flatten()) {
                        replay(&el, "tick", 700);
                    }
                });
            }
        }
        now
    });
}

// ---------------------------------------------------------------------
// Counters

/// `text` with every run of digits at `e` (0..=1) of its value, rounded —
/// `e = 1` is `text` itself.
pub(crate) fn frame_at(text: &str, e: f64) -> String {
    if e >= 1.0 {
        return text.to_string();
    }
    let mut out = String::with_capacity(text.len());
    let mut digits = String::new();
    let flush = |digits: &mut String, out: &mut String| {
        if !digits.is_empty() {
            let n: f64 = digits.parse().unwrap_or(0.0);
            out.push_str(&format!("{}", (n * e).round() as u64));
            digits.clear();
        }
    };
    for c in text.chars() {
        if c.is_ascii_digit() {
            digits.push(c);
        } else {
            flush(&mut digits, &mut out);
            out.push(c);
        }
    }
    flush(&mut digits, &mut out);
    out
}

/// The cubic ease-out every counter runs on.
pub(crate) fn ease_out(p: f64) -> f64 {
    1.0 - (1.0 - p.clamp(0.0, 1.0)).powi(3)
}

fn now_ms() -> f64 {
    dom::window().performance().map(|p| p.now()).unwrap_or_else(js_sys::Date::now)
}

/// `text` into `el`, its numbers counting up from zero over `ms` — under
/// `full`; otherwise at once. A newer count on the same element replaces a
/// running one.
pub(crate) fn count_text(el: web_sys::Element, text: String, ms: f64) {
    let gen = COUNTING.with(|c| {
        c.set(c.get().wrapping_add(1));
        c.get()
    });
    let _ = el.set_attribute("data-counting", &gen.to_string());
    if current() != Motion::Full || !text.bytes().any(|b| b.is_ascii_digit()) {
        el.set_text_content(Some(&text));
        return;
    }
    let t0 = now_ms();
    let f: Rc<RefCell<Option<Closure<dyn FnMut()>>>> = Rc::new(RefCell::new(None));
    let g = f.clone();
    *g.borrow_mut() = Some(Closure::new(move || {
        if el.get_attribute("data-counting").as_deref() != Some(gen.to_string().as_str()) {
            let _ = f.borrow_mut().take();
            return;
        }
        let p = ((now_ms() - t0) / ms).min(1.0);
        el.set_text_content(Some(&frame_at(&text, ease_out(p))));
        if p < 1.0 {
            if let Some(cb) = f.borrow().as_ref() {
                let _ = dom::window().request_animation_frame(cb.as_ref().unchecked_ref());
            }
        } else {
            let _ = f.borrow_mut().take();
        }
    }));
    let first = g.borrow();
    if let Some(cb) = first.as_ref() {
        let _ = dom::window().request_animation_frame(cb.as_ref().unchecked_ref());
    }
}

/// A text whose numbers count up when it first shows and whenever it
/// changes (tabular numbers: `.count-up`).
#[component]
pub(crate) fn Count(#[prop(into)] text: Signal<String>, #[prop(optional)] ms: Option<f64>) -> impl IntoView {
    let node = NodeRef::<leptos::html::Span>::new();
    let ms = ms.unwrap_or(900.0);
    Effect::new(move |prev: Option<String>| {
        let t = text.get();
        if let Some(el) = node.get() {
            if prev.as_deref() != Some(t.as_str()) {
                count_text(el.into(), t.clone(), ms);
            }
            t
        } else {
            String::new()
        }
    });
    view! { <span class="count-up" node_ref=node></span> }
}

// ---------------------------------------------------------------------
// The boot

#[derive(Clone, Copy, PartialEq, Eq)]
enum Boot {
    Never,
    Up,
    Playing,
    Done,
}

/// The carpet's cells in the order they light — the lifecycle, the
/// accent last (src/carpet.rs's).
fn boot_sheet(words: &str) -> Option<web_sys::Element> {
    let doc = dom::window().document()?;
    let sheet = doc.create_element("div").ok()?;
    sheet.set_id("boot");
    let _ = sheet.set_attribute("aria-hidden", "true");
    let inner = doc.create_element("div").ok()?;
    inner.set_class_name("boot-in");
    const SVG: &str = "http://www.w3.org/2000/svg";
    let svg = doc.create_element_ns(Some(SVG), "svg").ok()?;
    let _ = svg.set_attribute("class", "carpet boot-mark");
    let _ = svg.set_attribute("viewBox", "0 0 27 27");
    for (c, x, y) in crate::carpet::CELLS {
        let r = doc.create_element_ns(Some(SVG), "rect").ok()?;
        let _ = r.set_attribute("class", c);
        let _ = r.set_attribute("x", &x.to_string());
        let _ = r.set_attribute("y", &y.to_string());
        let _ = r.set_attribute("width", "9");
        let _ = r.set_attribute("height", "9");
        let _ = svg.append_child(&r);
    }
    let p = doc.create_element("p").ok()?;
    p.set_class_name("boot-count");
    let b = doc.create_element("b").ok()?;
    b.set_text_content(Some("0"));
    let _ = p.append_child(&b);
    let rest = doc.create_element("span").ok()?;
    rest.set_text_content(Some(&format!(" tickets · {words}")));
    let _ = p.append_child(&rest);
    let _ = inner.append_child(&svg);
    let _ = inner.append_child(&p);
    let _ = sheet.append_child(&inner);
    Some(sheet)
}

fn sheet() -> Option<web_sys::Element> {
    dom::window().document()?.get_element_by_id("boot")
}

/// How long the sheet waits for a first `health` before it wipes anyway.
const BOOT_WAIT_MS: i32 = 2_500;

/// The app's start: the sheet up (under `full`), the cells dark and the
/// words "connecting …", until [`boot_live`].
pub(crate) fn boot_begin(url: &str) {
    if BOOT.with(Cell::get) != Boot::Never {
        return;
    }
    BOOT.with(|b| b.set(Boot::Done));
    if current() != Motion::Full {
        return;
    }
    let Some(body) = dom::window().document().and_then(|d| d.body()) else { return };
    let Some(s) = boot_sheet(&format!("connecting {url}")) else { return };
    let _ = body.append_child(&s);
    BOOT.with(|b| b.set(Boot::Up));
    spawn_local(async move {
        dom::sleep_ms(BOOT_WAIT_MS).await;
        if BOOT.with(Cell::get) == Boot::Up {
            wipe().await;
        }
    });
}

/// The first `health`: the cells light one by one (in the lifecycle's
/// colours) while the count runs to `total`, then the sheet wipes away —
/// 1.3 s in all. Once: a reconnect's `health` finds the boot done.
pub(crate) fn boot_live(total: u64) {
    if BOOT.with(Cell::get) != Boot::Up {
        return;
    }
    BOOT.with(|b| b.set(Boot::Playing));
    let Some(s) = sheet() else { return };
    if let Ok(cells) = s.query_selector_all("rect") {
        for i in 0..cells.length() {
            let Some(r) = cells.item(i).and_then(|n| n.dyn_into::<web_sys::Element>().ok()) else { continue };
            spawn_local(async move {
                dom::sleep_ms(40 + i as i32 * 70).await;
                let _ = r.class_list().add_1("on");
            });
        }
    }
    if let Ok(Some(b)) = s.query_selector(".boot-count b") {
        count_text(b, total.to_string(), 640.0);
    }
    if let Ok(Some(w)) = s.query_selector(".boot-count span") {
        spawn_local(async move {
            dom::sleep_ms(560).await;
            w.set_text_content(Some(" tickets · live"));
        });
    }
    spawn_local(async move {
        dom::sleep_ms(700).await;
        wipe().await;
    });
}

/// The wipe (.6 s), then the sheet is gone.
async fn wipe() {
    BOOT.with(|b| b.set(Boot::Done));
    let Some(s) = sheet() else { return };
    let _ = s.class_list().add_1("done");
    dom::sleep_ms(600).await;
    s.remove();
}

/// The setting moved off `full` mid-boot: the sheet goes at once.
fn boot_end() {
    if matches!(BOOT.with(Cell::get), Boot::Up | Boot::Playing) {
        BOOT.with(|b| b.set(Boot::Done));
    }
    if let Some(s) = sheet() {
        s.remove();
    }
}

// ---------------------------------------------------------------------
// A ticket opening

/// Where an id stood when its ticket was opened from a row.
struct Origin {
    id: String,
    left: f64,
    top: f64,
    height: f64,
    at: f64,
}

fn note(id: &str, el: &web_sys::Element) {
    let r = el.get_bounding_client_rect();
    if r.height() <= 0.0 {
        return;
    }
    ORIGIN.with(|o| {
        *o.borrow_mut() = Some(Origin { id: id.to_string(), left: r.left(), top: r.top(), height: r.height(), at: now_ms() })
    });
}

/// `id` is being opened: where it stands now — the palette's highlighted
/// row, else its board row or nav row — is where its page's id moves from.
/// A row's own click noted it already.
pub(crate) fn note_origin(id: &str) {
    let Some(doc) = dom::window().document() else { return };
    let fresh = ORIGIN.with(|o| o.borrow().as_ref().is_some_and(|o| o.id == id && now_ms() - o.at < 300.0));
    if fresh {
        return;
    }
    if let Ok(Some(code)) = doc.query_selector("#palette-list li.hi .code") {
        if code.text_content().is_some_and(|t| t.trim() == id) {
            note(id, &code);
            return;
        }
    }
    let q = css_escape(id);
    for sel in [format!("tbody#board tr[data-ticket-id=\"{q}\"] td.id a"), format!("#nav .row[data-ticket-id=\"{q}\"] .code")] {
        if let Ok(Some(el)) = doc.query_selector(&sel) {
            note(id, &el);
            return;
        }
    }
}

/// How long a noted origin stays good for the page it opens.
const ORIGIN_MS: f64 = 1_500.0;

/// The ticket page's head is in: the title rises, the body develops (the
/// `opening` class on its article), and — opened from a row a moment ago —
/// the id moves from that row's id to here (a FLIP). Calm keeps it.
pub(crate) fn open_page(h2: web_sys::Element, id: String) {
    if current() == Motion::None {
        return;
    }
    if let Some(article) = h2.parent_element() {
        replay(&article, "opening", 900);
    }
    let origin = ORIGIN.with(|o| o.borrow_mut().take()).filter(|o| o.id == id && now_ms() - o.at < ORIGIN_MS);
    let (Some(o), Ok(Some(code))) = (origin, h2.query_selector(".id")) else { return };
    let Ok(code) = code.dyn_into::<web_sys::HtmlElement>() else { return };
    let last = code.get_bounding_client_rect();
    if last.height() <= 0.0 {
        return;
    }
    let (dx, dy, s) = (o.left - last.left(), o.top - last.top(), o.height / last.height());
    let style = code.style();
    let _ = style.set_property("transform-origin", "0 0");
    let _ = style.set_property("transform", &format!("translate({dx:.1}px, {dy:.1}px) scale({s:.3})"));
    let _ = code.offset_width();
    let _ = style.set_property("transition", "transform .5s cubic-bezier(.2,.8,.2,1)");
    let _ = style.set_property("transform", "none");
    spawn_local(async move {
        dom::sleep_ms(650).await;
        let style = code.style();
        for p in ["transition", "transform", "transform-origin"] {
            let _ = style.remove_property(p);
        }
    });
}

// ---------------------------------------------------------------------
// LOOK-8: the nav in motion

thread_local! {
    /// Ids a `watch` change brought into the nav, and when — a row drawn for
    /// one soon after arrives with a move ([`row_mounted`]).
    static FRESH: RefCell<Vec<(String, f64)>> = const { RefCell::new(Vec::new()) };
    /// When the nav's filter last changed: rows drawn soon after settle in.
    static RELENSED: Cell<f64> = const { Cell::new(f64::NEG_INFINITY) };
}

/// How long an arrival or a new filter stays good for the rows it draws —
/// the server's answer comes in between.
pub(crate) const FRESH_MS: f64 = 1_500.0;

/// What a nav row drawn now plays: `arrive` for an id a `watch` change
/// brought `age_fresh` ms ago, `settle` for any row drawn `age_lens` ms after
/// the filter changed, else nothing. Pure: the clock is the caller's.
pub(crate) fn row_move(age_fresh: Option<f64>, age_lens: f64) -> Option<&'static str> {
    if age_fresh.is_some_and(|a| (0.0..FRESH_MS).contains(&a)) {
        Some("arrive")
    } else if (0.0..FRESH_MS).contains(&age_lens) {
        Some("settle")
    } else {
        None
    }
}

/// A `watch` change brought `id` into the nav (a birth, a move, a filter's
/// entered).
pub(crate) fn fresh(id: &str) {
    let now = now_ms();
    FRESH.with(|f| {
        let mut f = f.borrow_mut();
        f.retain(|(_, at)| now - *at < FRESH_MS);
        f.push((id.to_string(), now));
    });
}

/// The nav's filter changed.
pub(crate) fn relensed() {
    RELENSED.with(|r| r.set(now_ms()));
}

/// A nav row for `id` was drawn: it arrives or settles when it should
/// ([`row_move`]). An arrival is taken: the next drawing of the row is plain.
pub(crate) fn row_mounted(el: web_sys::Element, id: &str) {
    if current() == Motion::None {
        return;
    }
    let now = now_ms();
    let age = FRESH.with(|f| {
        let mut f = f.borrow_mut();
        let at = f.iter().position(|(x, _)| x == id)?;
        Some(now - f.remove(at).1)
    });
    let lens = now - RELENSED.with(Cell::get);
    if let Some(class) = row_move(age, lens) {
        replay(&el, class, 700);
    }
}

/// A node of the nav was opened (`true`) or closed: its caret turns, and an
/// opened node's children drop in beneath it. Closing unmounts at once —
/// nothing waits on a move.
pub(crate) fn node_toggled(node: web_sys::Element, opened: bool) {
    if current() == Motion::None {
        return;
    }
    spawn_local(async move {
        dom::sleep_ms(0).await;
        if let Ok(Some(caret)) = node.query_selector(":scope > .row .toggle.clickable") {
            replay(&caret, if opened { "turn-open" } else { "turn-shut" }, 400);
        }
        if opened {
            if let Ok(Some(kids)) = node.query_selector(":scope > .kids") {
                replay(&kids, "opening", 500);
            }
        }
    });
}

/// Rows at `selector` are leaving the nav: each leaves a ghost where it
/// stood that fades and slides away (≤ 140 ms) while the real row goes at
/// once — the tree never waits on it.
pub(crate) fn ghost(selector: &str) {
    if current() == Motion::None {
        return;
    }
    let Some(doc) = dom::window().document() else { return };
    // Inside the tree, so it wears the tree's face and size.
    let Ok(Some(host)) = doc.query_selector("#nav .tree") else { return };
    let Ok(rows) = doc.query_selector_all(selector) else { return };
    for i in 0..rows.length() {
        let Some(row) = rows.item(i).and_then(|n| n.dyn_into::<web_sys::Element>().ok()) else { continue };
        let r = row.get_bounding_client_rect();
        if r.height() <= 0.0 {
            continue;
        }
        let Ok(copy) = row.clone_node_with_deep(true) else { continue };
        let Ok(copy) = copy.dyn_into::<web_sys::HtmlElement>() else { continue };
        for a in ["id", "tabindex", "data-ticket-id", "data-nav-id", "data-node-key", "data-pick", "href", "role", "aria-owns"] {
            let _ = copy.remove_attribute(a);
        }
        let _ = copy.set_attribute("aria-hidden", "true");
        let _ = copy.class_list().add_1("ghost");
        let st = copy.style();
        for (k, v) in [
            ("position", "fixed".to_string()),
            ("left", format!("{}px", r.left())),
            ("top", format!("{}px", r.top())),
            ("width", format!("{}px", r.width())),
            ("height", format!("{}px", r.height())),
            ("margin", "0".to_string()),
            ("pointer-events", "none".to_string()),
            ("z-index", "5".to_string()),
        ] {
            let _ = st.set_property(k, &v);
        }
        let _ = host.append_child(&copy);
        spawn_local(async move {
            dom::sleep_ms(220).await;
            copy.remove();
        });
    }
}

/// The selector for `id`'s rows in the nav.
pub(crate) fn nav_rows(id: &str) -> String {
    format!("#nav .row[data-ticket-id=\"{}\"]", css_escape(id))
}

// ---------------------------------------------------------------------
// The pip

/// The lifecycle's five, in order — the pip's colours.
pub(crate) const LIFECYCLE: [&str; 5] = ["Pending", "Ready", "Coding", "In Review", "Complete"];

/// The eight cells of the pip, each the index (into [`LIFECYCLE`]) of the
/// status its place falls into when the eight are divided among the counts
/// in proportion; `None` when there is nothing to divide.
pub(crate) fn pip_cells(counts: [u64; 5]) -> Option<[usize; 8]> {
    let total: u64 = counts.iter().sum();
    if total == 0 {
        return None;
    }
    let mut out = [0usize; 8];
    for (i, cell) in out.iter_mut().enumerate() {
        // The cell's middle, as a share of the total.
        let at = (2 * i as u64 + 1) * total;
        let mut run = 0u64;
        for (s, n) in counts.iter().enumerate() {
            run += n * 16;
            if at < run {
                *cell = s;
                break;
            }
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_setting_wins_and_the_system_decides_the_default() {
        assert_eq!(resolve(Pref::System, false), Motion::Full);
        assert_eq!(resolve(Pref::System, true), Motion::None);
        assert_eq!(resolve(Pref::Full, true), Motion::Full);
        assert_eq!(resolve(Pref::Calm, true), Motion::Calm);
        assert_eq!(resolve(Pref::None, false), Motion::None);
        for p in Pref::ALL {
            assert_eq!(Pref::parse(p.word()), p, "{p:?} round-trips");
        }
        assert_eq!(Pref::parse(Some("wild")), Pref::System);
    }

    /// DESIGN-43: Typed text — four speeds, `normal` the default (not
    /// stored), each round-tripping through the record and the attribute.
    #[test]
    fn typed_text_reads_back_and_defaults_to_normal() {
        assert_eq!(Typing::default(), Typing::Normal);
        assert_eq!(Typing::Normal.word(), None);
        for t in Typing::ALL {
            assert_eq!(Typing::parse(t.word()), t, "{t:?} round-trips as stored");
            assert_eq!(Typing::parse(Some(t.as_str())), t, "{t:?} round-trips as stamped");
        }
        assert_eq!(Typing::parse(None), Typing::Normal);
        assert_eq!(Typing::parse(Some(" fast ")), Typing::Fast);
        assert_eq!(Typing::parse(Some("ludicrous")), Typing::Normal);
        assert_eq!(Typing::ALL.map(Typing::as_str), ["off", "slow", "normal", "fast"]);
    }

    #[test]
    fn a_counter_frame_scales_every_number_and_ends_on_the_text() {
        assert_eq!(frame_at("722 tickets", 0.0), "0 tickets");
        assert_eq!(frame_at("722 tickets", 0.5), "361 tickets");
        assert_eq!(frame_at("3/5", 0.5), "2/3");
        assert_eq!(frame_at("12 of 722 tickets", 1.0), "12 of 722 tickets");
        assert_eq!(frame_at("…", 0.3), "…");
        assert_eq!(ease_out(0.0), 0.0);
        assert_eq!(ease_out(1.0), 1.0);
        assert!(ease_out(0.5) > 0.5, "ease-out runs ahead");
    }

    /// LOOK-8: a nav row drawn soon after a `watch` change brought it
    /// arrives; soon after the filter moved, settles; otherwise plays nothing.
    #[test]
    fn a_nav_row_arrives_or_settles_only_soon_after_its_cause() {
        assert_eq!(row_move(Some(10.0), f64::INFINITY), Some("arrive"));
        assert_eq!(row_move(Some(10.0), 10.0), Some("arrive"), "an arrival wins over a new filter");
        assert_eq!(row_move(None, 200.0), Some("settle"));
        assert_eq!(row_move(Some(FRESH_MS + 1.0), 200.0), Some("settle"));
        assert_eq!(row_move(Some(FRESH_MS + 1.0), FRESH_MS + 1.0), None);
        assert_eq!(row_move(None, f64::INFINITY), None, "a row scrolled into view later plays nothing");
    }

    #[test]
    fn the_pip_divides_its_eight_cells_in_proportion() {
        assert_eq!(pip_cells([0; 5]), None);
        assert_eq!(pip_cells([0, 0, 0, 0, 9]), Some([4; 8]));
        assert_eq!(pip_cells([1, 1, 0, 0, 2]), Some([0, 0, 1, 1, 4, 4, 4, 4]));
        // A status too small for a cell takes none; order is the lifecycle's.
        let cells = pip_cells([5, 1, 1, 1, 92]).unwrap();
        assert!(cells.windows(2).all(|w| w[0] <= w[1]));
        assert_eq!(cells[7], 4);
    }
}
