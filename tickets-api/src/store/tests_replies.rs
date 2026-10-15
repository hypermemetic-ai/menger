//! APP-24: what answers an agent waiting on a ticket — every row of the
//! ticket's table: an answer (any input, or the one named), a change
//! request, another's comment (the caller's own set aside), another's
//! unqueue (a snooze and a requeue are none), a cancel, a delete; `since`;
//! the earliest first.

use super::*;
use crate::comments::{Anchor, CommentIntent};
use crate::queue::{Ending, QueueAction, Reply, Urgency};
use crate::types::{AnswerSource, InputValue, TransitionSource};

fn ticket(kind: Kind, body: &str) -> NewTicket {
    NewTicket {
        epic: "W".into(),
        title: format!("a {}", kind.as_str()),
        kind,
        severity: None,
        blocked_by: Vec::new(),
        parent: None,
        path: None,
        body: body.into(),
    }
}

fn act(actor: &str) -> Act {
    Act { actor: actor.into(), name: None, source: TransitionSource::Api, message: None, cause: None }
}

const TWO: &str = "<m-decision data-field=\"decision\" id=\"landing\">\n<m-question data-field=\"question\">Archive or delete?</m-question>\n\
<m-option data-field=\"option\" name=\"Archive\" value=\"archive\"></m-option>\n<m-option data-field=\"option\" name=\"Delete\" value=\"delete\"></m-option>\n</m-decision>\n\n\
<m-input id=\"notes\" kind=\"text\"><m-question data-field=\"question\">Anything else?</m-question></m-input>\n";

/// W-1 a scope; W-2 asks `landing` and `notes`, queued at blocking by the
/// agent `idp:agent`; W-3 a plain build.
async fn store() -> Store {
    let store = Store::new();
    store.create(ticket(Kind::Scope, "root")).await.unwrap();
    store.create(ticket(Kind::Build, TWO)).await.unwrap();
    store.create(ticket(Kind::Build, "")).await.unwrap();
    store.queue_by("W-2", Urgency::Blocking, "idle", Some("landing"), act("idp:agent")).await.unwrap();
    store
}

fn agent(input: Option<&str>) -> Awaiting {
    Awaiting { ticket: "W-2".into(), since: 0, input: input.map(str::to_string), me: Some("idp:agent".into()) }
}

fn on(ticket: &str) -> Anchor {
    conversation_anchor(ticket)
}

fn reply(store: &Store, w: &Awaiting) -> Option<Reply> {
    store.reply_since(w, |_| true)
}

#[tokio::test]
async fn nothing_yet_is_no_reply_and_a_snooze_or_a_requeue_is_none() {
    let store = store().await;
    assert_eq!(reply(&store, &agent(None)), None);
    store.snooze_by("W-2", 10, act("idp:owner")).await.unwrap();
    store.queue_by("W-2", Urgency::Blocking, "still idle", Some("landing"), act("idp:owner")).await.unwrap();
    // The caller's own comment and unqueue are not responses either.
    store.add_comment_as(on("W-2"), "my own note".into(), None, Some("idp:agent".into())).await.unwrap();
    store.unqueue_by("W-2", act("idp:agent")).await.unwrap();
    assert_eq!(reply(&store, &agent(None)), None);
    // Another ticket's comment is not this one's.
    store.add_comment_as(on("W-3"), "elsewhere".into(), None, Some("idp:owner".into())).await.unwrap();
    assert_eq!(reply(&store, &agent(None)), None);
}

#[tokio::test]
async fn an_answer_is_a_reply_and_input_names_which() {
    let store = store().await;
    store.answer("W-2", "notes", InputValue::Text { text: "ship it".into() }, None, AnswerSource::App).await.unwrap();
    match reply(&store, &agent(None)) {
        Some(Reply::Answered { input, value, source, .. }) => {
            assert_eq!((input.as_str(), value, source), ("notes", InputValue::Text { text: "ship it".into() }, AnswerSource::App))
        }
        other => panic!("expected answered, got {other:?}"),
    }
    assert_eq!(reply(&store, &agent(Some("landing"))), None, "only the input named counts as answered");
    store.answer("W-2", "landing", InputValue::Choice { option: "archive".into() }, Some("holds are real".into()), AnswerSource::Api).await.unwrap();
    match reply(&store, &agent(Some("landing"))) {
        Some(Reply::Answered { input, rationale, .. }) => assert_eq!((input.as_str(), rationale.as_deref()), ("landing", Some("holds are real"))),
        other => panic!("expected answered, got {other:?}"),
    }
}

#[tokio::test]
async fn a_change_request_a_comment_and_an_unqueue_by_another_are_replies() {
    let store = store().await;
    let c = store.add_comment_as(on("W-2"), "not what I meant".into(), None, Some("idp:owner".into())).await.unwrap();
    assert_eq!(reply(&store, &agent(None)), Some(Reply::Commented { comment: c.clone() }));
    // With login off the caller is no one in particular: nothing is set aside.
    let anon = Awaiting { me: None, ..agent(None) };
    let own = store.add_comment_as(on("W-3"), "x".into(), None, None).await.unwrap();
    assert_eq!(store.reply_since(&Awaiting { ticket: "W-3".into(), ..anon }, |_| true), Some(Reply::Commented { comment: own }));
    // A comment left on the ticket's page, pinned to no ticket, is the ticket's.
    let store2 = self::store().await;
    let paged = Anchor { ticket: None, path: "#t-status".into(), label: "Status".into(), ..on("W-2") };
    let p = store2.add_comment_as(paged, "on the page".into(), None, Some("idp:owner".into())).await.unwrap();
    assert_eq!(reply(&store2, &agent(None)), Some(Reply::Commented { comment: p }));

    let store = self::store().await;
    let (cr, _) = store.request_changes_by("W-2", "split it".into(), None, Some("idp:owner".into()), act("idp:owner")).await.unwrap();
    assert_eq!(cr.intent, CommentIntent::Changes);
    assert_eq!(reply(&store, &agent(None)), Some(Reply::ChangesRequested { comment: cr }), "the change request, not its unqueue");

    let store = self::store().await;
    store.unqueue_by("W-2", act("idp:owner")).await.unwrap();
    assert!(matches!(reply(&store, &agent(None)), Some(Reply::Unqueued { action: QueueAction::Unqueued, ref by, .. }) if by == "idp:owner"));
}

#[tokio::test]
async fn closing_the_named_input_is_an_unqueue_by_the_system() {
    let store = store().await;
    store.close_input("W-2", "landing").await.unwrap();
    assert!(matches!(reply(&store, &agent(Some("notes"))), Some(Reply::Unqueued { action: QueueAction::Closed, .. })));
}

#[tokio::test]
async fn a_cancel_or_a_delete_ends_it() {
    let store = store().await;
    store.set_status_by("W-2", Status::Canceled, act("idp:owner"), false).await.unwrap();
    assert!(matches!(reply(&store, &agent(Some("nothing"))), Some(Reply::Ended { how: Ending::Canceled, ref by, .. }) if by == "idp:owner"));
    let store = self::store().await;
    store.delete(Selection::Ids(vec!["W-3".into()]), Deletion::default(), None).await.unwrap();
    let w = Awaiting { ticket: "W-3".into(), ..agent(None) };
    assert!(matches!(reply(&store, &w), Some(Reply::Ended { how: Ending::Deleted, .. })));
}

/// `since`: what came before it is no reply; the earliest after it is.
#[tokio::test]
async fn since_bounds_it_and_the_earliest_answers() {
    let store = store().await;
    let c = store.add_comment_as(on("W-2"), "first".into(), None, Some("idp:owner".into())).await.unwrap();
    assert_eq!(reply(&store, &Awaiting { since: c.at + 1, ..agent(None) }), None, "before since");
    assert_eq!(reply(&store, &Awaiting { since: c.at, ..agent(None) }), Some(Reply::Commented { comment: c.clone() }), "at since");
    // Two after since: the earliest — at one second, the table's order (an answer first).
    store.answer("W-2", "landing", InputValue::Choice { option: "delete".into() }, None, AnswerSource::App).await.unwrap();
    let answered_at = store.inputs(Some("W-2"), Some("W"), None).unwrap().into_iter().find(|i| i.id == "landing").unwrap().history[0].at_unix;
    let got = reply(&store, &agent(None)).unwrap();
    if answered_at == c.at {
        assert!(matches!(got, Reply::Answered { .. }), "{got:?}");
    } else {
        assert_eq!(got, Reply::Commented { comment: c });
    }
}
