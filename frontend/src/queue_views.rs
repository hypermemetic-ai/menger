//! APP-85: **a queue entry's views of mockups** in the tray (APP-51 Fig 2;
//! owner, 2026-10-17: *"you should be able to queue a view of the
//! mockup"*). An agent queues a ticket with `--mockup T#m1 --mockup T#m2`
//! (or APP-45's `mockup:T#N@W`); the entry carries them
//! ([`menger_tickets_api::MockupView`]) and the tray shows them:
//!
//! - **The row** says `▣ N views` ([`label`]).
//! - **The answer** shows them as thumbnails above the question — each the
//!   mocked page, live but untouchable, at its width (1440 unless the view
//!   names one), with its label: the option it sits in, else its title.
//!   A phone scrolls them sideways.
//! - **A thumbnail opens the viewer**: the mockup full size at 390 or 1440,
//!   `‹ ›` (and ← →) between them, *compare* (with more than one) lays them
//!   side by side at one width, as APP-44's compare does; Esc or *close*
//!   closes it. At a phone's width the viewer is the whole screen.
//! - **pick this** — on a thumbnail, in the viewer or in compare — picks the
//!   option the mockup sits in (APP-46: an `<m-option>` of a decision the
//!   entry names), the same pick as the option card's, and the viewer
//!   closes; Submit records it, as ever.
//!
//! A mockup is read and drawn as APP-44's compare reads and draws a column
//! ([`crate::compare::resolve`]: the ticket read over the viewer's socket,
//! its Nth placeholder, a frame of the page), so a body amended re-draws in
//! place and one the viewer cannot read is a named state, never a crash.
//! What decides — the labels, the step between views, the size of a device
//! in its box — is pure and pinned by native tests; the rest is DOM.

use leptos::prelude::*;
use menger_tickets_api::{MockupView, QueueEntry};
use wasm_bindgen::JsCast;

use crate::compare::{self, Col, Item};
use crate::frame::{Emulate, Frame};
use crate::present::Device;
use crate::{dom, md};

/// The row's word for `n` views.
pub(crate) fn label(n: usize) -> Option<String> {
    match n {
        0 => None,
        1 => Some("▣ 1 view".into()),
        n => Some(format!("▣ {n} views")),
    }
}

/// The device a view is drawn at: its width (390 a phone's height, else a
/// desk's), 1440 when it names none.
pub(crate) fn device(width: Option<u32>) -> Device {
    let w = width.unwrap_or(1440).clamp(240, 3840);
    let h = match w {
        0..=480 => 844,
        481..=1024 => 1024,
        _ => 900,
    };
    Device { w, h }
}

/// The scale that fits `d` in a box `bw` × `bh` (never above 1).
pub(crate) fn fit(d: Device, bw: f64, bh: f64) -> f64 {
    (bw / f64::from(d.w.max(1))).min(bh / f64::from(d.h.max(1))).clamp(0.02, 1.0)
}

/// The view `by` steps from `at`, of `n`, wrapping.
pub(crate) fn stepped(at: usize, n: usize, by: i32) -> usize {
    if n == 0 {
        return 0;
    }
    (at as i64 + i64::from(by)).rem_euclid(n as i64) as usize
}

/// The option a mockup sits in (APP-46): its decision's input, its value,
/// its name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Opt {
    pub(crate) input: String,
    pub(crate) value: String,
    pub(crate) name: Option<String>,
}

/// What a view shows: the page, its label, the option it sits in.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Shown {
    Loading,
    Page { page: String, scope: String, label: String, option: Option<Opt> },
    Failed(String),
}

/// `view` as it shows now — tracked (a read landing, a body amended).
fn shown(view: &MockupView) -> Shown {
    let item = Item { ticket: view.ticket.clone(), mock: view.mock };
    match compare::resolve(&item) {
        Col::Loading => Shown::Loading,
        Col::Failed(why) => Shown::Failed(why.text()),
        Col::Shown { page, caption, scope } => {
            let option = view.mock.and_then(|n| option_of(&view.ticket, n));
            let label = option
                .as_ref()
                .and_then(|o| o.name.clone())
                .or(caption.title.clone())
                .unwrap_or_else(|| compare::label(&caption, view.mock));
            Shown::Page { page, scope, label, option }
        }
    }
}

/// The option the `n`th mockup of `ticket`'s body sits in, if any — the
/// body rendered as its frame renders it, read from an inert `<template>`.
fn option_of(ticket: &str, n: u32) -> Option<Opt> {
    let t = compare::read_ticket(ticket).ok().flatten()?;
    let (_, src) = md::split_frontmatter(&t.body);
    let html = md::render_body_framed(src, &|_| None).html;
    let tpl = dom::window().document()?.create_element("template").ok()?.dyn_into::<web_sys::HtmlTemplateElement>().ok()?;
    tpl.set_inner_html(&html);
    let el = tpl.content().query_selector_all(md::MOCKUP_TAG).ok()?.get(n.checked_sub(1)?)?.dyn_into::<web_sys::Element>().ok()?;
    let opt = el.closest("m-option").ok()??;
    let dec = opt.closest("m-decision").ok()??;
    let input = dec.get_attribute("id").filter(|s| !s.trim().is_empty())?;
    let name = opt.get_attribute("name");
    let value = opt.get_attribute("value").or_else(|| name.clone())?;
    Some(Opt { input: input.trim().to_string(), value, name })
}

/// One device of a view, scaled into a box `bw` × `bh` — live, never
/// touched (a thumbnail), or used (the viewer).
fn screen(view: MockupView, d: Device, bw: f64, bh: f64, emulate: Signal<Emulate>, live: bool) -> impl IntoView {
    let s = fit(d, bw, bh);
    let outer = format!("width:{:.1}px;height:{:.1}px", f64::from(d.w) * s, f64::from(d.h) * s);
    let inner = format!("width:{}px;height:{}px;transform:scale({s:.4})", d.w, d.h);
    let got = Memo::new(move |_| shown(&view));
    let html = Signal::derive(move || match got.get() {
        Shown::Page { page, .. } => page,
        _ => String::new(),
    });
    move || match got.get() {
        Shown::Loading => view! { <div class="mvs-state" style=outer.clone()>"…"</div> }.into_any(),
        Shown::Failed(why) => view! { <div class="mvs-state failed" style=outer.clone()>{why}</div> }.into_any(),
        Shown::Page { scope, .. } => view! {
            <div class="mvs" class:live=live style=outer.clone()>
                <div class="mvs-in" style=inner.clone()>
                    <Frame html ticket=None scope emulate fixed=true />
                </div>
            </div>
        }
        .into_any(),
    }
}

/// What the viewer holds: the view open (its index) and whether it compares.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Viewing {
    pub(crate) at: usize,
    pub(crate) compare: bool,
}

/// The tray's handle on the views: which is open in the viewer, and what
/// *pick this* does (the tray's pick, for the input it names).
#[derive(Clone, Copy)]
pub(crate) struct Views {
    pub(crate) open: RwSignal<Option<Viewing>>,
    pub(crate) width: RwSignal<u32>,
    pub(crate) picked: Callback<Opt>,
}

/// The views of `e` above its question: a thumbnail each (with its label and
/// *pick this*), *compare all N* with more than one.
pub(crate) fn strip(e: &QueueEntry, v: Views, phone: bool) -> impl IntoView {
    let n = e.mockups.len();
    let named: Vec<String> = e.named().into_iter().map(str::to_string).collect();
    let emulate = Signal::derive(|| Emulate { scheme: Some(crate::frame::painted_scheme()), reduce: Some(true) });
    let (tw, th) = if phone { (240.0, 150.0) } else { (156.0, 98.0) };
    let thumbs = e
        .mockups
        .iter()
        .cloned()
        .enumerate()
        .map(|(i, view)| {
            let d = device(view.width);
            let got = {
                let view = view.clone();
                Memo::new(move |_| shown(&view))
            };
            let named = named.clone();
            let foot = move || {
                let (label, option) = match got.get() {
                    Shown::Page { label, option, .. } => (label, option),
                    _ => (String::new(), None),
                };
                let pick = option.filter(|o| named.contains(&o.input)).map(|o| {
                    view! { <button type="button" class="qbtn mv-pick" data-act="mock-pick" data-value=o.value.clone() on:click=move |ev| { ev.stop_propagation(); v.picked.run(o.clone()); }>"pick this"</button> }
                });
                view! { <span class="mv-l">{label}</span>{pick} }
            };
            let key = view.to_string();
            view! {
                <div class="mv-thumb" data-view=key.clone()>
                    <button type="button" class="mv-open" data-act="mock-view" aria-label=format!("open {key}") on:click=move |_| v.open.set(Some(Viewing { at: i, compare: false }))>
                        {screen(view.clone(), d, tw, th, emulate, false)}
                        <span class="mv-w" aria-hidden="true">{format!("⤢ {}", d.w)}</span>
                    </button>
                    <div class="mv-foot">{foot}</div>
                </div>
            }
        })
        .collect_view();
    let ticket = e.ticket.clone();
    view! {
        <div class="mv-strip">
            <div class="mv-head">
                <span>{format!("▣ {n} {} · {ticket}'s mockups", if n == 1 { "view" } else { "views" })}</span>
                {(n > 1).then(|| view! { <button type="button" class="mv-cmp" data-act="mock-compare" on:click=move |_| v.open.set(Some(Viewing { at: 0, compare: true }))>{format!("compare all {n} ⇆")}</button> })}
            </div>
            <div class="mv-row">{thumbs}</div>
        </div>
    }
}

/// The viewer: the open view full size (390 | 1440, ‹ ›), or all of them
/// side by side; *pick this*; close.
pub(crate) fn viewer(e: QueueEntry, v: Views) -> impl IntoView {
    let views = e.mockups.clone();
    let n = views.len();
    let named: Vec<String> = e.named().into_iter().map(str::to_string).collect();
    let emulate = Signal::derive(|| Emulate { scheme: Some(crate::frame::painted_scheme()), reduce: Some(false) });
    move || {
        let viewing = v.open.get()?;
        let at = viewing.at.min(n.saturating_sub(1));
        let view = views.get(at)?.clone();
        let w = v.width.get();
        let win = dom::window();
        let (vw, vh) = (win.inner_width().ok()?.as_f64()?, win.inner_height().ok()?.as_f64()?);
        let phone = vw <= 640.0;
        let (bw, bh) = if phone { (vw - 20.0, vh - 120.0) } else { (vw * 0.86 - 40.0, vh * 0.9 - 110.0) };
        let d = device(Some(w));
        let got = {
            let view = view.clone();
            Memo::new(move |_| shown(&view))
        };
        let pick = {
            let named = named.clone();
            move || match got.get() {
                Shown::Page { option: Some(o), .. } if named.contains(&o.input) && !viewing.compare => Some(view! {
                    <button type="button" class="qbtn mv-pick" data-act="mock-pick" data-value=o.value.clone() on:click=move |_| v.picked.run(o.clone())>"pick this"</button>
                }),
                _ => None,
            }
        };
        let title = move || match got.get() {
            Shown::Page { label, .. } => label,
            _ => String::new(),
        };
        let stage = if viewing.compare {
            let k = n.max(1) as f64;
            let (cw, ch) = if phone { (bw * 0.84, bh - 40.0) } else { ((bw - 16.0 * (k - 1.0)) / k, bh - 40.0) };
            let named = named.clone();
            views
                .iter()
                .cloned()
                .map(|x| {
                    let got = {
                        let x = x.clone();
                        Memo::new(move |_| shown(&x))
                    };
                    let named = named.clone();
                    let head = move || match got.get() {
                        Shown::Page { label, option, .. } => {
                            let pick = option.filter(|o| named.contains(&o.input)).map(|o| {
                                view! { <button type="button" class="qbtn mv-pick" data-act="mock-pick" data-value=o.value.clone() on:click=move |_| v.picked.run(o.clone())>"pick this"</button> }
                            });
                            view! { <span class="mv-l">{label}</span>{pick} }.into_any()
                        }
                        _ => ().into_any(),
                    };
                    view! {
                        <div class="mvc-col" data-view=x.to_string()>
                            <div class="mvc-head"><span class="code">{x.key()}</span>{head}</div>
                            {screen(x.clone(), d, cw, ch, emulate, true)}
                        </div>
                    }
                })
                .collect_view()
                .into_any()
        } else {
            screen(view.clone(), d, bw, bh, emulate, true).into_any()
        };
        let key = view.key();
        Some(view! {
            <div class="mvx" role="dialog" aria-modal="true" aria-label="the mockups" class:phone=phone>
                <div class="mvx-scrim" on:click=move |_| v.open.set(None)></div>
                <div class="mvx-box">
                    <div class="mvx-bar">
                        {(!viewing.compare).then(|| view! { <span class="code">{key.clone()}</span>" "<b class="mvx-t">{title}</b> })}
                        {viewing.compare.then(|| view! { <b class="mvx-t">{format!("{n} views side by side")}</b> })}
                        {(!viewing.compare && n > 1).then(|| view! {
                            <span class="mvx-of">{format!("view {} of {n}", at + 1)}</span>
                            <button type="button" class="mvx-nav" data-act="mock-prev" aria-label="previous view" on:click=move |_| v.open.set(Some(Viewing { at: stepped(at, n, -1), compare: false }))>"‹"</button>
                            <button type="button" class="mvx-nav" data-act="mock-next" aria-label="next view" on:click=move |_| v.open.set(Some(Viewing { at: stepped(at, n, 1), compare: false }))>"›"</button>
                        })}
                        <span class="sp"></span>
                        <span class="mvx-seg" role="group" aria-label="device width">
                            <button type="button" aria-pressed=(w == 390).to_string() data-act="mock-390" on:click=move |_| v.width.set(390)>"390"</button>
                            <button type="button" aria-pressed=(w == 1440).to_string() data-act="mock-1440" on:click=move |_| v.width.set(1440)>"1440"</button>
                        </span>
                        {(n > 1).then(|| view! {
                            <button type="button" class="qbtn" data-act="mock-compare-toggle" aria-pressed=viewing.compare.to_string()
                                on:click=move |_| v.open.set(Some(Viewing { at, compare: !viewing.compare }))>{if viewing.compare { "one at a time" } else { "compare" }}</button>
                        })}
                        {pick}
                        <button type="button" class="qx" data-act="mock-close" on:click=move |_| v.open.set(None)>"close "<kbd>"esc"</kbd></button>
                    </div>
                    <div class="mvx-stage" class:cmp=viewing.compare>{stage}</div>
                </div>
            </div>
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_row_counts_its_views() {
        assert_eq!(label(0), None);
        assert_eq!(label(1).as_deref(), Some("▣ 1 view"));
        assert_eq!(label(3).as_deref(), Some("▣ 3 views"));
    }

    #[test]
    fn a_view_is_drawn_at_its_width_and_fits_its_box() {
        assert_eq!(device(None), Device { w: 1440, h: 900 });
        assert_eq!(device(Some(390)), Device { w: 390, h: 844 });
        assert_eq!(device(Some(768)), Device { w: 768, h: 1024 });
        let s = fit(Device { w: 1440, h: 900 }, 190.0, 119.0);
        assert!((s - (190.0f64 / 1440.0).min(119.0 / 900.0)).abs() < 1e-9, "the tighter side: {s}");
        assert_eq!(fit(Device { w: 390, h: 844 }, 4000.0, 4000.0), 1.0, "never above full size");
    }

    #[test]
    fn the_viewer_steps_round() {
        assert_eq!(stepped(0, 3, -1), 2);
        assert_eq!(stepped(2, 3, 1), 0);
        assert_eq!(stepped(1, 3, 1), 2);
        assert_eq!(stepped(0, 0, 1), 0);
    }
}
