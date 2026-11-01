//! APP-86: a queue entry carries several questions — `--input` repeated and
//! `--blocking`: the inputs in order, the blocking one among them (required
//! at urgency `blocking` when there are several), closure only when every
//! named input is closed, a re-queue that adds inputs and raises, never
//! lowers; the one-input entry exactly as before; kept across a reopen;
//! and what `await_reply` hears.

use super::tests::Changes;
use super::*;
use crate::queue::{QueueAction, QueueEntry, Reply, Urgency};
use crate::types::{AnswerSource, InputValue, TransitionSource};

fn ticket(kind: Kind, body: &str) -> NewTicket {
    NewTicket { epic: "G".into(), title: format!("a {}", kind.as_str()), kind, severity: None, blocked_by: Vec::new(), parent: None, path: None, body: body.into() }
}

fn act(actor: &str) -> Act {
    Act { actor: actor.into(), name: None, source: TransitionSource::Api, message: None, cause: None }
}

const THREE: &str = "<m-decision data-field=\"decision\" id=\"format\">\n<m-question data-field=\"question\">Which format?</m-question>\n\
<m-option data-field=\"option\" name=\"Card\" value=\"card\"></m-option>\n<m-option data-field=\"option\" name=\"Sheet\" value=\"sheet\"></m-option>\n</m-decision>\n\n\
<m-input id=\"fields\" kind=\"multi\"><m-question data-field=\"question\">Which fields?</m-question>\n<m-choice value=\"cone\">Cone</m-choice><m-choice value=\"photos\">Photos</m-choice></m-input>\n\n\
<m-input id=\"naming\" kind=\"text\"><m-question data-field=\"question\">What is it called?</m-question></m-input>\n";

/// G-1 a scope; G-2 asks `format`, `fields` and `naming`; G-3 a plain build.
async fn store() -> Store {
    let store = Store::new();
    store.create(SYS, ticket(Kind::Scope, "root")).await.unwrap();
    store.create(SYS, ticket(Kind::Build, THREE)).await.unwrap();
    store.create(SYS, ticket(Kind::Build, "")).await.unwrap();
    store
}

fn ask(urgency: Urgency, inputs: &[&str], blocking: Option<&str>) -> QueueAsk {
    QueueAsk { urgency, reason: "the import is idle".into(), inputs: inputs.iter().map(|s| s.to_string()).collect(), blocking: blocking.map(str::to_string), ..QueueAsk::default() }
}

fn queued(c: QueueChange) -> (QueueEntry, QueueAction) {
    match c {
        QueueChange::Queued { entry, action, .. } => (entry, action),
        other => panic!("expected queued, got {other:?}"),
    }
}

fn names(e: &QueueEntry) -> Vec<&str> {
    e.named()
}

#[tokio::test]
async fn several_inputs_in_order_and_the_blocking_one_among_them() {
    let store = store().await;
    let (e, a) = queued(store.queue_with(SYS.permit("G-2"), ask(Urgency::Blocking, &["format", "fields", "naming", "format"], Some("naming")), act("agent:glaze")).await.unwrap());
    assert_eq!(a, QueueAction::Queued);
    assert_eq!(names(&e), ["format", "fields", "naming"], "in the order asked, once each");
    assert_eq!((e.blocking.as_deref(), e.input.as_deref()), (Some("naming"), Some("naming")), "`input` is the blocking one, for older readers");
    let rec = store.queue_history(SYS, "G-2").pop().unwrap();
    assert_eq!((rec.inputs.len(), rec.blocking.as_deref()), (3, Some("naming")));
    // Without a blocking one, `input` is the first.
    let (e, _) = queued(store.queue_with(SYS.permit("G-3"), ask(Urgency::Now, &[], None), act("a")).await.unwrap());
    assert!(e.inputs.is_empty() && e.input.is_none() && e.blocking.is_none());
}

#[tokio::test]
async fn blocking_with_several_names_which_and_it_must_be_one_of_them() {
    let store = store().await;
    let e = store.queue_with(SYS.permit("G-2"), ask(Urgency::Blocking, &["format", "naming"], None), act("a")).await.unwrap_err();
    assert!(e.to_string().contains("names the one the caller is idle on"), "{e}");
    let e = store.queue_with(SYS.permit("G-2"), ask(Urgency::Now, &["format"], Some("naming")), act("a")).await.unwrap_err();
    assert!(e.to_string().contains("not among the entry's inputs (format)"), "{e}");
    let e = store.queue_with(SYS.permit("G-2"), ask(Urgency::Now, &["format", "nope"], None), act("a")).await.unwrap_err();
    assert!(matches!(e, StoreError::InputUnknown { ref input, .. } if input == "nope"), "every input is checked: {e:?}");
    assert!(store.queued(SYS, true, 0).is_empty(), "nothing written");
    // Several at `now`, none blocking: fine.
    let (e, _) = queued(store.queue_with(SYS.permit("G-2"), ask(Urgency::Now, &["format", "naming"], None), act("a")).await.unwrap());
    assert_eq!((names(&e), e.blocking.as_deref(), e.input.as_deref()), (vec!["format", "naming"], None, Some("format")));
}

/// Backward compatibility: one `--input` at `blocking` is exactly MENGER-39's entry.
#[tokio::test]
async fn one_input_is_as_before() {
    let store = store().await;
    let (e, _) = queued(store.queue_by(SYS.permit("G-2"), Urgency::Blocking, "idle", Some("naming"), act("a")).await.unwrap());
    assert_eq!((e.input.as_deref(), names(&e), e.blocking_input()), (Some("naming"), vec!["naming"], Some("naming")));
    assert!(e.inputs.is_empty() && e.blocking.is_none(), "MENGER-39's entry, byte for byte: {e:?}");
    store.close_input(SYS.permit("G-2"), "naming").await.unwrap();
    assert!(store.queue_entry(SYS, "G-2").is_none(), "its one input closed: out");
    let last = store.queue_history(SYS, "G-2").pop().unwrap();
    assert_eq!((last.action, last.reason.as_deref()), (QueueAction::Closed, Some("its input `naming` was closed")));
}

#[tokio::test]
async fn the_entry_leaves_when_every_named_input_is_closed() {
    let store = store().await;
    store.queue_with(SYS.permit("G-2"), ask(Urgency::Blocking, &["format", "fields", "naming"], Some("naming")), act("agent:glaze")).await.unwrap();
    let mut rx = store.subscribe();
    store.answer(SYS.permit("G-2"), "format", InputValue::Choice { option: "card".into() }, None, AnswerSource::App).await.unwrap();
    store.close_input(SYS.permit("G-2"), "format").await.unwrap();
    store.close_input(SYS.permit("G-2"), "naming").await.unwrap();
    let held = store.queue_entry(SYS, "G-2").expect("two of three closed is progress, not done");
    assert_eq!(names(&held), ["format", "fields", "naming"], "the entry still names all three");
    assert_eq!(store.queue_history(SYS, "G-2").len(), 1, "progress writes no queue record");
    store.close_input(SYS.permit("G-2"), "fields").await.unwrap();
    assert!(store.queue_entry(SYS, "G-2").is_none(), "the last closed: out");
    let last = store.queue_history(SYS, "G-2").pop().unwrap();
    assert_eq!((last.action, last.reason.as_deref()), (QueueAction::Closed, Some("its inputs were closed, `fields` the last of 3")));
    let mut unqueued = 0;
    while let Ok(c) = rx.try_change() {
        if matches!(c, Change::Queue { change: QueueChange::Unqueued { action: QueueAction::Closed, .. }, .. }) {
            unqueued += 1;
        }
    }
    assert_eq!(unqueued, 1, "one unqueue, at the last close");
}

#[tokio::test]
async fn queueing_again_adds_inputs_and_raises_never_lowers() {
    let store = store().await;
    let (e, _) = queued(store.queue_with(SYS.permit("G-2"), ask(Urgency::Now, &["format"], None), act("a")).await.unwrap());
    let first = e.queued_unix;
    // Lower, with a new input: added; the urgency kept.
    let (e, a) = queued(store.queue_with(SYS.permit("G-2"), ask(Urgency::Today, &["fields", "format"], None), act("a")).await.unwrap());
    assert_eq!((names(&e), e.urgency, a), (vec!["format", "fields"], Urgency::Now, QueueAction::Requeued));
    // Raised to blocking on a new one: it is the blocking one.
    let (e, a) = queued(store.queue_with(SYS.permit("G-2"), ask(Urgency::Blocking, &["naming"], None), act("a")).await.unwrap());
    assert_eq!((names(&e), e.urgency, a, e.blocking.as_deref(), e.queued_unix), (vec!["format", "fields", "naming"], Urgency::Blocking, QueueAction::Raised, Some("naming"), first));
    // Again at `now`, naming nothing: everything kept, the blocking one too.
    let (e, _) = queued(store.queue_with(SYS.permit("G-2"), ask(Urgency::Now, &[], None), act("a")).await.unwrap());
    assert_eq!((names(&e).len(), e.urgency, e.blocking_input()), (3, Urgency::Blocking, Some("naming")));
    // A blocking named later moves it, among them.
    let (e, _) = queued(store.queue_with(SYS.permit("G-2"), ask(Urgency::Blocking, &[], Some("fields")), act("a")).await.unwrap());
    assert_eq!((e.blocking.as_deref(), e.input.as_deref()), (Some("fields"), Some("fields")));
}

#[tokio::test]
async fn a_closed_input_is_never_added() {
    let store = store().await;
    store.close_input(SYS.permit("G-2"), "format").await.unwrap();
    let e = store.queue_with(SYS.permit("G-2"), ask(Urgency::Now, &["fields", "format"], None), act("a")).await.unwrap_err();
    assert!(matches!(e, StoreError::InputClosed { .. }), "{e:?}");
    assert!(store.queue_entry(SYS, "G-2").is_none());
}

#[tokio::test]
async fn several_inputs_survive_a_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("g.db");
    {
        let store = Store::open(&path).unwrap();
        store.create(SYS, ticket(Kind::Scope, "root")).await.unwrap();
        store.create(SYS, ticket(Kind::Build, THREE)).await.unwrap();
        store.queue_with(SYS.permit("G-2"), ask(Urgency::Blocking, &["format", "fields", "naming"], Some("naming")), act("a")).await.unwrap();
    }
    let store = Store::open(&path).unwrap();
    let e = store.queue_entry(SYS, "G-2").unwrap();
    assert_eq!((names(&e), e.blocking.as_deref(), e.input.as_deref()), (vec!["format", "fields", "naming"], Some("naming"), Some("naming")));
    let rec = &store.queue_history(SYS, "G-2")[0];
    assert_eq!((rec.inputs.len(), rec.blocking.as_deref()), (3, Some("naming")));
}

/// A row written before APP-86 — `input` alone, no list — reads as an
/// entry naming that one.
#[tokio::test]
async fn an_entry_kept_before_reads_as_its_one_input() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("old.db");
    {
        let store = Store::open(&path).unwrap();
        store.create(SYS, ticket(Kind::Scope, "root")).await.unwrap();
        store.create(SYS, ticket(Kind::Build, THREE)).await.unwrap();
        store.queue_by(SYS.permit("G-2"), Urgency::Now, "look", Some("format"), act("a")).await.unwrap();
    }
    {
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.execute("UPDATE queue SET inputs = NULL, blocking = NULL", []).unwrap();
        conn.execute("UPDATE queue_log SET inputs = NULL, blocking = NULL", []).unwrap();
    }
    let store = Store::open(&path).unwrap();
    let e = store.queue_entry(SYS, "G-2").unwrap();
    assert_eq!((e.named(), e.input.as_deref()), (vec!["format"], Some("format")));
    assert_eq!(store.queue_history(SYS, "G-2")[0].input.as_deref(), Some("format"));
    // Closing it takes it out, as before.
    store.close_input(SYS.permit("G-2"), "format").await.unwrap();
    assert!(store.queue_entry(SYS, "G-2").is_none());
}

/// `await_reply` with no input hears each answer; with one, that one; an
/// input closed of several is no unqueue.
#[tokio::test]
async fn await_reply_hears_each_answer_and_only_the_last_close() {
    let store = store().await;
    store.queue_with(SYS.permit("G-2"), ask(Urgency::Blocking, &["format", "fields", "naming"], Some("naming")), act("agent:glaze")).await.unwrap();
    let w = |input: Option<&str>, since: i64| Awaiting { ticket: "G-2".into(), since, input: input.map(str::to_string), me: Some("agent:glaze".into()) };
    store.answer(SYS.permit("G-2"), "format", InputValue::Choice { option: "card".into() }, None, AnswerSource::App).await.unwrap();
    let r = store.reply_since(&w(None, 0));
    assert!(matches!(r, Some(Reply::Answered { ref input, .. }) if input == "format"), "{r:?}");
    assert_eq!(store.reply_since(&w(Some("naming"), 0)), None, "named: only that one");
    store.answer(SYS.permit("G-2"), "naming", InputValue::Text { text: "the firing card".into() }, None, AnswerSource::App).await.unwrap();
    let r = store.reply_since(&w(Some("naming"), 0));
    assert!(matches!(r, Some(Reply::Answered { ref input, .. }) if input == "naming"), "{r:?}");
    let all = store.replies_since(&w(None, 0));
    assert_eq!(all.iter().filter(|r| matches!(r, Reply::Answered { .. })).count(), 2, "each answer is a reply: {all:?}");
    // The agent closes two: progress, no unqueue heard.
    store.close_input(SYS.permit("G-2"), "format").await.unwrap();
    store.close_input(SYS.permit("G-2"), "naming").await.unwrap();
    assert!(!store.replies_since(&w(None, 0)).iter().any(|r| matches!(r, Reply::Unqueued { .. })));
}
