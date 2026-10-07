//! INPUT-6: the app's copy of the inputs store (INPUT-3) — what `#/inputs`
//! lists and what the header's `inputs` badge counts.
//!
//! **Asked for by what shows it** — COMP-5's registry rule (src/registry.rs),
//! held to the same discipline. Nothing at startup asks `inputs` (SCALE-26:
//! the session asks only for what the first screen shows). The first view
//! that needs the inputs — the `#/inputs` page ([`crate::inputs_page`]) —
//! calls [`want`]; from then on they are fetched on every new connection and
//! after every `lagged` ([`refresh`]), and kept in between by `watch`:
//! `input_changed` carries the whole input, so applying it is one upsert
//! ([`heard`]). The header's badge reads the same copy, so it shows from the
//! first time the page is opened, and stays live after the page is left.
//!
//! **What is fetched**: `inputs {}` — every input, every state, in the
//! store's order (tickets by shortcode and sequence, each ticket's inputs in
//! its body's order, the removed after). Not `--state open` alone: the page
//! groups all four states and a row moves between them live, which a copy of
//! the open ones could not show.
//!
//! **No change is lost to the fetch.** `watch` is subscribed before anything
//! is asked (session.rs), so an `inputs {}` answer may be older than an
//! event already heard, or newer: the events heard while the fetch is out
//! are kept and replayed onto its answer, in order. An upsert of a whole
//! input gives the newest state whichever side of the answer it fell on.
//!
//! **Where a new input goes.** An `input_changed` for an input the copy does
//! not hold (a body declared it after the fetch) is placed after its
//! ticket's inputs — in the store's order of tickets, which the id says. The
//! body's own position is not on the event; the next fetch (a reconnect, a
//! `lagged`) has it. Only an input's place among its own ticket's same-state
//! inputs can differ, and only until then.
//!
//! What decides — the answer's order, applying an event, the replay, the
//! count — is pure and pinned by native tests; the rest is the session's
//! socket.

use std::cell::{Cell, RefCell};

use leptos::prelude::*;
use menger_tickets_api::{Input, InputState, TicketsEvent};
use wasm_bindgen_futures::spawn_local;

use crate::session;

/// Where a ticket sits in the store's order: its shortcode, then its
/// sequence as a number (`MNG-9` before `MNG-10`) — the server's
/// `by_epic_seq`. An id without a sequence sorts after its shortcode's.
pub(crate) fn ticket_order(id: &str) -> (&str, u32) {
    match id.rsplit_once('-').and_then(|(e, n)| n.parse::<u32>().ok().map(|n| (e, n))) {
        Some(k) => k,
        None => (id, u32::MAX),
    }
}

/// The inputs as the app holds them: every input, in the store's order.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Inputs {
    list: Vec<Input>,
}

impl Inputs {
    /// An `inputs {}` answer: one `input` per input, in the store's order;
    /// anything else (an in-band error) contributes nothing.
    pub(crate) fn from_events(events: impl IntoIterator<Item = TicketsEvent>) -> Self {
        let list = events
            .into_iter()
            .filter_map(|ev| match ev {
                TicketsEvent::Input { input } => Some(input),
                _ => None,
            })
            .collect();
        Self { list }
    }

    /// Whether `ev` is one the copy takes.
    pub(crate) fn concerns(ev: &TicketsEvent) -> bool {
        matches!(ev, TicketsEvent::InputChanged { .. })
    }

    /// Apply a `watch` event; whether the copy moved.
    pub(crate) fn apply(&mut self, ev: &TicketsEvent) -> bool {
        match ev {
            TicketsEvent::InputChanged { input } => self.upsert(input.clone()),
            _ => false,
        }
    }

    /// The whole input, in place of the one with its ticket and id — or,
    /// new, after its ticket's inputs (before the first input of a ticket
    /// later in the store's order). Whether anything moved.
    pub(crate) fn upsert(&mut self, input: Input) -> bool {
        if let Some(held) = self.list.iter_mut().find(|i| i.ticket == input.ticket && i.id == input.id) {
            if *held == input {
                return false;
            }
            *held = input;
            return true;
        }
        let key = ticket_order(&input.ticket);
        let at = self.list.iter().position(|i| ticket_order(&i.ticket) > key).unwrap_or(self.list.len());
        self.list.insert(at, input);
        true
    }

    /// Every input, in the store's order.
    pub(crate) fn list(&self) -> &[Input] {
        &self.list
    }

    /// How many are `open` — what the header's badge says.
    pub(crate) fn open_count(&self) -> usize {
        self.list.iter().filter(|i| i.state == InputState::Open).count()
    }
}

/// The fetch's rule: `answer`, then every event heard while it was out, in
/// order.
pub(crate) fn settle(answer: Vec<TicketsEvent>, heard: &[TicketsEvent]) -> Inputs {
    let mut inputs = Inputs::from_events(answer);
    for ev in heard {
        inputs.apply(ev);
    }
    inputs
}

thread_local! {
    /// The inputs — `None` until the first fetch lands.
    static INPUTS: ArcRwSignal<Option<Inputs>> = ArcRwSignal::new(None);
    /// Whether some view has asked for them ([`want`]); from then on they
    /// are kept for the app's lifetime.
    static WANTED: Cell<bool> = const { Cell::new(false) };
    /// The newest fetch; an older one's answer is dropped.
    static FETCH: Cell<u32> = const { Cell::new(0) };
    /// While a fetch is out: the input events heard since it went.
    static HEARD: RefCell<Option<Vec<TicketsEvent>>> = const { RefCell::new(None) };
}

/// The inputs, as a signal — what `#/inputs` and the header's badge draw
/// from.
pub(crate) fn signal() -> ArcRwSignal<Option<Inputs>> {
    INPUTS.with(Clone::clone)
}

/// A view needs the inputs: fetch them now if this is the first ask (and
/// the socket is up — else the session's [`refresh`] will).
pub(crate) fn want() {
    if !WANTED.with(|w| w.replace(true)) {
        fetch();
    }
}

/// A new connection, or a `lagged`: what is held may be stale — ask again,
/// if anything wants it.
pub(crate) fn refresh() {
    if WANTED.with(Cell::get) {
        fetch();
    }
}

/// A `watch` event: an `input_changed` moves the copy (and is kept for
/// replay while a fetch is out). Called by the session for every event.
pub(crate) fn heard(ev: &TicketsEvent) {
    if !Inputs::concerns(ev) {
        return;
    }
    HEARD.with(|h| {
        if let Some(buf) = h.borrow_mut().as_mut() {
            buf.push(ev.clone());
        }
    });
    INPUTS.with(|s| {
        s.maybe_update(|held| held.as_mut().is_some_and(|held| held.apply(ev)));
    });
}

fn fetch() {
    let Some(client) = session::client() else {
        return; // not connected: the session refreshes once it is
    };
    let n = FETCH.with(|f| {
        let n = f.get().wrapping_add(1);
        f.set(n);
        n
    });
    HEARD.with(|h| *h.borrow_mut() = Some(Vec::new()));
    spawn_local(async move {
        let answer = client.inputs_all(None, None, None).await;
        if FETCH.with(Cell::get) != n {
            return; // a newer fetch went out; its answer is the one kept
        }
        let heard = HEARD.with(|h| h.borrow_mut().take()).unwrap_or_default();
        match answer {
            Ok(events) => {
                for ev in &events {
                    if let TicketsEvent::Error { message } = ev {
                        leptos::logging::log!("inputs: {message}");
                    }
                }
                let held = settle(events, &heard);
                INPUTS.with(|s| s.set(Some(held)));
            }
            Err(e) => leptos::logging::log!("inputs: {}", session::describe(&e)),
        }
    });
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use menger_tickets_api::{Answer, AnswerSource, InputAction, InputHistoryEntry, InputKind, InputOption, InputValue};

    /// A choice input, `open` — the fixtures' starting point.
    pub(crate) fn input(ticket: &str, id: &str) -> Input {
        Input {
            ticket: ticket.into(),
            id: id.into(),
            kind: InputKind::Choice,
            question: Some(format!("{id}?")),
            options: vec![
                InputOption { value: "archive".into(), label: "Archive".into(), recommended: true },
                InputOption { value: "delete".into(), label: "Delete".into(), recommended: false },
            ],
            recommendation: Some("Archive".into()),
            state: InputState::Open,
            answer: None,
            history: Vec::new(),
        }
    }

    /// `input` answered `option` at `at`, through the API.
    pub(crate) fn answered(mut i: Input, option: &str, at: i64) -> Input {
        let value = InputValue::Choice { option: option.into() };
        i.history.push(InputHistoryEntry {
            action: InputAction::Answered,
            value: Some(value.clone()),
            rationale: Some("because".into()),
            source: AnswerSource::Api,
            at_unix: at,
        });
        i.answer = Some(Answer { value, rationale: Some("because".into()), source: AnswerSource::Api, at_unix: at });
        i.state = InputState::Answered;
        i
    }

    fn ids(inputs: &Inputs) -> Vec<String> {
        inputs.list().iter().map(|i| format!("{}#{}", i.ticket, i.id)).collect()
    }

    fn changed(i: &Input) -> TicketsEvent {
        TicketsEvent::InputChanged { input: i.clone() }
    }

    #[test]
    fn an_answer_is_its_inputs_in_the_stores_order() {
        let inputs = Inputs::from_events([
            TicketsEvent::Input { input: input("INPUT-9", "landing") },
            TicketsEvent::Input { input: input("INPUT-9", "checks") },
            TicketsEvent::Input { input: input("MNG-2", "x") },
            TicketsEvent::Error { message: "ignored".into() },
        ]);
        assert_eq!(ids(&inputs), ["INPUT-9#landing", "INPUT-9#checks", "MNG-2#x"], "kept as the server sent them");
        assert_eq!(inputs.open_count(), 3);
    }

    /// The store's order of tickets: shortcode, then the sequence as a
    /// number.
    #[test]
    fn tickets_order_by_shortcode_then_sequence() {
        assert!(ticket_order("MNG-9") < ticket_order("MNG-10"));
        assert!(ticket_order("INPUT-10") < ticket_order("MNG-1"));
        assert_eq!(ticket_order("MNG-10"), ("MNG", 10));
        assert_eq!(ticket_order("ODD"), ("ODD", u32::MAX));
    }

    /// `input_changed` is an upsert of the whole input: in place when held
    /// (its place kept, a move between states included); the same input
    /// again moves nothing; a new one goes after its ticket's inputs, in the
    /// store's order of tickets; every other event is not the copy's.
    #[test]
    fn input_changed_upserts_the_whole_input() {
        let mut inputs = Inputs::from_events([
            TicketsEvent::Input { input: input("MNG-9", "a") },
            TicketsEvent::Input { input: input("MNG-9", "b") },
            TicketsEvent::Input { input: input("MNG-12", "c") },
        ]);
        let b = answered(input("MNG-9", "b"), "archive", 100);
        assert!(inputs.apply(&changed(&b)));
        assert!(!inputs.apply(&changed(&b)), "the same input again");
        assert_eq!(ids(&inputs), ["MNG-9#a", "MNG-9#b", "MNG-12#c"], "in place");
        assert_eq!(inputs.list()[1].state, InputState::Answered);
        assert_eq!(inputs.open_count(), 2);

        // New inputs: after their ticket's, before a later ticket's; a
        // ticket the copy holds nothing of, where its id sorts.
        assert!(inputs.apply(&changed(&input("MNG-9", "d"))));
        assert!(inputs.apply(&changed(&input("MNG-10", "e"))));
        assert!(inputs.apply(&changed(&input("AAA-1", "f"))));
        assert!(inputs.apply(&changed(&input("ZZZ-1", "g"))));
        assert_eq!(
            ids(&inputs),
            ["AAA-1#f", "MNG-9#a", "MNG-9#b", "MNG-9#d", "MNG-10#e", "MNG-12#c", "ZZZ-1#g"]
        );

        // Removed is a state like the others: still held, still in place.
        let mut gone = input("MNG-9", "a");
        gone.state = InputState::Removed;
        assert!(inputs.apply(&changed(&gone)));
        assert_eq!(inputs.list()[1], gone);

        assert!(!inputs.apply(&TicketsEvent::Lagged { missed: 1 }));
        assert!(!Inputs::concerns(&TicketsEvent::Lagged { missed: 1 }));
        assert!(!Inputs::concerns(&TicketsEvent::Input { input: input("MNG-9", "a") }), "an answer's item, not a change");
        assert!(Inputs::concerns(&changed(&gone)));
    }

    /// The fetch's rule: the events heard while it was out, replayed in
    /// order onto its answer, give the newest state whether the answer was
    /// taken before them or after.
    #[test]
    fn events_replayed_onto_an_answer_give_the_newest_state_either_way() {
        let a1 = input("INPUT-9", "landing");
        let a2 = answered(a1.clone(), "archive", 100);
        let mut a3 = a2.clone();
        a3.state = InputState::Closed;
        let born = input("INPUT-9", "notes");
        let heard = [changed(&a2), changed(&born), changed(&a3)];
        let older = vec![TicketsEvent::Input { input: a1 }];
        let newer = vec![TicketsEvent::Input { input: a3.clone() }, TicketsEvent::Input { input: born.clone() }];
        for answer in [older, newer] {
            let held = settle(answer, &heard);
            assert_eq!(held.list(), [a3.clone(), born.clone()]);
            assert_eq!(held.open_count(), 1);
        }
    }
}
