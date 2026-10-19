//! MENGER-15: how a framed body is shown — **full screen**, and **as a
//! website mockup**.
//!
//! Every framed body (src/frame.rs) sits in a [`Framed`] box: a slim bar
//! over the frame with a full-screen control (and `z`, src/keys.rs; `z`
//! inside a focused body too — the SDK and the page shim post it). Full
//! screen is the same box, the same `<iframe>` — never a new frame, origin
//! or window: its sandbox and CSP are unchanged — made to fill the viewport:
//! the Fullscreen API on the box when the browser allows it (a gesture is
//! under way), and always a fixed overlay over the app (`.framed.is-full`),
//! which is all a deep link gets. Esc leaves it (the browser's own Esc in
//! native full screen; the app's key, or the frame's posted Esc, in the
//! overlay), as does the bar's exit. Full, the bar names the ticket — its
//! id and title. On the ticket page the route says so: `#/ticket/ID?view=full`
//! opens the ticket straight into it, and entering or leaving rewrites the
//! query in place (no history entry, no hashchange).
//!
//! **A body declares its view** ([`declared`]) — in its frontmatter:
//!
//! ```yaml
//! ---
//! view: mockup            # inline (default) | full | mockup
//! widths: 390, 768, 1440  # a mockup's devices: W or WxH, at most 6
//! ---
//! ```
//!
//! or, for a whole HTML document that should stay a plain file, in its
//! head: `<meta name="menger-view" content="mockup">` and `<meta
//! name="menger-widths" content="390,768x1024,1440">` (frontmatter wins).
//! `full` opens the ticket page full screen, its exit reading "Back to the
//! ticket"; `mockup` draws the page in device chrome at each width.
//!
//! **A mockup** renders the body once per device, each frame `W` CSS px
//! wide and `H` tall (a device's own viewport: its media queries see `W`),
//! scaled to fit — side by side at one common scale (sizes stay true to one
//! another), or one device at a time — with a light/dark toggle and a
//! reduced-motion toggle that the framed page sees as its
//! `prefers-color-scheme` / `prefers-reduced-motion` (src/frame.rs
//! [`frame::Emulate`]; frontend/sdk/menger-page.js), applied live. Full
//! screen applies to a mockup too: the devices scale to the screen.
//! APP-33: the viewer sizes it — a Size menu of standard devices, Custom
//! `W×H`, a device's edge dragged, Rotate, Reset (src/mocksize.rs).
//!
//! APP-42: **a mockup is one view, and what it shows is said outside it.**
//! A mockup's caption ([`Caption`]) — an `<m-mockup>`'s `title` and
//! `description` (and its `<m-description>` child), a fence's `title=` /
//! `description=`, a `view: mockup` body's frontmatter `description:` (or a
//! whole document's `<meta name="menger-description">`) — is drawn by this
//! box above its bar and its devices, in the app's DOM and type: never in
//! the mocked page, never changing a device's viewport. And when the page
//! at the narrowest device shown is more than [`TALL_SCREENS`] of its
//! screens tall ([`screens`]) — the page shim reports its height,
//! `menger:extent` — the bar says so, quietly ([`tall_note`]): a hint to the
//! author that another view is another mockup, never a refusal.
//!
//! APP-60: **on a phone** (≤ [`crate::phonebar::PHONE_MAX`]) a mockup's
//! bar is one row — its option's *pick this* (in a decision), the device
//! picker, ⋯ and Full screen; the rest (*comment*, Size, Reset, Light /
//! Dark, Reduced motion, Compare, the tall note) folds into the ⋯ menu —
//! and it opens on one device at the column's width (src/mocksize.rs).
//!
//! APP-99: **on a phone a mockup is a preview** at most a third of the
//! screen tall ([`crate::mocksize::preview_scale`]) — inert; a tap or ⤢ Open
//! opens it full screen, and Back ([`push_full`]), Exit, Esc or a swipe down
//! ([`swiped_down`]) closes it. Any mockup **collapses** to one line (its
//! title, [`views_note`], ▸), kept per mockup on this device ([`fold_key`]).
//!
//! APP-66: **a phone is drawn as an iPhone's screen** ([`screen`]): the
//! display's rounded corners clip the page, the notch or the Dynamic Island
//! and the home indicator are drawn over it, and the page is told the
//! safe-area insets (the frame's [`Emulate`]`::safe`; the page shim answers
//! `env(safe-area-inset-*)` with them and sets `--sai-*`), so a page that
//! pads by them shows it and one that doesn't is clipped as on the phone.
//! On by default; the bar's *Screen shape* turns it off for the mockup.
//!
//! What decides — the declaration, the devices, the scale, the route's
//! query, the caption, the tall check — is pure and pinned by native tests;
//! the rest is DOM.

use std::cell::{Cell, RefCell};

use js_sys::{Function, Promise, Reflect};
use leptos::prelude::*;
use wasm_bindgen::closure::Closure;
use wasm_bindgen::{JsCast, JsValue};

use crate::dom;
use crate::frame::{Emulate, Frame, Scheme};

/// How a body asks to be shown.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub(crate) enum Mode {
    /// In the page, as every framed body (the default).
    #[default]
    Inline,
    /// Full screen when its ticket page opens.
    Full,
    /// In device chrome at each of its widths.
    Mockup,
}

impl Mode {
    fn parse(s: &str) -> Option<Self> {
        match s.trim().trim_matches(['"', '\'']).to_ascii_lowercase().as_str() {
            "inline" | "page" => Some(Mode::Inline),
            "full" | "fullscreen" | "full-screen" => Some(Mode::Full),
            "mockup" | "mockups" => Some(Mode::Mockup),
            _ => None,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Mode::Inline => "inline",
            Mode::Full => "full",
            Mode::Mockup => "mockup",
        }
    }
}

/// One mockup device: its viewport, in CSS px.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Device {
    pub(crate) w: u32,
    pub(crate) h: u32,
}

impl Device {
    /// A width alone: a phone's, a tablet's or a desktop's height.
    pub(crate) fn of_width(w: u32) -> Self {
        let h = if w < 600 {
            844
        } else if w < 1100 {
            1024
        } else {
            900
        };
        Device { w, h }
    }

    pub(crate) fn kind(self) -> &'static str {
        if self.w < 600 {
            "phone"
        } else if self.w < 1100 {
            "tablet"
        } else {
            "desktop"
        }
    }
}

/// APP-66: an iPhone's screen as a mockup's phone draws it — the display's
/// rounded corners, the notch or the Dynamic Island, the home indicator, and
/// the safe-area insets iOS reports for them, in CSS px (pt).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Screen {
    /// The display's corner radius.
    pub(crate) radius: u32,
    pub(crate) cutout: Cutout,
    /// Whether the device is on its side (the cutout on the left, the
    /// insets on both sides).
    pub(crate) landscape: bool,
    pub(crate) insets: crate::frame::Insets,
}

/// APP-66: what cuts into the top of an iPhone's screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Cutout {
    Notch,
    Island,
}

impl Cutout {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Cutout::Notch => "notch",
            Cutout::Island => "island",
        }
    }
}

/// APP-66: the screen a phone-sized device has — an iPhone with a notch or
/// an island, by its short side (375 the X–13 mini class, 390 the 12–14,
/// 393 the 14 Pro–16, 402 the 16 Pro, 428 the Pro Max with a notch, 430
/// the Plus and Pro Max with an island, 440 the 16 Pro Max), when it is as
/// tall as one (2 : 1 or more); none for a tablet, a desktop, or a phone
/// with a home button (375 × 667). Portrait: the top inset, 34 at the
/// bottom; on its side: the top inset on both sides, 21 at the bottom.
pub(crate) fn screen(d: Device) -> Option<Screen> {
    let (short, long) = (d.w.min(d.h), d.w.max(d.h));
    if short >= 600 || f64::from(long) < 2.0 * f64::from(short) {
        return None;
    }
    let (radius, cutout, top) = match short {
        0..=389 => (44, Cutout::Notch, 50),
        390..=392 => (47, Cutout::Notch, 47),
        393..=401 => (55, Cutout::Island, 59),
        402..=427 => (62, Cutout::Island, 62),
        428..=429 => (53, Cutout::Notch, 47),
        430..=439 => (55, Cutout::Island, 59),
        _ => (62, Cutout::Island, 62),
    };
    let landscape = d.w > d.h;
    let insets = if landscape {
        crate::frame::Insets { top: 0, right: top, bottom: 21, left: top }
    } else {
        crate::frame::Insets { top, right: 0, bottom: 34, left: 0 }
    };
    Some(Screen { radius, cutout, landscape, insets })
}

/// The widths a mockup shows when the body names none: a phone, a tablet,
/// a desktop.
pub(crate) const DEFAULT_WIDTHS: [u32; 3] = [390, 768, 1440];

/// The most devices a mockup draws.
pub(crate) const MAX_DEVICES: usize = 6;

/// What a body declares: its view, a mockup's devices, and (APP-42) what
/// the mockup shows, said above it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Decl {
    pub(crate) mode: Mode,
    pub(crate) devices: Vec<Device>,
    pub(crate) caption: Caption,
}

impl Default for Decl {
    fn default() -> Self {
        Decl {
            mode: Mode::Inline,
            devices: DEFAULT_WIDTHS.iter().map(|w| Device::of_width(*w)).collect(),
            caption: Caption::default(),
        }
    }
}

/// APP-42: a mockup's caption — a title (one line) and a description
/// (markdown, a line or a paragraph or two), drawn above the devices.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Caption {
    pub(crate) title: Option<String>,
    pub(crate) description: Option<String>,
}

/// The longest title a caption keeps, in characters.
pub(crate) const TITLE_MAX: usize = 200;
/// The longest description a caption keeps, in characters.
pub(crate) const DESCRIPTION_MAX: usize = 4_000;

impl Caption {
    /// A caption from what was written: each trimmed, blank ones none, a
    /// title one line (its first) and clipped, a description clipped.
    pub(crate) fn of(title: Option<&str>, description: Option<&str>) -> Self {
        let title = title
            .and_then(|t| t.lines().map(str::trim).find(|l| !l.is_empty()))
            .map(|t| clip_chars(t, TITLE_MAX));
        let description = description.map(str::trim).filter(|d| !d.is_empty()).map(|d| clip_chars(d, DESCRIPTION_MAX));
        Caption { title, description }
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.title.is_none() && self.description.is_none()
    }

    /// This caption, or — when it says nothing — `other`'s (an element's
    /// own caption wins over the referenced page's declared one).
    pub(crate) fn or(self, other: Option<&Caption>) -> Caption {
        if self.is_empty() {
            other.cloned().unwrap_or_default()
        } else {
            self
        }
    }
}

fn clip_chars(s: &str, max: usize) -> String {
    match s.char_indices().nth(max) {
        Some((i, _)) => format!("{}…", s[..i].trim_end()),
        None => s.to_string(),
    }
}

/// APP-42: a page more than this many of its device's screens tall draws
/// the bar's note.
pub(crate) const TALL_SCREENS: u32 = 3;

/// APP-42: how many screens a page `doc` px tall fills in a viewport `view`
/// px tall — `Some` only when that is more than [`TALL_SCREENS`] of them
/// (rounded up: a page 3.2 screens tall scrolls 4).
pub(crate) fn screens(doc: u32, view: u32) -> Option<u32> {
    if view == 0 || doc <= TALL_SCREENS.saturating_mul(view) {
        return None;
    }
    Some(doc.div_ceil(view))
}

/// APP-42: a frame's page, as the shim last measured it — its document's
/// height and the viewport it was measured in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Extent {
    pub(crate) doc: u32,
    pub(crate) view: u32,
}

/// APP-42: the tall check for a mockup — of the devices `shown` (each its
/// slot's key and size), the narrowest (the first, on a tie), and the extent
/// its frame last reported (`seen`, by slot key): the screens it scrolls,
/// when too many. A device not yet measured says nothing.
pub(crate) fn tall(shown: &[(u32, Device)], seen: &[(u32, Extent)]) -> Option<u32> {
    let (key, _) = shown.iter().fold(None::<(u32, Device)>, |best, &(k, d)| match best {
        Some((_, b)) if b.w <= d.w => best,
        _ => Some((k, d)),
    })?;
    let e = seen.iter().find(|(k, _)| *k == key)?.1;
    screens(e.doc, e.view)
}

/// APP-42: the bar's note for a page `n` screens tall.
pub(crate) fn tall_note(n: u32) -> String {
    format!("one view per mockup — this page scrolls {n} screens")
}

/// `390, 768x1024, 1440` (or `[390, 768, 1440]`) — each `W` or `WxH`, a
/// width 240–3840 and a height 240–4320; anything else is dropped. None
/// left: `None` (the defaults stand).
pub(crate) fn parse_widths(s: &str) -> Option<Vec<Device>> {
    let s = s.trim().trim_start_matches('[').trim_end_matches(']');
    let devices: Vec<Device> = s
        .split([',', ' '])
        .map(|w| w.trim().trim_matches(['"', '\'']))
        .filter(|w| !w.is_empty())
        .filter_map(|w| {
            let w = w.to_ascii_lowercase();
            let w = w.trim_end_matches("px");
            let (w, h) = match w.split_once(['x', '×']) {
                Some((w, h)) => (w.trim().parse::<u32>().ok()?, Some(h.trim().trim_end_matches("px").parse::<u32>().ok()?)),
                None => (w.parse::<u32>().ok()?, None),
            };
            if !(240..=3840).contains(&w) || h.is_some_and(|h| !(240..=4320).contains(&h)) {
                return None;
            }
            Some(h.map_or_else(|| Device::of_width(w), |h| Device { w, h }))
        })
        .take(MAX_DEVICES)
        .collect();
    (!devices.is_empty()).then_some(devices)
}

/// A frontmatter's `key: value`, for a top-level key.
fn fm_value<'a>(fm: &'a str, key: &str) -> Option<&'a str> {
    fm.lines().find_map(|line| {
        let (k, v) = line.split_once(':')?;
        (k == key).then(|| v.split(" #").next().unwrap_or(v).trim())
    })
}

/// APP-42: a frontmatter's text value — quoted (`"…"` / `'…'`: as written
/// inside the quotes, a ` #` included), or plain (a trailing ` # comment`
/// dropped). `None` when absent or blank.
fn fm_text<'a>(fm: &'a str, key: &str) -> Option<&'a str> {
    let v = fm.lines().find_map(|line| {
        let (k, v) = line.split_once(':')?;
        (k == key).then_some(v.trim())
    })?;
    let v = match v.chars().next() {
        Some(q @ ('"' | '\'')) => {
            let inner = &v[1..];
            &inner[..inner.rfind(q).unwrap_or(inner.len())]
        }
        _ => v.split(" #").next().unwrap_or(v).trim(),
    };
    (!v.trim().is_empty()).then_some(v.trim())
}

/// A whole document's `<meta name="NAME" content="…">`, anywhere in it.
fn meta_content<'a>(src: &'a str, name: &str) -> Option<&'a str> {
    let lower_needle = format!("name=\"{name}\"");
    let at = src.find(&lower_needle).or_else(|| src.find(&format!("name='{name}'")).or_else(|| src.find(&format!("name={name}"))))?;
    let open = src[..at].rfind('<')?;
    if !src[open..].get(..5).is_some_and(|t| t.eq_ignore_ascii_case("<meta")) {
        return None;
    }
    let close = at + src[at..].find('>')?;
    let tag = &src[open..close];
    let c = tag.find("content=")? + "content=".len();
    let rest = &tag[c..];
    let (q, rest) = match rest.chars().next()? {
        q @ ('"' | '\'') => (Some(q), &rest[1..]),
        _ => (None, rest),
    };
    let end = match q {
        Some(q) => rest.find(q)?,
        None => rest.find(|c: char| c.is_ascii_whitespace() || c == '/').unwrap_or(rest.len()),
    };
    Some(&rest[..end])
}

/// What a body declares — from its frontmatter (`view:`, `widths:`), else,
/// for a whole document, its `<meta name="menger-view">` and
/// `<meta name="menger-widths">`. Anything unreadable is the default.
pub(crate) fn declared(frontmatter: Option<&str>, src: &str) -> Decl {
    let mut decl = Decl::default();
    let page = crate::md::is_page(src);
    let view = frontmatter
        .and_then(|fm| fm_value(fm, "view"))
        .and_then(Mode::parse)
        .or_else(|| page.then(|| meta_content(src, "menger-view")).flatten().and_then(Mode::parse));
    if let Some(mode) = view {
        decl.mode = mode;
    }
    let widths = frontmatter
        .and_then(|fm| fm_value(fm, "widths"))
        .and_then(parse_widths)
        .or_else(|| page.then(|| meta_content(src, "menger-widths")).flatten().and_then(parse_widths));
    if let Some(devices) = widths {
        decl.devices = devices;
    }
    // APP-42: what the mockup shows, said above it.
    let description = frontmatter
        .and_then(|fm| fm_text(fm, "description"))
        .or_else(|| page.then(|| meta_content(src, "menger-description")).flatten());
    decl.caption = Caption::of(None, description);
    decl
}

/// APP-29: the devices an `<m-mockup>` draws — the widths it names (a
/// fence's `widths=`, the element's `widths="…"`) when any is readable, else
/// what the page it shows declares (`declared`: a referenced ticket's
/// `widths:` or `<meta name="menger-widths">`), else the three defaults.
pub(crate) fn mock_devices(named: Option<&str>, declared: Option<&Decl>) -> Vec<Device> {
    named
        .and_then(parse_widths)
        .or_else(|| declared.map(|d| d.devices.clone()))
        .unwrap_or_else(|| Decl::default().devices)
}

/// The scale that fits `devices` (all of them side by side, `gap` px
/// apart) into `avail_w` px — and, when the height counts (full screen),
/// `avail_h` px — never above 1.
pub(crate) fn fit(devices: &[Device], gap: f64, avail_w: f64, avail_h: Option<f64>) -> f64 {
    if devices.is_empty() || avail_w <= 0.0 {
        return 1.0;
    }
    let total_w: f64 = devices.iter().map(|d| f64::from(d.w)).sum::<f64>();
    let gaps = gap * (devices.len() as f64 - 1.0);
    let mut s = ((avail_w - gaps).max(1.0) / total_w).min(1.0);
    if let Some(h) = avail_h.filter(|h| *h > 0.0) {
        let tallest = devices.iter().map(|d| f64::from(d.h)).fold(0.0, f64::max);
        s = s.min(h / tallest);
    }
    s.max(0.05)
}

/// APP-99: the localStorage key a mockup's collapse is kept under, by its
/// name (its sizes' name, [`crate::mocksize::storage_key`]'s).
pub(crate) fn fold_key(mockup: &str) -> String {
    format!("menger.mockup.folded.{mockup}")
}

/// APP-99: a collapsed mockup's one line past its title — its views, the
/// devices it declares.
pub(crate) fn views_note(n: usize) -> String {
    match n {
        1 => "1 view".into(),
        n => format!("{n} views"),
    }
}

/// APP-99: the full-screen viewer's swipe down, from the pointer's first
/// point to its last: far enough down, and more down than across.
pub(crate) fn swiped_down(dx: f64, dy: f64) -> bool {
    dy >= SWIPE_DOWN && dy > 2.0 * dx.abs()
}

/// APP-99: how far down a swipe closes the full-screen viewer, px.
pub(crate) const SWIPE_DOWN: f64 = 60.0;

/// Does the route's query ask for full screen (`?view=full`)?
pub(crate) fn wants_full(hash: &str) -> bool {
    hash.split_once('?').is_some_and(|(_, q)| q.split('&').any(|p| p == "view=full"))
}

/// The hash with `view=full` in its query (`on`) or out of it, the rest of
/// the query kept in order.
pub(crate) fn with_full(hash: &str, on: bool) -> String {
    let (path, query) = hash.split_once('?').unwrap_or((hash, ""));
    let mut params: Vec<&str> = query.split('&').filter(|p| !p.is_empty() && *p != "view=full").collect();
    if on {
        params.push("view=full");
    }
    if params.is_empty() {
        path.to_string()
    } else {
        format!("{path}?{}", params.join("&"))
    }
}

// -- the DOM half ------------------------------------------------------------

thread_local! {
    /// Which box is full screen (its number), if any.
    static FULL: ArcRwSignal<Option<u32>> = ArcRwSignal::new(None);
    static NEXT: Cell<u32> = const { Cell::new(0) };
    /// The ticket page's box — what `z` turns.
    static PAGE: Cell<Option<u32>> = const { Cell::new(None) };
    /// Whether the full box is in the browser's own full screen.
    static NATIVE: Cell<bool> = const { Cell::new(false) };
    static LISTENING: Cell<bool> = const { Cell::new(false) };
    /// Each mounted box's element, by number.
    static ROOTS: RefCell<Vec<(u32, web_sys::Element)>> = const { RefCell::new(Vec::new()) };
    /// APP-42: each mounted mockup's measured pages, by box number — each
    /// device's slot key and its frame's last [`Extent`].
    static EXTENTS: RefCell<Vec<(u32, ArcRwSignal<Vec<(u32, Extent)>>)>> = const { RefCell::new(Vec::new()) };
    /// APP-99: the box whose full screen pushed a history entry — Back
    /// closes it; leaving it another way takes the entry back.
    static PUSHED: Cell<Option<u32>> = const { Cell::new(None) };
}

fn fold_storage() -> Option<web_sys::Storage> {
    dom::window().local_storage().ok().flatten()
}

/// APP-99: whether the mockup `name` was left collapsed on this device.
fn folded_kept(name: Option<&str>) -> bool {
    name.and_then(|n| fold_storage()?.get_item(&fold_key(n)).ok().flatten()).is_some()
}

fn keep_folded(name: Option<&str>, folded: bool) {
    let (Some(n), Some(st)) = (name, fold_storage()) else { return };
    let _ = if folded { st.set_item(&fold_key(n), "1") } else { st.remove_item(&fold_key(n)) };
}

/// APP-99: a phone's full screen is one step of history — Back leaves it.
fn push_full(id: u32) {
    let w = dom::window();
    let Ok(h) = w.history() else { return };
    let url = w.location().href().ok();
    if h.push_state_with_url(&JsValue::from_str("menger-full"), "", url.as_deref()).is_ok() {
        PUSHED.with(|p| p.set(Some(id)));
    }
}

/// Leaving full screen not by Back: the entry it pushed is taken back.
fn pop_full(id: u32) {
    if PUSHED.with(|p| p.get()) != Some(id) {
        return;
    }
    PUSHED.with(|p| p.set(None));
    if let Ok(h) = dom::window().history() {
        let _ = h.back();
    }
}

/// APP-42: `menger:extent` from a device's frame — its page is `height` px
/// tall in its viewport. Kept for the box and slot the frame sits in;
/// anything else (a frame in no mockup) is ignored.
pub(crate) fn extent(frame: &web_sys::HtmlIFrameElement, height: u32) {
    let attr = |sel: &str, name: &str| {
        frame.closest(sel).ok().flatten().and_then(|el| el.get_attribute(name)).and_then(|n| n.parse::<u32>().ok())
    };
    let (Some(id), Some(slot)) = (attr(".framed[data-framed]", "data-framed"), attr(".device[data-slot]", "data-slot")) else {
        return;
    };
    let view = u32::try_from(frame.client_height()).unwrap_or(0);
    let Some(seen) = EXTENTS.with(|x| x.borrow().iter().find(|(n, _)| *n == id).map(|(_, s)| s.clone())) else { return };
    let now = Extent { doc: height, view };
    let known = seen.with_untracked(|v| v.iter().any(|(k, e)| *k == slot && *e == now));
    if !known {
        seen.update(|v| {
            v.retain(|(k, _)| *k != slot);
            v.push((slot, now));
        });
    }
}

fn full() -> ArcRwSignal<Option<u32>> {
    FULL.with(Clone::clone)
}

/// `z`: the full box leaves full screen; else the ticket page's box enters
/// it. Whether anything moved.
pub(crate) fn toggle_page() -> bool {
    let f = full();
    if f.get_untracked().is_some() {
        f.set(None);
        return true;
    }
    match PAGE.with(Cell::get) {
        Some(id) => {
            f.set(Some(id));
            true
        }
        None => false,
    }
}

/// `z` posted by a frame: its own box turns.
pub(crate) fn toggle_from(frame: &web_sys::HtmlIFrameElement) {
    let id = frame
        .closest(".framed[data-framed]")
        .ok()
        .flatten()
        .and_then(|el| el.get_attribute("data-framed"))
        .and_then(|n| n.parse::<u32>().ok());
    let f = full();
    match (f.get_untracked(), id) {
        (Some(_), _) => f.set(None),
        (None, Some(id)) => f.set(Some(id)),
        (None, None) => {}
    }
}

/// Esc: leave full screen, if a box is in it. Whether one was.
pub(crate) fn escape() -> bool {
    let f = full();
    if f.get_untracked().is_some() {
        f.set(None);
        return true;
    }
    false
}

/// Call `method` on `target` (a Fullscreen API method); a rejected promise
/// is caught (a deep link has no gesture to ask with: the overlay stands).
fn call(target: &JsValue, method: &str) {
    let Ok(f) = Reflect::get(target, &JsValue::from_str(method)) else { return };
    let Ok(f) = f.dyn_into::<Function>() else { return };
    if let Ok(p) = f.call0(target) {
        if let Ok(p) = p.dyn_into::<Promise>() {
            let noop = Closure::<dyn FnMut(JsValue)>::new(|_| {});
            let _ = p.catch(&noop);
            noop.forget();
        }
    }
}

/// Whether a gesture is under way (`navigator.userActivation.isActive`) —
/// only then is native full screen worth asking for.
fn activation() -> bool {
    let nav: JsValue = dom::window().navigator().into();
    Reflect::get(&nav, &JsValue::from_str("userActivation"))
        .ok()
        .filter(|a| a.is_object())
        .and_then(|a| Reflect::get(&a, &JsValue::from_str("isActive")).ok())
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
}

/// MENGER-34: whether the browser's own full screen is on — nothing of the
/// app's outside the box (the ticket menu) is drawn over it.
pub(crate) fn native() -> bool {
    fullscreen_element().is_some()
}

fn fullscreen_element() -> Option<web_sys::Element> {
    let doc: JsValue = dom::window().document()?.into();
    Reflect::get(&doc, &JsValue::from_str("fullscreenElement")).ok()?.dyn_into::<web_sys::Element>().ok()
}

/// The browser left its full screen (its own Esc): so does the box.
fn install() {
    if LISTENING.with(|l| l.replace(true)) {
        return;
    }
    let Some(doc) = dom::window().document() else { return };
    let cb = Closure::<dyn FnMut()>::new(|| {
        if fullscreen_element().is_some() {
            NATIVE.with(|n| n.set(true));
        } else if NATIVE.with(|n| n.replace(false)) {
            full().set(None);
            refocus_dialog();
        }
    });
    let _ = doc.add_event_listener_with_callback("fullscreenchange", cb.as_ref().unchecked_ref());
    cb.forget();
    // APP-99: Back on a phone's full screen closes it.
    let back = Closure::<dyn FnMut()>::new(|| {
        if let Some(id) = PUSHED.with(|p| p.take()) {
            let f = full();
            if f.get_untracked() == Some(id) {
                f.set(None);
            }
        }
    });
    let _ = dom::window().add_event_listener_with_callback("popstate", back.as_ref().unchecked_ref());
    back.forget();
}

/// Leaving the browser's full screen hands focus back to the element that
/// held it — the frame, for a link clicked in it — after a dialog that
/// opened meanwhile (MENGER-14's leave dialog) took it: the dialog takes it
/// again, so its keys (Esc, Enter, Tab) stay its own.
fn refocus_dialog() {
    let Some(doc) = dom::window().document() else { return };
    let in_dialog = doc.active_element().and_then(|a| a.closest("[role=dialog]").ok().flatten()).is_some();
    if in_dialog {
        return;
    }
    let target = doc
        .query_selector("[role=dialog] [data-act=open], [role=dialog] button")
        .ok()
        .flatten()
        .and_then(|el| el.dyn_into::<web_sys::HtmlElement>().ok());
    if let Some(el) = target {
        let _ = el.focus();
    }
}

/// The route's query, rewritten in place — no history entry, no hashchange.
fn set_route_full(on: bool) {
    let w = dom::window();
    let hash = w.location().hash().unwrap_or_default();
    if wants_full(&hash) == on || !hash.starts_with("#/ticket/") {
        return;
    }
    let next = with_full(&hash, on);
    if let Ok(h) = w.history() {
        let _ = h.replace_state_with_url(&JsValue::NULL, "", Some(&next));
    }
}

/// The box a framed body sits in (see the module doc): the bar, then the
/// frame — or, for `view: mockup`, its devices. `title` is the ticket
/// page's (a box elsewhere — a board note, the editor's preview — has
/// none): it names the ticket in the full-screen bar, and makes this the
/// box `z` and `?view=full` (and `view: full`) open.
#[component]
pub(crate) fn Framed(
    #[prop(into)] html: Signal<String>,
    #[prop(into)] decl: Signal<Decl>,
    ticket: Option<String>,
    #[prop(optional)] scope: Option<String>,
    #[prop(optional)] title: Option<Signal<String>>,
    #[prop(optional)] remember: Option<String>,
    // APP-46: what the box's owner adds at the head of its bar (a mockup's
    // *pick this* and *comment*, src/mockpick.rs).
    #[prop(optional)] actions: Option<ViewFn>,
    // APP-60: what it adds after them that a phone folds into ⋯ (*comment*).
    #[prop(optional)] more: Option<ViewFn>,
    // COMMENT-10: a draft's ticket, whose images its frame draws.
    #[prop(optional)] media: Option<String>,
) -> impl IntoView {
    install();
    let id = NEXT.with(|n| {
        let v = n.get().wrapping_add(1);
        n.set(v);
        v
    });
    let on_page = title.is_some();
    let root = NodeRef::<leptos::html::Div>::new();
    let stage = NodeRef::<leptos::html::Div>::new();
    let f = full();
    let is_full = {
        let f = f.clone();
        Memo::new(move |_| f.get() == Some(id))
    };
    if on_page {
        PAGE.with(|p| p.set(Some(id)));
    }
    root.on_load(move |el: web_sys::HtmlDivElement| {
        ROOTS.with(|r| r.borrow_mut().push((id, el.into())));
    });
    let seen = ArcRwSignal::new(Vec::<(u32, Extent)>::new());
    EXTENTS.with(|x| x.borrow_mut().push((id, seen.clone())));
    on_cleanup(move || {
        EXTENTS.with(|x| x.borrow_mut().retain(|(n, _)| *n != id));
        ROOTS.with(|r| r.borrow_mut().retain(|(n, _)| *n != id));
        if PAGE.with(Cell::get) == Some(id) {
            PAGE.with(|p| p.set(None));
        }
        let f = full();
        if f.get_untracked() == Some(id) {
            f.set(None);
        }
    });
    // The ticket page opens full when the route or the body says so — once,
    // as it mounts.
    if on_page {
        let declared_full = decl.with_untracked(|d| d.mode == Mode::Full);
        if declared_full || wants_full(&dom::window().location().hash().unwrap_or_default()) {
            f.set(Some(id));
        }
    }
    // Entering and leaving: the browser's full screen when a gesture allows,
    // the route's query on the ticket page.
    Effect::new(move |was: Option<bool>| {
        let now = is_full.get();
        if was == Some(now) || (was.is_none() && !now) {
            return now;
        }
        let el = ROOTS.with(|r| r.borrow().iter().find(|(n, _)| *n == id).map(|(_, e)| e.clone()));
        if now {
            if let Some(el) = el.filter(|_| activation()) {
                call(&el.into(), "requestFullscreen");
            }
        } else if fullscreen_element().is_some() {
            if let Some(doc) = dom::window().document() {
                call(&doc.into(), "exitFullscreen");
            }
        }
        if on_page {
            set_route_full(now);
        }
        now
    });
    let toggle = {
        let f = f.clone();
        move |_| {
            if f.get_untracked() == Some(id) {
                f.set(None);
            } else {
                f.set(Some(id));
            }
        }
    };
    let declared_full = move || decl.with(|d| d.mode == Mode::Full);
    let label = move || match (is_full.get(), declared_full() && on_page) {
        (true, true) => "← Back to the ticket",
        (true, false) => "Exit full screen",
        (false, _) => "⤢ Full screen",
    };
    let mode = move || decl.with(|d| d.mode.as_str());
    let mockup = Memo::new(move |_| decl.with(|d| d.mode == Mode::Mockup));
    // APP-33: the devices it declares, sized by the viewer — kept per
    // mockup (its own name, else its ticket's) in this browser.
    let declared_devices = Memo::new(move |_| decl.with(|d| d.devices.clone()));
    // APP-44: a mockup's name — the same as its sizes' — is what the page's
    // comparison holds it by.
    let compare_key = remember.or_else(|| ticket.clone());
    // APP-60: whether the viewport is a phone's, as it turns; APP-99: and
    // how tall it is, for a phone's preview.
    let phone = RwSignal::new(phone_now());
    let vh = RwSignal::new(height_now());
    let turned = window_event_listener(leptos::ev::resize, move |_| {
        let now = phone_now();
        if phone.try_get_untracked().is_some_and(|was| was != now) {
            phone.set(now);
        }
        let h = height_now();
        if vh.try_get_untracked().is_some_and(|was| was != h) {
            vh.set(h);
        }
    });
    on_cleanup(move || turned.remove());
    let size = RwSignal::new((0.0f64, 0.0f64));
    stage.on_load(move |el: web_sys::HtmlDivElement| observe(el, size));
    let sizer = crate::mocksize::Sizer::new(declared_devices.into(), compare_key.clone(), phone.into());
    let compare_attr = compare_key.clone();
    // APP-99: collapsed (kept per mockup on this device), and, on a phone
    // that cannot show it whole, a preview a third of the screen tall —
    // inert, a tap opens it full screen.
    let fold_name = StoredValue::new(compare_key.clone());
    let folded_mock = RwSignal::new(fold_name.with_value(|n| folded_kept(n.as_deref())));
    let collapsed = Memo::new(move |_| mockup.get() && folded_mock.get() && !is_full.get());
    let toggle_fold = move |_| {
        let now = !folded_mock.get_untracked();
        folded_mock.set(now);
        fold_name.with_value(|n| keep_folded(n.as_deref(), now));
    };
    let preview = Memo::new(move |_| {
        if !mockup.get() || !phone.get() || is_full.get() {
            return false;
        }
        let shown: Vec<Device> = sizer.shown().into_iter().map(|(_, d)| d).collect();
        crate::mocksize::preview_scale(&shown, size.get().0, vh.get()).is_some()
    });
    let views = Memo::new(move |_| views_note(declared_devices.get().len()));
    let compare_button = move || compare_key.clone().map(|k| crate::compare::toggle_button(k, mockup));
    // APP-42: what the mockup shows, said above it; and the note when its
    // page at the narrowest device shown scrolls several screens.
    let caption = Memo::new(move |_| decl.with(|d| if d.mode == Mode::Mockup && !d.caption.is_empty() { Some(d.caption.clone()) } else { None }));
    let tall_screens = Memo::new(move |_| if mockup.get() { tall(&sizer.shown(), &seen.get()) } else { None });
    // APP-66: the Screen shape toggle, where a device shown is a phone with one.
    let has_screen = Memo::new(move |_| mockup.get() && sizer.shown().iter().any(|(_, d)| screen(*d).is_some()));
    let ticket_id = ticket.clone();
    let heading = move || {
        let t = title?;
        Some(view! {
            <span class="framed-id code">{ticket_id.clone()}</span>
            <span class="framed-title">{move || t.get()}</span>
        })
    };
    // A mockup's toggles, kept while the box is mounted.
    let app_scheme = crate::frame::painted_scheme();
    let scheme = RwSignal::new(app_scheme);
    let reduce = RwSignal::new(prefers_reduced_motion());
    // APP-66: a phone drawn with its screen's shape — on until turned off.
    let shaped = RwSignal::new(true);
    let emulate = Signal::derive(move || Emulate { scheme: Some(scheme.get()), reduce: Some(reduce.get()), safe: None });
    let body = move || {
        let ticket = ticket.clone();
        let scope = scope.clone().unwrap_or_default();
        let media = media.clone().unwrap_or_default();
        if mockup.get() {
            sizer.devices(html, ticket, scope, emulate, shaped.into(), size, is_full, vh.into()).into_any()
        } else {
            view! { <Frame html ticket scope media /> }.into_any()
        }
    };
    let looks = move || {
        view! {
            <span class="framed-group" role="group" aria-label="colour scheme">
                <button class="framed-btn" aria-pressed=move || (scheme.get() == Scheme::Light).to_string() on:click=move |_| scheme.set(Scheme::Light)>"Light"</button>
                <button class="framed-btn" aria-pressed=move || (scheme.get() == Scheme::Dark).to_string() on:click=move |_| scheme.set(Scheme::Dark)>"Dark"</button>
            </span>
            <button class="framed-btn framed-motion" title="prefers-reduced-motion, as the page sees it" aria-pressed=move || reduce.get().to_string() on:click=move |_| reduce.update(|r| *r = !*r)>
                "Reduced motion"
            </button>
            {move || has_screen.get().then(|| view! {
                <button class="framed-btn framed-shape" title="An iPhone's corners, island and home indicator, and the safe-area insets the page sees"
                    aria-pressed=move || shaped.get().to_string() on:click=move |_| shaped.update(|v| *v = !*v)>
                    "Screen shape"
                </button>
            })}
        }
    };
    let note = move || {
        tall_screens.get().map(|n| view! { <span class="framed-note" role="note" title="A mockup is one screen at one state; another view is another mockup">{tall_note(n)}</span> })
    };
    // APP-60: a mockup on a phone folds its bar into one row.
    let folded = Memo::new(move |_| mockup.get() && phone.get());
    let more_open = RwSignal::new(false);
    let more_shown = Memo::new(move |_| folded.get() && (more_open.get() || sizer.field_open()));
    let close_more = move || {
        more_open.set(false);
        sizer.close_field();
    };
    let head_more = more.clone();
    let controls = {
        let compare_button = compare_button.clone();
        move || {
            if !mockup.get() {
                return None;
            }
            let compare_button = compare_button.clone();
            Some(if folded.get() {
                let more = more.clone();
                view! {
                    {sizer.picker()}
                    <span class="framed-fold">
                        <button class="framed-btn framed-more" title="More: comment, size, colour scheme, motion, compare" aria-label="More mockup controls"
                            aria-haspopup="true" aria-expanded=move || more_shown.get().to_string()
                            on:click=move |_| if more_shown.get_untracked() { close_more() } else { more_open.set(true) }>
                            "⋯"
                        </button>
                        <span class="framed-scrim" hidden=move || !more_shown.get() on:click=move |_| close_more()></span>
                        <span class="framed-panel" role="group" aria-label="More mockup controls" hidden=move || !more_shown.get()>
                            {more.map(|m| m.run())}
                            {sizer.tools()}
                            {looks()}
                            {compare_button()}
                            {note}
                        </span>
                    </span>
                }
                .into_any()
            } else {
                view! {
                    {sizer.picker()}
                    {sizer.tools()}
                    {looks()}
                    {compare_button()}
                }
                .into_any()
            })
        }
    };
    let data_compare = move || if mockup.get() { compare_attr.clone() } else { None };
    // APP-99: a phone's full screen of a mockup is a step of history (Back
    // leaves it) — not the ticket page's own box, whose route says it.
    Effect::new(move |was: Option<bool>| {
        let now = is_full.get();
        if was.is_some_and(|w| w != now) || (was.is_none() && now) {
            if now && mockup.get_untracked() && phone.get_untracked() && !on_page {
                push_full(id);
            } else if !now {
                pop_full(id);
            }
        }
        now
    });
    on_cleanup(move || pop_full(id));
    let open = {
        let f = f.clone();
        move |_| f.set(Some(id))
    };
    // APP-99: a swipe down the full screen's bar or caption closes it.
    let swipe = StoredValue::new(None::<(f64, f64)>);
    let swipe_down = move |ev: web_sys::PointerEvent| {
        let on_head = ev
            .target()
            .and_then(|t| t.dyn_into::<web_sys::Element>().ok())
            .and_then(|t| t.closest(".framed-bar, .framed-caption").ok().flatten())
            .is_some();
        swipe.set_value((is_full.get_untracked() && on_head).then(|| (f64::from(ev.client_x()), f64::from(ev.client_y()))));
    };
    let swipe_up = {
        let f = f.clone();
        move |ev: web_sys::PointerEvent| {
            let Some((x, y)) = swipe.get_value() else { return };
            swipe.set_value(None);
            if swiped_down(f64::from(ev.client_x()) - x, f64::from(ev.client_y()) - y) && f.get_untracked() == Some(id) {
                f.set(None);
            }
        }
    };
    let collapsed_title = move || decl.with(|d| d.caption.title.clone()).unwrap_or_else(|| "Mockup".into());
    view! {
        <div class="framed" class:is-full=move || is_full.get() data-framed=id.to_string() data-view=mode data-compare=data_compare node_ref=root
            data-preview=move || preview.get().then_some("") data-collapsed=move || collapsed.get().then_some("")
            on:pointerdown=swipe_down on:pointerup=swipe_up>
            {move || collapsed.get().then(|| view! {
                <button class="framed-collapsed" aria-expanded="false" title="Show the mockup" on:click=toggle_fold>
                    <span class="framed-collapsed-title">{collapsed_title}</span>
                    <span class="framed-collapsed-views">{move || views.get()}</span>
                    <span class="framed-collapse-mark" aria-hidden="true">"▸"</span>
                </button>
            })}
            {move || (!collapsed.get()).then(|| caption.get().map(caption_view))}
            <div class="framed-bar" data-folded=move || folded.get().then_some("") hidden=move || collapsed.get()>
                {move || (mockup.get() && !is_full.get()).then(|| view! {
                    <button class="framed-btn framed-collapse" aria-expanded="true" title="Collapse the mockup" aria-label="Collapse the mockup" on:click=toggle_fold>"▾"</button>
                })}
                {move || if is_full.get() { heading() } else { None }}
                {actions.map(|a| a.run())}
                {move || (!folded.get()).then(|| view! { {head_more.clone().map(|m| m.run())} {note} })}
                <span class="framed-gap"></span>
                {controls}
                <button class="framed-btn framed-full" title="Full screen (z) — Esc leaves it" aria-pressed=move || is_full.get().to_string() on:click=toggle>
                    {move || if preview.get() { "⤢ Open" } else { label() }}
                </button>
            </div>
            <div class="framed-stage" node_ref=stage hidden=move || collapsed.get()
                title=move || preview.get().then_some("Tap to open full screen")
                on:click=move |ev| if preview.get_untracked() { open(ev) }>{body}</div>
        </div>
    }
}

/// APP-99: the viewport's height.
fn height_now() -> f64 {
    dom::window().inner_height().ok().and_then(|v| v.as_f64()).unwrap_or(900.0)
}

/// APP-60: whether the viewport is a phone's (APP-50's shape).
fn phone_now() -> bool {
    crate::phonebar::is_phone(dom::window().inner_width().ok().and_then(|v| v.as_f64()).unwrap_or(1440.0))
}

/// APP-42: a mockup's caption, in the app's DOM and type — the title a line,
/// the description markdown rendered as a comment's is (safe for
/// `inner_html`; its links the app's, `.md`).
fn caption_view(c: Caption) -> impl IntoView {
    let html = c.description.as_deref().map(|d| crate::md::render_body(d).html);
    view! {
        <div class="framed-caption">
            {c.title.map(|t| view! { <p class="framed-caption-title">{t}</p> })}
            {html.map(|h| view! { <div class="framed-caption-text md" inner_html=h></div> })}
        </div>
    }
}

/// The viewer's own `prefers-reduced-motion` — a mockup's toggle starts there.
pub(crate) fn prefers_reduced_motion() -> bool {
    dom::window()
        .match_media("(prefers-reduced-motion: reduce)")
        .ok()
        .flatten()
        .is_some_and(|m| m.matches())
}

/// Keep `size` the stage's inner size, as it changes. MENGER-20: the stage
/// leaving the page (its ticket closed) is one more change, seen after
/// `size` has been disposed with its box — then the observer lets go of the
/// stage instead of reading the dead signal, which panicked on every ticket
/// left.
pub(crate) fn observe(el: web_sys::HtmlDivElement, size: RwSignal<(f64, f64)>) {
    // false once the box is gone
    let measure = {
        let el = el.clone();
        move || {
            let Some(was) = size.try_get_untracked() else { return false };
            let w = f64::from(el.client_width());
            let h = f64::from(el.client_height());
            if was != (w, h) {
                size.set((w, h));
            }
            true
        }
    };
    measure();
    let Ok(ctor) = Reflect::get(&dom::window(), &JsValue::from_str("ResizeObserver")) else { return };
    let Ok(ctor) = ctor.dyn_into::<Function>() else { return };
    // A ResizeObserver's callback is given the observer itself.
    let cb = Closure::<dyn FnMut(JsValue, JsValue)>::new(move |_entries: JsValue, ro: JsValue| {
        if !measure() {
            if let Ok(disconnect) = Reflect::get(&ro, &JsValue::from_str("disconnect")).and_then(|f| f.dyn_into::<Function>()) {
                let _ = disconnect.call0(&ro);
            }
        }
    });
    let Ok(ro) = Reflect::construct(&ctor, &js_sys::Array::of1(cb.as_ref())) else { return };
    cb.forget();
    if let Ok(observe) = Reflect::get(&ro, &JsValue::from_str("observe")).and_then(|f| f.dyn_into::<Function>()) {
        let _ = observe.call1(&ro, &el);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frame::Insets;

    /// APP-66: a phone-sized device is drawn as the iPhone of its width —
    /// corners, cutout and insets — portrait or on its side; a tablet, a
    /// desktop and a home-button phone have none.
    #[test]
    fn a_phone_has_its_iphones_screen() {
        let s = screen(Device { w: 390, h: 844 }).unwrap();
        assert_eq!((s.radius, s.cutout, s.landscape), (47, Cutout::Notch, false));
        assert_eq!(s.insets, Insets { top: 47, right: 0, bottom: 34, left: 0 });
        let s = screen(Device { w: 393, h: 852 }).unwrap();
        assert_eq!((s.radius, s.cutout), (55, Cutout::Island));
        assert_eq!(s.insets, Insets { top: 59, right: 0, bottom: 34, left: 0 });
        assert_eq!(screen(Device { w: 430, h: 932 }).map(|s| (s.radius, s.cutout)), Some((55, Cutout::Island)));
        assert_eq!(screen(Device { w: 428, h: 926 }).map(|s| s.cutout), Some(Cutout::Notch));
        assert_eq!(screen(Device { w: 375, h: 812 }).map(|s| s.insets.top), Some(50));
        // On its side: the cutout's inset on both sides, 21 at the bottom.
        let s = screen(Device { w: 844, h: 390 }).unwrap();
        assert!(s.landscape);
        assert_eq!(s.insets, Insets { top: 0, right: 47, bottom: 21, left: 47 });
        // No shape: a home-button phone, a tablet, a desktop, a square.
        for d in [(375, 667), (768, 1024), (1440, 900), (390, 390)] {
            assert_eq!(screen(Device { w: d.0, h: d.1 }), None, "{d:?}");
        }
        // The default phone (390) and every phone preset with the ratio have one.
        assert!(screen(Device::of_width(DEFAULT_WIDTHS[0])).is_some());
    }

    /// APP-99: a collapsed mockup says its views; its collapse is kept by
    /// its name; a swipe down (not across) closes the full screen.
    #[test]
    fn app_99_collapse_is_kept_by_name_and_a_swipe_down_closes() {
        assert_eq!(views_note(1), "1 view");
        assert_eq!(views_note(3), "3 views");
        assert_eq!(fold_key("APP-83#m2"), "menger.mockup.folded.APP-83#m2");
        assert!(swiped_down(10.0, 80.0));
        assert!(!swiped_down(0.0, 40.0), "too short");
        assert!(!swiped_down(70.0, 80.0), "more across than down");
        assert!(!swiped_down(0.0, -90.0), "up");
    }

    #[test]
    fn a_body_declares_its_view_in_its_frontmatter() {
        let d = declared(Some("title: x\nview: mockup\nwidths: 390, 768x1000, 1440\n"), "<p>x</p>");
        assert_eq!(d.mode, Mode::Mockup);
        assert_eq!(d.devices, vec![Device { w: 390, h: 844 }, Device { w: 768, h: 1000 }, Device { w: 1440, h: 900 }]);
        assert_eq!(declared(Some("view: full"), "x").mode, Mode::Full);
        assert_eq!(declared(Some("view: \"Mockup\"  # the landing"), "x").mode, Mode::Mockup);
        assert_eq!(declared(Some("widths: [320, 1920]"), "x").devices, vec![Device::of_width(320), Device::of_width(1920)]);
        // Nothing, or nothing readable: inline, the three defaults.
        for fm in [None, Some("view: sideways"), Some("  view: mockup"), Some("widths: wide")] {
            let d = declared(fm, "x");
            assert_eq!(d, Decl::default(), "{fm:?}");
        }
        assert_eq!(Decl::default().devices.iter().map(|d| d.w).collect::<Vec<_>>(), DEFAULT_WIDTHS);
    }

    /// APP-42: a `view: mockup` body says what it shows in its
    /// frontmatter's `description:` (a whole document, in its head too).
    #[test]
    fn a_body_describes_its_mockup_in_its_frontmatter() {
        let d = declared(Some("view: mockup\ndescription: The cart, one item.  # the first view\n"), "<p>x</p>");
        assert_eq!(d.caption, Caption { title: None, description: Some("The cart, one item.".into()) });
        let d = declared(Some("view: mockup\ndescription: \"Step #2: pay\"\n"), "x");
        assert_eq!(d.caption.description.as_deref(), Some("Step #2: pay"));
        assert!(declared(Some("view: mockup\ndescription:   \n"), "x").caption.is_empty());
        let page = "<!doctype html><html><head><meta name=\"menger-description\" content=\"The landing, dark.\"></head><body></body></html>";
        assert_eq!(declared(None, page).caption.description.as_deref(), Some("The landing, dark."));
        assert_eq!(declared(Some("description: mine"), page).caption.description.as_deref(), Some("mine"));
    }

    /// APP-42: a caption is trimmed, one-line titled, clipped; an element's
    /// own wins over its page's.
    #[test]
    fn a_caption_is_trimmed_and_the_elements_own_wins() {
        let c = Caption::of(Some("  \n Checkout \n more"), Some("  "));
        assert_eq!(c, Caption { title: Some("Checkout".into()), description: None });
        assert!(Caption::of(Some(" "), None).is_empty());
        let long = "x".repeat(TITLE_MAX + 5);
        assert_eq!(Caption::of(Some(&long), None).title.unwrap().chars().count(), TITLE_MAX + 1);
        let page = Caption::of(None, Some("the page's"));
        assert_eq!(Caption::default().or(Some(&page)), page);
        assert_eq!(c.clone().or(Some(&page)), c);
        assert_eq!(Caption::default().or(None), Caption::default());
    }

    /// APP-42: the note shows for a page taller than three of its screens at
    /// the narrowest device shown — not for one that fits, not before that
    /// device's frame has said how tall its page is.
    #[test]
    fn a_page_taller_than_three_screens_at_its_narrowest_is_noted() {
        assert_eq!(screens(844 * 3, 844), None);
        assert_eq!(screens(844 * 3 + 1, 844), Some(4));
        assert_eq!(screens(9000, 900), Some(10));
        assert_eq!(screens(9000, 0), None);
        let phone = Device { w: 390, h: 844 };
        let desk = Device { w: 1440, h: 900 };
        let shown = [(0, desk), (1, phone)];
        // The phone is narrowest: its page decides, the desktop's does not.
        assert_eq!(tall(&shown, &[(0, Extent { doc: 900, view: 900 }), (1, Extent { doc: 5000, view: 844 })]), Some(6));
        assert_eq!(tall(&shown, &[(0, Extent { doc: 9000, view: 900 }), (1, Extent { doc: 1000, view: 844 })]), None);
        assert_eq!(tall(&shown, &[(0, Extent { doc: 9000, view: 900 })]), None);
        assert_eq!(tall(&[], &[]), None);
        // Resized (APP-33): the new extent is what counts.
        assert_eq!(tall(&[(1, Device { w: 390, h: 2000 })], &[(1, Extent { doc: 5000, view: 2000 })]), None);
        assert_eq!(tall_note(6), "one view per mockup — this page scrolls 6 screens");
    }

    #[test]
    fn a_whole_document_may_declare_it_in_its_head() {
        let page = "<!doctype html><html><head><meta name=\"menger-view\" content=\"mockup\"><meta name='menger-widths' content='375,1280x800'></head><body></body></html>";
        let d = declared(None, page);
        assert_eq!((d.mode, d.devices), (Mode::Mockup, vec![Device::of_width(375), Device { w: 1280, h: 800 }]));
        // The frontmatter wins; a fragment's meta is no declaration.
        assert_eq!(declared(Some("view: full"), page).mode, Mode::Full);
        assert_eq!(declared(None, "<div><meta name=\"menger-view\" content=\"mockup\"></div>").mode, Mode::Inline);
    }

    #[test]
    fn widths_outside_reason_are_dropped() {
        assert_eq!(parse_widths("100, 390, 99999, 768x10, 1024x768px"), Some(vec![Device::of_width(390), Device { w: 1024, h: 768 }]));
        assert_eq!(parse_widths("1,2,3"), None);
        assert_eq!(parse_widths("400 500 600 700 800 900 1000 1100").map(|d| d.len()), Some(MAX_DEVICES));
        assert_eq!(Device::of_width(390).kind(), "phone");
        assert_eq!(Device::of_width(768).kind(), "tablet");
        assert_eq!(Device::of_width(1440).kind(), "desktop");
    }

    #[test]
    fn devices_side_by_side_share_one_scale_that_fits() {
        let d: Vec<Device> = DEFAULT_WIDTHS.iter().map(|w| Device::of_width(*w)).collect();
        // 2598 px of devices and two 24 px gaps into 1000 px.
        let s = fit(&d, 24.0, 1000.0, None);
        assert!((s - (952.0 / 2598.0)).abs() < 1e-9, "{s}");
        // Never larger than life; the height counts in full screen.
        assert_eq!(fit(&d[..1], 24.0, 4000.0, None), 1.0);
        assert!((fit(&d[2..], 24.0, 4000.0, Some(450.0)) - 0.5).abs() < 1e-9);
        assert_eq!(fit(&[], 24.0, 100.0, None), 1.0);
    }

    #[test]
    fn the_route_says_full_screen_in_its_query() {
        assert!(wants_full("#/ticket/A-1?view=full"));
        assert!(wants_full("#/ticket/A-1?component=m-x&view=full"));
        assert!(!wants_full("#/ticket/A-1") && !wants_full("#/ticket/A-1?view=fuller"));
        assert_eq!(with_full("#/ticket/A-1", true), "#/ticket/A-1?view=full");
        assert_eq!(with_full("#/ticket/A-1?view=full", false), "#/ticket/A-1");
        assert_eq!(with_full("#/ticket/A-1?field=x&view=full&g=1", false), "#/ticket/A-1?field=x&g=1");
        assert_eq!(with_full("#/ticket/A-1?field=x", true), "#/ticket/A-1?field=x&view=full");
        assert_eq!(with_full("#/ticket/A-1?view=full", true), "#/ticket/A-1?view=full");
    }

    /// APP-29: a fence's or element's widths win; else the referenced
    /// page's declaration; else the defaults — and widths past reason are
    /// dropped as `widths:`'s are.
    #[test]
    fn a_mockup_component_takes_its_widths_from_itself_then_its_page() {
        let page = "<!doctype html><html><head><meta name=\"menger-view\" content=\"mockup\"><meta name=\"menger-widths\" content=\"375,1280x800\"></head><body></body></html>";
        let decl = declared(None, page);
        assert_eq!(mock_devices(Some("390,1440"), Some(&decl)), vec![Device::of_width(390), Device::of_width(1440)]);
        assert_eq!(mock_devices(Some("390, 768x1000"), None), vec![Device::of_width(390), Device { w: 768, h: 1000 }]);
        assert_eq!(mock_devices(None, Some(&decl)), vec![Device::of_width(375), Device { w: 1280, h: 800 }]);
        // Nothing readable named: the page's, then the defaults.
        assert_eq!(mock_devices(Some("wide"), Some(&decl)), decl.devices);
        assert_eq!(mock_devices(Some("10"), None), Decl::default().devices);
        assert_eq!(mock_devices(None, None).iter().map(|d| d.w).collect::<Vec<_>>(), DEFAULT_WIDTHS);
        // A referenced page that declares nothing: its defaults.
        assert_eq!(mock_devices(None, Some(&declared(None, "<!doctype html><html><body></body></html>"))), Decl::default().devices);
    }
}
