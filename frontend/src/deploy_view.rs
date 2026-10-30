//! DEPLOY-30: `#/deploy` — what we're working on, grouped by area and laid
//! out by where it will land (the model is src/flight.rs).
//!
//! **At a desk, the canvas**: a column per lane — coding, in review,
//! waiting, on test — and the destination (prod) at the right, each
//! heading saying where its work lands; a band per area (a shortcode), its
//! tickets as nodes in their lane's column, wrapped in a hull, its summary
//! above it; blockers as arrows. Drag pans, the wheel scrolls (with Ctrl or
//! a trackpad's pinch it zooms), two fingers pinch and pan, a double-tap
//! zooms in (DEPLOY-38), `+` / `−` / `⤢` zoom and fit; the legend lights one
//! state. A node opens its summary at the side — where it runs in each
//! environment, its commits, its blockers — then *open ticket*; an area's
//! label opens the area: one line per ticket, the ticket cited at its end.
//!
//! **On a phone, the clusters**: the same areas as a list, a row per
//! ticket (its state, its marks per environment), the graph one tap away —
//! at 390 the whole graph fits only at a third of its size.
//!
//! **Live.** One filtered `list` (`flight::membership`, at most 1,000)
//! answers the work in flight; its membership watch says when a ticket
//! enters or leaves (a status change, a deploy, a promote) and the session's
//! `watch` when one held changes — each asks the list again, once, a
//! moment later. Where each runs is the app's copy (src/deploys.rs), kept
//! by `deployments.watch`: a deploy moves the marks and the lanes at once.
//!
//! **→ test** (DEPLOY-37, src/to_test.rs) and **→ prod** (DEPLOY-40,
//! src/to_prod.rs) sit under the heading.
//!
//! `#/deploy` opens on the project that has `prod` (MENGER's); `#/deploy/
//! <SHORTCODE>` one; `?to=<env>` the destination, when it has several.

use std::collections::HashSet;
use std::time::Duration;

use leptos::prelude::*;
use menger_tickets_api::{TicketSummary, TicketsEvent};
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::spawn_local;

use crate::deploys::{self, Held};
use crate::flight::{self, Cluster, Laid, Node, State, NH, NW};
use crate::to_prod::ToProd;
use crate::to_test::ToTest;
use crate::{session, st_class};

/// How many tickets the page asks for at most.
const LIMIT: u32 = 1000;

/// `?<key>=<value>` of a hash.
fn param_of(hash: &str, key: &str) -> Option<String> {
    let (_, q) = hash.split_once('?')?;
    q.split('&').find_map(|p| p.strip_prefix(key)?.strip_prefix('=')).filter(|v| !v.is_empty()).map(str::to_string)
}

/// `?to=<env>` of a hash.
fn to_of(hash: &str) -> Option<String> {
    param_of(hash, "to")
}

/// DEPLOY-39: `?stage=<state>` of a hash — the stage pill that is on.
fn stage_of(hash: &str) -> Option<State> {
    State::of_class(&param_of(hash, "stage")?)
}

/// The page's own hash: `#/deploy/MENGER?to=app-prod&stage=ready`.
fn href(project: &str, to: Option<&str>, stage: Option<State>) -> String {
    let q: Vec<String> = to.map(|t| format!("to={t}")).into_iter().chain(stage.map(|s| format!("stage={}", s.class()))).collect();
    if q.is_empty() { format!("#/deploy/{project}") } else { format!("#/deploy/{project}?{}", q.join("&")) }
}

/// Put the page's own hash in the address bar, no history entry.
fn keep_hash(url: &str) {
    if let Ok(h) = crate::dom::window().history() {
        let _ = h.replace_state_with_url(&wasm_bindgen::JsValue::NULL, "", Some(url));
    }
}

/// The project a bare `#/deploy` opens on: one with `prod`, else the first.
fn default_project(h: &Held) -> Option<String> {
    let ps = h.projects();
    ps.iter().find(|p| h.of_project(p).iter().any(|e| e.name == "prod")).or(ps.first()).cloned()
}

/// The work in flight: one filtered `list` — the rows, and how many more
/// matched than it answered.
async fn in_flight(filter: String) -> Result<(Vec<TicketSummary>, u64), String> {
    let client = session::client().ok_or_else(|| "not connected".to_string())?;
    let filter = crate::typed::typed(&filter)?;
    let events = client
        .list_all(None, None, None, None, Some(LIMIT), Some(true), Some(filter), None, None, session::zone())
        .await
        .map_err(|e| session::describe(&e))?;
    let mut out = Vec::new();
    let mut more = 0;
    for ev in events {
        match ev {
            TicketsEvent::Summary { summary } => out.push(summary),
            TicketsEvent::Page { remaining, .. } => more = remaining,
            TicketsEvent::Error { message } => return Err(message),
            _ => {}
        }
    }
    Ok((out, more))
}

/// The id a session `watch` event moves, when it carries one whole.
fn moved_id(ev: &TicketsEvent) -> Option<String> {
    match ev {
        TicketsEvent::StatusChanged { ticket, .. }
        | TicketsEvent::KindChanged { ticket, .. }
        | TicketsEvent::BlockersChanged { ticket, .. }
        | TicketsEvent::UnlocksChanged { ticket, .. }
        | TicketsEvent::UnlocksRemoved { ticket, .. }
        | TicketsEvent::SeverityChanged { ticket, .. } => Some(ticket.id.to_string()),
        _ => None,
    }
}

/// What the sheet shows.
#[derive(Clone, Debug, PartialEq)]
enum Open {
    Ticket(String),
    Area(String),
}

#[component]
pub(crate) fn DeployPage(project: Option<String>) -> impl IntoView {
    deploys::want();
    let held = deploys::held();
    let hash0 = crate::dom::window().location().hash().unwrap_or_default();
    let asked_to = to_of(&hash0);
    let asked_stage = stage_of(&hash0);
    let project = {
        let held = held.clone();
        Memo::new(move |_| project.clone().or_else(|| held.with(|h| h.as_ref().and_then(default_project))))
    };
    let dests = {
        let held = held.clone();
        Memo::new(move |_| {
            let p = project.get();
            held.with(|h| match (h, p) {
                (Some(h), Some(p)) => flight::destinations(h.envs(), &p),
                _ => Vec::new(),
            })
        })
    };
    let retired = {
        let held = held.clone();
        Memo::new(move |_| match (project.get(), held.with(|h| h.is_some())) {
            (Some(p), true) => held.with(|h| h.as_ref().map(|h| flight::retired(h.envs(), &p)).unwrap_or_default()),
            _ => Vec::new(),
        })
    };
    let to = RwSignal::new(asked_to);
    // A `?to=` of the address that names a retired destination is not honoured; a pick from the menu is.
    let from_url = StoredValue::new(true);
    let dest = {
        let held = held.clone();
        Memo::new(move |_| {
            let (p, ds) = (project.get()?, dests.get());
            match to.get() {
                Some(t) if ds.contains(&t) && !(from_url.get_value() && retired.with(|r| r.contains(&t))) => Some(t),
                _ => held.with(|h| h.as_ref().and_then(|h| flight::default_destination(h.envs(), &p))),
            }
        })
    };
    let fallback = {
        let held = held.clone();
        Memo::new(move |_| {
            let p = project.get()?;
            held.with(|h| h.as_ref().and_then(|h| flight::default_destination(h.envs(), &p)))
        })
    };
    // DEPLOY-43: a saved `?to=` naming a retired destination is moved to
    // where the project's prod is now, with a line saying so (once).
    let moved = RwSignal::new(None::<String>);
    let checked = StoredValue::new(false);
    let held_fx = held.clone();
    Effect::new(move |_| {
        let held = held_fx.clone();
        let (Some(p), false) = (project.get(), checked.get_value()) else { return };
        if !held.with(|h| h.is_some()) {
            return;
        }
        checked.set_value(true);
        if let Some(t) = to.get_untracked().filter(|t| retired.with(|r| r.contains(t))) {
            if let Some(now) = held.with_untracked(|h| h.as_ref().and_then(|h| flight::default_destination(h.envs(), &p))) {
                moved.set(Some(format!("{t} moved to {now} (RUN-13)")));
                to.set(Some(now.clone()));
                keep_hash(&href(&p, Some(&now), None));
            }
        }
    });
    let track = {
        let held = held.clone();
        Memo::new(move |_| match (project.get(), dest.get()) {
            (Some(p), Some(d)) => held.with(|h| h.as_ref().map(|h| flight::track(h.envs(), &p, &d)).unwrap_or_default()),
            _ => Vec::new(),
        })
    };
    let filter = Memo::new(move |_| {
        let t = track.get();
        let p = project.get()?;
        (!t.is_empty()).then(|| flight::membership(&p, &t))
    });

    // The rows: asked on each connection and filter, and again a moment
    // after anything says they moved.
    let rows = RwSignal::new(None::<Result<(Vec<TicketSummary>, u64), String>>);
    let link = session::link();
    let fetch = move || {
        let Some(f) = filter.get_untracked() else { return };
        spawn_local(async move {
            let answer = in_flight(f).await;
            rows.try_set(Some(answer));
        });
    };
    let pending = StoredValue::new(false);
    let soon = move || {
        if pending.get_value() {
            return;
        }
        pending.set_value(true);
        set_timeout(
            move || {
                if pending.try_set_value(false).is_none() {
                    fetch();
                }
            },
            Duration::from_millis(300),
        );
    };
    Effect::new(move |_| {
        let up = link.get();
        if up && filter.get().is_some() {
            fetch();
        }
    });
    session::follow_membership(move || filter.get(), move |_| soon());
    let ids = Memo::new(move |_| {
        rows.with(|r| match r {
            Some(Ok((ts, _))) => ts.iter().map(|t| t.id.to_string()).collect::<HashSet<_>>(),
            _ => HashSet::new(),
        })
    });
    session::listen(move |ev| {
        if moved_id(ev).is_some_and(|id| ids.with_untracked(|s| s.contains(&id))) {
            soon();
        }
    });

    let nodes = {
        let held = held.clone();
        Memo::new(move |_| {
            let (Some(p), t) = (project.get(), track.get()) else { return Vec::new() };
            rows.with(|r| {
                let Some(Ok((ts, _))) = r else { return Vec::new() };
                held.with(|h| {
                    let Some(h) = h else { return Vec::new() };
                    ts.iter().filter_map(|row| flight::node_of(row, &h.marks(&row.id.to_string()), &p, &t)).collect()
                })
            })
        })
    };
    // DEPLOY-39: a stage pill filters — only its tickets are drawn, an area
    // with none is gone, every count is of the matches.
    let hi = RwSignal::new(asked_stage);
    let shown = Memo::new(move |_| flight::only(nodes.get(), hi.get()));
    let clusters = Memo::new(move |_| flight::clusters(shown.get()));
    let lanes = Memo::new(move |_| flight::lanes(&track.get()));
    let laid = Memo::new(move |_| clusters.with(|cs| flight::lay_out(cs, &lanes.get())));
    let open = RwSignal::new(None::<Open>);
    let graph = RwSignal::new(false);

    let on_key = window_event_listener(leptos::ev::keydown, move |e| {
        if crate::bindings::is(&e, crate::bindings::Context::Global, "app.escape") {
            open.set(None);
        }
    });
    on_cleanup(move || on_key.remove());

    let head = {
        let held = held.clone();
        move || {
            let t = track.get();
            let p = project.get().unwrap_or_default();
            let marks = held.with(|h| {
                t.iter()
                    .map(|name| {
                        let env = h.as_ref().and_then(|h| h.of_project(&p).into_iter().find(|e| &e.name == name));
                        let runs = env.as_ref().and_then(|e| Some(format!("{} · {}", deploys::short(e.revision.as_deref()?), deploys::since(e.since?))));
                        view! {
                            <span class="envmark big">{name.clone()}</span>
                            <span class="rev">{runs.unwrap_or_else(|| "nothing yet".into())}</span>
                        }
                    })
                    .collect_view()
            });
            view! { <span class="dests">{marks}</span> }
        }
    };
    let counts = move || {
        rows.with(|r| match r {
            Some(Ok((_, more))) => {
                let n = nodes.with(Vec::len);
                let more = (*more > 0).then(|| format!(" · {more} more not asked"));
                format!("{n} in flight · {} → {}{}", project.get().unwrap_or_default(), dest.get().unwrap_or_default(), more.unwrap_or_default())
            }
            _ => String::new(),
        })
    };
    let pick_dest = move |ev: leptos::ev::Event| {
        let Some(sel) = ev.target().and_then(|t| t.dyn_into::<web_sys::HtmlSelectElement>().ok()) else { return };
        let v = sel.value();
        if let Some(p) = project.get_untracked() {
            keep_hash(&href(&p, Some(&v), hi.get_untracked()));
        }
        from_url.set_value(false);
        moved.set(None);
        to.set(Some(v));
    };
    // The pill that is on lives in the address (`?stage=ready`).
    Effect::new(move |prev: Option<()>| {
        let s = hi.get();
        if prev.is_some() {
            if let Some(p) = project.get_untracked() {
                keep_hash(&href(&p, to.get_untracked().as_deref(), s));
            }
        }
    });

    view! {
        <div id="deploy-page" class:graph=move || graph.get()>
            <div class="ptitle">
                <h2>"What we're working on"</h2>
                <span class="sub">{counts}</span>
                {head}
            </div>
            {move || moved.get().map(|m| view! { <p class="movednote" role="status">{m}</p> })}
            <ToTest project />
            <ToProd project track dest fallback />
            <div class="tools">
                <div class="legend" role="group" aria-label="filter by stage">
                    {State::ALL.into_iter().map(|s| {
                        let n = move || nodes.with(|ns| ns.iter().filter(|x| x.state == s).count());
                        view! {
                            <button
                                type="button"
                                class=format!("ns {}", s.class())
                                data-state=s.class()
                                aria-pressed=move || (hi.get() == Some(s)).to_string()
                                on:click=move |_| hi.update(|h| *h = if *h == Some(s) { None } else { Some(s) })
                            >
                                <span class="n">{n}</span>" "{s.words()}
                                {move || (hi.get() == Some(s)).then(|| view! { <span class="x" aria-label="clear the filter">" ✕"</span> })}
                            </button>
                        }
                    }).collect_view()}
                </div>
                <div class="seg" role="tablist" aria-label="layout">
                    <button type="button" role="tab" id="seg-list" aria-selected=move || (!graph.get()).to_string() on:click=move |_| graph.set(false)>"clusters"</button>
                    <button type="button" role="tab" id="seg-graph" aria-selected=move || graph.get().to_string() on:click=move |_| graph.set(true)>"graph"</button>
                </div>
                {move || (dests.with(Vec::len) > 1).then(|| view! {
                    <label class="to">
                        "to "
                        <select on:change=pick_dest aria-label="destination">
                            {move || {
                                let r = retired.get();
                                let (mut live, old): (Vec<String>, Vec<String>) = dests.get().into_iter().partition(|d| !r.contains(d));
                                live.extend(old);
                                live.into_iter().map(|d| {
                                    let on = dest.get().as_deref() == Some(d.as_str());
                                    let word = if r.contains(&d) { format!("{d} (retired)") } else { d.clone() };
                                    view! { <option value=d.clone() selected=on>{word}</option> }
                                }).collect_view()
                            }}
                        </select>
                    </label>
                })}
            </div>
            {move || match rows.get() {
                None => view! { <p class="loading">"loading the work in flight…"</p> }.into_any(),
                Some(Err(e)) => view! { <p class="derr">{e}</p> }.into_any(),
                Some(Ok(_)) if nodes.with(Vec::is_empty) => view! { <p class="none">"Nothing in flight: everything finished runs on " {dest.get()} "."</p> }.into_any(),
                Some(Ok(_)) => view! {
                    <div class="stage">
                        <Canvas laid clusters track open graph />
                        <List clusters open />
                    </div>
                }.into_any(),
            }}
            <div class="scrim" class:on=move || open.with(Option::is_some) on:click=move |_| open.set(None)></div>
            <aside class="sheet" class:on=move || open.with(Option::is_some) aria-label="summary" aria-hidden=move || open.with(Option::is_none).to_string()>
                {move || open.get().map(|o| match o {
                    Open::Ticket(id) => view! { <TicketSheet id nodes project open /> }.into_any(),
                    Open::Area(code) => view! { <AreaSheet code clusters open /> }.into_any(),
                })}
            </aside>
        </div>
    }
}

/// The canvas: lanes as columns, areas as hulls, tickets as nodes, blockers
/// as arrows; pan and zoom.
#[component]
fn Canvas(laid: Memo<Laid>, clusters: Memo<Vec<Cluster>>, track: Memo<Vec<String>>, open: RwSignal<Option<Open>>, graph: RwSignal<bool>) -> impl IntoView {
    let cv = NodeRef::<leptos::html::Div>::new();
    // (k, tx, ty): the zoom and the pan.
    let view_at = RwSignal::new((1.0_f64, 12.0_f64, 0.0_f64));
    let fitted = StoredValue::new(false);
    // DEPLOY-43: `fit` runs in an animation frame, which can land after the
    // canvas is gone (a destination with nothing in flight draws none): every
    // read and write here is the one that answers "gone" rather than panics.
    let size = move || cv.try_get_untracked().flatten().map(|el| {
        let r = el.get_bounding_client_rect();
        (r.width(), r.height())
    });
    let fit = move || {
        if let Some((w, _)) = size().filter(|(w, _)| *w > 0.0) {
            if let Some(v) = laid.try_with_untracked(|l| flight::fit_width(l, w)) {
                view_at.try_set(v);
            }
            fitted.try_set_value(true);
        }
    };
    // Fit once there is something drawn and room to draw it — at once at a
    // desk, when the graph is picked on a phone.
    Effect::new(move |_| {
        laid.track();
        graph.track();
        if !fitted.get_value() {
            request_animation_frame(fit);
        }
    });
    Effect::new(move |prev: Option<bool>| {
        let g = graph.get();
        if g && prev == Some(false) {
            request_animation_frame(fit);
        }
        g
    });
    let on_resize = window_event_listener(leptos::ev::resize, move |_| fit());
    on_cleanup(move || on_resize.remove());

    // Drag pans; a press that does not move opens what it pressed. Two
    // fingers pinch (DEPLOY-38): the zoom about their midpoint, the pan
    // with it (`flight::pinch`); a double-tap off a node zooms in. Points
    // are the canvas's own (its top-left is 0, 0).
    let drag = StoredValue::new(None::<(f64, f64, f64, f64, bool, Option<Open>)>);
    let fingers = StoredValue::new(Vec::<(i32, f64, f64)>::new());
    let pinch_from = StoredValue::new(None::<((f64, f64, f64), (f64, f64), (f64, f64))>);
    let last_tap = StoredValue::new(None::<(f64, f64, f64)>);
    // What a press that did not move opens, at its click: on a phone the
    // tap's click comes after the pointerup and lands on whatever is there
    // then — opened at the pointerup, the sheet's scrim took it and shut it.
    let tapped = StoredValue::new(None::<Open>);
    let local = move |ev: &leptos::ev::PointerEvent| {
        let (l, t) = cv.get_untracked().map_or((0.0, 0.0), |el| {
            let r = el.get_bounding_client_rect();
            (r.left(), r.top())
        });
        (ev.client_x() as f64 - l, ev.client_y() as f64 - t)
    };
    let two = move || {
        let f = fingers.get_value();
        (f.len() == 2).then(|| ((f[0].1, f[0].2), (f[1].1, f[1].2)))
    };
    let down = move |ev: leptos::ev::PointerEvent| {
        let Some(t) = ev.target().and_then(|t| t.dyn_into::<web_sys::Element>().ok()) else { return };
        if t.closest(".zoom").ok().flatten().is_some() {
            return;
        }
        let p = local(&ev);
        fingers.update_value(|f| {
            f.retain(|(id, _, _)| *id != ev.pointer_id());
            f.push((ev.pointer_id(), p.0, p.1));
        });
        if let Some(el) = cv.get_untracked() {
            let _ = el.set_pointer_capture(ev.pointer_id());
        }
        if let Some((a, b)) = two() {
            // A second finger: the drag becomes a pinch, and opens nothing.
            pinch_from.set_value(Some((view_at.get_untracked(), a, b)));
            drag.set_value(None);
            return;
        }
        let what = t
            .closest("[data-open-id]")
            .ok()
            .flatten()
            .and_then(|n| n.get_attribute("data-open-id"))
            .map(Open::Ticket)
            .or_else(|| t.closest("[data-open-area]").ok().flatten().and_then(|n| n.get_attribute("data-open-area")).map(Open::Area));
        let (_, tx, ty) = view_at.get_untracked();
        drag.set_value(Some((p.0, p.1, tx, ty, false, what)));
    };
    let moved = move |ev: leptos::ev::PointerEvent| {
        let p = local(&ev);
        fingers.update_value(|f| {
            if let Some(e) = f.iter_mut().find(|(id, _, _)| *id == ev.pointer_id()) {
                (e.1, e.2) = p;
            }
        });
        if let (Some((v0, a0, b0)), Some((a1, b1))) = (pinch_from.get_value(), two()) {
            ev.prevent_default();
            view_at.set(flight::pinch(v0, a0, b0, a1, b1));
            return;
        }
        let Some((x, y, tx, ty, was, what)) = drag.get_value() else { return };
        let (dx, dy) = (p.0 - x, p.1 - y);
        let now = was || dx.hypot(dy) > 4.0;
        if now {
            view_at.update(|v| {
                v.1 = tx + dx;
                v.2 = ty + dy;
            });
        }
        drag.set_value(Some((x, y, tx, ty, now, what)));
    };
    // A finger lifted: the last of a pinch goes on as a drag that opens
    // nothing; a press that did not move opens, or is half a double-tap.
    let lift = move |ev: &leptos::ev::PointerEvent| {
        fingers.update_value(|f| f.retain(|(id, _, _)| *id != ev.pointer_id()));
        if pinch_from.get_value().is_some() {
            pinch_from.set_value(None);
            let (_, tx, ty) = view_at.get_untracked();
            drag.set_value(fingers.get_value().first().map(|&(_, x, y)| (x, y, tx, ty, true, None)));
            last_tap.set_value(None);
            return false;
        }
        true
    };
    let up = move |ev: leptos::ev::PointerEvent| {
        if !lift(&ev) {
            return;
        }
        match drag.get_value() {
            Some((_, _, _, _, false, Some(what))) => tapped.set_value(Some(what)),
            Some((x, y, _, _, false, None)) => {
                let at = ev.time_stamp();
                if flight::double_tap(last_tap.get_value(), at, (x, y)) {
                    view_at.update(|v| *v = flight::zoom_at(*v, 1.6, x, y));
                    last_tap.set_value(None);
                } else {
                    last_tap.set_value(Some((at, x, y)));
                }
            }
            _ => {}
        }
        drag.set_value(None);
    };
    let cancel = move |ev: leptos::ev::PointerEvent| {
        lift(&ev);
        drag.set_value(None);
        tapped.set_value(None);
    };
    let click = move |_: leptos::ev::MouseEvent| {
        if let Some(what) = tapped.get_value() {
            tapped.set_value(None);
            open.set(Some(what));
        }
    };
    let wheel = move |ev: leptos::ev::WheelEvent| {
        ev.prevent_default();
        // A trackpad's pinch arrives as a wheel with ctrlKey set; Ctrl or ⌘ with a wheel zooms too.
        if ev.ctrl_key() || ev.meta_key() { // bindings-lint: a wheel's pinch, the browser's gesture, not a chord
            let Some(el) = cv.get_untracked() else { return };
            let r = el.get_bounding_client_rect();
            let f = if ev.delta_y() < 0.0 { 1.12 } else { 0.89 };
            view_at.update(|v| *v = flight::zoom_at(*v, f, ev.client_x() as f64 - r.left(), ev.client_y() as f64 - r.top()));
        } else {
            view_at.update(|v| {
                v.1 -= ev.delta_x();
                v.2 -= ev.delta_y();
            });
        }
    };
    let zoom_by = move |f: f64| {
        if let Some((w, h)) = size() {
            view_at.update(|v| *v = flight::zoom_at(*v, f, w / 2.0, h / 2.0));
        }
    };
    let fit_all = move |_| {
        if let Some((w, h)) = size() {
            view_at.set(laid.with_untracked(|l| flight::fit_all(l, w, h)));
        }
    };
    let world = move || {
        let (k, tx, ty) = view_at.get();
        format!("translate({tx:.1} {ty:.1}) scale({k:.3})")
    };
    let heads = move || {
        let (k, tx, _) = view_at.get();
        format!("translate({tx:.1} 0) scale({k:.3})")
    };
    let picked = move || match open.get() {
        Some(Open::Ticket(id)) => Some(id),
        _ => None,
    };

    view! {
        <div class="canvas" node_ref=cv on:pointerdown=down on:pointermove=moved on:pointerup=up on:pointercancel=cancel on:wheel=wheel on:click=click>
            <svg role="img" aria-label="the work in flight by area, where each lands, blockers as arrows">
                <defs>
                    <marker id="fah" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="7" markerHeight="7" orient="auto-start-reverse">
                        <path d="M0 0L10 5L0 10z" class="ahead"></path>
                    </marker>
                </defs>
                <g transform=world>
                    // The areas' hulls and labels.
                    {move || {
                        let l = laid.get();
                        clusters.with(|cs| {
                            l.bands.iter().map(|b| {
                                let c = cs.iter().find(|c| c.code == b.code);
                                let (loud, words) = c.map(Cluster::summary).unwrap_or((State::Coding, String::new()));
                                let n = c.map_or(0, |c| c.nodes.len());
                                let d = flight::path_of(&b.hull);
                                view! {
                                    <g class=format!("hull {}", loud.class()) data-area=b.code.clone()>
                                        <path d=d.clone() class="o"></path>
                                        <path d=d class="i"></path>
                                        <g class="hl" data-open-area=b.code.clone() role="button" tabindex="0" aria-label=format!("{} — its summary", b.code)>
                                            <text x=b.label.0 y=b.label.1 class="hlabel">{b.code.clone()}</text>
                                            <text x=b.label.0 y=b.label.1 + 16.0 class="hsub">{format!("{n} ticket{} · {words}", if n == 1 { "" } else { "s" })}</text>
                                        </g>
                                    </g>
                                }
                            }).collect_view()
                        })
                    }}
                    // Blockers: an arrow from the blocker to what waits on it.
                    {move || {
                        let l = laid.get();
                        clusters.with(|cs| {
                            let pos = &l.pos;
                            cs.iter().flat_map(|c| c.nodes.iter()).flat_map(|n| {
                                let to = pos.get(&n.id).copied();
                                n.blocked_by.iter().filter_map(move |b| Some(flight::edge_path(*pos.get(b)?, to?))).collect::<Vec<_>>()
                            }).map(|d| view! { <path d=d class="edge"></path> }).collect_view()
                        })
                    }}
                    // The tickets.
                    {move || {
                        let l = laid.get();
                        clusters.with(|cs| {
                            cs.iter().flat_map(|c| c.nodes.iter()).filter_map(|n| {
                                let (x, y) = *l.pos.get(&n.id)?;
                                let s = n.state;
                                let id = n.id.clone();
                                let pid = id.clone();
                                Some(view! {
                                    <g
                                        class=format!("fnode {}", s.class())
                                        class:picked=move || picked().as_deref() == Some(pid.as_str())
                                        transform=format!("translate({x} {y})")
                                        data-open-id=id.clone()
                                        data-ticket-id=id.clone()
                                        tabindex="0"
                                        role="button"
                                        aria-label=format!("{id} — {}", n.state_words())
                                        on:keydown={
                                            let id = id.clone();
                                            move |e: leptos::ev::KeyboardEvent| if crate::bindings::is(&e, crate::bindings::Context::Board, "open.focused") {
                                                e.prevent_default();
                                                open.set(Some(Open::Ticket(id.clone())));
                                            }
                                        }
                                    >
                                        <title>{format!("{id} · {} — {}", n.state_words(), n.title)}</title>
                                        <rect class="b" width=NW height=NH rx="6"></rect>
                                        <circle class="dot" cx="18" cy=NH / 2.0 r="10"></circle>
                                        <text class="gl" x="18" y=NH / 2.0 + 1.0>{s.glyph()}</text>
                                        <text class="id" x="36" y="19">{id.clone()}</text>
                                        <text class="tt" x="36" y="35">{flight::clip(&n.title, 23)}</text>
                                    </g>
                                })
                            }).collect_view()
                        })
                    }}
                </g>
                // The lanes' headings, held at the top as the canvas pans.
                <g class="heads" transform=heads>
                    {move || {
                        let l = laid.get();
                        let t = track.get();
                        let n_lanes = l.columns.len();
                        let cols = l.columns.iter().enumerate().map(|(i, (lane, x))| {
                            let count = clusters.with(|cs| cs.iter().flat_map(|c| c.nodes.iter()).filter(|n| n.lane == *lane).count());
                            let arrow = (i + 1 <= n_lanes).then(|| format!("M{:.0} 34L{:.0} 34", x + NW + 6.0, x + NW + 58.0));
                            view! {
                                <g class="lanehead" data-lane=lane.key() transform=format!("translate({x} 8)")>
                                    <rect width=NW height="56" rx="6"></rect>
                                    <text class="ln" x="12" y="22">{format!("{} · {count}", lane.words())}</text>
                                    <text class="ll" x="12" y="41">{flight::clip(&lane.lands(&t), 28)}</text>
                                </g>
                                {arrow.map(|d| view! { <path d=d class="edge go"></path> })}
                            }
                        }).collect_view();
                        let dest = t.last().cloned().unwrap_or_default();
                        view! {
                            {cols}
                            <g class="dest" transform=format!("translate({} 8)", l.dest_x)>
                                <rect width="168" height="56" rx="8"></rect>
                                <text class="dn" x="14" y="24">{dest}</text>
                                <text class="dr" x="14" y="43">"where it ships"</text>
                            </g>
                        }
                    }}
                </g>
            </svg>
            <div class="zoom">
                <button class="cbtn" type="button" aria-label="zoom in" on:click=move |_| zoom_by(1.25)>"+"</button>
                <button class="cbtn" type="button" aria-label="zoom out" on:click=move |_| zoom_by(0.8)>"−"</button>
                <button class="cbtn" type="button" aria-label="fit" on:click=fit_all>"⤢"</button>
                <span class="zpct">{move || format!("{:.0}%", view_at.get().0 * 100.0)}</span>
            </div>
            <div class="hint">"drag to pan · wheel to scroll, ctrl+wheel or pinch to zoom · double-tap zooms in · a node opens its summary"</div>
        </div>
    }
}

/// The phone's clusters: each area, a row per ticket.
#[component]
fn List(clusters: Memo<Vec<Cluster>>, open: RwSignal<Option<Open>>) -> impl IntoView {
    view! {
        <div class="list">
            {move || clusters.get().into_iter().map(|c| {
                let (loud, words) = c.summary();
                let code = c.code.clone();
                view! {
                    <section class="cl" data-area=c.code.clone()>
                        <button type="button" class="clh" on:click=move |_| open.set(Some(Open::Area(code.clone())))>
                            <b>{c.code.clone()}</b>
                            <span class="n">{c.nodes.len()}</span>
                            <span class=format!("ns {}", loud.class())>{words}</span>
                        </button>
                        {c.nodes.into_iter().map(|n| row(n, open)).collect_view()}
                    </section>
                }
            }).collect_view()}
        </div>
    }
}

/// One ticket's row in the clusters list.
fn row(n: Node, open: RwSignal<Option<Open>>) -> impl IntoView {
    let s = n.state;
    let id = n.id.clone();
    view! {
        <button
            type="button"
            class="row"
            data-ticket-id=n.id.clone()
            on:click=move |_| open.set(Some(Open::Ticket(id.clone())))
        >
            <span class="l1"><span class="id">{n.id.clone()}</span><span class="t">{n.title.clone()}</span></span>
            <span class="l2">
                <span class=format!("ns {}", s.class())>{n.state_words()}</span>
                {deploys::row_marks(n.id.clone())}
            </span>
        </button>
    }
}

/// A chip that opens a ticket's summary when it is in flight, else its page.
fn chip(id: String, in_flight: bool, open: RwSignal<Option<Open>>) -> impl IntoView {
    let href = crate::links::ticket_href(&id);
    let target = id.clone();
    view! {
        <a
            class="code"
            href=href
            on:click=move |e| if in_flight {
                e.prevent_default();
                open.set(Some(Open::Ticket(target.clone())));
            }
        >
            {id}
        </a>
    }
}

/// A ticket's summary: its state, where it runs in each environment, its
/// commits, what it waits on and unlocks — then *open ticket*.
#[component]
fn TicketSheet(id: String, nodes: Memo<Vec<Node>>, project: Memo<Option<String>>, open: RwSignal<Option<Open>>) -> impl IntoView {
    let node = {
        let id = id.clone();
        Memo::new(move |_| nodes.with(|ns| ns.iter().find(|n| n.id == id).cloned()))
    };
    let held = deploys::held();
    let in_flight = move |x: &str| nodes.with_untracked(|ns| ns.iter().any(|n| n.id == x));
    let href = crate::links::ticket_href(&id);
    view! {
        <div class="shead">
            <span class="code">{id.clone()}</span>
            {move || node.get().map(|n| view! {
                <span class=format!("tag {}", st_class(n.status.as_str()))>{n.status.as_str()}</span>
                <span class="lbl">{n.epic.clone()}</span>
            })}
            <button class="cbtn x" type="button" aria-label="close" on:click=move |_| open.set(None)>"×"</button>
        </div>
        <div class="sbody">
            {move || match node.get() {
                None => view! { <p class="none">"Not in flight any more — it shipped, or moved out."</p> }.into_any(),
                Some(n) => {
                    let p = project.get().unwrap_or_default();
                    let envs = held.with(|h| h.as_ref().map(|h| h.of_project(&p)).unwrap_or_default());
                    let marks = deploys::marks_of(&n.id);
                    let wheres = envs.into_iter().map(|e| {
                        let m = marks.iter().find(|m| m.project == p && m.env == e.name);
                        let from = e.from.clone();
                        let (cls, words) = match m {
                            Some(m) if m.stage.is_some() => {
                                let s = m.stage.expect("just matched");
                                (format!("envmark {}", match s {
                                    menger_tickets_api::deployments::DeliveryStage::ReadyFor => "ready",
                                    menger_tickets_api::deployments::DeliveryStage::Deploying => "deploying",
                                    menger_tickets_api::deployments::DeliveryStage::Failed => "failed",
                                    menger_tickets_api::deployments::DeliveryStage::Waiting => "waiting",
                                }), deploys::stage_words(s, &e.name, from.as_deref()))
                            }
                            Some(m) if !m.behind.is_empty() => ("envmark behind".to_string(), deploys::behind_words(&e.name, m.version.as_deref(), &m.behind)),
                            Some(m) => ("envmark".to_string(), format!("running since {}", deploys::since(m.since))),
                            None => ("envmark off".to_string(), "not there".to_string()),
                        };
                        view! {
                            <dt><span class=cls>{e.name.clone()}</span></dt>
                            <dd>{words}</dd>
                        }
                    }).collect_view();
                    let commits = (!n.commits.is_empty()).then(|| view! {
                        <dt>"commits"</dt>
                        <dd>{n.commits.iter().map(|c| view! { <span class="rev">{deploys::short(c)}</span>" " }).collect_view()}</dd>
                    });
                    let after = (!n.blocked_by.is_empty()).then(|| view! {
                        <dt>"after"</dt>
                        <dd>{n.blocked_by.iter().map(|b| { let f = in_flight(b); view! { {chip(b.clone(), f, open)}" " } }).collect_view()}</dd>
                    });
                    let unlocks = (!n.unlocks.is_empty()).then(|| view! {
                        <dt>"unlocks"</dt>
                        <dd>{n.unlocks.iter().map(|b| { let f = in_flight(b); view! { {chip(b.clone(), f, open)}" " } }).collect_view()}</dd>
                    });
                    view! {
                        <h3>{n.title.clone()}</h3>
                        <div class="sline"><span class=format!("ns {}", n.state.class())>{n.state_words()}</span></div>
                        <dl class="kv">
                            {wheres}
                            {commits}
                            {after}
                            {unlocks}
                        </dl>
                    }.into_any()
                }
            }}
        </div>
        <div class="sfoot">
            <a class="btn primary" href=href on:click=move |_| open.set(None)>"open ticket ↗"</a>
            <button class="btn" type="button" on:click=move |_| open.set(None)>"close"</button>
        </div>
    }
}

/// An area's summary: one line per ticket, what it is, the ticket cited at
/// its end, its state beside it.
#[component]
fn AreaSheet(code: String, clusters: Memo<Vec<Cluster>>, open: RwSignal<Option<Open>>) -> impl IntoView {
    let cluster = {
        let code = code.clone();
        Memo::new(move |_| clusters.with(|cs| cs.iter().find(|c| c.code == code).cloned()))
    };
    let lens = crate::lens::use_lens();
    let on_board = {
        let code = code.clone();
        move |_| {
            if let Ok(f) = menger_tickets_api::filter::Filter::parse(&format!("under {code}")) {
                lens.apply(crate::lens::Level::Global, Some(crate::saved::chosen(f)));
                crate::dom::window().location().set_hash("#/").ok();
            }
        }
    };
    view! {
        <div class="shead">
            <span class="code">{code.clone()}</span>
            {move || cluster.get().map(|c| {
                let (loud, words) = c.summary();
                view! { <span class=format!("ns {}", loud.class())>{words}</span> }
            })}
            <button class="cbtn x" type="button" aria-label="close" on:click=move |_| open.set(None)>"×"</button>
        </div>
        <div class="sbody">
            {move || match cluster.get() {
                None => view! { <p class="none">"Nothing of it in flight any more."</p> }.into_any(),
                Some(c) => view! {
                    <ul class="wl">
                        {c.nodes.into_iter().map(|n| {
                            let id = n.id.clone();
                            view! {
                                <li data-ticket-id=n.id.clone()>
                                    <span class="w">{n.title.clone()}</span>
                                    <span class=format!("ns {}", n.state.class())>{n.state_words()}</span>
                                    {chip(id, true, open)}
                                </li>
                            }
                        }).collect_view()}
                    </ul>
                }.into_any(),
            }}
        </div>
        <div class="sfoot">
            <button class="btn" type="button" title="filter every view by this area" on:click=on_board>"on the board ›"</button>
            <button class="btn" type="button" on:click=move |_| open.set(None)>"close"</button>
        </div>
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_pages_query_and_hash() {
        assert_eq!(to_of("#/deploy/MENGER?to=app-prod"), Some("app-prod".into()));
        assert_eq!(to_of("#/deploy/MENGER?g=x&to=prod"), Some("prod".into()));
        assert_eq!(to_of("#/deploy?to="), None);
        assert_eq!(to_of("#/deploy"), None);
        assert_eq!(href("MENGER", Some("prod"), None), "#/deploy/MENGER?to=prod");
        assert_eq!(href("MENGER", None, None), "#/deploy/MENGER");
        assert_eq!(href("MENGER", Some("prod"), Some(State::Ready)), "#/deploy/MENGER?to=prod&stage=ready");
        assert_eq!(href("MENGER", None, Some(State::Failed)), "#/deploy/MENGER?stage=failed");
        assert_eq!(stage_of("#/deploy?to=prod&stage=ready"), Some(State::Ready));
        assert_eq!(stage_of("#/deploy?stage=nope"), None);
        assert_eq!(stage_of("#/deploy"), None);
    }
}
