//! APP-33: **a mockup at any size.** Owner: *"it should allow the user to
//! arbitrarily change the view to smaller and bigger sizes. with standard
//! options"*.
//!
//! Every mockup the host draws — the `<m-mockup>` element, a ```` ```mockup ````
//! fence, `ref=`, a `view: mockup` body — goes through one box
//! ([`crate::present::Framed`]), and that box's devices are sized here:
//!
//! - it opens at its **declared** widths, as before;
//! - the bar's **Size** menu holds the standard devices ([`PRESETS`]):
//!   choosing one draws the mockup on that single device, Shift adds it
//!   beside the ones shown; **Custom…** (or a device's `W × H` readout
//!   clicked) takes `W` or `W×H` ([`parse_size`]: 320–3840 by 240–2160),
//!   applied on Enter, refused in place;
//! - a device's **right edge or corner** drags its viewport live, the
//!   readout saying `W × H`, the width snapping to a standard one within
//!   [`SNAP_WITHIN`] px ([`snap`]; Alt drags free); the edge is a focusable,
//!   labelled handle whose arrow keys move ±10 px, ±1 with Shift
//!   ([`nudge`]); at the app's phone width the handles give way to the
//!   field;
//! - **Rotate** swaps a device's width and height; **Reset** goes back to
//!   the declared widths ([`resized`]).
//!
//! A device is a slot that keeps its frame: a resize only restyles the box
//! the frame sits in, so the page reflows as in a resized window and never
//! reloads — its scroll, its state and the emulated scheme and motion stay.
//! Choosing a single device keeps the frame of the one shown alone (else
//! the first); only a device added is a new frame.
//!
//! The sizes are the **viewer's**, never the ticket's: nothing is written to
//! the body. They are kept per mockup in this browser's localStorage
//! ([`storage_key`], [`encode`]) — tolerant of a storage that throws or is
//! empty — and Reset clears them.
//!
//! What decides is pure and pinned by native tests; the rest is DOM.

use leptos::prelude::*;
use wasm_bindgen::JsCast;

use crate::bindings::{self, Context};
use crate::dom;
use crate::frame::{Emulate, Frame};
use crate::present::{fit, parse_widths, Device, MAX_DEVICES};

/// A standard device in the Size menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Preset {
    pub(crate) name: &'static str,
    pub(crate) device: Device,
}

const fn preset(name: &'static str, w: u32, h: u32) -> Preset {
    Preset { name, device: Device { w, h } }
}

/// The Size menu, in its order.
pub(crate) const PRESETS: [Preset; 8] = [
    preset("Phone", 375, 667),
    preset("Phone", 390, 844),
    preset("Phone large", 430, 932),
    preset("Tablet", 768, 1024),
    preset("Tablet landscape", 1024, 768),
    preset("Laptop", 1280, 800),
    preset("Desktop", 1440, 900),
    preset("Wide", 1920, 1080),
];

/// A size a viewer chooses: a width 320–3840, a height 240–2160.
pub(crate) const MIN_W: u32 = 320;
pub(crate) const MAX_W: u32 = 3840;
pub(crate) const MIN_H: u32 = 240;
pub(crate) const MAX_H: u32 = 2160;

/// A dragged width this close (viewport px) to a standard one lands on it.
pub(crate) const SNAP_WITHIN: f64 = 8.0;

/// What the Custom field takes: `W`, or `W×H` (`x`, `×` or `*`, spaces and
/// a `px` allowed) — or why not, said in place.
pub(crate) fn parse_size(s: &str) -> Result<(u32, Option<u32>), String> {
    let t = s.trim().to_ascii_lowercase().replace("px", "");
    if t.is_empty() {
        return Err("a width, or width × height".into());
    }
    let num = |p: &str| p.trim().parse::<u32>().ok();
    let (w, h) = match t.split_once(['x', '×', '*']) {
        Some((w, h)) => (num(w), Some(num(h))),
        None => (num(&t), None),
    };
    let bad = || format!("“{}” is no size: W or W×H", s.trim().chars().take(24).collect::<String>());
    let w = w.ok_or_else(bad)?;
    let h = match h {
        Some(h) => Some(h.ok_or_else(bad)?),
        None => None,
    };
    if !(MIN_W..=MAX_W).contains(&w) {
        return Err(format!("width {w}: {MIN_W} to {MAX_W}"));
    }
    if let Some(h) = h.filter(|h| !(MIN_H..=MAX_H).contains(h)) {
        return Err(format!("height {h}: {MIN_H} to {MAX_H}"));
    }
    Ok((w, h))
}

fn clamp_w(w: f64) -> u32 {
    (w.round().max(f64::from(MIN_W)).min(f64::from(MAX_W))) as u32
}

fn clamp_h(h: f64) -> u32 {
    (h.round().max(f64::from(MIN_H)).min(f64::from(MAX_H))) as u32
}

/// A dragged width: the nearest standard width within [`SNAP_WITHIN`] px,
/// else itself — kept to 320–3840.
pub(crate) fn snap(w: f64) -> u32 {
    let near = PRESETS
        .iter()
        .map(|p| p.device.w)
        .map(|s| (s, (f64::from(s) - w).abs()))
        .filter(|(_, d)| *d <= SNAP_WITHIN)
        .min_by(|a, b| a.1.total_cmp(&b.1));
    match near {
        Some((s, _)) => s,
        None => clamp_w(w),
    }
}

/// A drag of `dx`, `dy` viewport px from `from`: the width (snapped unless
/// `free`), and the height too when the corner is dragged.
pub(crate) fn dragged(from: Device, dx: f64, dy: f64, corner: bool, free: bool) -> Device {
    let w = f64::from(from.w) + dx;
    let w = if free { clamp_w(w) } else { snap(w) };
    let h = if corner { clamp_h(f64::from(from.h) + dy) } else { from.h };
    Device { w, h }
}

/// A key on a device's handle, by its binding (src/bindings.rs, Context
/// Mockup): `mockup.wider` / `narrower` its width, `taller` / `shorter` its
/// height, 10 px — 1 px for the `-1` ones. Any other: `None`.
pub(crate) fn nudge(d: Device, binding: &str) -> Option<Device> {
    // (Δw, Δh) per binding.
    let (dw, dh) = match binding {
        "mockup.narrower" => (-10.0, 0.0),
        "mockup.wider" => (10.0, 0.0),
        "mockup.shorter" => (0.0, -10.0),
        "mockup.taller" => (0.0, 10.0),
        "mockup.narrower-1" => (-1.0, 0.0),
        "mockup.wider-1" => (1.0, 0.0),
        "mockup.shorter-1" => (0.0, -1.0),
        "mockup.taller-1" => (0.0, 1.0),
        _ => return None,
    };
    let w = if dw == 0.0 { d.w } else { clamp_w(f64::from(d.w) + dw) };
    let h = if dh == 0.0 { d.h } else { clamp_h(f64::from(d.h) + dh) };
    Some(Device { w, h })
}

/// What a viewer does to a mockup's devices.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Resize {
    /// Draw it on this device alone.
    Only(Device),
    /// Add this device beside the ones shown.
    Add(Device),
    /// Device `i` takes this size.
    Set(usize, Device),
    /// Device `i`'s width and height swap.
    Rotate(usize),
    /// Back to the declared devices.
    Reset,
}

/// The devices after `op` — each with the index, in `current`, of the
/// device whose frame it keeps (`None`: a new frame). `focus` is the device
/// shown alone, if one is: [`Resize::Only`] keeps its frame (else the
/// first's). An add past [`MAX_DEVICES`], or of a size already shown,
/// changes nothing.
pub(crate) fn resized(current: &[Device], declared: &[Device], focus: Option<usize>, op: Resize) -> Vec<(Option<usize>, Device)> {
    let kept = || current.iter().copied().enumerate().map(|(i, d)| (Some(i), d)).collect::<Vec<_>>();
    match op {
        Resize::Only(d) => {
            let keep = focus.filter(|i| *i < current.len()).or((!current.is_empty()).then_some(0));
            vec![(keep, d)]
        }
        Resize::Add(d) => {
            let mut v = kept();
            if v.len() < MAX_DEVICES && !current.contains(&d) {
                v.push((None, d));
            }
            v
        }
        Resize::Set(i, d) => {
            let mut v = kept();
            if let Some(slot) = v.get_mut(i) {
                slot.1 = d;
            }
            v
        }
        Resize::Rotate(i) => {
            let mut v = kept();
            if let Some(slot) = v.get_mut(i) {
                slot.1 = Device { w: slot.1.h, h: slot.1.w };
            }
            v
        }
        Resize::Reset => declared.iter().enumerate().map(|(i, d)| ((i < current.len()).then_some(i), *d)).collect(),
    }
}

/// The localStorage key a mockup's sizes are kept under.
pub(crate) fn storage_key(mockup: &str) -> String {
    format!("menger.mockup-sizes:{mockup}")
}

/// Sizes as kept: `390x844,1440x900`.
pub(crate) fn encode(devices: &[Device]) -> String {
    devices.iter().map(|d| format!("{}x{}", d.w, d.h)).collect::<Vec<_>>().join(",")
}

/// Kept sizes read back — anything unreadable is none (the declared stand).
pub(crate) fn decode(kept: Option<&str>) -> Option<Vec<Device>> {
    kept.and_then(parse_widths)
}

/// `W × H`, as every readout says it.
pub(crate) fn says(d: Device) -> String {
    format!("{} × {}", d.w, d.h)
}

// -- the DOM half -------------------------------------------------------------

fn storage() -> Option<web_sys::Storage> {
    dom::window().local_storage().ok().flatten()
}

/// One device on the stage: its frame stays while its size moves.
#[derive(Clone)]
struct Slot {
    key: u32,
    dev: ArcRwSignal<Device>,
}

/// Where the Custom field's size goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Target {
    /// The menu's Custom…: like a standard device (Shift+Enter adds).
    Menu,
    /// A device's readout: that device.
    Slot(u32),
}

/// A drag under way: where it began, from what size, at what scale.
#[derive(Clone, Copy)]
struct Grab {
    key: u32,
    x: f64,
    y: f64,
    from: Device,
    scale: f64,
    corner: bool,
}

/// A mockup's sizes, while its box is mounted.
#[derive(Clone, Copy)]
pub(crate) struct Sizer {
    slots: RwSignal<Vec<Slot>>,
    next: StoredValue<u32>,
    /// The device shown alone (its slot's key), if one is.
    solo: RwSignal<Option<u32>>,
    /// Whether the viewer chose these sizes (else they are the declared).
    chosen: RwSignal<bool>,
    declared: Signal<Vec<Device>>,
    remember: StoredValue<Option<String>>,
    menu: RwSignal<bool>,
    field: RwSignal<Option<Target>>,
    refused: RwSignal<Option<String>>,
    grab: RwSignal<Option<Grab>>,
}

impl Sizer {
    /// The declared devices — or, when this browser kept the viewer's for
    /// `remember` (the mockup's own name), those.
    pub(crate) fn new(declared: Signal<Vec<Device>>, remember: Option<String>) -> Self {
        let kept = remember
            .as_deref()
            .and_then(|r| decode(storage().and_then(|s| s.get_item(&storage_key(r)).ok().flatten()).as_deref()));
        let chosen = kept.is_some();
        let start = kept.unwrap_or_else(|| declared.get_untracked());
        let slots: Vec<Slot> = start.into_iter().enumerate().map(|(i, d)| Slot { key: i as u32, dev: ArcRwSignal::new(d) }).collect();
        let n = slots.len() as u32;
        let s = Sizer {
            slots: RwSignal::new(slots),
            next: StoredValue::new(n),
            solo: RwSignal::new(None),
            chosen: RwSignal::new(chosen),
            declared,
            remember: StoredValue::new(remember),
            menu: RwSignal::new(false),
            field: RwSignal::new(None),
            refused: RwSignal::new(None),
            grab: RwSignal::new(None),
        };
        // The body re-declares its widths: a mockup at its declared ones
        // follows; one the viewer sized keeps the viewer's.
        Effect::new(move |was: Option<Vec<Device>>| {
            let now = declared.get();
            if was.is_some_and(|w| w != now) && !s.chosen.get_untracked() {
                s.apply(Resize::Reset);
            }
            now
        });
        s
    }

    /// APP-42: the devices shown — every slot, or the one shown alone —
    /// each its slot's key and its size, tracked.
    pub(crate) fn shown(&self) -> Vec<(u32, Device)> {
        let solo = self.solo.get();
        self.slots.with(|v| v.iter().filter(|x| solo.is_none_or(|k| k == x.key)).map(|x| (x.key, x.dev.get())).collect())
    }

    fn devices_untracked(&self) -> Vec<Device> {
        self.slots.with_untracked(|v| v.iter().map(|s| s.dev.get_untracked()).collect())
    }

    fn index_of(&self, key: u32) -> Option<usize> {
        self.slots.with_untracked(|v| v.iter().position(|s| s.key == key))
    }

    /// Do `op`: slots that keep their frame keep it, new ones are made.
    fn apply(&self, op: Resize) {
        let cur = self.slots.get_untracked();
        let devs = self.devices_untracked();
        let focus = self.solo.get_untracked().and_then(|k| self.index_of(k));
        let declared = self.declared.get_untracked();
        let plan = resized(&devs, &declared, focus, op);
        let next: Vec<Slot> = plan
            .into_iter()
            .map(|(keep, d)| match keep.and_then(|i| cur.get(i)) {
                Some(slot) => {
                    if slot.dev.get_untracked() != d {
                        slot.dev.set(d);
                    }
                    slot.clone()
                }
                None => {
                    let key = self.next.get_value();
                    self.next.set_value(key.wrapping_add(1));
                    Slot { key, dev: ArcRwSignal::new(d) }
                }
            })
            .collect();
        let keys = |v: &[Slot]| v.iter().map(|s| s.key).collect::<Vec<_>>();
        if keys(&next) != keys(&cur) {
            self.slots.set(next);
        }
        if matches!(op, Resize::Only(_) | Resize::Add(_) | Resize::Reset)
            || self.solo.get_untracked().is_some_and(|k| self.index_of(k).is_none())
        {
            self.solo.set(None);
        }
        let reset = op == Resize::Reset || self.devices_untracked() == declared;
        self.chosen.set(!reset);
        self.keep();
    }

    /// The viewer's sizes into this browser's storage — or out of it, at the
    /// declared ones. A storage that refuses is let be.
    fn keep(&self) {
        let Some(name) = self.remember.get_value() else { return };
        let Some(s) = storage() else { return };
        let k = storage_key(&name);
        if self.chosen.get_untracked() {
            let _ = s.set_item(&k, &encode(&self.devices_untracked()));
        } else {
            let _ = s.remove_item(&k);
        }
    }

    fn pick(&self, d: Device, add: bool) {
        self.menu.set(false);
        self.apply(if add { Resize::Add(d) } else { Resize::Only(d) });
    }

    /// The Custom field's Enter.
    fn enter(&self, text: &str, add: bool) {
        match parse_size(text) {
            Err(why) => self.refused.set(Some(why)),
            Ok((w, h)) => {
                match self.field.get_untracked() {
                    Some(Target::Slot(k)) => {
                        if let Some(i) = self.index_of(k) {
                            let was = self.devices_untracked()[i];
                            self.apply(Resize::Set(i, Device { w, h: h.unwrap_or(was.h) }));
                        }
                    }
                    _ => {
                        let d = h.map_or_else(|| Device::of_width(w), |h| Device { w, h });
                        self.apply(if add { Resize::Add(d) } else { Resize::Only(d) });
                    }
                }
                self.close_field();
            }
        }
    }

    fn open_field(&self, t: Target) {
        self.menu.set(false);
        self.refused.set(None);
        self.field.set(Some(t));
    }

    fn close_field(&self) {
        self.refused.set(None);
        self.field.set(None);
    }

    /// The bar's controls: All and each device, the Size menu, Reset, and
    /// the Custom field when it is open.
    pub(crate) fn controls(self) -> impl IntoView {
        let s = self;
        let devices = move || {
            s.slots
                .get()
                .into_iter()
                .map(|slot| {
                    let key = slot.key;
                    let dev = slot.dev.clone();
                    let tip = move || {
                        let d = dev.get();
                        format!("{} ({})", says(d), d.kind())
                    };
                    let dev = slot.dev.clone();
                    view! {
                        <button class="framed-btn" title=tip aria-pressed=move || (s.solo.get() == Some(key)).to_string() on:click=move |_| s.solo.set(Some(key))>
                            {move || dev.get().w.to_string()}
                        </button>
                    }
                })
                .collect_view()
        };
        let menu_ref = NodeRef::<leptos::html::Div>::new();
        let toggle = move |_| {
            let open = !s.menu.get_untracked();
            s.menu.set(open);
            if open {
                // The first item takes focus, so the arrows and Esc work.
                if let Some(first) = menu_ref.get_untracked().and_then(|m| m.query_selector("button").ok().flatten()) {
                    if let Ok(b) = first.dyn_into::<web_sys::HtmlElement>() {
                        let _ = b.focus();
                    }
                }
            }
        };
        let items = PRESETS
            .iter()
            .map(|p| {
                let d = p.device;
                let on = move || s.slots.with(|v| v.iter().any(|x| x.dev.get() == d));
                view! {
                    <button class="framed-menu-item" role="menuitem" class:is-on=on title="Shift adds it beside the ones shown"
                        on:click=move |ev: web_sys::MouseEvent| s.pick(d, bindings::hit_mouse(&ev, Context::Menu) == Some("mockup.add-size"))>
                        <span>{p.name}</span>
                        <span class="code">{says(d)}</span>
                    </button>
                }
            })
            .collect_view();
        let menu_keys = move |ev: web_sys::KeyboardEvent| {
            let Some(m) = menu_ref.get_untracked() else { return };
            let Ok(list) = m.query_selector_all("button") else { return };
            let n = list.length();
            let active = dom::window().document().and_then(|d| d.active_element());
            let at = (0..n).find(|i| list.get(*i).map(|x| Some(x.unchecked_into::<web_sys::Element>()) == active) == Some(true));
            let to = match bindings::hit(&ev, Context::Menu) {
                Some("dialog.close") => {
                    ev.stop_propagation();
                    ev.prevent_default();
                    s.menu.set(false);
                    return;
                }
                // Shift+Enter on a standard device adds it (on Custom…, opens the field).
                Some("mockup.add-size") => {
                    ev.prevent_default();
                    match at.and_then(|i| PRESETS.get(i as usize)) {
                        Some(p) => s.pick(p.device, true),
                        None => s.open_field(Target::Menu),
                    }
                    return;
                }
                Some("list.next") => at.map_or(0, |i| (i + 1) % n),
                Some("list.prev") => at.map_or(n.saturating_sub(1), |i| (i + n - 1) % n),
                Some("list.first") => 0,
                Some("list.last") => n.saturating_sub(1),
                _ => return,
            };
            ev.prevent_default();
            if let Some(b) = list.get(to).and_then(|x| x.dyn_into::<web_sys::HtmlElement>().ok()) {
                let _ = b.focus();
            }
        };
        // Focus leaving the menu closes it.
        let wrap_ref = NodeRef::<leptos::html::Span>::new();
        let left = move |ev: web_sys::FocusEvent| {
            let into = ev.related_target().and_then(|t| t.dyn_into::<web_sys::Node>().ok());
            let inside = wrap_ref.get_untracked().is_some_and(|w| into.is_some_and(|n| w.contains(Some(&n))));
            if !inside {
                s.menu.set(false);
            }
        };
        view! {
            <span class="framed-group" role="group" aria-label="devices">
                <button class="framed-btn" title="every device, side by side" aria-pressed=move || s.solo.get().is_none().to_string() on:click=move |_| s.solo.set(None)>"All"</button>
                {devices}
            </span>
            <span class="framed-sizes" node_ref=wrap_ref on:focusout=left>
                <button class="framed-btn" aria-haspopup="menu" aria-expanded=move || s.menu.get().to_string() title="Standard devices, or a size of your own" on:click=toggle>
                    "Size ▾"
                </button>
                <div class="framed-menu" role="menu" aria-label="sizes" hidden=move || !s.menu.get() node_ref=menu_ref on:keydown=menu_keys>
                    {items}
                    <button class="framed-menu-item" role="menuitem" on:click=move |_| s.open_field(Target::Menu)>
                        <span>"Custom…"</span>
                        <span class="code">"W × H"</span>
                    </button>
                </div>
            </span>
            <button class="framed-btn" title="Back to the declared widths" disabled=move || !s.chosen.get() on:click=move |_| s.apply(Resize::Reset)>
                "Reset"
            </button>
            {move || s.field.get().map(|t| s.field_view(t))}
        }
    }

    /// The Custom field: `W` or `W×H`; Enter applies, Esc leaves, a size
    /// past reason is refused in place.
    fn field_view(self, t: Target) -> impl IntoView {
        let s = self;
        let input = NodeRef::<leptos::html::Input>::new();
        let (label, start) = match t {
            Target::Menu => ("Custom size: W or W×H — Enter draws it alone, Shift+Enter adds it".to_string(), String::new()),
            Target::Slot(k) => {
                let d = s.index_of(k).map(|i| s.devices_untracked()[i]);
                ("This device's size: W or W×H, then Enter".to_string(), d.map(|d| format!("{}x{}", d.w, d.h)).unwrap_or_default())
            }
        };
        input.on_load(move |el: web_sys::HtmlInputElement| {
            let _ = el.focus();
            el.select();
        });
        let keys = move |ev: web_sys::KeyboardEvent| {
            ev.stop_propagation();
            match bindings::hit(&ev, Context::Field) {
                Some(id @ ("form.submit" | "mockup.add-size")) => {
                    ev.prevent_default();
                    let text = input.get_untracked().map(|i| i.value()).unwrap_or_default();
                    s.enter(&text, id == "mockup.add-size");
                }
                Some("form.cancel") => {
                    ev.prevent_default();
                    s.close_field();
                }
                _ => {
                    if s.refused.get_untracked().is_some() {
                        s.refused.set(None);
                    }
                }
            }
        };
        view! {
            <span class="framed-field">
                <input class="framed-input code" type="text" node_ref=input value=start placeholder="W or W×H" aria-label=label.clone() title=label
                    aria-invalid=move || s.refused.get().is_some().to_string() on:keydown=keys />
                <span class="framed-refused" role="alert" hidden=move || s.refused.get().is_none()>{move || s.refused.get()}</span>
                <button class="framed-btn" title="Close (Esc)" aria-label="Close the size field" on:click=move |_| s.close_field()>"×"</button>
            </span>
        }
    }

    /// The stage: each device a frame of `html` at its viewport, scaled to
    /// fit `size` (the stage's; its height too when `is_full`).
    pub(crate) fn devices(
        self,
        html: Signal<String>,
        ticket: Option<String>,
        scope: String,
        emulate: Signal<Emulate>,
        size: RwSignal<(f64, f64)>,
        is_full: Memo<bool>,
    ) -> impl IntoView {
        let s = self;
        let scale = Memo::new(move |_| {
            // Held while a drag is under way, so the pointer stays on its edge.
            if let Some(g) = s.grab.get() {
                return g.scale;
            }
            let (w, h) = size.get();
            let solo = s.solo.get();
            let shown: Vec<Device> =
                s.slots.with(|v| v.iter().filter(|x| solo.is_none_or(|k| k == x.key)).map(|x| x.dev.get()).collect());
            let avail_h = is_full.get().then_some(h - CHROME_H);
            // The stage's padding, each device's bezel and its grip are not scaled.
            fit(&shown, GAP, w - 12.0 - GRIP_W - BEZEL_W * shown.len() as f64, avail_h)
        });
        view! {
            <For each=move || s.slots.get() key=|slot| slot.key children=move |slot: Slot| {
                s.device(slot, html, ticket.clone(), scope.clone(), emulate, scale)
            } />
        }
    }

    fn device(self, slot: Slot, html: Signal<String>, ticket: Option<String>, scope: String, emulate: Signal<Emulate>, scale: Memo<f64>) -> impl IntoView {
        let s = self;
        let key = slot.key;
        let dev = slot.dev;
        let d = {
            let dev = dev.clone();
            move || dev.get()
        };
        let shown = move || s.solo.get().is_none_or(|k| k == key);
        let screen = {
            let d = d.clone();
            move || {
                let (d, sc) = (d(), scale.get());
                format!("width:{:.1}px;height:{:.1}px", f64::from(d.w) * sc, f64::from(d.h) * sc)
            }
        };
        let inner = {
            let d = d.clone();
            move || {
                let d = d();
                format!("width:{}px;height:{}px;transform:scale({:.4})", d.w, d.h, scale.get())
            }
        };
        let dragging = move || s.grab.get().is_some_and(|g| g.key == key);
        // Pointer: the edge drags the width, the corner both.
        let down = {
            let dev = dev.clone();
            move |corner: bool, ev: web_sys::PointerEvent| {
                if !matches!(bindings::hit_mouse(&ev, Context::Mockup), Some("mockup.drag" | "mockup.drag-free")) {
                    return;
                }
                ev.prevent_default();
                if let Some(t) = ev.current_target().and_then(|t| t.dyn_into::<web_sys::Element>().ok()) {
                    let _ = t.set_pointer_capture(ev.pointer_id());
                }
                let g = Grab {
                    key,
                    x: f64::from(ev.client_x()),
                    y: f64::from(ev.client_y()),
                    from: dev.get_untracked(),
                    scale: scale.get_untracked().max(0.05),
                    corner,
                };
                s.grab.set(Some(g));
            }
        };
        let moved = {
            let dev = dev.clone();
            move |ev: web_sys::PointerEvent| {
                let Some(g) = s.grab.get_untracked().filter(|g| g.key == key) else { return };
                let dx = (f64::from(ev.client_x()) - g.x) / g.scale;
                let dy = (f64::from(ev.client_y()) - g.y) / g.scale;
                let free = bindings::hit_mouse(&ev, Context::Mockup) == Some("mockup.drag-free");
                let next = dragged(g.from, dx, dy, g.corner, free);
                if dev.get_untracked() != next {
                    dev.set(next);
                }
            }
        };
        let up = {
            let dev = dev.clone();
            move |_ev: web_sys::PointerEvent| {
                let Some(g) = s.grab.get_untracked().filter(|g| g.key == key) else { return };
                s.grab.set(None);
                let now = dev.get_untracked();
                if now != g.from {
                    if let Some(i) = s.index_of(key) {
                        s.apply(Resize::Set(i, now));
                    }
                }
            }
        };
        let arrows = {
            let dev = dev.clone();
            move |ev: web_sys::KeyboardEvent| {
                let Some(next) = bindings::hit(&ev, Context::Mockup).and_then(|id| nudge(dev.get_untracked(), id)) else { return };
                ev.prevent_default();
                ev.stop_propagation();
                if let Some(i) = s.index_of(key) {
                    s.apply(Resize::Set(i, next));
                }
            }
        };
        let (down_e, down_c) = (down.clone(), down);
        let (moved_e, moved_c) = (moved.clone(), moved);
        let (up_e, up_c, up_e2, up_c2) = (up.clone(), up.clone(), up.clone(), up);
        let kind = {
            let d = d.clone();
            move || d().kind()
        };
        let readout = {
            let d = d.clone();
            move || says(d())
        };
        let handle_label = {
            let d = d.clone();
            move || format!("Resize the {} device, {}: ← → its width, ↑ ↓ its height, Shift for 1 px", d().kind(), says(d()))
        };
        let now_w = {
            let d = d.clone();
            move || d().w.to_string()
        };
        let data_w = now_w.clone();
        let readout2 = readout.clone();
        let data_kind = kind.clone();
        view! {
            <figure class="device" data-slot=key.to_string() data-kind=data_kind data-w=data_w class:hidden=move || !shown() class:is-dragging=dragging>
                <figcaption class="device-label">
                    <span>{kind}</span>
                    <button class="device-size code" title="Type a size for this device" on:click=move |_| s.open_field(Target::Slot(key))>{readout}</button>
                    <button class="device-rotate" title="Rotate: swap its width and height" aria-label="Rotate this device"
                        on:click=move |_| if let Some(i) = s.index_of(key) { s.apply(Resize::Rotate(i)) }>"rotate"</button>
                    <span class="device-scale">{move || format!("{:.0}%", scale.get() * 100.0)}</span>
                </figcaption>
                <div class="device-body">
                    <div class="device-screen" style=screen>
                        <div class="device-inner" style=inner>
                            <Frame html ticket scope emulate fixed=true />
                        </div>
                    </div>
                    <span class="device-grip device-grip-e" tabindex="0" role="slider" aria-label=handle_label
                        aria-valuemin=MIN_W.to_string() aria-valuemax=MAX_W.to_string() aria-valuenow=now_w aria-valuetext=readout2.clone()
                        on:pointerdown=move |ev| down_e(false, ev) on:pointermove=moved_e on:pointerup=up_e on:pointercancel=up_e2
                        on:keydown=arrows></span>
                    <span class="device-grip device-grip-se" aria-hidden="true"
                        on:pointerdown=move |ev| down_c(true, ev) on:pointermove=moved_c on:pointerup=up_c on:pointercancel=up_c2></span>
                    <span class="device-readout code" aria-live="polite" hidden=move || !dragging()>{readout2}</span>
                </div>
            </figure>
        }
    }
}

/// The gap between devices side by side, px.
const GAP: f64 = 24.0;
/// A device's label and chrome, above and around its screen, px.
const CHROME_H: f64 = 84.0;
/// A device's bezel, left and right of its screen (at most), px.
const BEZEL_W: f64 = 20.0;
/// The last device's grip, past its right edge, px.
const GRIP_W: f64 = 14.0;

#[cfg(test)]
mod tests {
    use super::*;

    /// The menu's list, in order — the eight the ticket names.
    #[test]
    fn the_menu_holds_the_standard_devices() {
        let said: Vec<String> = PRESETS.iter().map(|p| format!("{} {}x{}", p.name, p.device.w, p.device.h)).collect();
        assert_eq!(
            said,
            [
                "Phone 375x667",
                "Phone 390x844",
                "Phone large 430x932",
                "Tablet 768x1024",
                "Tablet landscape 1024x768",
                "Laptop 1280x800",
                "Desktop 1440x900",
                "Wide 1920x1080",
            ]
        );
        // Every one is a size the field would take.
        for p in PRESETS {
            assert_eq!(parse_size(&encode(&[p.device])), Ok((p.device.w, Some(p.device.h))));
        }
    }

    /// Custom takes `W` or `W×H`, in any of the spellings, and refuses past
    /// 320–3840 by 240–2160, saying why.
    #[test]
    fn the_custom_field_parses_and_holds_its_limits() {
        assert_eq!(parse_size("1024"), Ok((1024, None)));
        assert_eq!(parse_size(" 1024×768 "), Ok((1024, Some(768))));
        assert_eq!(parse_size("1024 x 768"), Ok((1024, Some(768))));
        assert_eq!(parse_size("1024X768px"), Ok((1024, Some(768))));
        assert_eq!(parse_size("1024*768"), Ok((1024, Some(768))));
        assert_eq!(parse_size("320x240"), Ok((320, Some(240))));
        assert_eq!(parse_size("3840x2160"), Ok((3840, Some(2160))));
        for (bad, why) in [
            ("", "a width"),
            ("wide", "no size"),
            ("1024x", "no size"),
            ("x768", "no size"),
            ("-5", "no size"),
            ("1024x768x2", "no size"),
            ("319", "width 319: 320 to 3840"),
            ("3841x900", "width 3841"),
            ("1024x239", "height 239: 240 to 2160"),
            ("1024x2161", "height 2161"),
        ] {
            let err = parse_size(bad).unwrap_err();
            assert!(err.contains(why), "{bad:?}: {err}");
        }
    }

    /// A drag lands on a standard width within 8 px; Alt drags free; both
    /// keep to the limits, and only the corner moves the height.
    #[test]
    fn a_dragged_width_snaps_to_a_standard_one() {
        assert_eq!(snap(1020.0), 1024);
        assert_eq!(snap(1032.0), 1024);
        assert_eq!(snap(1033.0), 1033);
        assert_eq!(snap(383.0), 390); // 375 is 8 away, 390 is 7
        assert_eq!(snap(100.0), MIN_W);
        assert_eq!(snap(9000.0), MAX_W);
        let from = Device { w: 1000, h: 800 };
        assert_eq!(dragged(from, 22.0, 50.0, false, false), Device { w: 1024, h: 800 });
        assert_eq!(dragged(from, 22.0, 50.0, false, true), Device { w: 1022, h: 800 });
        assert_eq!(dragged(from, 22.0, 50.0, true, true), Device { w: 1022, h: 850 });
        assert_eq!(dragged(from, -5000.0, -5000.0, true, false), Device { w: MIN_W, h: MIN_H });
    }

    /// The handle's keys: ±10, ±1 for the `-1` bindings; width on
    /// wider/narrower, height on taller/shorter — and the table holds them
    /// as the ticket says: arrows, Shift for 1 px.
    #[test]
    fn arrow_keys_nudge_the_handle() {
        let d = Device { w: 390, h: 844 };
        assert_eq!(nudge(d, "mockup.wider"), Some(Device { w: 400, h: 844 }));
        assert_eq!(nudge(d, "mockup.narrower-1"), Some(Device { w: 389, h: 844 }));
        assert_eq!(nudge(d, "mockup.taller"), Some(Device { w: 390, h: 854 }));
        assert_eq!(nudge(d, "mockup.shorter-1"), Some(Device { w: 390, h: 843 }));
        assert_eq!(nudge(Device { w: 320, h: 240 }, "mockup.narrower"), Some(Device { w: 320, h: 240 }));
        assert_eq!(nudge(d, "list.next"), None);
        let chords = |id: &str| bindings::BINDINGS.iter().find(|b| b.id == id).map(|b| b.chords.to_vec()).unwrap();
        assert_eq!(chords("mockup.wider"), ["ArrowRight"]);
        assert_eq!(chords("mockup.wider-1"), ["Shift+ArrowRight"]);
        assert_eq!(chords("mockup.shorter"), ["ArrowUp"]);
        assert_eq!(chords("mockup.drag-free"), ["Alt+Click"]);
        assert_eq!(chords("mockup.add-size"), ["Shift+Click", "Shift+Enter"]);
    }

    /// Only keeps the frame shown alone (else the first); Add appends a new
    /// frame, never a seventh or a duplicate; Set and Rotate keep every
    /// frame; Reset is the declared, keeping frames by position.
    #[test]
    fn a_resize_keeps_its_frames_and_reset_returns_to_the_declared() {
        let declared = [Device { w: 390, h: 844 }, Device { w: 768, h: 1024 }, Device { w: 1440, h: 900 }];
        let tab = PRESETS[4].device;
        assert_eq!(resized(&declared, &declared, None, Resize::Only(tab)), vec![(Some(0), tab)]);
        assert_eq!(resized(&declared, &declared, Some(2), Resize::Only(tab)), vec![(Some(2), tab)]);
        assert_eq!(resized(&[], &declared, None, Resize::Only(tab)), vec![(None, tab)]);
        let added = resized(&declared, &declared, None, Resize::Add(tab));
        assert_eq!(added.len(), 4);
        assert_eq!(added[3], (None, tab));
        assert!(added[..3].iter().enumerate().all(|(i, (k, _))| *k == Some(i)));
        assert_eq!(resized(&declared, &declared, None, Resize::Add(declared[1])).len(), 3);
        let six = [declared, declared].concat().into_iter().enumerate().map(|(i, d)| Device { w: d.w + i as u32, h: d.h }).collect::<Vec<_>>();
        assert_eq!(resized(&six, &declared, None, Resize::Add(tab)).len(), MAX_DEVICES);
        let set = resized(&declared, &declared, None, Resize::Set(1, Device { w: 800, h: 600 }));
        assert_eq!(set[1], (Some(1), Device { w: 800, h: 600 }));
        assert_eq!(resized(&declared, &declared, None, Resize::Rotate(0))[0], (Some(0), Device { w: 844, h: 390 }));
        // Reset from one device: it keeps its frame as the first, the rest are new.
        assert_eq!(
            resized(&[tab], &declared, None, Resize::Reset),
            vec![(Some(0), declared[0]), (None, declared[1]), (None, declared[2])]
        );
    }

    /// Kept as `WxH,…`, read back the same; anything unreadable is none.
    #[test]
    fn the_viewers_sizes_are_kept_per_mockup() {
        let v = vec![Device { w: 1024, h: 768 }, Device { w: 390, h: 844 }];
        assert_eq!(encode(&v), "1024x768,390x844");
        assert_eq!(decode(Some(&encode(&v))), Some(v));
        assert_eq!(decode(None), None);
        assert_eq!(decode(Some("")), None);
        assert_eq!(decode(Some("garbage")), None);
        assert_eq!(storage_key("APP-33#m1"), "menger.mockup-sizes:APP-33#m1");
        assert_eq!(says(Device { w: 1280, h: 800 }), "1280 × 800");
    }
}
