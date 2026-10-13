//! FILTER-27: the When category's pane in the filter menu — every form of
//! the language's time clauses built by clicks (src/when.rs decides; this
//! draws).
//!
//! - **The rows**: Created, Moved into ▸, Moved out of ▸, In status for,
//!   Changed, Hasn't changed (FILTER-30: `not touched`, its windows 1d · 3d
//!   · 1w · 2w · 1mo and Custom… for since). A row opens its first choice — Moved into / out of a status,
//!   In status for a length, the others a window — and the choice makes the
//!   pill; the pane then edits it.
//! - **Editing a clause**: its status (one, for Moved into / out of; any,
//!   categories or statuses, for In status for), its window (presets, ever,
//!   and Custom…), its length (over · under, presets, custom), and who
//!   (anyone, me, the system, the members, or a name typed). A pill's
//!   segment opens the pane on that part alone.
//! - **Custom… — the window editor**: within (a number and a unit), since
//!   or before a moment — a day on the calendar with a time, or a length
//!   ago — or between two moments, a range on one calendar with a time for
//!   each end. "now" and "start of today" are a click each, and the zone
//!   the moments are read in is shown (the filter is sent with it).
//! - **DEPLOY-13: Deployed** — the filter menu's own category, this pane
//!   with two rows: Deployed to ▸ and Not deployed to ▸ an environment (a
//!   chip for each the app holds, or one typed), then its window; a
//!   deployed pill's environment segment reopens the environments.

use leptos::prelude::*;
use menger_tickets_api::filter::{HeldOf, Unit, When, Who, Window};
use menger_tickets_api::Status;
use wasm_bindgen_futures::spawn_local;

use crate::calendar::{now_local, reader_h12, today, Calendar, Day, TimeSelect};
use crate::anyof::{self, Slot};
use crate::filter::Pop;
use crate::lens::{Lens, Level};
use crate::when::{self, Kind, Part, Timed};

/// Where the When pane is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Step {
    /// The five rows.
    Root,
    /// A new clause of this kind: its first choice.
    New(Kind),
    /// The clause at `at` among the level's groups, or among the members
    /// of the group there (FILTER-31) — `only` one part of it (opened from a
    /// pill's segment).
    Edit { at: Slot, only: Option<Part> },
}

/// Focus kept inside the menu once the control that had it is gone (the
/// window editor's Apply): on the pane's Done — so Escape still closes it.
fn keep_focus(level: Level) {
    request_animation_frame(move || {
        let Some(doc) = crate::dom::window().document() else { return };
        let id = crate::filter::pop_id(level);
        let inside = doc.active_element().is_some_and(|a| a.closest(&format!("#{id}")).ok().flatten().is_some());
        if inside {
            return;
        }
        let done = doc.query_selector(&format!("#{id} .wdone .wbtn")).ok().flatten();
        if let Some(el) = done.and_then(|e| wasm_bindgen::JsCast::dyn_into::<web_sys::HtmlElement>(e).ok()) {
            let _ = el.focus();
        }
    });
}

/// The time clause at `at` among `level`'s groups (FILTER-31: or a
/// group's members).
fn clause_at(lens: Lens, level: Level, at: Slot) -> Option<Timed> {
    lens.kept(level).and_then(|k| anyof::time_in(&k.filter.groups, at))
}

/// `t` written at `at` (replacing that clause) or — `None` — added: to the
/// group the menu adds to (FILTER-31's Or…), else last; where it now is.
fn put(lens: Lens, pop: Pop, at: Option<Slot>, t: &Timed) -> Option<Slot> {
    let into = pop.into.get_untracked();
    match lens.edit_groups(pop.level, |gs| anyof::put_time(gs, into, at, t)) {
        Ok(slot) => Some(slot),
        Err(why) => {
            pop.refusal.set(Some(why));
            None
        }
    }
}

/// The When pane, as `pop`'s step says.
pub(crate) fn pane(lens: Lens, pop: Pop) -> impl IntoView {
    let level = pop.level;
    move || match pop.when.get() {
        Step::Root => root_view(pop).into_any(),
        Step::New(kind) => new_view(lens, pop, kind).into_any(),
        Step::Edit { at, only } => match untrack(|| clause_at(lens, level, at)) {
            Some(_) => edit_view(lens, pop, at, only).into_any(),
            None => root_view(pop).into_any(),
        },
    }
}

fn root_view(pop: Pop) -> impl IntoView {
    // DEPLOY-13: under Deployed, its two rows; else When's six.
    let deployed = untrack(|| pop.open_cat_tracked()) == Some(crate::filter::Cat::Deployed);
    let rows: Vec<Kind> = if deployed { Kind::DEPLOYED.to_vec() } else { Kind::ALL.to_vec() };
    view! {
        <div class="when-rows wform" role="menu" aria-label=if deployed { "deployed" } else { "when" }>
            {rows.into_iter().map(|k| view! {
                <button
                    type="button"
                    class="frow cat when-row"
                    role="menuitem"
                    data-when=k.key()
                    on:mousedown=|ev| ev.prevent_default()
                    on:click=move |_| pop.when.set(Step::New(k))
                >
                    <span class="flabel">{k.label()}</span>
                    <span class="arrow" aria-hidden="true">"▸"</span>
                </button>
            }).collect_view()}
        </div>
    }
}

/// A chip: a choice, `on` when it is the clause's.
fn chip(label: String, data: String, on: impl Fn() -> bool + Send + Sync + 'static, click: impl Fn() + 'static) -> impl IntoView {
    let on = Signal::derive(on);
    view! {
        <button
            type="button"
            class="wchip"
            class:on=move || on.get()
            aria-pressed=move || on.get().to_string()
            data-v=data
            on:mousedown=|ev| ev.prevent_default()
            on:click=move |_| click()
        >
            {label}
        </button>
    }
}

fn status_label(s: Status) -> String {
    s.as_str().to_string()
}

/// A new clause's first choice.
fn new_view(lens: Lens, pop: Pop, kind: Kind) -> impl IntoView {
    let start = move |t: Option<Timed>| {
        if let Some(at) = t.and_then(|t| put(lens, pop, None, &t)) {
            pop.when.set(Step::Edit { at, only: None });
        }
    };
    let body = match kind {
        Kind::Into | Kind::OutOf => view! {
            <div class="wsec" data-part="status">
                <div class="whead">"status"</div>
                <div class="wchips">
                    {when::STATUSES.into_iter().map(|s| chip(status_label(s), s.as_str().into(), || false, move || start(when::new_clause(kind, Some(s), None)))).collect_view()}
                </div>
            </div>
        }
        .into_any(),
        // DEPLOY-13: an environment first — every one the app holds, or
        // one typed.
        Kind::Deployed | Kind::NotDeployed => {
            let not = kind == Kind::NotDeployed;
            env_view(|| None, move |env: String| start(when::new_deployed(&env, not, None))).into_any()
        }
        Kind::Held => view! {
            <div class="wsec" data-part="length">
                <div class="whead">"in its status for over"</div>
                <div class="wchips">
                    {when::LENGTH_PRESETS.into_iter().map(|(n, u)| {
                        let t = when::new_clause(Kind::Held, None, None).map(|t| when::with_length(&t, true, n, u));
                        chip(when::length_words(n, u), format!("{n}{}", u.suffix()), || false, move || start(t.clone()))
                    }).collect_view()}
                    {chip("custom…".into(), "custom".into(), || false, move || start(when::new_clause(Kind::Held, None, None)))}
                </div>
            </div>
        }
        .into_any(),
        _ => view! {
            <div class="wsec" data-part="window">
                <div class="whead">{if kind == Kind::Untouched { "not touched within" } else { "window" }}</div>
                <div class="wchips">
                    {kind.presets().iter().copied().map(|p| {
                        let w = when::window_of(p);
                        let words = w.as_ref().map(|w| kind.preset_words(w, false)).unwrap_or_default();
                        chip(words, p.into(), || false, move || start(when::new_clause(kind, None, w.clone())))
                    }).collect_view()}
                    {chip("custom…".into(), "custom".into(), || false, move || {
                        if let Some(at) = when::new_clause(kind, None, None).and_then(|t| put(lens, pop, None, &t)) {
                            pop.when.set(Step::Edit { at, only: Some(Part::Window) });
                            pop.custom.set(true);
                        }
                    })}
                </div>
            </div>
        }
        .into_any(),
    };
    view! {
        <div class="when-pane wform" data-when=kind.key()>
            <button type="button" class="wback" on:mousedown=|ev| ev.prevent_default() on:click=move |_| pop.when.set(Step::Root)>
                <span class="glyph">"‹"</span>
                <span class="flabel">{kind.label()}</span>
            </button>
            {body}
        </div>
    }
}

/// Editing the clause at `at`.
fn edit_view(lens: Lens, pop: Pop, at: Slot, only: Option<Part>) -> impl IntoView {
    let level = pop.level;
    let h12 = reader_h12();
    let current = Memo::new(move |_| clause_at(lens, level, at));
    let set = move |f: &dyn Fn(&Timed) -> Timed| {
        if let Some(t) = current.get_untracked() {
            put(lens, pop, Some(at), &f(&t));
        }
    };
    let kind = untrack(|| current.get().map(|t| Kind::of(&t.h.event))).unwrap_or(Kind::Changed);
    let shows = move |p: Part| only.is_none_or(|o| o == p);
    let status_sec = (shows(Part::Status) && kind.needs_status()).then(|| {
        view! {
            <div class="wsec" data-part="status">
                <div class="whead">"status"</div>
                <div class="wchips">
                    {when::STATUSES.into_iter().map(|s| {
                        let on = move || current.get().is_some_and(|t| matches!(t.h.event, menger_tickets_api::filter::Event::Entered(x) | menger_tickets_api::filter::Event::Left(x) if x == s));
                        chip(status_label(s), s.as_str().into(), on, move || set(&|t| when::with_status(t, s)))
                    }).collect_view()}
                </div>
            </div>
        }
    });
    let env_sec = (shows(Part::Env) && kind.needs_env()).then(|| {
        env_view(
            move || current.get().and_then(|t| when::env_of(&t).map(str::to_string)),
            move |env: String| set(&|t| when::with_env(t, &env)),
        )
    });
    let scope_sec = (shows(Part::Status) && kind == Kind::Held).then(|| {
        let of = move || current.get().and_then(|t| when::held(&t).map(|h| h.of.clone())).unwrap_or(HeldOf::Any);
        view! {
            <div class="wsec" data-part="status">
                <div class="whead">"in"</div>
                <div class="wchips">
                    {chip("any status".into(), "any".into(), move || of() == HeldOf::Any, move || set(&|t| when::with_scope(t, HeldOf::Any)))}
                </div>
                <div class="whead">"categories"</div>
                <div class="wchips">
                    {when::CATEGORIES.into_iter().map(|c| {
                        let on = move || matches!(of(), HeldOf::Categories(v) if v.contains(&c));
                        chip(c.as_str().into(), c.as_str().into(), on, move || {
                            let next = when::toggle_category(&of(), c);
                            set(&|t| when::with_scope(t, next.clone()))
                        })
                    }).collect_view()}
                </div>
                <div class="whead">"statuses"</div>
                <div class="wchips">
                    {when::STATUSES.into_iter().filter(|s| *s != Status::Doc).map(|s| {
                        let on = move || matches!(of(), HeldOf::Statuses(v) if v.contains(&s));
                        chip(status_label(s), s.as_str().into(), on, move || {
                            let next = when::toggle_status(&of(), s);
                            set(&|t| when::with_scope(t, next.clone()))
                        })
                    }).collect_view()}
                </div>
            </div>
        }
    });
    let length_sec = (shows(Part::Length) && kind == Kind::Held).then(|| length_view(current, set));
    let window_sec = (shows(Part::Window) && kind.windowed()).then(|| {
        let custom = pop.custom;
        if untrack(|| current.get().is_some_and(|t| when::custom_window(&t))) && only == Some(Part::Window) {
            custom.set(true);
        }
        view! {
            <div class="wsec" data-part="window">
                <div class="whead">"window"</div>
                <div class="wchips">
                    {kind.has_ever().then(|| chip("ever".into(), "ever".into(), move || current.get().is_some_and(|t| t.h.window.is_none()), move || set(&|t| when::with_window(t, None))))}
                    {kind.presets().iter().copied().map(|p| {
                        let w = when::window_of(p);
                        let words = w.as_ref().map(|w| kind.preset_words(w, h12)).unwrap_or_default();
                        let on = move || current.get().is_some_and(|t| when::has_window(&t, p));
                        chip(words, p.into(), on, move || {
                            custom.set(false);
                            set(&|t| when::with_window(t, w.clone()))
                        })
                    }).collect_view()}
                    {chip("custom…".into(), "custom".into(), move || custom.get() || current.get().is_some_and(|t| when::custom_window(&t)), move || custom.update(|c| *c = !*c))}
                </div>
                {move || custom.get().then(|| {
                    let initial = untrack(|| current.get().and_then(|t| t.h.window));
                    let apply = Callback::new(move |w: Window| {
                        set(&|t| when::with_window(t, Some(w.clone())));
                        custom.set(false);
                        keep_focus(level);
                    });
                    view! { <WindowEditor initial apply h12 /> }
                })}
            </div>
        }
    });
    let who_sec = (shows(Part::Who) && kind.has_who()).then(|| who_view(current, set));
    let said = move || current.get().map(|t| when::words(&t, h12)).unwrap_or_default();
    view! {
        <div class="when-pane wform" data-when=kind.key() data-at=at.at data-member=at.member.map(|m| m.to_string())>
            <button type="button" class="wback" title="another time clause" on:mousedown=|ev| ev.prevent_default() on:click=move |_| pop.when.set(Step::Root)>
                <span class="glyph">"‹"</span>
                <span class="flabel">"another time clause"</span>
            </button>
            <div class="wsaid" aria-live="polite">{said}</div>
            {status_sec}
            {env_sec}
            {scope_sec}
            {window_sec}
            {length_sec}
            {who_sec}
            <div class="wdone">
                <button type="button" class="wbtn" on:click=move |_| pop.close(true)>"Done"</button>
            </div>
        </div>
    }
}

/// In status for's length: over · under, the presets, a number and a unit.
fn length_view(current: Memo<Option<Timed>>, set: impl Fn(&dyn Fn(&Timed) -> Timed) + Copy + Send + Sync + 'static) -> impl IntoView {
    let held = move || current.get().and_then(|t| when::held(&t).cloned());
    let over = move || held().is_none_or(|h| h.over);
    let n = RwSignal::new(untrack(|| held().map_or(1, |h| h.n)));
    let unit = RwSignal::new(untrack(|| held().map_or(Unit::Week, |h| h.unit)));
    view! {
        <div class="wsec" data-part="length">
            <div class="whead">"held"</div>
            <div class="wchips">
                {chip("over".into(), "over".into(), over, move || if let Some(h) = held() { set(&|t| when::with_length(t, true, h.n, h.unit)) })}
                {chip("under".into(), "under".into(), move || !over(), move || if let Some(h) = held() { set(&|t| when::with_length(t, false, h.n, h.unit)) })}
            </div>
            <div class="wchips">
                {when::LENGTH_PRESETS.into_iter().map(|(pn, pu)| {
                    let on = move || held().is_some_and(|h| h.n == pn && h.unit == pu);
                    chip(when::length_words(pn, pu), format!("{pn}{}", pu.suffix()), on, move || {
                        n.set(pn);
                        unit.set(pu);
                        set(&|t| when::with_length(t, over(), pn, pu))
                    })
                }).collect_view()}
            </div>
            <div class="wrow">
                <span class="whint">"custom"</span>
                <NumberUnit n unit />
                <button type="button" class="wbtn" data-apply="length" on:click=move |_| set(&|t| when::with_length(t, over(), n.get_untracked(), unit.get_untracked()))>
                    "Set"
                </button>
            </div>
        </div>
    }
}

/// DEPLOY-13: the environment — a chip for each one the app holds
/// (src/deploys.rs: each name, then each project's own when there are
/// several), `on` when it is the clause's; or one typed.
fn env_view(on: impl Fn() -> Option<String> + Copy + Send + Sync + 'static, pick: impl Fn(String) + Copy + Send + Sync + 'static) -> impl IntoView {
    crate::deploys::want();
    let held = crate::deploys::held();
    let choices = Memo::new(move |_| held.with(|h| h.as_ref().map(crate::deploys::Held::choices).unwrap_or_default()));
    let typed = RwSignal::new(String::new());
    let problem = RwSignal::new(None::<String>);
    let take = move || {
        let name = typed.get_untracked().trim().to_string();
        if name.is_empty() {
            return;
        }
        match menger_tickets_api::deployments::environment_of(&name) {
            Ok(env) => {
                problem.set(None);
                typed.set(String::new());
                pick(env);
            }
            Err(why) => problem.set(Some(why)),
        }
    };
    view! {
        <div class="wsec" data-part="env">
            <div class="whead">"environment"</div>
            <div class="wchips">
                {move || {
                    let cs = choices.get();
                    if cs.is_empty() {
                        return view! { <span class="whint">"no deployment recorded yet"</span> }.into_any();
                    }
                    cs.into_iter().map(|e| {
                        let v = e.clone();
                        let mine = e.clone();
                        chip(e.clone(), e, move || on().as_deref() == Some(mine.as_str()), move || pick(v.clone()))
                    }).collect_view().into_any()
                }}
                {move || on().filter(|e| !choices.get().contains(e)).map(|e| chip(e.clone(), e, || true, || {}))}
            </div>
            <div class="wrow">
                <input
                    class="win wenv"
                    type="text"
                    autocomplete="off"
                    spellcheck="false"
                    aria-label="another environment, by name"
                    placeholder="another environment…"
                    prop:value=move || typed.get()
                    on:input=move |ev| typed.set(event_target_value(&ev))
                    on:keydown=move |ev: web_sys::KeyboardEvent| {
                        if crate::bindings::is(&ev, crate::bindings::Context::Field, "form.submit") {
                            ev.prevent_default();
                            ev.stop_propagation();
                            take();
                        }
                    }
                />
                <button type="button" class="wbtn" data-apply="env" on:click=move |_| take()>"Set"</button>
            </div>
            {move || problem.get().map(|p| view! { <p class="werr" role="alert">{p}</p> })}
        </div>
    }
}

/// Who: anyone, me, the system, the members, or a name typed.
fn who_view(current: Memo<Option<Timed>>, set: impl Fn(&dyn Fn(&Timed) -> Timed) + Copy + Send + Sync + 'static) -> impl IntoView {
    let by = move || current.get().and_then(|t| t.h.by);
    let members = RwSignal::new(Vec::<String>::new());
    if let Some(client) = crate::session::members_client() {
        spawn_local(async move {
            if let Ok(evs) = client.list_all(None).await {
                let mut out: Vec<String> = Vec::new();
                for ev in evs {
                    if let menger_tickets_api::MembersEvent::Grant { grant } = ev {
                        if !out.contains(&grant.principal) {
                            out.push(grant.principal);
                        }
                    }
                }
                members.try_set(out);
            }
        });
    }
    let typed = RwSignal::new(String::new());
    let take = move || {
        let name = typed.get_untracked().trim().to_string();
        if !name.is_empty() {
            let who = Who::read(&name);
            set(&|t| when::with_by(t, Some(who.clone())));
            typed.set(String::new());
        }
    };
    view! {
        <div class="wsec" data-part="who">
            <div class="whead">"by"</div>
            <div class="wchips">
                {chip("anyone".into(), "anyone".into(), move || by().is_none(), move || set(&|t| when::with_by(t, None)))}
                {chip("me".into(), "me".into(), move || by() == Some(Who::Me), move || set(&|t| when::with_by(t, Some(Who::Me))))}
                {chip("the system".into(), "system".into(), move || by() == Some(Who::System), move || set(&|t| when::with_by(t, Some(Who::System))))}
                {move || members.get().into_iter().map(|p| {
                    let who = Who::Principal(p.clone());
                    let w2 = who.clone();
                    chip(p.clone(), p, move || by().as_ref() == Some(&who), move || set(&|t| when::with_by(t, Some(w2.clone()))))
                }).collect_view()}
                {move || match by() {
                    Some(Who::Principal(p)) if !members.get().contains(&p) => Some(chip(p.clone(), p, || true, || {})),
                    _ => None,
                }}
            </div>
            <div class="wrow">
                <input
                    class="win wwho"
                    type="text"
                    autocomplete="off"
                    spellcheck="false"
                    aria-label="someone else, by name or principal"
                    placeholder="someone else…"
                    prop:value=move || typed.get()
                    on:input=move |ev| typed.set(event_target_value(&ev))
                    on:keydown=move |ev: web_sys::KeyboardEvent| {
                        if crate::bindings::is(&ev, crate::bindings::Context::Field, "form.submit") {
                            ev.prevent_default();
                            ev.stop_propagation();
                            take();
                        }
                    }
                />
                <button type="button" class="wbtn" data-apply="who" on:click=move |_| take()>"Set"</button>
            </div>
        </div>
    }
}

/// A length typed: a number and a unit.
#[component]
fn NumberUnit(n: RwSignal<u32>, unit: RwSignal<Unit>) -> impl IntoView {
    view! {
        <input
            class="win wnum"
            type="text"
            inputmode="numeric"
            aria-label="how many"
            prop:value=move || n.get().to_string()
            on:input=move |ev| {
                if let Some(v) = event_target_value(&ev).trim().parse::<u32>().ok().filter(|v| (1..=100_000).contains(v)) {
                    n.set(v);
                }
            }
            on:keydown=move |ev: web_sys::KeyboardEvent| {
                let by = match crate::bindings::hit(&ev, crate::bindings::Context::TimeField) {
                    Some("time.up") => 1i64,
                    Some("time.down") => -1,
                    _ => return,
                };
                ev.prevent_default();
                ev.stop_propagation();
                n.update(|v| *v = (i64::from(*v) + by).clamp(1, 100_000) as u32);
            }
        />
        <span class="wunits" role="radiogroup" aria-label="unit">
            {[Unit::Minute, Unit::Hour, Unit::Day, Unit::Week, Unit::Month].into_iter().map(|u| view! {
                <button
                    type="button"
                    class="wchip wunit"
                    role="radio"
                    class:on=move || unit.get() == u
                    aria-checked=move || (unit.get() == u).to_string()
                    data-v=u.suffix()
                    on:mousedown=|ev| ev.prevent_default()
                    on:click=move |_| unit.set(u)
                >
                    {move || u.word(n.get() != 1)}
                </button>
            }).collect_view()}
        </span>
    }
}

/// The window editor's mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    Within,
    Since,
    Before,
    Between,
}

impl Mode {
    const ALL: [Mode; 4] = [Mode::Within, Mode::Since, Mode::Before, Mode::Between];
    fn word(self) -> &'static str {
        match self {
            Mode::Within => "within",
            Mode::Since => "since",
            Mode::Before => "before",
            Mode::Between => "between",
        }
    }
}

/// One end of a window: a day with an optional time, or a length ago.
#[derive(Clone, Copy)]
struct End {
    day: RwSignal<Option<Day>>,
    timed: RwSignal<bool>,
    time: RwSignal<(u8, u8)>,
    ago: RwSignal<bool>,
    n: RwSignal<u32>,
    unit: RwSignal<Unit>,
}

impl End {
    fn of(w: Option<&When>) -> Self {
        let e = End {
            day: RwSignal::new(None),
            timed: RwSignal::new(false),
            time: RwSignal::new((0, 0)),
            ago: RwSignal::new(false),
            n: RwSignal::new(1),
            unit: RwSignal::new(Unit::Week),
        };
        match w {
            Some(When::At(m)) => {
                e.day.set(Some(Day::of_moment(m)));
                if let Some(t) = m.time {
                    e.timed.set(true);
                    e.time.set(t);
                }
            }
            Some(When::Ago(n, u)) => {
                e.ago.set(true);
                e.n.set(*n);
                e.unit.set(*u);
            }
            None => {}
        }
        e
    }

    fn when(self) -> Option<When> {
        if self.ago.get_untracked() {
            return Some(When::Ago(self.n.get_untracked(), self.unit.get_untracked()));
        }
        let d = self.day.get_untracked()?;
        Some(When::At(d.moment(self.timed.get_untracked().then(|| self.time.get_untracked()))))
    }

    fn now(self) {
        let (d, t) = now_local();
        self.ago.set(false);
        self.day.set(Some(d));
        self.timed.set(true);
        self.time.set(t);
    }

    fn start_of_today(self) {
        self.ago.set(false);
        self.day.set(Some(today()));
        self.timed.set(true);
        self.time.set((0, 0));
    }
}

/// The time of one end, its toggle and its quick picks.
fn end_time(e: End, h12: bool, label: &'static str) -> impl IntoView {
    view! {
        <div class="wrow wend" data-end=label>
            <label class="wcheck">
                <input type="checkbox" prop:checked=move || e.timed.get() on:change=move |ev| e.timed.set(event_target_checked(&ev)) />
                {format!("{label} at a time")}
            </label>
            {move || e.timed.get().then(|| view! { <TimeSelect value=e.time h12 label=format!("{label} time") /> })}
            <button type="button" class="wlink" data-quick="now" on:click=move |_| e.now()>"now"</button>
            <button type="button" class="wlink" data-quick="today" on:click=move |_| e.start_of_today()>"start of today"</button>
        </div>
    }
}

/// Custom…: within, since, before, between — moments on the calendar with a
/// time, or a length ago.
#[component]
fn WindowEditor(initial: Option<Window>, apply: Callback<Window>, h12: bool) -> impl IntoView {
    let (mode, a, b, n, unit) = {
        let (mode, a, b) = match &initial {
            Some(Window::Since(w)) => (Mode::Since, Some(w), None),
            Some(Window::Before(w)) => (Mode::Before, Some(w), None),
            Some(Window::Between(x, y)) => (Mode::Between, Some(x), Some(y)),
            _ => (Mode::Within, None, None),
        };
        let (n, u) = match &initial {
            Some(Window::Within(n, u)) => (*n, *u),
            _ => (1, Unit::Day),
        };
        (RwSignal::new(mode), End::of(a), End::of(b), RwSignal::new(n), RwSignal::new(u))
    };
    let zone = crate::session::tz().unwrap_or_else(|| "UTC".into());
    let problem = RwSignal::new(None::<String>);
    let built = move || -> Result<Window, String> {
        let need = |e: End| e.when().ok_or_else(|| "choose a day on the calendar".to_string());
        Ok(match mode.get_untracked() {
            Mode::Within => Window::Within(n.get_untracked(), unit.get_untracked()),
            Mode::Since => Window::Since(need(a)?),
            Mode::Before => Window::Before(need(a)?),
            Mode::Between => {
                if a.day.get_untracked().is_none() || b.day.get_untracked().is_none() {
                    return Err("choose the first day, then the last".into());
                }
                Window::Between(need(a)?, need(b)?)
            }
        })
    };
    let go = move || match built() {
        // Held to the language: what it writes, it reads.
        Ok(w) if when::window_of(&w.format()).as_ref() == Some(&w) => {
            problem.set(None);
            apply.run(w);
        }
        Ok(w) => problem.set(Some(format!("`{}` is not a window", w.format()))),
        Err(why) => problem.set(Some(why)),
    };
    view! {
        <div class="weditor" role="group" aria-label="a window of your own">
            <div class="wtabs" role="tablist">
                {Mode::ALL.into_iter().map(|m| view! {
                    <button
                        type="button"
                        role="tab"
                        class="wtab"
                        data-mode=m.word()
                        aria-selected=move || (mode.get() == m).to_string()
                        class:on=move || mode.get() == m
                        on:click=move |_| mode.set(m)
                    >
                        {m.word()}
                    </button>
                }).collect_view()}
            </div>
            {move || match mode.get() {
                Mode::Within => view! {
                    <div class="wrow"><span class="whint">"the last"</span><NumberUnit n unit /></div>
                }.into_any(),
                Mode::Since | Mode::Before => view! {
                    <div class="wchips" role="radiogroup" aria-label="a moment">
                        {chip("on a day".into(), "day".into(), move || !a.ago.get(), move || a.ago.set(false))}
                        {chip("a length ago".into(), "ago".into(), move || a.ago.get(), move || a.ago.set(true))}
                    </div>
                    {move || if a.ago.get() {
                        view! { <div class="wrow"><NumberUnit n=a.n unit=a.unit /><span class="whint">"ago"</span></div> }.into_any()
                    } else {
                        view! {
                            <Calendar start=a.day label="the day" />
                            {end_time(a, h12, "the moment")}
                        }.into_any()
                    }}
                }.into_any(),
                Mode::Between => view! {
                    <p class="whint">"Click the first day, then the last."</p>
                    <Calendar start=a.day end=b.day label="the first and last days" />
                    {end_time(a, h12, "from")}
                    {end_time(b, h12, "to")}
                }.into_any(),
            }}
            <div class="wrow wfoot">
                <span class="wzone" title="the zone the moments are read in">{format!("times in {zone}")}</span>
                <button type="button" class="wbtn primary" data-apply="window" on:click=move |_| go()>"Apply"</button>
            </div>
            {move || problem.get().map(|p| view! { <p class="werr" role="alert">{p}</p> })}
        </div>
    }
}
