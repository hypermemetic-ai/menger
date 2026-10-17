//! APP-46: **answering on the mock** — the controls the host draws on a
//! mockup's bar (owner, 2026-10-16: *"I want the decision to contain
//! mockups maybe, or for feedback to be left directly on the mockup."*).
//!
//! - **pick this** — a mockup inside an `<m-option>` of an `<m-decision
//!   id>` is that option's look. The SDK's placeholder posts the option it
//!   sits in (`option: { input, value, name }`, [`choice`]); the bar then
//!   carries *pick this*, which records that option as the input's answer
//!   through **the same answer path** a frame's Submit takes
//!   ([`bridge::admit`], then [`bridge::submit`]: the ticket's inputs read,
//!   the value checked against them, `tickets.inputs.answer`, its source the
//!   server's from the caller) — the ticket the host stamped on the body's
//!   frame, never one the frame names; display only in a cited window. The
//!   gesture it waits for is the host's own ([`gesture::on_host`]): the
//!   browser's own click on this button, inside the app's activation. The
//!   button is in the app's DOM, where nothing in a frame reaches; inside
//!   the mocked page nothing answers an input (src/mockup.rs).
//! - **The mark** ([`mark`]) — what the input's record says of this
//!   option: the answer (the box takes the pass border), decided (closed
//!   on it), weighed (closed on another), or why it cannot be picked.
//!   The record is the ticket's inputs, read over the viewer's socket the
//!   first time a box asks and kept by `watch`'s `input_changed`
//!   ([`heard`]). A DECIDED decision draws its chosen option first — the
//!   SDK's ordering in the body, which the boxes follow.
//! - **comment** — every mockup's bar opens the composer for the mockup as
//!   a whole (in a decision, for that option), without annotation mode. Its
//!   anchor is APP-45's form for an element inside a mockup
//!   (src/mock_anchor.rs: `mockup:<ticket>#n[=ref]@<width> >> body`), at
//!   the mocked page's root ([`comment_anchor`]) — so its pin is drawn on
//!   the device and `#/comments`' link back finds it as any comment inside
//!   a mockup is found.
//! - **compare these** — on an option's look, opens APP-44's compare view
//!   (src/compare.rs) on every mockup of that decision; each column carries
//!   this same [`bar`] — the option's name, its mark and *pick this*, so the
//!   same answer path and the same gesture check ([`COMPARE`]).
//!
//! What decides is pure and pinned by native tests; the rest is DOM.

use std::cell::RefCell;
use std::collections::HashMap;

use leptos::prelude::*;
use menger_tickets_api::{Anchor, Input, InputKind, InputState, InputValue, TicketsEvent};
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::spawn_local;

use crate::mock_anchor::{self, MockRef};
use crate::{anchor, bridge, gesture, mockup, session};

/// APP-44's compare view is there to open: an option's look carries
/// *compare these* (in the page's bar, not in a compare column's own).
pub(crate) const COMPARE: bool = true;

/// The longest input id or option value a placeholder may name (the house
/// writes short ones; a longer one is no option of a body).
const NAME_MAX: usize = 200;

/// The option a mockup sits in: its decision's input, its value, its name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Choice {
    pub(crate) input: String,
    pub(crate) value: String,
    pub(crate) name: Option<String>,
}

/// What the placeholder posted as its option, as a [`Choice`] — none when
/// it names no input or no value, or one past reason.
pub(crate) fn choice(input: Option<&str>, value: Option<&str>, name: Option<&str>) -> Option<Choice> {
    let ok = |s: Option<&str>| s.map(str::trim).filter(|s| !s.is_empty() && s.len() <= NAME_MAX).map(str::to_string);
    Some(Choice { input: ok(input)?, value: ok(value)?, name: ok(name) })
}

/// What the input's record says of one option's mockup.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Mark {
    /// The record is not read yet.
    Loading,
    /// It may be picked.
    Open,
    /// It is the recorded answer (the input still open to a change).
    Answer,
    /// It is the answer, and the input is closed: DECIDED.
    Decided,
    /// The input is closed on another option.
    Weighed,
    /// It cannot be picked, and why.
    Shut(String),
}

/// [`Mark`] for `value` of input `id`, from `inputs` — the ticket's inputs
/// as read (`None`: not yet).
pub(crate) fn mark(inputs: Option<&[Input]>, id: &str, value: &str) -> Mark {
    let Some(inputs) = inputs else { return Mark::Loading };
    let Some(input) = inputs.iter().find(|i| i.id.as_str() == id) else {
        return Mark::Shut(format!("this ticket's saved body has no input '{id}'"));
    };
    if input.kind != InputKind::Choice {
        return Mark::Shut(format!("'{id}' is no decision"));
    }
    if !input.options.iter().any(|o| o.value == value) {
        return Mark::Shut(format!("'{value}' is not one of '{id}'s options"));
    }
    let answered = matches!(&input.answer, Some(a) if matches!(&a.value, InputValue::Choice { option } if option == value));
    match input.state {
        InputState::Removed => Mark::Shut(format!("'{id}' was removed from the body")),
        InputState::Withdrawn => Mark::Shut(format!("'{id}' is withdrawn: the ticket is canceled")),
        InputState::Closed if answered => Mark::Decided,
        InputState::Closed if input.answer.is_some() => Mark::Weighed,
        InputState::Closed => Mark::Shut(format!("'{id}' is closed")),
        InputState::Open | InputState::Answered if answered => Mark::Answer,
        InputState::Open | InputState::Answered => Mark::Open,
    }
}

/// The option's name as the reader sees it: the store's label for it, else
/// the name the body wrote, else its value.
pub(crate) fn label(inputs: Option<&[Input]>, c: &Choice) -> String {
    inputs
        .and_then(|l| l.iter().find(|i| i.id.as_str() == c.input))
        .and_then(|i| i.options.iter().find(|o| o.value == c.value))
        .map(|o| o.label.trim().to_string())
        .filter(|l| !l.is_empty())
        .or_else(|| c.name.clone())
        .unwrap_or_else(|| c.value.clone())
}

/// What a *comment* on a mockup names: the mockup, and in a decision the
/// option it is the look of.
pub(crate) fn comment_label(option: Option<(&str, &str)>, title: Option<&str>) -> String {
    match (option, title.map(str::trim).filter(|t| !t.is_empty())) {
        (Some((input, name)), _) => format!("Mockup of option “{name}” ({input})"),
        (None, Some(t)) => format!("Mockup “{t}”"),
        (None, None) => "Mockup".to_string(),
    }
}

/// The anchor of a *comment* on mockup `m` as a whole: its page's root, in
/// APP-45's form; the ticket the body is. None for a mockup in no ticket's
/// saved body (a draft's preview).
pub(crate) fn comment_anchor(m: &MockRef, label: String, route: String) -> Option<Anchor> {
    let parsed = MockRef::parse(&m.text()).filter(|p| p == m && p.mock.is_some())?;
    Some(Anchor {
        path: mock_anchor::path_in(&parsed, anchor::FRAME_ROOT),
        label,
        excerpt: String::new(),
        ticket: Some(parsed.ticket),
        route,
        version: None,
    })
}

/// The word the bar shows for a mark (none: the *pick this* button alone).
pub(crate) fn mark_word(m: &Mark) -> Option<&'static str> {
    match m {
        Mark::Answer => Some("✓ answer"),
        Mark::Decided => Some("✓ decided"),
        Mark::Weighed => Some("weighed"),
        _ => None,
    }
}

/// The attribute the box wears for its mark (src/index.html's outline).
pub(crate) fn mark_attr(m: &Mark) -> &'static str {
    match m {
        Mark::Loading => "loading",
        Mark::Open => "open",
        Mark::Answer => "answer",
        Mark::Decided => "decided",
        Mark::Weighed => "weighed",
        Mark::Shut(_) => "shut",
    }
}

// -- the record ---------------------------------------------------------------

/// A ticket's inputs as read: `None` until the first answer.
type Held = ArcRwSignal<Option<Vec<Input>>>;

thread_local! {
    static HELD: RefCell<HashMap<String, Held>> = RefCell::new(HashMap::new());
}

/// `ticket`'s inputs, read the first time a box asks.
fn held(ticket: &str) -> Held {
    if let Some(h) = HELD.with(|m| m.borrow().get(ticket).cloned()) {
        return h;
    }
    let h: Held = ArcRwSignal::new(None);
    HELD.with(|m| m.borrow_mut().insert(ticket.to_string(), h.clone()));
    fetch(ticket.to_string());
    h
}

/// Read `ticket`'s inputs over the viewer's socket.
fn fetch(ticket: String) {
    spawn_local(async move {
        let Some(client) = session::client() else { return };
        let Ok(id) = crate::typed::typed(&ticket) else { return };
        let Ok(events) = client.inputs().list_all(Some(id), None, None, None, session::zone()).await else { return };
        let list = bridge::inputs_of(bridge::SUBMIT, events).unwrap_or_default();
        if let Some(h) = HELD.with(|m| m.borrow().get(&ticket).cloned()) {
            h.set(Some(list));
        }
    });
}

/// Every `watch` event: an input of a held ticket changed — its whole
/// record, kept in place.
pub(crate) fn heard(ev: &TicketsEvent) {
    let TicketsEvent::InputChanged { input } = ev else { return };
    let Some(h) = HELD.with(|m| m.borrow().get(input.ticket.as_str()).cloned()) else { return };
    h.update(|l| {
        let Some(l) = l else { return };
        match l.iter_mut().find(|i| i.id == input.id) {
            Some(i) => *i = input.clone(),
            None => l.push(input.clone()),
        }
    });
}

// -- the bar --------------------------------------------------------------------

/// *pick this*: `choice` recorded as its input's answer on the ticket the
/// host stamped on the mockup `key`'s body frame — the answer path a
/// frame's Submit takes, behind the host's own gesture.
fn pick(ev: &web_sys::MouseEvent, key: &str, c: &Choice, busy: ArcRwSignal<bool>, problem: ArcRwSignal<Option<String>>) {
    let gesture = gesture::on_host(ev.as_ref());
    let Some(citer) = mockup::citer_of(key) else { return };
    let ticket = citer.get_attribute("data-ticket");
    let cited = citer.has_attribute("data-cite-key");
    let submit = bridge::Submit { input: c.input.clone(), value: InputValue::Choice { option: c.value.clone() }, rationale: None };
    busy.set(true);
    problem.set(None);
    spawn_local(async move {
        let sent = async {
            if cited {
                return Err(bridge::display_only(bridge::SUBMIT));
            }
            if !gesture {
                return Err("pick this: only on your own click on it (nothing sent)".to_string());
            }
            let ticket = bridge::admit(bridge::SUBMIT, ticket.as_deref(), gesture)?;
            let client = session::client().ok_or_else(|| format!("{}: not connected (nothing sent)", bridge::SUBMIT))?;
            bridge::submit(&client, ticket.clone(), submit).await.map(|_| ticket)
        }
        .await;
        busy.set(false);
        match sent {
            // The watch carries the change; read once more so the mark
            // never waits on it.
            Ok(ticket) => fetch(ticket),
            Err(why) => problem.set(Some(why)),
        }
    });
}

/// What the mockup `key`'s bar adds at its head: in a decision, the option's
/// name, its mark and *pick this*; on every mockup, *comment*. `ticket` is
/// the body frame's (none: a preview or a help card — nothing to answer).
pub(crate) fn bar(key: String, ticket: Option<String>, c: Option<Choice>, title: Option<String>) -> ViewFn {
    bar_with(key, ticket, c, title, COMPARE)
}

/// [`bar`], in a compare column (APP-44): the same option name, mark, *pick
/// this* and *comment* for the page's box `key`, without *compare these*
/// (the view is open already).
pub(crate) fn column_bar(key: String, ticket: Option<String>, c: Option<Choice>, title: Option<String>) -> ViewFn {
    bar_with(key, ticket, c, title, false)
}

fn bar_with(key: String, ticket: Option<String>, c: Option<Choice>, title: Option<String>, compare_these: bool) -> ViewFn {
    let inputs = match (&c, &ticket) {
        (Some(_), Some(t)) => Some(held(t)),
        _ => None,
    };
    ViewFn::from(move || {
        let key = key.clone();
        let busy = ArcRwSignal::new(false);
        let problem = ArcRwSignal::new(None::<String>);
        let option = c.clone().map(|c| {
            let inputs = inputs.clone();
            let read = {
                let inputs = inputs.clone();
                move || inputs.as_ref().map(|h| h.get()).unwrap_or(Some(Vec::new()))
            };
            let m = {
                let (read, c) = (read.clone(), c.clone());
                Memo::new(move |_| match &inputs {
                    Some(_) => mark(read().as_deref(), &c.input, &c.value),
                    None => Mark::Shut("this frame shows no ticket's saved body — nothing to answer".into()),
                })
            };
            let name = {
                let (read, c) = (read.clone(), c.clone());
                move || label(read().as_deref(), &c)
            };
            let (key, busy2, problem2) = (key.clone(), busy.clone(), problem.clone());
            let disabled = {
                let busy = busy.clone();
                move || busy.get() || !matches!(m.get(), Mark::Open)
            };
            let input_id = c.input.clone();
            let why = move || match m.get() {
                Mark::Shut(why) => why,
                Mark::Loading => "reading the decision…".to_string(),
                _ => format!("Record this option as the answer to '{input_id}'"),
            };
            view! {
                <span class="mpick" data-mark=move || mark_attr(&m.get())>
                    <span class="mpick-name" title="the option this mockup is the look of">{name}</span>
                    {move || mark_word(&m.get()).map(|w| view! { <span class="mpick-mark">{w}</span> })}
                    {move || {
                        let shown = matches!(m.get(), Mark::Open | Mark::Loading | Mark::Shut(_));
                        let (key, c, busy2, problem2) = (key.clone(), c.clone(), busy2.clone(), problem2.clone());
                        shown.then(|| view! {
                            <button
                                class="framed-btn mpick-go"
                                title=why.clone()
                                disabled=disabled.clone()
                                on:click=move |ev: web_sys::MouseEvent| pick(&ev, &key, &c, busy2.clone(), problem2.clone())
                            >
                                "pick this"
                            </button>
                        })
                    }}
                </span>
            }
        });
        let comment_label = comment_label(c.as_ref().map(|c| (c.input.as_str(), c.name.as_deref().unwrap_or(c.value.as_str()))), title.as_deref());
        let comment_key = key.clone();
        let comment = ticket.is_some().then(|| view! {
            <button
                class="framed-btn mcomment"
                title="Leave a comment on this mockup"
                on:click=move |ev: web_sys::MouseEvent| {
                    let at = ev.current_target().and_then(|t| t.dyn_into::<web_sys::Element>().ok());
                    if let Some(at) = at {
                        mockup::comment_on(&comment_key, comment_label.clone(), at);
                    }
                }
            >
                "comment"
            </button>
        });
        // APP-44: an option's look opens the decision's looks side by side.
        let compare = (compare_these && c.is_some() && ticket.is_some()).then(|| {
            let k = key.clone();
            view! {
                <button class="framed-btn mcompare" title="Every option of this decision side by side, each with its pick this"
                    on:click=move |_| crate::compare::open_decision(&k)>
                    "compare these"
                </button>
            }
        });
        view! {
            {option}
            {compare}
            {comment}
            {move || problem.get().map(|p| view! { <span class="mpick-err" role="alert">{p}</span> })}
        }
        .into_any()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use menger_tickets_api::{Answer, AnswerSource, InputOption, TicketId};

    fn input(state: InputState, answer: Option<&str>) -> Input {
        Input {
            ticket: TicketId::new("APP-46"),
            id: crate::typed::typed("look").unwrap(),
            kind: InputKind::Choice,
            question: Some("Which look?".into()),
            options: ["a", "b", "c"].iter().map(|v| InputOption { value: v.to_string(), label: format!("Look {v}"), recommended: *v == "a" }).collect(),
            recommendation: None,
            state,
            answer: answer.map(|o| Answer { value: InputValue::Choice { option: o.into() }, rationale: None, source: AnswerSource::App, at_unix: 1 }),
            history: Vec::new(),
        }
    }

    /// A placeholder names its option by input and value; blank or
    /// unreasonable names are none.
    #[test]
    fn a_placeholder_names_its_option() {
        assert_eq!(
            choice(Some(" look "), Some("b"), Some("Look B")),
            Some(Choice { input: "look".into(), value: "b".into(), name: Some("Look B".into()) })
        );
        assert_eq!(choice(Some("look"), Some("b"), Some(" ")).unwrap().name, None);
        assert_eq!(choice(None, Some("b"), None), None);
        assert_eq!(choice(Some("look"), Some(""), None), None);
        assert_eq!(choice(Some(&"x".repeat(NAME_MAX + 1)), Some("b"), None), None);
    }

    /// The record marks each option: open to a pick, the answer, decided,
    /// weighed — and says why an option cannot be picked.
    #[test]
    fn the_record_marks_each_option() {
        assert_eq!(mark(None, "look", "a"), Mark::Loading);
        let open = [input(InputState::Open, None)];
        assert_eq!(mark(Some(&open), "look", "b"), Mark::Open);
        let answered = [input(InputState::Answered, Some("b"))];
        assert_eq!(mark(Some(&answered), "look", "b"), Mark::Answer);
        assert_eq!(mark(Some(&answered), "look", "a"), Mark::Open, "another option may still be picked");
        let closed = [input(InputState::Closed, Some("b"))];
        assert_eq!(mark(Some(&closed), "look", "b"), Mark::Decided);
        assert_eq!(mark(Some(&closed), "look", "c"), Mark::Weighed);
        let shut = |m: Mark| matches!(m, Mark::Shut(_));
        assert!(shut(mark(Some(&[input(InputState::Closed, None)]), "look", "a")));
        assert!(shut(mark(Some(&[input(InputState::Removed, None)]), "look", "a")));
        assert!(shut(mark(Some(&[input(InputState::Withdrawn, None)]), "look", "a")));
        assert!(shut(mark(Some(&open), "look", "zz")), "no option of the input");
        assert!(shut(mark(Some(&open), "other", "a")), "no such input in the saved body");
        let mut text = input(InputState::Open, None);
        text.kind = InputKind::Text;
        assert!(shut(mark(Some(&[text]), "look", "a")));
        assert_eq!(mark_word(&Mark::Answer), Some("✓ answer"));
        assert_eq!(mark_word(&Mark::Open), None);
        assert_eq!(mark_attr(&Mark::Decided), "decided");
    }

    /// The option is named as the store reads it, else as the body wrote it.
    #[test]
    fn an_option_is_named_by_the_store_first() {
        let c = choice(Some("look"), Some("b"), Some("Body's B")).unwrap();
        assert_eq!(label(Some(&[input(InputState::Open, None)]), &c), "Look b");
        assert_eq!(label(None, &c), "Body's B");
        assert_eq!(label(None, &choice(Some("look"), Some("b"), None).unwrap()), "b");
    }

    /// A comment on a mockup names it — and, in a decision, its option.
    #[test]
    fn a_comment_names_the_mockup_and_its_option() {
        assert_eq!(comment_label(Some(("look", "Dense")), Some("ignored")), "Mockup of option “Dense” (look)");
        assert_eq!(comment_label(None, Some(" Checkout ")), "Mockup “Checkout”");
        assert_eq!(comment_label(None, None), "Mockup");
    }

    /// A comment on a whole mockup is APP-45's anchor at the page's root —
    /// read back as that mockup — and none for a mockup in no saved body.
    #[test]
    fn a_comment_on_a_mockup_anchors_at_its_pages_root() {
        let m = MockRef { ticket: "APP-46".into(), mock: Some(3), reference: None, width: 1440 };
        let a = comment_anchor(&m, "Mockup of option “Dense” (look)".into(), "#/ticket/APP-46".into()).unwrap();
        assert_eq!(a.path, "mockup:APP-46#3@1440 >> body");
        assert_eq!(mock_anchor::of_path(&a.path), Some(m.clone()));
        assert_eq!(a.ticket.as_deref(), Some("APP-46"));
        assert!(anchor::still_there(&a.path, &a.excerpt, Some("any text")));
        let r = MockRef { reference: Some("DESIGN-49".into()), ..m.clone() };
        assert_eq!(comment_anchor(&r, String::new(), String::new()).unwrap().path, "mockup:APP-46#3=DESIGN-49@1440 >> body");
        assert_eq!(comment_anchor(&MockRef { ticket: String::new(), ..m.clone() }, String::new(), String::new()), None);
        assert_eq!(comment_anchor(&MockRef { mock: None, ..m }, String::new(), String::new()), None);
    }
}
