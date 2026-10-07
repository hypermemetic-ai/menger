//! INPUT-3: the inputs store — a test per row of the ticket's Required
//! behavior: inputs parsed on create and amend (and a duplicate id refused),
//! `answer` checked and recorded (and each refusal), `clear_answer` /
//! `close_input` / `reopen_input` as history, `inputs` and its filters,
//! `input_changed` after the write's other events, an input leaving its body,
//! and a database from before the tables — plus durability, the body never
//! touched by an answer, and bodies without inputs storing nothing.

use super::*;
use crate::types::{AnswerSource, InputAction, InputKind, InputOption, InputState, InputValue};

fn ticket(epic: &str, kind: Kind, body: &str) -> NewTicket {
    NewTicket {
        epic: epic.into(),
        title: format!("a {} in {epic}", kind.as_str()),
        kind,
        severity: None,
        blocked_by: Vec::new(),
        parent: None,
        path: None,
        body: body.into(),
    }
}

/// A store with a scope root, `Q-1`, to file builds under.
async fn store() -> Store {
    let store = Store::new();
    store.create(ticket("Q", Kind::Scope, "the root")).await.unwrap();
    store
}

async fn born(store: &Store, body: &str) -> String {
    store.create(ticket("Q", Kind::Build, body)).await.unwrap().id
}

async fn amend(store: &Store, id: &str, body: &str) {
    store.amend(id, None, Some(body.into())).await.unwrap();
}

/// The house decision, with an id.
fn decision(id: &str, options: &[&str]) -> String {
    let options: String = options
        .iter()
        .map(|o| format!("<m-option data-field=\"option\" name=\"{o}\" value=\"{}\"></m-option>\n", o.to_lowercase()))
        .collect();
    format!(
        "<m-decision data-field=\"decision\" id=\"{id}\">\n\n<m-question data-field=\"question\">Pick for {id}?</m-question>\n\n\
         {options}\n<m-recommendation data-field=\"recommendation\">The first.</m-recommendation>\n\n</m-decision>\n"
    )
}

fn multi(id: &str, choices: &[&str]) -> String {
    let choices: String = choices.iter().map(|c| format!("<m-choice value=\"{c}\">{c}</m-choice>")).collect();
    format!("<m-input id=\"{id}\" kind=\"multi\"><m-question>Which?</m-question>{choices}</m-input>\n")
}

fn text(id: &str) -> String {
    format!("<m-input id=\"{id}\" kind=\"text\"><m-question>Anything else?</m-question></m-input>\n")
}

fn choice(o: &str) -> InputValue {
    InputValue::Choice { option: o.into() }
}

fn one(store: &Store, ticket: &str, id: &str) -> Input {
    store
        .inputs(Some(ticket), None, None)
        .unwrap()
        .into_iter()
        .find(|i| i.id == id)
        .unwrap_or_else(|| panic!("{ticket} has no input {id}"))
}

fn changed(w: InputWrite) -> Input {
    match w {
        InputWrite::Changed(i) => i,
        other => panic!("expected a change, got {other:?}"),
    }
}

fn unchanged(w: InputWrite) -> Input {
    match w {
        InputWrite::Unchanged(i) => i,
        other => panic!("expected no change, got {other:?}"),
    }
}

/// Everything on the channel now.
fn drain(rx: &mut broadcast::Receiver<Change>) -> Vec<Change> {
    std::iter::from_fn(|| rx.try_recv().ok()).collect()
}

/// The input events of a write, as (input id, state).
fn heard(changes: &[Change]) -> Vec<(String, InputState)> {
    changes
        .iter()
        .filter_map(|c| match c {
            Change::InputChanged { input, .. } => Some((input.id.clone(), input.state)),
            _ => None,
        })
        .collect()
}

fn actions(input: &Input) -> Vec<InputAction> {
    input.history.iter().map(|e| e.action).collect()
}

/// Row 1: a body created or amended — its inputs parsed and stored: kind,
/// question, options (value, else name; m-choice), recommendation; born
/// open, with no answer and no history.
#[tokio::test]
async fn a_body_written_declares_its_inputs() {
    let store = store().await;
    let id = born(&store, &format!("Intro.\n\n{}\n{}\n{}", decision("landing", &["Archive", "Delete"]), multi("checks", &["unit", "wire"]), text("notes"))).await;
    let inputs = store.inputs(Some(&id), None, None).unwrap();
    assert_eq!(inputs.iter().map(|i| (i.id.as_str(), i.kind)).collect::<Vec<_>>(), [
        ("landing", InputKind::Choice),
        ("checks", InputKind::Multi),
        ("notes", InputKind::Text)
    ]);
    let landing = &inputs[0];
    assert_eq!(landing.ticket, id);
    assert_eq!(landing.question.as_deref(), Some("Pick for landing?"));
    assert_eq!(
        landing.options,
        vec![
            InputOption { value: "archive".into(), label: "Archive".into(), recommended: false },
            InputOption { value: "delete".into(), label: "Delete".into(), recommended: false },
        ]
    );
    assert_eq!(landing.recommendation.as_deref(), Some("The first."));
    for i in &inputs {
        assert_eq!((i.state, i.answer.as_ref(), i.history.len()), (InputState::Open, None, 0), "{}", i.id);
    }
    assert_eq!(inputs[2].options, vec![], "a text input has no options");

    // An amend declares too, and re-declares: new options, history kept.
    let other = born(&store, "nothing yet").await;
    assert!(store.inputs(Some(&other), None, None).unwrap().is_empty(), "a body without inputs stores nothing");
    amend(&store, &other, &decision("go", &["Yes", "No"])).await;
    changed(store.answer(&other, "go", choice("yes"), None, AnswerSource::Api).await.unwrap());
    amend(&store, &other, &decision("go", &["Yes", "No", "Later"])).await;
    let go = one(&store, &other, "go");
    assert_eq!(go.options.len(), 3);
    assert_eq!((go.state, actions(&go)), (InputState::Answered, vec![InputAction::Answered]), "history kept");
}

/// Row 1's refusal: a duplicate id in one body refuses the write in-band,
/// naming the id — a create stores nothing and spends no id; an amend
/// changes nothing, title included; nothing is heard. Other malformed
/// markup the same way.
#[tokio::test]
async fn a_duplicate_input_id_refuses_the_write_naming_it() {
    let store = store().await;
    let mut rx = store.subscribe();
    let twice = format!("{}\n{}", decision("x", &["A"]), text("x"));
    let e = store.create(ticket("Q", Kind::Build, &twice)).await.unwrap_err();
    assert!(matches!(&e, StoreError::InputMarkup { id: None, problem: crate::inputs::Problem::Twice { input } } if input == "x"), "{e:?}");
    assert!(e.to_string().contains("the input 'x' twice") && e.to_string().contains("nothing was created"), "{e}");
    assert!(drain(&mut rx).is_empty(), "nothing heard");
    let id = born(&store, "plain").await;
    assert_eq!(id, "Q-2", "no id spent");

    let before = store.get(&id).await.unwrap();
    let e = store.amend(&id, Some("new title".into()), Some(twice)).await.unwrap_err();
    assert!(e.to_string().contains("'x' twice") && e.to_string().contains("nothing changed for 'Q-2'"), "{e}");
    assert_eq!(store.get(&id).await.unwrap(), before, "title and body as they were");
    let e = store.amend(&id, None, Some("<m-input id=\"k\" kind=\"choice\"></m-input>".into())).await.unwrap_err();
    assert!(e.to_string().contains("a choice is an <m-decision>"), "{e}");
    assert!(store.inputs(None, None, None).unwrap().is_empty());
}

/// Row 2: `answer` — checked against the kind and options; recorded with
/// its source (`app` when given, else `api`), rationale and time; a change
/// of answer is another entry. The body and the ticket's row untouched.
#[tokio::test]
async fn an_answer_is_checked_and_recorded_with_source_and_time() {
    let store = store().await;
    let id = born(&store, &format!("{}{}{}", decision("d", &["A", "B"]), multi("m", &["x", "y", "z"]), text("t"))).await;
    let before = store.get(&id).await.unwrap();
    let t0 = chrono::Utc::now().timestamp();

    let d = changed(store.answer(&id, "d", choice("a"), Some("first look".into()), AnswerSource::App).await.unwrap());
    assert_eq!(d.state, InputState::Answered);
    let a = d.answer.clone().unwrap();
    assert_eq!((a.value, a.rationale.as_deref(), a.source), (choice("a"), Some("first look"), AnswerSource::App));
    assert!(a.at_unix >= t0);
    let d = changed(store.answer(&id, "d", choice("b"), None, AnswerSource::Api).await.unwrap());
    assert_eq!(d.answer.as_ref().map(|a| (&a.value, a.source)), Some((&choice("b"), AnswerSource::Api)));
    assert_eq!(d.history.len(), 2, "every answer is history");
    assert_eq!(d.history[0].value, Some(choice("a")));
    assert_eq!(one(&store, &id, "d"), d, "inputs shows what answer said");

    let m = changed(
        store
            .answer(&id, "m", InputValue::Multi { options: vec!["z".into(), "x".into(), "z".into()] }, None, AnswerSource::Api)
            .await
            .unwrap(),
    );
    assert_eq!(m.answer.unwrap().value, InputValue::Multi { options: vec!["x".into(), "z".into()] }, "a set, in the body's order");
    let t = changed(store.answer(&id, "t", InputValue::Text { text: "Yes, ship it".into() }, None, AnswerSource::Api).await.unwrap());
    assert_eq!(t.answer.unwrap().value, InputValue::Text { text: "Yes, ship it".into() });

    let after = store.get(&id).await.unwrap();
    assert_eq!((after.body, after.updated_unix), (before.body, before.updated_unix), "a click never rewrites a body");
}

/// Row 2's refusals, in-band, nothing written, nothing heard: the ticket is
/// none (`NotFound`), the input unknown, the value invalid (another kind, an
/// option not declared, a blank reply), closed, removed.
#[tokio::test]
async fn an_answer_is_refused_unknown_invalid_closed_or_removed() {
    let store = store().await;
    let id = born(&store, &format!("{}{}", decision("d", &["A"]), text("t"))).await;
    let mut rx = store.subscribe();
    let e = store.answer("Q-99", "d", choice("a"), None, AnswerSource::Api).await.unwrap_err();
    assert!(matches!(e, StoreError::NotFound(ref t) if t == "Q-99"), "{e:?}");
    let e = store.answer(&id, "nope", choice("a"), None, AnswerSource::Api).await.unwrap_err();
    assert_eq!(e.to_string(), "Q-2 declares no input 'nope' — its inputs are 'd', 't' (nothing changed)");
    let e = store.answer("Q-1", "d", choice("a"), None, AnswerSource::Api).await.unwrap_err();
    assert!(e.to_string().contains("its body holds none"), "{e}");
    for (input, value, says) in [
        ("d", choice("z"), "'z' is not one of its options ('a')"),
        ("d", InputValue::Text { text: "a".into() }, "the input is a choice and the value a text"),
        ("t", InputValue::Text { text: " ".into() }, "a reply is not blank"),
    ] {
        let e = store.answer(&id, input, value, None, AnswerSource::Api).await.unwrap_err();
        assert!(matches!(e, StoreError::InputInvalid { .. }), "{e:?}");
        assert!(e.to_string().contains(says) && e.to_string().starts_with(&format!("not an answer to Q-2's '{input}'")), "{e}");
    }
    assert!(drain(&mut rx).is_empty(), "no refusal is heard");
    assert!(store.inputs(Some(&id), None, None).unwrap().iter().all(|i| i.history.is_empty()), "nothing written");

    changed(store.close_input(&id, "d").await.unwrap());
    let e = store.answer(&id, "d", choice("a"), None, AnswerSource::Api).await.unwrap_err();
    assert_eq!(e.to_string(), "Q-2's 'd' is closed: its answer is frozen — reopen_input it first (nothing changed)");
    let e = store.clear_answer(&id, "d", AnswerSource::Api).await.unwrap_err();
    assert!(matches!(e, StoreError::InputClosed { .. }), "{e:?}");

    amend(&store, &id, &text("t")).await; // `d` leaves the body
    for e in [
        store.answer(&id, "d", choice("a"), None, AnswerSource::Api).await.unwrap_err(),
        store.clear_answer(&id, "d", AnswerSource::Api).await.unwrap_err(),
        store.close_input(&id, "d").await.unwrap_err(),
        store.reopen_input(&id, "d").await.unwrap_err(),
    ] {
        assert!(matches!(e, StoreError::InputRemoved { .. }), "{e:?}");
        assert!(e.to_string().contains("was removed from its body"), "{e}");
    }
}

/// Row 3: `clear_answer`, `close_input`, `reopen_input` move the state and
/// each is history; what changes nothing writes nothing.
#[tokio::test]
async fn clear_close_and_reopen_move_the_state_and_are_history() {
    let store = store().await;
    let id = born(&store, &decision("d", &["A", "B"])).await;
    let mut rx = store.subscribe();
    assert_eq!(unchanged(store.clear_answer(&id, "d", AnswerSource::Api).await.unwrap()).state, InputState::Open, "nothing to clear");
    assert_eq!(unchanged(store.reopen_input(&id, "d").await.unwrap()).state, InputState::Open, "not closed");
    assert!(drain(&mut rx).is_empty(), "a no-op is not heard");

    changed(store.answer(&id, "d", choice("a"), None, AnswerSource::App).await.unwrap());
    let d = changed(store.clear_answer(&id, "d", AnswerSource::App).await.unwrap());
    assert_eq!((d.state, d.answer.as_ref()), (InputState::Open, None));
    changed(store.answer(&id, "d", choice("b"), Some("settled".into()), AnswerSource::Api).await.unwrap());
    let d = changed(store.close_input(&id, "d").await.unwrap());
    assert_eq!(d.state, InputState::Closed);
    assert_eq!(d.answer.as_ref().map(|a| &a.value), Some(&choice("b")), "closed keeps its answer");
    assert_eq!(unchanged(store.close_input(&id, "d").await.unwrap()).state, InputState::Closed);
    let d = changed(store.reopen_input(&id, "d").await.unwrap());
    assert_eq!(d.state, InputState::Answered, "reopened to its answer");
    use InputAction::*;
    assert_eq!(actions(&d), [Answered, Cleared, Answered, Closed, Reopened]);
    assert_eq!(d.history[1].source, AnswerSource::App, "INPUT-9: a clear is the source it is given");
    assert!(d.history[3..].iter().all(|e| e.source == AnswerSource::Api), "close and reopen are the api's");
    assert_eq!(heard(&drain(&mut rx)).len(), 5, "one input_changed per change");

    // Closed without an answer, reopened to open.
    let e = born(&store, &decision("e", &["A"])).await;
    assert_eq!(changed(store.close_input(&e, "e").await.unwrap()).state, InputState::Closed);
    assert_eq!(changed(store.reopen_input(&e, "e").await.unwrap()).state, InputState::Open);
}

/// Row 4: `inputs { ticket?, epic?, state? }` — every input in the store's
/// order of tickets (Q-10 after Q-9), each ticket's in its body's order;
/// filtered by ticket, by shortcode, by state; a ticket that is none is
/// `NotFound`.
#[tokio::test]
async fn inputs_answers_in_the_stores_order_and_filters() {
    let store = store().await;
    for _ in 2..=8 {
        born(&store, "plain").await;
    }
    let q9 = born(&store, &format!("{}{}", decision("b", &["A"]), decision("a", &["A"]))).await;
    let q10 = born(&store, &text("c")).await;
    store.create(ticket("R", Kind::Scope, &text("r"))).await.unwrap();
    changed(store.answer(&q9, "a", choice("a"), None, AnswerSource::Api).await.unwrap());

    let all: Vec<(String, String)> = store.inputs(None, None, None).unwrap().into_iter().map(|i| (i.ticket, i.id)).collect();
    let pairs = |v: &[(&str, &str)]| v.iter().map(|(t, i)| (t.to_string(), i.to_string())).collect::<Vec<_>>();
    assert_eq!(all, pairs(&[("Q-9", "b"), ("Q-9", "a"), ("Q-10", "c"), ("R-1", "r")]));
    let ids = |v: Vec<Input>| v.into_iter().map(|i| i.id).collect::<Vec<_>>();
    assert_eq!(ids(store.inputs(Some("q-10"), None, None).unwrap()), ["c"], "ids fold to upper case");
    assert_eq!(ids(store.inputs(None, Some("r"), None).unwrap()), ["r"]);
    assert_eq!(ids(store.inputs(None, None, Some(InputState::Answered)).unwrap()), ["a"]);
    assert_eq!(ids(store.inputs(None, Some("Q"), Some(InputState::Open)).unwrap()), ["b", "c"]);
    assert!(store.inputs(Some(&q10), Some("R"), None).unwrap().is_empty(), "both filters hold");
    assert!(store.inputs(Some("Q-3"), None, None).unwrap().is_empty(), "a ticket without inputs answers nothing");
    assert!(matches!(store.inputs(Some("Q-99"), None, None), Err(StoreError::NotFound(t)) if t == "Q-99"));
}

/// Row 5: `input_changed` after the write's other events — a birth's
/// after its `created` and its component events; an amend's after its
/// `amended`, in the body's order, the removed after; an answer's alone;
/// `watch { epic }` filters it by its ticket's shortcode.
#[tokio::test]
async fn input_changed_comes_after_the_writes_other_events() {
    let store = store().await;
    let mut rx = store.subscribe();
    let body = format!("```component m-q\n<template>q</template>\n```\n\n<m-q></m-q>\n\n{}{}", decision("one", &["A"]), text("two"));
    let id = born(&store, &body).await;
    let changes = drain(&mut rx);
    assert!(matches!(changes.first(), Some(Change::Created { .. })), "{changes:?}");
    let first_input = changes.iter().position(|c| matches!(c, Change::InputChanged { .. })).unwrap();
    assert!(changes[..first_input].iter().any(|c| matches!(c, Change::ComponentChanged { .. })), "after the components");
    assert!(changes[first_input..].iter().all(|c| matches!(c, Change::InputChanged { .. })), "last of all: {changes:?}");
    assert_eq!(heard(&changes), [("one".to_string(), InputState::Open), ("two".to_string(), InputState::Open)]);

    amend(&store, &id, &format!("{}{}", text("three"), text("two"))).await;
    let changes = drain(&mut rx);
    assert!(matches!(changes[0], Change::Amended { .. }));
    assert_eq!(heard(&changes), [("three".to_string(), InputState::Open), ("one".to_string(), InputState::Removed)], "an unchanged input is not heard");

    changed(store.answer(&id, "two", InputValue::Text { text: "hi".into() }, None, AnswerSource::App).await.unwrap());
    let changes = drain(&mut rx);
    assert_eq!(changes.len(), 1, "{changes:?}");
    match &changes[0] {
        Change::InputChanged { epic, input } => {
            assert_eq!((epic.as_str(), input.ticket.as_str(), input.state), ("Q", id.as_str(), InputState::Answered));
            assert_eq!(input.answer.as_ref().map(|a| a.source), Some(AnswerSource::App));
            assert!(changes[0].clone().in_epic(Some("Q")).is_some());
            assert!(changes[0].clone().in_epic(Some("OTHER")).is_none(), "filtered by its ticket's shortcode");
        }
        other => panic!("{other:?}"),
    }
}

/// Row 6: an input that leaves its body is `removed`, history kept; one
/// declared again comes back to what its history says — closed stays closed.
#[tokio::test]
async fn an_input_leaving_its_body_is_removed_with_its_history() {
    let store = store().await;
    let id = born(&store, &format!("{}{}", decision("d", &["A", "B"]), text("t"))).await;
    changed(store.answer(&id, "d", choice("b"), Some("why".into()), AnswerSource::App).await.unwrap());
    changed(store.close_input(&id, "d").await.unwrap());

    amend(&store, &id, &text("t")).await;
    let d = one(&store, &id, "d");
    assert_eq!(d.state, InputState::Removed);
    assert_eq!(actions(&d), [InputAction::Answered, InputAction::Closed], "history kept");
    assert_eq!(d.answer.as_ref().map(|a| &a.value), Some(&choice("b")), "its last answer too");
    assert_eq!(store.inputs(Some(&id), None, None).unwrap().iter().map(|i| i.id.as_str()).collect::<Vec<_>>(), ["t", "d"], "the removed after the body's");
    assert_eq!(store.inputs(None, None, Some(InputState::Removed)).unwrap().len(), 1);

    // Every input gone: still kept.
    amend(&store, &id, "no inputs now").await;
    assert_eq!(store.inputs(Some(&id), None, Some(InputState::Removed)).unwrap().len(), 2);

    amend(&store, &id, &decision("d", &["A", "B"])).await;
    let d = one(&store, &id, "d");
    assert_eq!(d.state, InputState::Closed, "back to its history's state");
    assert_eq!(d.history.len(), 2);
}

/// What INPUT-1 decided: an answer stays what was recorded when a later
/// body changes the options it named — the history is a record.
#[tokio::test]
async fn an_answer_stays_when_the_body_changes_its_options() {
    let store = store().await;
    let id = born(&store, &decision("d", &["A", "B"])).await;
    changed(store.answer(&id, "d", choice("a"), None, AnswerSource::Api).await.unwrap());
    amend(&store, &id, &decision("d", &["B", "C"])).await;
    let d = one(&store, &id, "d");
    assert_eq!((d.state, d.answer.map(|a| a.value)), (InputState::Answered, Some(choice("a"))));
    assert!(store.answer(&id, "d", choice("a"), None, AnswerSource::Api).await.is_err(), "a new answer is checked against the new options");
}

/// The store is durable: a reopened file holds every input, its answer and
/// its history as acknowledged, removed ones included, and keeps moving them.
#[tokio::test]
async fn inputs_and_their_history_survive_a_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("inputs.db");
    let before = {
        let store = Store::open(&path).unwrap();
        store.create(ticket("Q", Kind::Scope, "root")).await.unwrap();
        let id = born(&store, &format!("{}{}{}", decision("d", &["A", "B"]), multi("m", &["x", "y"]), text("t"))).await;
        changed(store.answer(&id, "d", choice("a"), Some("r".into()), AnswerSource::App).await.unwrap());
        changed(store.answer(&id, "m", InputValue::Multi { options: vec!["y".into()] }, None, AnswerSource::Api).await.unwrap());
        changed(store.close_input(&id, "m").await.unwrap());
        changed(store.answer(&id, "t", InputValue::Text { text: "a \"quoted\", reply".into() }, None, AnswerSource::Api).await.unwrap());
        amend(&store, &id, &format!("{}{}", multi("m", &["x", "y"]), decision("d", &["A", "B"]))).await; // t removed
        store.inputs(None, None, None).unwrap()
    };
    assert_eq!(before.iter().map(|i| (i.id.as_str(), i.state)).collect::<Vec<_>>(), [
        ("m", InputState::Closed),
        ("d", InputState::Answered),
        ("t", InputState::Removed)
    ]);
    let store = Store::open(&path).unwrap();
    assert_eq!(store.inputs(None, None, None).unwrap(), before);
    changed(store.reopen_input("Q-2", "m").await.unwrap());
    drop(store);
    let store = Store::open(&path).unwrap();
    assert_eq!(one(&store, "Q-2", "m").state, InputState::Answered);
}

/// A database without the tables — this build's own file with them taken
/// away, as prod's is — gains them on open, durably, once, with every
/// body's inputs read in (born open, no history); a malformed input is
/// skipped (of two with one id, the first kept) and the ticket's next body
/// change still holding it is refused. Every ticket as it was.
#[tokio::test]
async fn a_database_without_the_tables_gains_them_read_from_the_bodies() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("before.db");
    {
        let store = Store::open(&path).unwrap();
        store.create(ticket("Q", Kind::Scope, "root")).await.unwrap();
        born(&store, "plain").await; // Q-2: will declare two inputs
        born(&store, "plain").await; // Q-3: will hold a duplicate id
        born(&store, "plain").await; // Q-4: will hold markup in code only
    }
    let count = {
        // As a binary from before INPUT-3 would leave it: no tables, and bodies
        // written without the store hearing.
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch("DROP TABLE inputs; DROP TABLE answers;").unwrap();
        for (id, body) in [
            ("Q-2", format!("{}{}", decision("d", &["A", "B"]), text("t"))),
            ("Q-3", format!("{}{}", text("x"), multi("x", &["a"]))),
            ("Q-4", "```html\n<m-decision id=\"code\"></m-decision>\n```\n".to_string()),
        ] {
            conn.execute("UPDATE tickets SET body = ?1 WHERE id = ?2", [body, id.to_string()]).unwrap();
        }
        conn.query_row("SELECT count(*) FROM tickets", [], |r| r.get::<_, i64>(0)).unwrap()
    };
    let store = Store::open(&path).unwrap();
    assert_eq!(store.count() as i64, count, "every ticket kept");
    let all = store.inputs(None, None, None).unwrap();
    assert_eq!(all.iter().map(|i| (i.ticket.as_str(), i.id.as_str(), i.kind)).collect::<Vec<_>>(), [
        ("Q-2", "d", InputKind::Choice),
        ("Q-2", "t", InputKind::Text),
        ("Q-3", "x", InputKind::Text)
    ]);
    assert!(all.iter().all(|i| i.state == InputState::Open && i.history.is_empty()));
    assert_eq!(all[0].options.len(), 2);
    let e = store.amend("Q-3", None, Some(format!("{}{}", text("x"), text("x")))).await.unwrap_err();
    assert!(e.to_string().contains("'x' twice"), "{e}");
    changed(store.answer("Q-2", "d", choice("a"), None, AnswerSource::Api).await.unwrap());
    drop(store);

    // Once: the tables are there, and the next open reads them.
    let mut conn = Connection::open(&path).unwrap();
    assert!(!inputs::migrate(&mut conn).unwrap(), "the next open reads nothing");
    let rows: i64 = conn.query_row("SELECT count(*) FROM inputs", [], |r| r.get(0)).unwrap();
    let answers: i64 = conn.query_row("SELECT count(*) FROM answers", [], |r| r.get(0)).unwrap();
    assert_eq!((rows, answers), (3, 1));
    drop(conn);
    let store = Store::open(&path).unwrap();
    assert_eq!(one(&store, "Q-2", "d").state, InputState::Answered);
}

/// What must not change: a seed writes no inputs — synthetic bodies hold
/// none, held to every body a seed generates.
#[test]
fn synthetic_bodies_declare_no_inputs() {
    let generated = synthetic::generate(
        SeedSpec {
            count: 2_000,
            seed: 7,
            into_existing: false,
        },
        &HashMap::new(),
    );
    for t in &generated {
        assert_eq!(crate::inputs::read(&t.body), (vec![], vec![]), "{}", t.id);
    }
}
