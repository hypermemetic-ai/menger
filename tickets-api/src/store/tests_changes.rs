//! APP-23: a change request — the comment and the unqueue in one write, the
//! named input untouched, a ticket not queued refused, both kept across a
//! restart, and `list { intent }`.

use super::tests::Changes;
use super::*;
use crate::comments::{CommentIntent, CommentState};
use crate::queue::{QueueAction, Urgency};
use crate::types::{InputState, TransitionSource};

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
    Act { actor: actor.into(), name: None, source: TransitionSource::App, message: None, cause: None }
}

const DECISION: &str = "<m-decision data-field=\"decision\" id=\"landing\">\n<m-question data-field=\"question\">Archive or delete?</m-question>\n\
<m-option data-field=\"option\" name=\"Archive\" value=\"archive\"></m-option>\n<m-option data-field=\"option\" name=\"Delete\" value=\"delete\"></m-option>\n</m-decision>\n";

/// Q-1 a scope; Q-2 a build asking `landing`, queued at blocking by an
/// agent; Q-3 a plain build, not queued.
async fn fill(store: &Store) {
    store.create(ticket(Kind::Scope, "root")).await.unwrap();
    store.create(ticket(Kind::Build, DECISION)).await.unwrap();
    store.create(ticket(Kind::Build, "")).await.unwrap();
    store.queue_by("Q-2", Urgency::Blocking, "idle until decided", Some("landing"), act("idp:agent")).await.unwrap();
}

/// One write: the comment on the conversation with intent `changes`, the
/// entry out with `changes_requested` and its record naming the comment —
/// each heard once, on its own channel. The input stays open.
#[tokio::test]
async fn a_change_request_is_a_comment_and_an_unqueue_in_one_write() {
    let store = Store::new();
    fill(&store).await;
    let mut tickets = store.subscribe();
    let mut comments = store.subscribe_comments();
    let (c, change) = store
        .request_changes_by("q-2", "Neither: **split** the import first.".into(), None, Some("idp:owner".into()), act("idp:owner"))
        .await
        .unwrap();
    assert_eq!(c.intent, CommentIntent::Changes);
    assert_eq!(c.state, CommentState::Open);
    assert_eq!((c.anchor.route.as_str(), c.anchor.path.as_str(), c.anchor.label.as_str()), ("#/ticket/Q-2", "conversation", "Conversation"));
    assert_eq!((c.anchor.ticket.as_deref(), c.author.as_str(), c.principal.as_deref()), (Some("Q-2"), "owner", Some("idp:owner")));
    assert!(c.anchor.version.is_some(), "the ticket's version, as comments.add keeps it");
    match &change {
        QueueChange::Unqueued { id, entry, action, by, .. } => {
            assert_eq!((id.as_str(), *action, by.as_str()), ("Q-2", QueueAction::ChangesRequested, "idp:owner"));
            assert_eq!((entry.urgency, entry.input.as_deref()), (Urgency::Blocking, Some("landing")), "the entry as it was");
        }
        other => panic!("expected unqueued, got {other:?}"),
    }
    assert!(store.queue_entry("Q-2").is_none(), "out of the queue");
    assert_eq!(store.comment(c.id.get()), Some(c.clone()));
    let rec = store.queue_history("Q-2").pop().unwrap();
    assert_eq!((rec.action, rec.comment, rec.actor.as_str(), rec.source), (QueueAction::ChangesRequested, Some(c.id.get()), "idp:owner", TransitionSource::App));
    // The named input is untouched: requesting changes is no answer.
    let input = store.inputs(Some("Q-2"), None, None).unwrap().into_iter().find(|i| i.id == "landing").unwrap();
    assert_eq!(input.state, InputState::Open);
    // Heard: the comment's `added`, and the queue's `unqueued`.
    assert_eq!(comments.try_recv().unwrap(), CommentChange::Added(c.clone()));
    assert!(comments.try_recv().is_err(), "once");
    let mut unq = 0;
    while let Ok(ch) = tickets.try_change() {
        if let Change::Queue { change: QueueChange::Unqueued { action: QueueAction::ChangesRequested, .. }, .. } = ch {
            unq += 1;
        }
    }
    assert_eq!(unq, 1);
    // `list { intent }`.
    store.add_comment(crate::comments::Anchor { ..conversation_anchor("Q-2") }, "a plain one".into(), None).await.unwrap();
    let only = |i| CommentQuery { intent: Some(i), ..CommentQuery::default() };
    let ids = |q: &CommentQuery| store.comments(q, None, None).unwrap().comments.into_iter().map(|c| c.id).collect::<Vec<_>>();
    assert_eq!(ids(&only(CommentIntent::Changes)), [c.id]);
    assert_eq!(ids(&only(CommentIntent::Comment)), [c.id.get() + 1]);
    // Re-queued after acting: back in the queue, a new entry.
    let again = store.queue_by("Q-2", Urgency::Now, "split it; look again", None, act("idp:agent")).await.unwrap();
    assert!(matches!(again, QueueChange::Queued { action: QueueAction::Queued, .. }));
}

/// A ticket not queued, one that is none, and a body comments.add would
/// refuse are each refused — and nothing is written: no comment, no record,
/// no event, the entry where it was.
#[tokio::test]
async fn a_refused_change_request_writes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("changes.db");
    let store = Store::open(&path).unwrap();
    fill(&store).await;
    let mut comments = store.subscribe_comments();
    let records = store.queue_history("Q-2").len();
    let e = store.request_changes_by("Q-3", "x".into(), None, None, act("o")).await.unwrap_err();
    assert!(e.to_string().contains("Q-3 is not queued"), "{e}");
    assert!(matches!(store.request_changes_by("Q-99", "x".into(), None, None, act("o")).await, Err(StoreError::NotFound(_))));
    let e = store.request_changes_by("Q-2", " \n".into(), None, None, act("o")).await.unwrap_err();
    assert!(e.to_string().contains("the comment's body is empty"), "{e}");
    assert!(comments.try_recv().is_err(), "nothing said");
    assert!(store.comments(&CommentQuery::default(), None, None).unwrap().comments.is_empty(), "nothing kept");
    assert!(store.queue_entry("Q-2").is_some(), "still queued");
    assert_eq!(store.queue_history("Q-2").len(), records, "no record");
}

/// Both halves are durable and read back as written: the comment's intent,
/// the record's comment — and a file from before APP-23 gains the columns.
#[tokio::test]
async fn a_change_request_is_kept_across_a_restart_and_an_older_file_gains_the_columns() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("kept.db");
    {
        let store = Store::open(&path).unwrap();
        fill(&store).await;
    }
    let conn = rusqlite::Connection::open(&path).unwrap();
    conn.execute_batch("ALTER TABLE comments DROP COLUMN intent; ALTER TABLE queue_log DROP COLUMN comment;").unwrap();
    drop(conn);
    let c = {
        let store = Store::open(&path).unwrap();
        store.request_changes_by("Q-2", "split it".into(), Some("ada".into()), None, act("idp:ada")).await.unwrap().0
    };
    let store = Store::open(&path).unwrap();
    assert_eq!(store.comment(c.id.get()).map(|c| (c.intent, c.author)), Some((CommentIntent::Changes, "ada".to_string())));
    let rec = store.queue_history("Q-2").pop().unwrap();
    assert_eq!((rec.action, rec.comment), (QueueAction::ChangesRequested, Some(c.id.get())));
    assert!(store.queue_entry("Q-2").is_none());
}
