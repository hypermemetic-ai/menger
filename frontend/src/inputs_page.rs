//! INPUT-6: `#/inputs` — every input across tickets (INPUT-1's "what is
//! waiting on me"), grouped by state, live.
//!
//! **The groups**, in this order: **open** (waiting for an answer), then
//! **answered**, then **closed** (frozen — a decision's DECIDED); the
//! **removed** (their body no longer declares them; history kept) only
//! behind a toggle, last. Inside a group, the store's order: tickets by
//! shortcode and sequence, each ticket's inputs in its body's order.
//!
//! **A row** is one input: its ticket (a link to `#/ticket/<id>`) and its
//! id, its kind, its question, its options with the recommended one marked
//! (and the chosen ones ticked), the recommendation where it says more than
//! that option's label, the current answer — value, rationale, source, time
//! — and its history, folded.
//!
//! **The filter** is the route's query, as the wave board's is (WAVE-12):
//! `#/inputs?sc=INPUT,MNG&state=open,answered&removed` — the shortcodes
//! shown (none named: every one), the states shown (none named: all three),
//! and whether the removed show. No query is the default; a malformed one is
//! the default. Read once when the page mounts (a new hash mounts a new
//! page) and rewritten in place (`replaceState`) on every change, so a
//! reload or a pasted URL restores it.
//!
//! **Live.** The rows are the app's copy of the inputs (src/inputs.rs),
//! which the page asks for ([`inputs::want`]) and `watch` keeps — an
//! `input_changed` replaces the input, so its row leaves one group and
//! enters another as it happens. A row whose input did not change keeps its
//! DOM (an opened history stays open).
//!
//! What decides — the filter and its query, the groups, the facets' counts,
//! a row's words — is pure and pinned by native tests.

use std::collections::{BTreeMap, BTreeSet};

use leptos::prelude::*;
use menger_tickets_api::{AnswerSource, Input, InputAction, InputKind, InputOption, InputState, InputValue};

use crate::inputs::{self, ticket_order};
use crate::{dom, fmt_relative, fmt_unix, lens, now_unix};

/// The states the filter chooses among, in the page's order; `removed` is
/// the toggle's.
pub(crate) const SHOWN: [InputState; 3] = [InputState::Open, InputState::Answered, InputState::Closed];

/// Every group the page can show, in its order.
const GROUPS: [InputState; 4] = [InputState::Open, InputState::Answered, InputState::Closed, InputState::Removed];

/// The shortcode an input's ticket is in.
pub(crate) fn shortcode_of(ticket: &str) -> &str {
    ticket_order(ticket).0
}

// ---------------------------------------------------------------------
// The filter

/// What the page shows — held in the route's query.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Filter {
    /// Upper case; empty: every shortcode.
    pub(crate) shortcodes: BTreeSet<String>,
    /// A subset of [`SHOWN`], in its order; empty: all three.
    pub(crate) states: Vec<InputState>,
    /// Whether the removed show (their own group, last).
    pub(crate) removed: bool,
}

impl Filter {
    /// Whether the group `state` shows.
    pub(crate) fn shows(&self, state: InputState) -> bool {
        match state {
            InputState::Removed => self.removed,
            s => self.states.is_empty() || self.states.contains(&s),
        }
    }

    /// Whether an input of `ticket` passes the shortcode filter.
    pub(crate) fn admits_ticket(&self, ticket: &str) -> bool {
        self.shortcodes.is_empty() || self.shortcodes.contains(shortcode_of(ticket))
    }

    /// Whether `input` shows.
    pub(crate) fn admits(&self, input: &Input) -> bool {
        self.shows(input.state) && self.admits_ticket(&input.ticket)
    }

    /// `sc` in the shortcode filter, or out of it.
    pub(crate) fn toggle_shortcode(&mut self, sc: &str) {
        let sc = sc.to_ascii_uppercase();
        if !self.shortcodes.remove(&sc) {
            self.shortcodes.insert(sc);
        }
    }

    /// `state` (one of [`SHOWN`]) in the state filter, or out of it — kept
    /// in [`SHOWN`]'s order.
    pub(crate) fn toggle_state(&mut self, state: InputState) {
        if !SHOWN.contains(&state) {
            return;
        }
        let mut set: Vec<InputState> = self.states.clone();
        if let Some(at) = set.iter().position(|s| *s == state) {
            set.remove(at);
        } else {
            set.push(state);
        }
        self.states = SHOWN.into_iter().filter(|s| set.contains(s)).collect();
    }

    /// The filter as the route's query (no `?`): `sc=A,B&state=open,closed&
    /// removed`, each part only when it narrows — the default is no query.
    pub(crate) fn to_query(&self) -> String {
        let mut parts = Vec::new();
        if !self.shortcodes.is_empty() {
            parts.push(format!("sc={}", self.shortcodes.iter().cloned().collect::<Vec<_>>().join(",")));
        }
        if !self.states.is_empty() {
            parts.push(format!("state={}", self.states.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(",")));
        }
        if self.removed {
            parts.push("removed".into());
        }
        parts.join("&")
    }

    /// The route's query as a filter — the default for no query, and for a
    /// malformed one (an unknown key, a key twice, no values, a shortcode
    /// that is not one, a state outside [`SHOWN`]).
    pub(crate) fn parse_query(q: &str) -> Self {
        Self::try_parse(q).unwrap_or_default()
    }

    fn try_parse(q: &str) -> Option<Self> {
        let q = q.trim().trim_start_matches('?');
        let mut f = Filter::default();
        let mut seen = BTreeSet::new();
        for pair in q.split('&').filter(|p| !p.is_empty()) {
            let (key, value) = pair.split_once('=').map_or((pair, None), |(k, v)| (k, Some(v)));
            if !seen.insert(key) {
                return None;
            }
            match (key, value) {
                ("sc", Some(v)) => {
                    for sc in v.split(',') {
                        if sc.is_empty() || !sc.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') {
                            return None;
                        }
                        f.shortcodes.insert(sc.to_ascii_uppercase());
                    }
                }
                ("state", Some(v)) => {
                    let named: Vec<InputState> = v.split(',').map(|s| s.parse().ok()).collect::<Option<_>>()?;
                    if named.iter().any(|s| !SHOWN.contains(s)) {
                        return None;
                    }
                    f.states = SHOWN.into_iter().filter(|s| named.contains(s)).collect();
                }
                ("removed", None | Some("1")) => f.removed = true,
                _ => return None,
            }
        }
        Some(f)
    }
}

// ---------------------------------------------------------------------
// The groups and the facets

/// One group of the page: a state and its inputs, in the store's order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Group {
    pub(crate) state: InputState,
    pub(crate) inputs: Vec<Input>,
}

/// The page's groups — each state `f` shows, in the page's order (open,
/// answered, closed, then removed), with the inputs `f` admits; a group
/// with none is still a group (its heading says 0).
pub(crate) fn groups(list: &[Input], f: &Filter) -> Vec<Group> {
    GROUPS
        .into_iter()
        .filter(|s| f.shows(*s))
        .map(|state| Group {
            state,
            inputs: list.iter().filter(|i| i.state == state && f.admits(i)).cloned().collect(),
        })
        .collect()
}

/// What the filter bar offers, with counts.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Facets {
    /// Every shortcode an input is in, and any the filter names, in order —
    /// each with the inputs of it the state filter shows.
    pub(crate) shortcodes: Vec<(String, usize)>,
    /// Each of [`SHOWN`], with the inputs in it the shortcode filter admits.
    pub(crate) states: Vec<(InputState, usize)>,
    /// The removed inputs the shortcode filter admits.
    pub(crate) removed: usize,
}

pub(crate) fn facets(list: &[Input], f: &Filter) -> Facets {
    let mut sc: BTreeMap<String, usize> = f.shortcodes.iter().map(|s| (s.clone(), 0)).collect();
    for i in list {
        let n = sc.entry(shortcode_of(&i.ticket).to_string()).or_default();
        if f.shows(i.state) {
            *n += 1;
        }
    }
    let admitted = |state: InputState| list.iter().filter(|i| i.state == state && f.admits_ticket(&i.ticket)).count();
    Facets {
        shortcodes: sc.into_iter().collect(),
        states: SHOWN.into_iter().map(|s| (s, admitted(s))).collect(),
        removed: admitted(InputState::Removed),
    }
}

// ---------------------------------------------------------------------
// A row

/// One option as a row shows it.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct Opt {
    pub(crate) label: String,
    /// The value, where it is not the label.
    pub(crate) value: Option<String>,
    pub(crate) recommended: bool,
    /// The current answer names it.
    pub(crate) chosen: bool,
}

/// One answer, or one history entry, as a row shows it.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct Said {
    pub(crate) action: InputAction,
    /// The value in words (an `answered`'s; none for the other actions).
    pub(crate) value: Option<String>,
    pub(crate) rationale: Option<String>,
    pub(crate) source: AnswerSource,
    pub(crate) at_unix: i64,
}

/// One input as the page shows it — its key, too: a row whose input did
/// not change keeps its DOM.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct Row {
    pub(crate) ticket: String,
    pub(crate) id: String,
    pub(crate) kind: InputKind,
    pub(crate) state: InputState,
    pub(crate) question: Option<String>,
    pub(crate) options: Vec<Opt>,
    /// The recommendation, where it says more than the recommended
    /// option's label.
    pub(crate) recommendation: Option<String>,
    pub(crate) answer: Option<Said>,
    /// Oldest first.
    pub(crate) history: Vec<Said>,
}

/// A value in words: a choice's option by its label (by its value, if the
/// body no longer declares it — an answer is never rewritten); a multi's,
/// joined, or "none ticked"; a text reply as it is.
pub(crate) fn value_words(options: &[InputOption], v: &InputValue) -> String {
    let label = |value: &str| {
        options.iter().find(|o| o.value == value).map_or_else(|| value.to_string(), |o| o.label.clone())
    };
    match v {
        InputValue::Choice { option } => label(option),
        InputValue::Multi { options: chosen } if chosen.is_empty() => "none ticked".into(),
        InputValue::Multi { options: chosen } => chosen.iter().map(|o| label(o)).collect::<Vec<_>>().join(", "),
        InputValue::Text { text } => text.clone(),
    }
}

/// The values the answer names.
fn chosen(v: Option<&InputValue>) -> Vec<&str> {
    match v {
        Some(InputValue::Choice { option }) => vec![option.as_str()],
        Some(InputValue::Multi { options }) => options.iter().map(String::as_str).collect(),
        _ => Vec::new(),
    }
}

pub(crate) fn row(i: &Input) -> Row {
    let picked = chosen(i.answer.as_ref().map(|a| &a.value));
    let options = i
        .options
        .iter()
        .map(|o| Opt {
            label: o.label.clone(),
            value: (o.value != o.label).then(|| o.value.clone()),
            recommended: o.recommended,
            chosen: picked.contains(&o.value.as_str()),
        })
        .collect();
    let recommendation = i
        .recommendation
        .clone()
        .filter(|r| !r.trim().is_empty() && !i.options.iter().any(|o| o.recommended && o.label == *r));
    let answer = i.answer.as_ref().map(|a| Said {
        action: InputAction::Answered,
        value: Some(value_words(&i.options, &a.value)),
        rationale: a.rationale.clone(),
        source: a.source,
        at_unix: a.at_unix,
    });
    let history = i
        .history
        .iter()
        .map(|e| Said {
            action: e.action,
            value: e.value.as_ref().map(|v| value_words(&i.options, v)),
            rationale: e.rationale.clone(),
            source: e.source,
            at_unix: e.at_unix,
        })
        .collect();
    Row {
        ticket: i.ticket.clone(),
        id: i.id.clone(),
        kind: i.kind,
        state: i.state,
        question: i.question.clone(),
        options,
        recommendation,
        answer,
        history,
    }
}

/// A group's heading words: "open", "answered", "closed", "removed".
pub(crate) fn group_title(s: InputState) -> &'static str {
    s.as_str()
}

/// What a group with no rows says.
pub(crate) fn group_empty(s: InputState) -> &'static str {
    match s {
        InputState::Open => "Nothing is waiting for an answer.",
        InputState::Answered => "Nothing answered and still open to change.",
        InputState::Closed => "Nothing closed.",
        InputState::Removed => "Nothing removed.",
    }
}

/// The history's fold: "history · 3 changes".
pub(crate) fn history_label(n: usize) -> String {
    format!("history · {n} change{}", if n == 1 { "" } else { "s" })
}

/// The header badge's tooltip.
pub(crate) fn badge_title(open: usize) -> String {
    match open {
        0 => "no input is waiting for an answer".into(),
        1 => "1 open input — waiting for an answer".into(),
        n => format!("{n} open inputs — waiting for an answer"),
    }
}

// ---------------------------------------------------------------------
// The view

fn source_title(s: AnswerSource) -> &'static str {
    match s {
        AnswerSource::App => "app: a viewer's click",
        AnswerSource::Api => "api: synapse or an agent",
    }
}

fn stamp_view(at: i64) -> impl IntoView {
    view! { <time class="at" title=fmt_unix(at)>{fmt_relative(now_unix(), at)}</time> }
}

/// An answer or a history entry: the value, the rationale, who, when.
fn said_view(s: Said, with_action: bool) -> impl IntoView {
    let action = with_action.then(|| view! { <span class="act">{s.action.as_str()}</span> });
    let value = s.value.map(|v| view! { <span class="val">{v}</span> });
    let rationale = s.rationale.map(|r| view! { <span class="why">{format!("— {r}")}</span> });
    view! {
        {action}
        {value}
        {rationale}
        <span class="src" title=source_title(s.source)>{s.source.as_str()}</span>
        {stamp_view(s.at_unix)}
    }
}

fn opt_view(o: Opt) -> impl IntoView {
    let title = o.value.as_ref().map(|v| format!("value: {v}"));
    let mark = o.chosen.then_some("✓ ");
    let rec = o.recommended.then(|| view! { <span class="rec" title="recommended">"★ recommended"</span> });
    view! {
        <li class="opt" class:recommended=o.recommended class:chosen=o.chosen title=title>
            {mark}
            {o.label}
            {rec}
        </li>
    }
}

fn row_view(r: Row) -> impl IntoView {
    let Row { ticket, id, kind, state, question, options, recommendation, answer, history } = r;
    let href = format!("#/ticket/{ticket}");
    let (data_ticket, data_input) = (ticket.clone(), id.clone());
    let question = match question {
        Some(q) => view! { <div class="iq">{q}</div> }.into_any(),
        None => view! { <div class="iq none">"no question"</div> }.into_any(),
    };
    let options = if options.is_empty() {
        view! { <div class="iopts none">"a free reply"</div> }.into_any()
    } else {
        view! { <ul class="iopts">{options.into_iter().map(opt_view).collect_view()}</ul> }.into_any()
    };
    let recommendation = recommendation.map(|r| {
        view! { <div class="irec"><span class="lbl">"recommendation"</span>" "{r}</div> }
    });
    let answer = match answer {
        Some(a) => view! { <div class="ians"><span class="lbl">"answer"</span>" "{said_view(a, false)}</div> }.into_any(),
        None => view! { <div class="ians none">"no answer"</div> }.into_any(),
    };
    let history = (!history.is_empty()).then(|| {
        let label = history_label(history.len());
        view! {
            <details class="ihist">
                <summary>{label}</summary>
                <ol>{history.into_iter().map(|s| view! { <li>{said_view(s, true)}</li> }).collect_view()}</ol>
            </details>
        }
    });
    view! {
        <li class="irow" data-ticket=data_ticket data-input=data_input data-state=state.as_str()>
            <div class="ihead">
                <a class="code" href=href title="the ticket this input is in">{ticket}</a>
                <span class="iid" title="the input's id in its ticket">{format!("#{id}")}</span>
                <span class="ikind" data-kind=kind.as_str()>{kind.as_str()}</span>
            </div>
            {question}
            {options}
            {recommendation}
            {answer}
            {history}
        </li>
    }
}

/// One group: its heading and count, its rows — live off `all`.
fn group_view(state: InputState, all: Memo<Option<Vec<Group>>>) -> impl IntoView {
    let rows = Memo::new(move |_| {
        all.with(|g| {
            g.as_ref()
                .and_then(|g| g.iter().find(|g| g.state == state))
                .map(|g| g.inputs.iter().map(row).collect::<Vec<_>>())
                .unwrap_or_default()
        })
    });
    let n = move || rows.with(Vec::len);
    view! {
        <section class="igroup" data-state=state.as_str()>
            <h3>
                <span class="ist" data-state=state.as_str()>{group_title(state)}</span>
                <span class="n">{n}</span>
            </h3>
            {move || rows.with(Vec::is_empty).then(|| view! { <p class="none empty">{group_empty(state)}</p> })}
            <ul class="irows">
                <For each=move || rows.get() key=|row| row.clone() children=row_view />
            </ul>
        </section>
    }
}

/// The filter bar: shortcodes, states, the removed toggle, and `clear`.
fn bar_view(facets: Memo<Facets>, f: RwSignal<Filter>) -> impl IntoView {
    move || {
        let fx = facets.get();
        let now = f.get();
        let all_sc = now.shortcodes.is_empty();
        let all_st = now.states.is_empty();
        let scs = fx
            .shortcodes
            .into_iter()
            .map(|(sc, n)| {
                let on = now.shortcodes.contains(&sc);
                let (pick, data_sc) = (sc.clone(), sc.clone());
                view! {
                    <button class="fchip" class:on=on aria-pressed=on.to_string() data-sc=data_sc
                        on:click=move |_| f.update(|f| f.toggle_shortcode(&pick))>
                        {sc}<span class="n">{n}</span>
                    </button>
                }
            })
            .collect_view();
        let sts = fx
            .states
            .into_iter()
            .map(|(s, n)| {
                let on = now.states.contains(&s);
                view! {
                    <button class="fchip" class:on=on aria-pressed=on.to_string() data-state=s.as_str()
                        on:click=move |_| f.update(|f| f.toggle_state(s))>
                        {s.as_str()}<span class="n">{n}</span>
                    </button>
                }
            })
            .collect_view();
        let removed = now.removed;
        let clear = (now != Filter::default()).then(|| {
            view! { <button class="fclear" on:click=move |_| f.set(Filter::default())>"clear filter"</button> }
        });
        view! {
            <div class="ibar">
                <div class="fgroup">
                    <span class="flabel">"shortcode"</span>
                    <button class="fchip" class:on=all_sc aria-pressed=all_sc.to_string()
                        on:click=move |_| f.update(|f| f.shortcodes.clear())>"all"</button>
                    {scs}
                </div>
                <div class="fgroup">
                    <span class="flabel">"state"</span>
                    <button class="fchip" class:on=all_st aria-pressed=all_st.to_string()
                        on:click=move |_| f.update(|f| f.states.clear())>"all"</button>
                    {sts}
                    <label class="fremoved" title="inputs their body no longer declares — history kept">
                        <input type="checkbox" prop:checked=removed
                            on:change=move |ev| f.update(|f| f.removed = event_target_checked(&ev)) />
                        "removed"<span class="n">{fx.removed}</span>
                    </label>
                </div>
                {clear}
            </div>
        }
    }
}

/// The route's query rewritten to `q` in place — `replaceState`, so no
/// history entry and no `hashchange` (the page stays mounted) — and only
/// while the hash is still this page's. FILTER-11: the lens's keys (`g`,
/// `n`, `f`) ride beside the page's own, kept.
fn write_query(q: &str) {
    let win = dom::window();
    let hash = win.location().hash().unwrap_or_default();
    if lens::path_of(&hash).trim_end_matches('/') != "#/inputs" {
        return;
    }
    let next = lens::with_page_query(&hash, q);
    if next != hash {
        if let Ok(history) = win.history() {
            let _ = history.replace_state_with_url(&wasm_bindgen::JsValue::NULL, "", Some(&next));
        }
    }
}

/// `#/inputs`.
#[component]
pub(crate) fn InputsPage() -> impl IntoView {
    inputs::want();
    let held = inputs::signal();
    let f = RwSignal::new(Filter::parse_query(&lens::page_query(&dom::window().location().hash().unwrap_or_default())));
    Effect::new(move |_| write_query(&f.with(Filter::to_query)));
    let held_groups = held.clone();
    let all = Memo::new(move |_| held_groups.with(|h| h.as_ref().map(|h| f.with(|f| groups(h.list(), f)))));
    let held_facets = held.clone();
    let fx = Memo::new(move |_| held_facets.with(|h| h.as_ref().map(|h| f.with(|f| facets(h.list(), f))).unwrap_or_default()));
    let loaded = Memo::new(move |_| held.with(Option::is_some));
    view! {
        <section id="inputs">
            <h2>"Inputs"</h2>
            <p class="none lede">
                "Every input a ticket body declares — an "
                <code>"<m-decision id>"</code>
                " or an "
                <code>"<m-input id kind>"</code>
                " — and its answer, live; what is waiting first."
            </p>
            {move || if loaded.get() {
                view! {
                    {bar_view(fx, f)}
                    {move || {
                        GROUPS
                            .into_iter()
                            .filter(|s| f.with(|f| f.shows(*s)))
                            .map(|s| group_view(s, all))
                            .collect_view()
                    }}
                }
                .into_any()
            } else {
                view! { <p class="loading">"loading the inputs…"</p> }.into_any()
            }}
        </section>
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inputs::tests::{answered, input};
    use menger_tickets_api::InputHistoryEntry;

    fn states(f: &str) -> Filter {
        Filter::parse_query(f)
    }

    /// The fixture: three tickets in two shortcodes, every state.
    fn fixture() -> Vec<Input> {
        let mut closed = answered(input("INPUT-9", "landing"), "archive", 100);
        closed.state = InputState::Closed;
        let mut gone = input("INPUT-9", "old");
        gone.state = InputState::Removed;
        vec![
            input("INPUT-6", "a"),
            answered(input("INPUT-6", "b"), "delete", 50),
            closed,
            input("INPUT-9", "notes"),
            gone,
            input("MNG-2", "c"),
        ]
    }

    fn keys(g: &Group) -> Vec<String> {
        g.inputs.iter().map(|i| format!("{}#{}", i.ticket, i.id)).collect()
    }

    /// Acceptance 1 — grouping: open, answered, closed, in that order, each
    /// in the store's order; the removed only with the toggle, last; a group
    /// with nothing in it still shows.
    #[test]
    fn inputs_group_by_state_open_first() {
        let g = groups(&fixture(), &Filter::default());
        assert_eq!(g.iter().map(|g| g.state).collect::<Vec<_>>(), SHOWN);
        assert_eq!(keys(&g[0]), ["INPUT-6#a", "INPUT-9#notes", "MNG-2#c"]);
        assert_eq!(keys(&g[1]), ["INPUT-6#b"]);
        assert_eq!(keys(&g[2]), ["INPUT-9#landing"]);

        let with_removed = Filter { removed: true, ..Filter::default() };
        let g = groups(&fixture(), &with_removed);
        assert_eq!(g.len(), 4);
        assert_eq!(g[3].state, InputState::Removed);
        assert_eq!(keys(&g[3]), ["INPUT-9#old"]);

        let none_closed: Vec<Input> = fixture().into_iter().filter(|i| i.state != InputState::Closed).collect();
        let g = groups(&none_closed, &Filter::default());
        assert_eq!(g[2].state, InputState::Closed);
        assert!(g[2].inputs.is_empty(), "an empty group is still a group");
    }

    /// Acceptance 1 — filtering: by shortcode, by state, both; the facets
    /// count what each choice would show.
    #[test]
    fn the_filter_narrows_by_shortcode_and_state() {
        let mut f = Filter::default();
        f.toggle_shortcode("mng");
        let g = groups(&fixture(), &f);
        assert_eq!(keys(&g[0]), ["MNG-2#c"]);
        assert!(g[1].inputs.is_empty() && g[2].inputs.is_empty());

        f.toggle_shortcode("INPUT");
        f.toggle_state(InputState::Answered);
        f.toggle_state(InputState::Open);
        assert_eq!(f.states, [InputState::Open, InputState::Answered], "kept in the page's order");
        let g = groups(&fixture(), &f);
        assert_eq!(g.iter().map(|g| g.state).collect::<Vec<_>>(), [InputState::Open, InputState::Answered]);
        assert_eq!(keys(&g[0]), ["INPUT-6#a", "INPUT-9#notes", "MNG-2#c"]);

        f.toggle_shortcode("MNG");
        f.toggle_state(InputState::Removed); // not the state filter's: the toggle's
        assert_eq!(f.states, [InputState::Open, InputState::Answered]);
        assert!(f.admits(&fixture()[0]) && !f.admits(&fixture()[5]) && !f.admits(&fixture()[2]));

        // The facets: every shortcode held, counted under the state filter;
        // every state, counted under the shortcode filter.
        let fx = facets(&fixture(), &f);
        assert_eq!(fx.shortcodes, [("INPUT".to_string(), 3), ("MNG".to_string(), 1)]);
        assert_eq!(fx.states, [(InputState::Open, 2), (InputState::Answered, 1), (InputState::Closed, 1)]);
        assert_eq!(fx.removed, 1);

        // A shortcode the filter names and no input is in is still offered.
        let mut ghost = Filter::default();
        ghost.toggle_shortcode("NOPE");
        let fx = facets(&fixture(), &ghost);
        assert!(fx.shortcodes.contains(&("NOPE".to_string(), 0)));
        assert!(groups(&fixture(), &ghost).iter().all(|g| g.inputs.is_empty()));
    }

    /// The filter is the route's query, both ways; no query, or a malformed
    /// one, is the default.
    #[test]
    fn the_filter_round_trips_through_the_query() {
        let f = Filter {
            shortcodes: ["INPUT".to_string(), "MNG".to_string()].into(),
            states: vec![InputState::Open, InputState::Closed],
            removed: true,
        };
        let q = f.to_query();
        assert_eq!(q, "sc=INPUT,MNG&state=open,closed&removed");
        assert_eq!(Filter::parse_query(&q), f);
        assert_eq!(Filter::parse_query(&format!("?{q}")), f, "a leading ? is the query's");
        assert_eq!(states("sc=input&state=closed,open"), Filter {
            shortcodes: ["INPUT".to_string()].into(),
            states: vec![InputState::Open, InputState::Closed],
            removed: false,
        });
        assert_eq!(Filter::default().to_query(), "");
        assert_eq!(states(""), Filter::default());
        assert_eq!(states("removed=1"), Filter { removed: true, ..Filter::default() });
        for bad in ["sc=", "sc=A-B", "state=removed", "state=open,shut", "kind=choice", "sc=A&sc=B", "removed=yes", "state"] {
            assert_eq!(states(bad), Filter::default(), "{bad}");
        }
    }

    /// A row's words: the options with the recommended marked and the
    /// chosen ticked; the recommendation only where it says more than the
    /// recommended option's label; the answer and the history in words.
    #[test]
    fn a_row_shows_its_options_answer_and_history() {
        let r = row(&answered(input("INPUT-9", "landing"), "archive", 100));
        assert_eq!(r.kind, InputKind::Choice);
        assert_eq!(r.question.as_deref(), Some("landing?"));
        assert_eq!(
            r.options,
            [
                Opt { label: "Archive".into(), value: Some("archive".into()), recommended: true, chosen: true },
                Opt { label: "Delete".into(), value: Some("delete".into()), recommended: false, chosen: false },
            ]
        );
        assert_eq!(r.recommendation, None, "the recommended option's label says it");
        let a = r.answer.expect("answered");
        assert_eq!((a.value.as_deref(), a.rationale.as_deref(), a.source, a.at_unix), (Some("Archive"), Some("because"), AnswerSource::Api, 100));
        assert_eq!(r.history.len(), 1);
        assert_eq!(r.history[0].action, InputAction::Answered);

        // A multi, with an `<m-recommendation>` of its own; a clear in its
        // history.
        let mut m = input("INPUT-9", "checks");
        m.kind = InputKind::Multi;
        m.options = vec![
            InputOption { value: "unit".into(), label: "Unit tests".into(), recommended: false },
            InputOption { value: "wire".into(), label: "wire".into(), recommended: false },
        ];
        m.recommendation = Some("Run both before merging.".into());
        let v = InputValue::Multi { options: vec!["unit".into(), "wire".into()] };
        m.history = vec![
            InputHistoryEntry { action: InputAction::Answered, value: Some(v.clone()), rationale: None, source: AnswerSource::App, at_unix: 10 },
            InputHistoryEntry { action: InputAction::Cleared, value: None, rationale: None, source: AnswerSource::Api, at_unix: 20 },
        ];
        let r = row(&m);
        assert_eq!(r.recommendation.as_deref(), Some("Run both before merging."));
        assert_eq!(r.options[1].value, None, "a value that is its label is not repeated");
        assert!(r.options.iter().all(|o| !o.chosen), "cleared: nothing chosen");
        assert!(r.answer.is_none());
        assert_eq!(r.history[0].value.as_deref(), Some("Unit tests, wire"));
        assert_eq!((r.history[1].action, r.history[1].value.as_deref()), (InputAction::Cleared, None));

        assert_eq!(value_words(&m.options, &InputValue::Multi { options: vec![] }), "none ticked");
        assert_eq!(value_words(&m.options, &InputValue::Choice { option: "gone".into() }), "gone", "an option the body no longer declares");
        assert_eq!(value_words(&[], &InputValue::Text { text: "Yes, ship it".into() }), "Yes, ship it");
    }

    /// Live apply, as the page sees it: an `input_changed` moves its row
    /// from one group to another, and the header's count with it.
    #[test]
    fn an_answer_moves_its_row_between_groups() {
        let mut held = inputs::Inputs::from_events(fixture().into_iter().map(|input| menger_tickets_api::TicketsEvent::Input { input }));
        assert_eq!(held.open_count(), 3);
        let before = groups(held.list(), &Filter::default());
        assert_eq!(keys(&before[0]), ["INPUT-6#a", "INPUT-9#notes", "MNG-2#c"]);

        let notes = answered(input("INPUT-9", "notes"), "archive", 200);
        assert!(held.apply(&menger_tickets_api::TicketsEvent::InputChanged { input: notes }));
        let after = groups(held.list(), &Filter::default());
        assert_eq!(keys(&after[0]), ["INPUT-6#a", "MNG-2#c"]);
        assert_eq!(keys(&after[1]), ["INPUT-6#b", "INPUT-9#notes"], "in the store's order");
        assert_eq!(held.open_count(), 2);
        // The rows that did not move are the same rows (their DOM is kept).
        assert_eq!(row(&before[0].inputs[0]), row(&after[0].inputs[0]));
    }

    #[test]
    fn the_labels() {
        assert_eq!(history_label(1), "history · 1 change");
        assert_eq!(history_label(3), "history · 3 changes");
        assert_eq!(badge_title(0), "no input is waiting for an answer");
        assert_eq!(badge_title(1), "1 open input — waiting for an answer");
        assert_eq!(badge_title(4), "4 open inputs — waiting for an answer");
        assert_eq!(shortcode_of("INPUT-9"), "INPUT");
        assert_eq!(group_title(InputState::Open), "open");
    }
}
