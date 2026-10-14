//! MENGER-39: the queue — queueing and its refusals, a requeue that raises
//! and never lowers, unqueue, snooze, the queue's order, closing the named
//! input, the filter cells `queued` and `queued <urgency>` (counts and a
//! watch's membership), the queue's history, and a reopen.

use super::tests::Changes;
use super::*;
use crate::queue::{QueueAction, QueueEntry, Urgency};
use crate::types::{AnswerSource, InputValue, TransitionSource};

fn ticket(kind: Kind, body: &str) -> NewTicket {
    NewTicket {
        epic: "Q".into(),
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

const DECISION: &str = "<m-decision data-field=\"decision\" id=\"landing\">\n<m-question data-field=\"question\">Archive or delete?</m-question>\n\
<m-option data-field=\"option\" name=\"Archive\" value=\"archive\"></m-option>\n<m-option data-field=\"option\" name=\"Delete\" value=\"delete\"></m-option>\n</m-decision>\n";

/// Q-1 a scope; Q-2 a build asking `landing`; Q-3 and Q-4 plain builds.
async fn store() -> Store {
    let store = Store::new();
    store.create(ticket(Kind::Scope, "root")).await.unwrap();
    store.create(ticket(Kind::Build, DECISION)).await.unwrap();
    store.create(ticket(Kind::Build, "")).await.unwrap();
    store.create(ticket(Kind::Build, "")).await.unwrap();
    store
}

fn entry(c: QueueChange) -> (QueueEntry, QueueAction) {
    match c {
        QueueChange::Queued { entry, action, .. } => (entry, action),
        other => panic!("expected queued, got {other:?}"),
    }
}

fn count(store: &Store, filter: &str) -> u64 {
    store.counts(&Lens::parse(filter).unwrap()).into_iter().map(|(_, n)| n).sum()
}

#[tokio::test]
async fn queueing_is_one_entry_raised_never_lowered() {
    let store = store().await;
    let (e, a) = entry(store.queue_by("q-3", Urgency::Today, " the promote rolled back ", None, act("idp:ci")).await.unwrap());
    assert_eq!((e.ticket.as_str(), e.urgency, e.reason.as_str(), a), ("Q-3", Urgency::Today, "the promote rolled back", QueueAction::Queued));
    let first = e.queued_unix;
    // Lower: the urgency kept, the reason the new one.
    let (e, a) = entry(store.queue_by("Q-3", Urgency::Whenever, "still rolled back", None, act("idp:bob")).await.unwrap());
    assert_eq!((e.urgency, e.reason.as_str(), e.by.as_str(), a), (Urgency::Today, "still rolled back", "idp:bob", QueueAction::Requeued));
    // Higher: raised.
    let (e, a) = entry(store.queue_by("Q-3", Urgency::Now, "prod is down", None, act("idp:ci")).await.unwrap());
    assert_eq!((e.urgency, a, e.queued_unix), (Urgency::Now, QueueAction::Raised, first), "the oldest stays oldest");
    assert_eq!(store.queued(true, 0, |_| true).len(), 1, "one entry a ticket");
    let history = store.queue_history("Q-3");
    let seen: Vec<(QueueAction, Urgency, &str)> = history.iter().map(|r| (r.action, r.urgency, r.actor.as_str())).collect();
    assert_eq!(
        seen,
        [(QueueAction::Queued, Urgency::Today, "idp:ci"), (QueueAction::Requeued, Urgency::Today, "idp:bob"), (QueueAction::Raised, Urgency::Now, "idp:ci")]
    );
    assert_eq!(history[2].reason.as_deref(), Some("prod is down"));
}

#[tokio::test]
async fn a_queue_entry_is_refused_without_its_parts() {
    let store = store().await;
    let e = store.queue_by("Q-3", Urgency::Today, "  ", None, act("a")).await.unwrap_err();
    assert!(e.to_string().contains("reason"), "{e}");
    let e = store.queue_by("Q-2", Urgency::Blocking, "idle", None, act("a")).await.unwrap_err();
    assert!(e.to_string().contains("`blocking` names the input"), "{e}");
    let e = store.queue_by("Q-2", Urgency::Blocking, "idle", Some("nope"), act("a")).await.unwrap_err();
    assert!(matches!(e, StoreError::InputUnknown { .. }), "{e:?}");
    let e = store.queue_by("Q-3", Urgency::Blocking, "idle", Some("landing"), act("a")).await.unwrap_err();
    assert!(matches!(e, StoreError::InputUnknown { .. }), "an input of another ticket: {e:?}");
    assert!(matches!(store.queue_by("Q-99", Urgency::Now, "x", None, act("a")).await.unwrap_err(), StoreError::NotFound(_)));
    let e = store.snooze_by("Q-3", 10, act("a")).await.unwrap_err();
    assert!(e.to_string().contains("not queued"), "{e}");
    assert!(store.queued(true, 0, |_| true).is_empty(), "nothing written");
    assert!(store.queue_history("Q-2").is_empty());
}

#[tokio::test]
async fn the_queue_is_most_urgent_first_then_oldest() {
    let store = store().await;
    store.queue_by("Q-3", Urgency::Today, "a", None, act("a")).await.unwrap();
    store.queue_by("Q-4", Urgency::Whenever, "b", None, act("a")).await.unwrap();
    store.queue_by("Q-2", Urgency::Blocking, "c", Some("landing"), act("a")).await.unwrap();
    store.queue_by("Q-1", Urgency::Today, "d", None, act("a")).await.unwrap();
    let ids: Vec<String> = store.queued(true, 0, |_| true).into_iter().map(|(e, _)| e.ticket).collect();
    assert_eq!(ids, ["Q-2", "Q-3", "Q-1", "Q-4"]);
    let ids: Vec<String> = store.queued(true, 0, |id| id != "Q-3").into_iter().map(|(e, _)| e.ticket).collect();
    assert_eq!(ids, ["Q-2", "Q-1", "Q-4"], "what the caller cannot see is left out");
}

#[tokio::test]
async fn unqueue_and_snooze() {
    let store = store().await;
    store.queue_by("Q-3", Urgency::Now, "look", None, act("a")).await.unwrap();
    let QueueChange::Snoozed { entry, .. } = store.snooze_by("Q-3", 10, act("b")).await.unwrap() else { panic!() };
    let until = entry.snoozed_until_unix.expect("snoozed");
    let now = chrono::Utc::now().timestamp();
    assert!((now + 590..=now + 601).contains(&until), "ten minutes: {until} vs {now}");
    assert!(store.queued(false, now, |_| true).is_empty(), "off the alerting surfaces");
    assert_eq!(store.queued(false, until, |_| true).len(), 1, "back on time");
    assert_eq!(count(&store, "queued"), 1, "still queued");
    assert!(store.snooze_by("Q-3", 0, act("b")).await.is_err() && store.snooze_by("Q-3", 7 * 24 * 60 + 1, act("b")).await.is_err());
    // A requeue ends the snooze.
    let (e, _) = entry_of(store.queue_by("Q-3", Urgency::Now, "again", None, act("a")).await.unwrap());
    assert_eq!(e.snoozed_until_unix, None);
    let Some(QueueChange::Unqueued { id, action, by, .. }) = store.unqueue_by("Q-3", act("c")).await.unwrap() else { panic!() };
    assert_eq!((id.as_str(), action, by.as_str()), ("Q-3", QueueAction::Unqueued, "c"));
    assert!(store.unqueue_by("Q-3", act("c")).await.unwrap().is_none(), "not queued: nothing");
    assert!(store.queue_entry("Q-3").is_none());
    let actions: Vec<QueueAction> = store.queue_history("Q-3").iter().map(|r| r.action).collect();
    assert_eq!(actions, [QueueAction::Queued, QueueAction::Snoozed, QueueAction::Requeued, QueueAction::Unqueued]);
    assert_eq!(store.queue_history("Q-3")[1].until_unix, Some(until));
}

fn entry_of(c: QueueChange) -> (QueueEntry, QueueAction) {
    entry(c)
}

#[tokio::test]
async fn closing_the_named_input_unqueues() {
    let store = store().await;
    store.queue_by("Q-2", Urgency::Blocking, "idle until answered", Some("landing"), act("idp:agent")).await.unwrap();
    store.queue_by("Q-3", Urgency::Now, "unrelated", None, act("idp:agent")).await.unwrap();
    let mut rx = store.subscribe();
    // An answer does not: the agent acts, then closes.
    store.answer("Q-2", "landing", InputValue::Choice { option: "archive".into() }, None, AnswerSource::App).await.unwrap();
    assert!(store.queue_entry("Q-2").is_some(), "answered, still queued");
    store.close_input("Q-2", "landing").await.unwrap();
    assert!(store.queue_entry("Q-2").is_none(), "closed: out of the queue");
    assert!(store.queue_entry("Q-3").is_some(), "another ticket's entry stays");
    let mut heard = Vec::new();
    while let Ok(c) = rx.try_change() {
        heard.push(c);
    }
    let unq = heard.iter().find_map(|c| match c {
        Change::Queue { change: QueueChange::Unqueued { id, action, by, .. }, .. } => Some((id.clone(), *action, by.clone())),
        _ => None,
    });
    assert_eq!(unq, Some(("Q-2".into(), QueueAction::Closed, crate::types::SYSTEM_ACTOR.into())));
    let last = store.queue_history("Q-2").pop().unwrap();
    assert_eq!((last.action, last.reason.as_deref()), (QueueAction::Closed, Some("its input `landing` was closed")));
    // Closing an input no entry names is no queue write.
    let ids: Vec<String> = store.queued(true, 0, |_| true).into_iter().map(|(e, _)| e.ticket).collect();
    assert_eq!(ids, ["Q-3"]);
    // A closed input cannot be named again.
    let e = store.queue_by("Q-2", Urgency::Blocking, "again", Some("landing"), act("a")).await.unwrap_err();
    assert!(matches!(e, StoreError::InputClosed { .. }), "{e:?}");
}

#[tokio::test]
async fn the_filter_cells_are_answered_from_the_queue() {
    let store = store().await;
    store.queue_by("Q-3", Urgency::Today, "a", None, act("a")).await.unwrap();
    store.queue_by("Q-2", Urgency::Blocking, "b", Some("landing"), act("a")).await.unwrap();
    assert_eq!(count(&store, "queued"), 2);
    assert_eq!(count(&store, "queued today"), 1);
    assert_eq!(count(&store, "queued blocking"), 1);
    assert_eq!(count(&store, "queued now"), 0);
    assert_eq!(count(&store, "not queued"), 2);
    assert_eq!(count(&store, "queued or asking or label needs-you"), 2, "Needs you's filter");
    assert_eq!(count(&store, "queued and kind scope"), 0);
    for bad in ["queued soon", "queued today within 1h", "queued by me"] {
        assert!(Lens::parse(bad).is_err(), "{bad}");
    }
    assert_eq!(crate::filter::Filter::parse("queued Now").unwrap().format(), "queued now");
    // A watch of `queued now` hears a raise into it and an unqueue out.
    let watcher = store.watcher(Lens::parse("queued now").unwrap());
    let mut rx = store.subscribe_filtered();
    store.queue_by("Q-3", Urgency::Now, "raised", None, act("a")).await.unwrap();
    store.queue_by("Q-4", Urgency::Whenever, "quiet", None, act("a")).await.unwrap();
    store.unqueue_by("Q-3", act("a")).await.unwrap();
    let mut heard = Vec::new();
    while let Ok(c) = rx.try_change() {
        if let Change::UnderMoved { moves } = c {
            heard.extend(watcher.under_membership(&moves));
        }
    }
    assert_eq!(
        heard,
        [Membership::Entered { id: "Q-3".into(), epic: "Q".into(), seq: 3 }, Membership::Left { id: "Q-3".into(), epic: "Q".into(), seq: 3 }]
    );
}

#[tokio::test]
async fn the_queue_survives_a_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("q.db");
    {
        let store = Store::open(&path).unwrap();
        store.create(ticket(Kind::Scope, "root")).await.unwrap();
        store.create(ticket(Kind::Build, DECISION)).await.unwrap();
        store.queue_by("Q-2", Urgency::Blocking, "idle", Some("landing"), act("idp:a")).await.unwrap();
        store.queue_by("Q-1", Urgency::Today, "look", None, act("idp:a")).await.unwrap();
        store.snooze_by("Q-1", 30, act("idp:b")).await.unwrap();
        store.unqueue_by("Q-1", act("idp:b")).await.unwrap();
    }
    let store = Store::open(&path).unwrap();
    let rows = store.queued(true, 0, |_| true);
    assert_eq!(rows.len(), 1);
    assert_eq!((rows[0].0.ticket.as_str(), rows[0].0.urgency, rows[0].0.input.as_deref()), ("Q-2", Urgency::Blocking, Some("landing")));
    assert_eq!(count(&store, "queued blocking"), 1);
    assert_eq!(store.queue_history("Q-1").len(), 3);
    // The log's ids go on from where they were.
    store.queue_by("Q-1", Urgency::Now, "again", None, act("idp:a")).await.unwrap();
    let ids: Vec<u64> = store.queue_history("Q-1").iter().map(|r| r.id).collect();
    assert_eq!(ids.len(), 4);
    assert!(ids.windows(2).all(|w| w[0] < w[1]), "{ids:?}");
    assert!(store.queue_history("Q-2")[0].id < ids[0]);
}
