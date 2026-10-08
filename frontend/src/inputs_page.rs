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
//! FILTER-16: **the filter is the lens's** — FILTER-11's generic bar at the
//! `inputs` level: the page's own categories, **State** (open · answered ·
//! closed: which groups show; none chosen, all three) and **Removed** (shown:
//! the removed group, last), then the server-backed ticket categories, which
//! filter the inputs by their ticket. The rows are the inputs of the tickets
//! `global ∧ inputs` admits — `inputs { filter }` (FILTER-15) — then the
//! page's own categories over them; with no ticket group at either level,
//! the app's copy of every input, as before. The menu counts the page's own
//! categories on the rows it holds and the ticket categories by `facets`
//! (FILTER-12). The filter rides the URL as every level's does (`f=…&state=
//! is:open&removed=is:yes`, src/lens.rs); an INPUT-6 link
//! (`#/inputs?sc=INPUT,MNG&state=open,answered&removed`) still opens the
//! filter it named — Shortcode is any of its codes — and is written as `f=`
//! ([`read_input_6`]).
//!
//! **Live.** With no ticket filter the rows are the app's copy of the inputs
//! (src/inputs.rs), which the page asks for ([`inputs::want`]) and `watch`
//! keeps — an `input_changed` replaces the input, so its row leaves one group
//! and enters another as it happens. A row whose input did not change keeps
//! its DOM (an opened history stays open). Under a ticket filter the
//! answer is kept the same way ([`Lensed`]): an `input_changed` of a ticket
//! it knows inside applies; one of a ticket it does not know is asked of
//! `member`; the membership watch's `entered` brings a ticket's inputs (from
//! the app's copy) and `left` takes them; a new connection, a `recount` or a
//! `lagged` asks again.
//!
//! What decides — the page's own categories and an INPUT-6 link, the groups,
//! the counts, the answer kept live, a row's words — is pure and pinned by
//! native tests.

use std::collections::{BTreeSet, HashMap};

use leptos::prelude::*;
use menger_tickets_api::{AnswerSource, Input, InputAction, InputKind, InputOption, InputState, InputValue, TicketsEvent};
use wasm_bindgen_futures::spawn_local;

use crate::filter::{Cat, Clausal, Clauses, Counts, CountsProvider, FilterBar, Op, INPUT_STATES};
use crate::inputs::{self, ticket_order, Inputs};
use crate::lens::{self, Kept, Level};
use crate::session::{self, Member};
use crate::{counts, fmt_relative, fmt_unix, now_unix};

/// The states State chooses among, in the page's order; `removed` is
/// Removed's.
pub(crate) const SHOWN: [InputState; 3] = [InputState::Open, InputState::Answered, InputState::Closed];

/// Every group the page can show, in its order.
const GROUPS: [InputState; 4] = [InputState::Open, InputState::Answered, InputState::Closed, InputState::Removed];

/// The shortcode an input's ticket is in.
pub(crate) fn shortcode_of(ticket: &str) -> &str {
    ticket_order(ticket).0
}

// ---------------------------------------------------------------------
// The page's own categories

/// FILTER-16: what the page's own categories show — the groups of the
/// states State admits (no State clause: all three), and the removed when
/// Removed says `shown`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Shown {
    /// A subset of [`SHOWN`], in its order.
    pub(crate) states: Vec<InputState>,
    /// Whether the removed show (their own group, last).
    pub(crate) removed: bool,
}

impl Default for Shown {
    fn default() -> Self {
        Shown { states: SHOWN.to_vec(), removed: false }
    }
}

impl Shown {
    /// The page's local clauses as what shows.
    pub(crate) fn of(local: &Clauses) -> Self {
        let states = match local.get(Cat::State) {
            Some(cl) => SHOWN.into_iter().filter(|s| cl.admits(Some(s.as_str()))).collect(),
            None => SHOWN.to_vec(),
        };
        let removed = local.get(Cat::Removed).is_some_and(|cl| cl.values.first().is_some_and(|v| v == "yes"));
        Shown { states, removed }
    }

    /// Whether the group `state` shows.
    pub(crate) fn shows(&self, state: InputState) -> bool {
        match state {
            InputState::Removed => self.removed,
            s => self.states.contains(&s),
        }
    }
}

/// FILTER-16: an INPUT-6 link's filter — `sc=A,B` (the shortcodes shown),
/// `state=open,closed` (the states shown), `removed` (the removed too),
/// the lens's `g` and `n` aside — as the inputs level's: Shortcode is any of
/// the codes (the language's `in`), State and Removed the page's own. `None`
/// when it names none of INPUT-6's keys; `Some(None)` when it is malformed
/// (an unknown key, a key twice, no values, a shortcode that is not one, a
/// state outside [`SHOWN`]) — the default, as INPUT-6 read it.
pub(crate) fn read_input_6(q: &str) -> Option<Option<Kept>> {
    let ps: Vec<lens::Pair> = lens::pairs(q).into_iter().filter(|p| !matches!(p.key.as_str(), "g" | "n" | "f")).collect();
    if !ps.iter().any(|p| lens::INPUT_6_KEYS.contains(&p.key.as_str())) {
        return None;
    }
    Some(parse_input_6(&ps))
}

fn parse_input_6(ps: &[lens::Pair]) -> Option<Kept> {
    let mut seen = BTreeSet::new();
    let mut k = Kept::default();
    for p in ps {
        if !seen.insert(p.key.as_str()) {
            return None;
        }
        match (p.key.as_str(), p.value.as_deref()) {
            ("sc", Some(v)) => {
                let mut codes = Vec::new();
                for sc in v.split(',') {
                    if sc.is_empty() || !sc.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') {
                        return None;
                    }
                    codes.push(sc.to_ascii_uppercase());
                }
                k.set(Cat::Shortcode, Op::Is, codes);
            }
            ("state", Some(v)) => {
                let named: Vec<InputState> = v.split(',').map(|s| s.parse().ok()).collect::<Option<_>>()?;
                if named.iter().any(|s| !SHOWN.contains(s)) {
                    return None;
                }
                k.set(Cat::State, Op::Is, named.iter().map(|s| s.as_str().to_string()));
            }
            ("removed", None | Some("1")) => k.set(Cat::Removed, Op::Is, ["yes".to_string()]),
            _ => return None,
        }
    }
    Some(k)
}

// ---------------------------------------------------------------------
// The groups and the counts

/// One group of the page: a state and its inputs, in the store's order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Group {
    pub(crate) state: InputState,
    pub(crate) inputs: Vec<Input>,
}

/// The page's groups — each state `shown` shows, in the page's order (open,
/// answered, closed, then removed), with its inputs of `list`; a group with
/// none is still a group (its heading says 0).
pub(crate) fn groups(list: &[Input], shown: &Shown) -> Vec<Group> {
    GROUPS
        .into_iter()
        .filter(|s| shown.shows(*s))
        .map(|state| Group { state, inputs: list.iter().filter(|i| i.state == state).cloned().collect() })
        .collect()
}

/// FILTER-16: the page's own categories counted on the rows it holds (the
/// ticket filter's) — what a click would give, each category's own clause
/// set aside: State, the inputs in each state; Removed, `shown` the removed
/// and `hidden` the rest.
pub(crate) fn local_counts(list: &[Input]) -> Counts {
    let n = |s: InputState| list.iter().filter(|i| i.state == s).count();
    let mut out: Counts = HashMap::new();
    out.insert(Cat::State, INPUT_STATES.iter().zip(SHOWN).map(|(t, s)| (t.to_string(), n(s))).collect());
    let removed = n(InputState::Removed);
    out.insert(Cat::Removed, HashMap::from([("yes".to_string(), removed), ("no".to_string(), list.len() - removed)]));
    out
}

// ---------------------------------------------------------------------
// The answer under a ticket filter

/// FILTER-16: the inputs of the tickets a filter admits — `inputs { filter
/// }`'s answer, kept live: an `input_changed` of a ticket known inside
/// applies; one of a ticket known outside is dropped; one of a ticket it
/// does not know is to be asked of `member`; the membership watch's
/// `entered` brings a ticket's inputs (the app's copy holds them) and `left`
/// takes them.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Lensed {
    held: Inputs,
    /// Tickets known inside the filter — the answer's, and any entered or
    /// admitted since, with inputs or not.
    inside: BTreeSet<String>,
    /// Tickets known outside it.
    outside: BTreeSet<String>,
}

/// What an `input_changed` does to a [`Lensed`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Heard {
    /// Applied or dropped — whether the rows moved.
    Moved(bool),
    /// Its ticket is not known: ask `member`, then [`Lensed::entered`] or
    /// [`Lensed::left`].
    Ask(String),
}

impl Lensed {
    /// `inputs { filter }`'s answer.
    pub(crate) fn from_answer(events: impl IntoIterator<Item = TicketsEvent>) -> Self {
        let held = Inputs::from_events(events);
        let inside = held.list().iter().map(|i| i.ticket.clone()).collect();
        Lensed { held, inside, outside: BTreeSet::new() }
    }

    /// The rows, in the store's order.
    pub(crate) fn list(&self) -> &[Input] {
        self.held.list()
    }

    /// An `input_changed`.
    pub(crate) fn changed(&mut self, input: &Input) -> Heard {
        if self.inside.contains(&input.ticket) {
            Heard::Moved(self.held.upsert(input.clone()))
        } else if self.outside.contains(&input.ticket) {
            Heard::Moved(false)
        } else {
            Heard::Ask(input.ticket.clone())
        }
    }

    /// `ticket` is inside the filter (`entered`, or `member` said so): its
    /// inputs — the app's copy's — are held. Whether the rows moved.
    pub(crate) fn entered(&mut self, ticket: &str, inputs: Vec<Input>) -> bool {
        self.outside.remove(ticket);
        self.inside.insert(ticket.to_string());
        inputs.into_iter().filter(|i| i.ticket == ticket).fold(false, |moved, i| self.held.upsert(i) | moved)
    }

    /// `ticket` is outside it (`left`, or `member` said so): its inputs go.
    pub(crate) fn left(&mut self, ticket: &str) -> bool {
        self.inside.remove(ticket);
        self.outside.insert(ticket.to_string());
        self.held.drop_ticket(ticket)
    }
}

/// Something the page heard for its answer — kept while a fetch is out,
/// to replay onto the answer in order.
#[derive(Clone, Debug)]
enum Note {
    /// `input_changed`.
    Changed(Input),
    /// `entered`: the ticket's inputs from the app's copy.
    Entered(String),
    /// `member` admitted the ticket: these inputs of it.
    Admitted(String, Vec<Input>),
    /// `left`, or `member` did not admit it.
    Left(String),
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

/// What the page lists, by the ticket filter `global ∧ inputs`.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Mode {
    /// No ticket group at either level: the app's copy of every input.
    Every,
    /// Groups that exclude each other: nothing (FILTER-11's notice says
    /// why), asked of no one.
    Nothing,
    /// The inputs of the tickets this filter (wire text) admits.
    Under(String),
}

fn mode_of(f: &menger_tickets_api::filter::Filter) -> Mode {
    if f.groups.is_empty() {
        Mode::Every
    } else if f.admits_nothing().is_some() {
        Mode::Nothing
    } else {
        Mode::Under(counts::wire_text(f))
    }
}

/// The page's state under a ticket filter — every handle `Copy`.
#[derive(Clone, Copy)]
struct Lens16 {
    mode: Memo<Mode>,
    /// The answer, kept live — `None` until it is in (for this filter).
    lensed: RwSignal<Option<Lensed>>,
    /// While a fetch is out: what it heard, to replay onto its answer.
    heard: StoredValue<Option<Vec<Note>>>,
    /// The newest fetch; an older one's answer is dropped.
    asked: StoredValue<u64>,
    /// The server did not take the filter — its words.
    refused: RwSignal<Option<String>>,
}

impl Lens16 {
    /// The filter the page is under — none with no ticket filter, or once
    /// the page is gone (an answer that lands late is dropped).
    fn text(self) -> Option<String> {
        match self.mode.try_get_untracked()? {
            Mode::Under(t) => Some(t),
            _ => None,
        }
    }

    /// `inputs { filter }`, asked now; what is heard meanwhile is kept and
    /// replayed onto its answer.
    fn fetch(self) {
        let Some(text) = self.text() else { return };
        let Some(n) = self.asked.try_update_value(|n| {
            *n = n.wrapping_add(1);
            *n
        }) else {
            return;
        };
        self.heard.try_set_value(Some(Vec::new()));
        spawn_local(async move {
            let answer = session::inputs_under(text.clone()).await;
            if self.asked.try_get_value() != Some(n) || self.text().as_deref() != Some(text.as_str()) {
                return; // a newer fetch, or another filter
            }
            let heard = self.heard.try_update_value(Option::take).flatten().unwrap_or_default();
            match answer {
                Ok(events) => {
                    let mut l = Lensed::from_answer(events);
                    for h in heard {
                        self.apply(&mut l, h);
                    }
                    self.refused.set(None);
                    self.lensed.set(Some(l));
                }
                Err(why) => {
                    self.refused.set(Some(why));
                    self.lensed.set(Some(Lensed::default()));
                }
            }
        });
    }

    /// One heard thing onto `l`; an input of a ticket it does not know is
    /// asked of `member`.
    fn apply(self, l: &mut Lensed, h: Note) -> bool {
        match h {
            Note::Changed(input) => match l.changed(&input) {
                Heard::Moved(m) => m,
                Heard::Ask(ticket) => {
                    self.ask(ticket, input);
                    false
                }
            },
            Note::Entered(t) => l.entered(&t, of_ticket(&t)),
            Note::Admitted(t, inputs) => l.entered(&t, inputs),
            Note::Left(t) => l.left(&t),
        }
    }

    /// Something heard: kept while a fetch is out, else applied.
    fn hear(self, h: Note) {
        if self.text().is_none() {
            return;
        }
        let kept = self.heard.try_update_value(|b| b.as_mut().map(|b| b.push(h.clone())).is_some()).unwrap_or(true);
        if !kept {
            self.lensed.try_update(|l| {
                if let Some(l) = l {
                    self.apply(l, h);
                }
            });
        }
    }

    /// `member { ids: [ticket], filter }`: inside, the ticket's inputs come
    /// in (`input` among them); outside, it is known so.
    fn ask(self, ticket: String, input: Input) {
        let Some(text) = self.text() else { return };
        spawn_local(async move {
            let Ok(answer) = session::member(vec![ticket.clone()], text.clone()).await else { return };
            if self.text().as_deref() != Some(text.as_str()) {
                return;
            }
            if answer.iter().any(|(id, m)| *id == ticket && *m) {
                // The app's copy has applied the event by now; the input
                // that was asked about, in case it has not.
                let mut held = of_ticket(&ticket);
                if !held.iter().any(|i| i.id == input.id) {
                    held.push(input);
                }
                self.hear(Note::Admitted(ticket, held));
            } else {
                self.hear(Note::Left(ticket));
            }
        });
    }
}

/// The app's copy's inputs of `ticket` (none while it is not in).
fn of_ticket(ticket: &str) -> Vec<Input> {
    inputs::signal().with_untracked(|h| h.as_ref().map(|h| h.of_ticket(ticket)).unwrap_or_default())
}

/// `#/inputs`.
#[component]
pub(crate) fn InputsPage() -> impl IntoView {
    inputs::want();
    let held = inputs::signal();
    let lens = lens::use_lens();
    let eff = lens.effective(Level::Inputs);
    let mode = Memo::new(move |_| eff.with(mode_of));
    let l16 = Lens16 {
        mode,
        lensed: RwSignal::new(None),
        heard: StoredValue::new(None),
        asked: StoredValue::new(0),
        refused: RwSignal::new(None),
    };
    // A new ticket filter: nothing held for it until its answer is in (its
    // membership watch says when to ask).
    Effect::new(move |_| {
        mode.track();
        l16.asked.update_value(|n| *n = n.wrapping_add(1));
        l16.heard.set_value(None);
        l16.lensed.set(None);
        l16.refused.set(None);
    });
    session::follow_membership(
        move || match mode.get() {
            Mode::Under(t) => Some(t),
            _ => None,
        },
        move |m: &Member| match m {
            Member::Opened | Member::Recount { .. } | Member::Lagged => l16.fetch(),
            Member::Entered { id, .. } => l16.hear(Note::Entered(id.clone())),
            Member::Left { id, .. } => l16.hear(Note::Left(id.clone())),
            Member::Refused(why) => {
                l16.refused.set(Some(why.clone()));
                l16.lensed.set(Some(Lensed::default()));
            }
        },
    );
    session::listen(move |ev: &TicketsEvent| {
        if let TicketsEvent::InputChanged { input } = ev {
            l16.hear(Note::Changed(input.clone()));
        }
    });

    // The rows: the ticket filter's, then the page's own categories.
    let held_list = held.clone();
    let list = Memo::new(move |_| match mode.get() {
        Mode::Every => held_list.with(|h| h.as_ref().map(|h| h.list().to_vec())),
        Mode::Nothing => Some(Vec::new()),
        Mode::Under(_) => l16.lensed.with(|l| l.as_ref().map(|l| l.list().to_vec())),
    });
    let shown = Memo::new(move |_| lens.kept(Level::Inputs).map(|k| Shown::of(&k.local)).unwrap_or_default());
    let all = Memo::new(move |_| list.with(|l| l.as_ref().map(|l| shown.with(|s| groups(l, s)))));
    // The menu's counts: the page's own on its rows, the tickets' by facets.
    let server = counts::menu_counts(Level::Inputs);
    let local = Memo::new(move |_| list.with(|l| l.as_deref().map(local_counts).unwrap_or_default()));
    let provider = CountsProvider::of(Signal::derive(move || {
        let mut c = server.get();
        c.extend(local.get());
        c
    }));
    let loaded = Memo::new(move |_| list.with(Option::is_some));
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
            <div class="ibar">
                <FilterBar level=Level::Inputs cats=Cat::INPUTS.to_vec() counts=provider hint="Filter the inputs (f)" />
            </div>
            {move || l16.refused.get().map(|why| view! {
                <p class="irefused" role="status">"The server did not take this filter: "<code>{why}</code></p>
            })}
            {move || if loaded.get() {
                view! {
                    {move || {
                        GROUPS
                            .into_iter()
                            .filter(|s| shown.with(|f| f.shows(*s)))
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
    use crate::filter::Clausal;
    use crate::inputs::tests::{answered, input};
    use menger_tickets_api::InputHistoryEntry;

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

    fn clauses(state: Option<(Op, &[&str])>, removed: Option<&str>) -> Clauses {
        let mut c = Clauses::default();
        if let Some((op, v)) = state {
            c.set(Cat::State, op, v.iter().map(|s| s.to_string()));
        }
        if let Some(r) = removed {
            c.set(Cat::Removed, Op::Is, [r.to_string()]);
        }
        c
    }

    /// Acceptance 1 — grouping: open, answered, closed, in that order, each
    /// in the store's order; the removed only with the toggle, last; a group
    /// with nothing in it still shows.
    #[test]
    fn inputs_group_by_state_open_first() {
        let g = groups(&fixture(), &Shown::default());
        assert_eq!(g.iter().map(|g| g.state).collect::<Vec<_>>(), SHOWN);
        assert_eq!(keys(&g[0]), ["INPUT-6#a", "INPUT-9#notes", "MNG-2#c"]);
        assert_eq!(keys(&g[1]), ["INPUT-6#b"]);
        assert_eq!(keys(&g[2]), ["INPUT-9#landing"]);

        let with_removed = Shown { removed: true, ..Shown::default() };
        let g = groups(&fixture(), &with_removed);
        assert_eq!(g.len(), 4);
        assert_eq!(g[3].state, InputState::Removed);
        assert_eq!(keys(&g[3]), ["INPUT-9#old"]);

        let none_closed: Vec<Input> = fixture().into_iter().filter(|i| i.state != InputState::Closed).collect();
        let g = groups(&none_closed, &Shown::default());
        assert_eq!(g[2].state, InputState::Closed);
        assert!(g[2].inputs.is_empty(), "an empty group is still a group");
    }

    /// FILTER-16 acceptance 1 — the page's own categories over the
    /// fixture: State chooses the groups (is any of, is not), Removed shows
    /// the removed; none, INPUT-6's default; the menu counts each on the rows.
    #[test]
    fn the_pages_own_categories_choose_the_groups() {
        let states = |c: &Clauses| groups(&fixture(), &Shown::of(c)).iter().map(|g| g.state).collect::<Vec<_>>();
        assert_eq!(Shown::of(&Clauses::default()), Shown::default());
        assert_eq!(states(&Clauses::default()), SHOWN, "none chosen: all three, the removed hidden");
        assert_eq!(states(&clauses(Some((Op::Is, &["answered", "open"])), None)), [InputState::Open, InputState::Answered], "in the page's order");
        assert_eq!(states(&clauses(Some((Op::Not, &["closed"])), None)), [InputState::Open, InputState::Answered]);
        assert_eq!(states(&clauses(Some((Op::Not, &["open", "answered", "closed"])), None)), Vec::<InputState>::new(), "none of them");
        assert_eq!(states(&clauses(None, Some("yes"))), GROUPS, "the removed too, last");
        assert_eq!(states(&clauses(Some((Op::Is, &["closed"])), Some("yes"))), [InputState::Closed, InputState::Removed]);
        assert_eq!(states(&clauses(None, Some("no"))), SHOWN, "Removed is hidden: the default");
        let g = groups(&fixture(), &Shown::of(&clauses(Some((Op::Is, &["open"])), None)));
        assert_eq!(keys(&g[0]), ["INPUT-6#a", "INPUT-9#notes", "MNG-2#c"]);

        // The counts: each state's inputs, the removed and the rest.
        let c = local_counts(&fixture());
        let get = |cat: Cat, v: &str| c.get(&cat).and_then(|m| m.get(v)).copied();
        assert_eq!((get(Cat::State, "open"), get(Cat::State, "answered"), get(Cat::State, "closed")), (Some(3), Some(1), Some(1)));
        assert_eq!((get(Cat::Removed, "yes"), get(Cat::Removed, "no")), (Some(1), Some(5)));
        assert_eq!(get(Cat::Shortcode, "INPUT"), None, "the tickets' categories are facets'");
        let empty = local_counts(&[]);
        assert_eq!(empty.get(&Cat::State).and_then(|m| m.get("open")).copied(), Some(0));
    }

    /// FILTER-16 acceptance 1 — the query, old and new: an INPUT-6 link is
    /// read as the inputs level (Shortcode the language's `in`, State and
    /// Removed the page's own), the lens's `g` and `n` aside; malformed, the
    /// default; and the level's own form (`f=` and its local pairs) round
    /// trips.
    #[test]
    fn the_query_old_and_new() {
        let k = read_input_6("sc=INPUT,mng&state=closed,open&removed").expect("names INPUT-6's keys").expect("well formed");
        assert_eq!(k.filter.format(), "in (INPUT, MNG)");
        assert_eq!(Shown::of(&k.local), Shown { states: vec![InputState::Open, InputState::Closed], removed: true });
        let new = lens::join(&lens::pairs_of(Level::Inputs, &k));
        assert_eq!(new, "f=in%20%28INPUT%2C%20MNG%29&state=is:open,closed&removed=is:yes", "written as f=");
        assert_eq!(lens::read(&new, Level::Inputs), Some(Some(k.clone())), "and read back");

        assert_eq!(read_input_6("g=kind%20build&sc=INPUT").flatten().map(|k| k.filter.format()).as_deref(), Some("in INPUT"), "g is the lens's");
        assert_eq!(read_input_6("removed=1").flatten().map(|k| Shown::of(&k.local)), Some(Shown { removed: true, ..Shown::default() }));
        assert_eq!(read_input_6("state=answered").flatten().map(|k| (k.filter.groups.is_empty(), Shown::of(&k.local).states)), Some((true, vec![InputState::Answered])));
        assert_eq!(read_input_6(""), None, "no INPUT-6 key: not one");
        assert_eq!(read_input_6("g=ready&n=in%20MNG"), None);
        for bad in ["sc=", "sc=A-B", "state=removed", "state=open,shut", "sc=A&kind=choice", "sc=A&sc=B", "removed=yes", "state"] {
            assert_eq!(read_input_6(bad), Some(None), "{bad}");
        }
    }

    /// FILTER-16: the answer under a ticket filter, kept live — an input of
    /// a ticket it holds applies; one of a ticket it does not know is asked;
    /// `entered` brings a ticket's inputs, `left` takes them, and an input of
    /// a ticket known outside is dropped.
    #[test]
    fn the_answer_under_a_ticket_filter_is_kept_live() {
        let answer = [input("INPUT-6", "a"), answered(input("INPUT-6", "b"), "delete", 50)].map(|input| TicketsEvent::Input { input });
        let mut l = Lensed::from_answer(answer);
        let ids = |l: &Lensed| l.list().iter().map(|i| format!("{}#{}", i.ticket, i.id)).collect::<Vec<_>>();
        assert_eq!(ids(&l), ["INPUT-6#a", "INPUT-6#b"]);

        assert_eq!(l.changed(&answered(input("INPUT-6", "a"), "archive", 60)), Heard::Moved(true));
        assert_eq!(l.list()[0].state, InputState::Answered);
        assert_eq!(l.changed(&input("INPUT-6", "new")), Heard::Moved(true), "a new input of a ticket inside");
        assert_eq!(l.changed(&input("MNG-2", "c")), Heard::Ask("MNG-2".into()), "a ticket it does not know");

        assert!(l.entered("MNG-2", vec![input("MNG-2", "c"), input("MNG-3", "not-its")]));
        assert_eq!(ids(&l), ["INPUT-6#a", "INPUT-6#b", "INPUT-6#new", "MNG-2#c"], "only the ticket's own");
        assert!(l.left("INPUT-6"));
        assert_eq!(ids(&l), ["MNG-2#c"]);
        assert_eq!(l.changed(&input("INPUT-6", "a")), Heard::Moved(false), "known outside: dropped");
        assert!(!l.entered("INPUT-9", Vec::new()), "inside, with no input");
        assert_eq!(l.changed(&input("INPUT-9", "first")), Heard::Moved(true), "its first input applies");
        assert!(!l.left("NOPE-1"));
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
        let before = groups(held.list(), &Shown::default());
        assert_eq!(keys(&before[0]), ["INPUT-6#a", "INPUT-9#notes", "MNG-2#c"]);

        let notes = answered(input("INPUT-9", "notes"), "archive", 200);
        assert!(held.apply(&menger_tickets_api::TicketsEvent::InputChanged { input: notes }));
        let after = groups(held.list(), &Shown::default());
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
