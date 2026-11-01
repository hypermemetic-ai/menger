//! V2-20: a page of `queued` and of `inputs.list` is cut from the kept order
//! and goes on from where the last stopped; reach holds in the rows and in
//! the counts; a reshortcode takes the queue and the inputs with it; a file
//! v2 wrote without reading its bodies gains their inputs on form 17's one
//! build, its records kept.

use super::tests_reach::viewer_of;
use super::*;
use crate::queue::{QueueAction, Urgency};
use crate::types::TransitionSource;

fn new(epic: &str, kind: Kind, body: &str) -> NewTicket {
    NewTicket { epic: epic.into(), title: format!("a {}", kind.as_str()), kind, severity: None, blocked_by: Vec::new(), parent: None, path: None, body: body.into() }
}

fn act(actor: &str) -> Act {
    Act { actor: actor.into(), name: None, source: TransitionSource::Api, message: None, cause: None }
}

fn text(id: &str) -> String {
    format!("<m-input id=\"{id}\" kind=\"text\"><m-question>{id}?</m-question></m-input>\n\n")
}

/// Q-1 and R-1 scopes; Q-2..Q-13 builds, each with two text inputs.
async fn store() -> Store {
    let store = Store::new();
    store.create(SYS, new("Q", Kind::Scope, "root")).await.unwrap();
    store.create(SYS, new("R", Kind::Scope, "root")).await.unwrap();
    for _ in 0..12 {
        store.create(SYS, new("Q", Kind::Build, &format!("{}{}", text("a"), text("b")))).await.unwrap();
    }
    store
}

async fn queue(store: &Store, id: &str, u: Urgency) {
    store.queue_by(SYS.permit(id), u, "look", None, act("idp:ada")).await.unwrap();
}

fn tickets(page: &QueuedPage) -> Vec<&str> {
    page.rows.iter().map(|(e, _)| e.ticket.as_str()).collect()
}

#[tokio::test]
async fn pages_of_the_queue_walk_it_in_order_and_a_raise_moves_it() {
    let store = store().await;
    for (id, u) in [("Q-2", Urgency::Today), ("Q-3", Urgency::Now), ("Q-4", Urgency::Today), ("Q-5", Urgency::Blocking), ("R-1", Urgency::Whenever)] {
        if u == Urgency::Blocking {
            store.queue_by(SYS.permit(id), u, "idle", Some("a"), act("idp:ada")).await.unwrap();
        } else {
            queue(&store, id, u).await;
        }
    }
    let all = store.queued(SYS, true, 0);
    assert_eq!(all.iter().map(|(e, _)| e.ticket.as_str()).collect::<Vec<_>>(), ["Q-5", "Q-3", "Q-2", "Q-4", "R-1"], "most urgent, then the first queued");

    let ask = |after: Option<String>| QueuedAsk { id: None, snoozed: true, limit: Some(2), after };
    let first = store.queued_page(SYS, ask(None), 0).unwrap();
    assert_eq!((tickets(&first), first.remaining, first.count), (vec!["Q-5", "Q-3"], 3, 5));
    let second = store.queued_page(SYS, ask(first.next.clone()), 0).unwrap();
    assert_eq!((tickets(&second), second.remaining), (vec!["Q-2", "Q-4"], 1));
    // The page's last ticket leaves the queue: the walk goes on from its place.
    store.unqueue_by(SYS.permit("Q-4"), act("idp:ada")).await.unwrap();
    let third = store.queued_page(SYS, ask(second.next.clone()), 0).unwrap();
    assert_eq!((tickets(&third), third.remaining, third.next.clone()), (vec!["R-1"], 0, None));
    assert!(matches!(store.queued_page(SYS, ask(Some("no-such-place".into())), 0), Err(StoreError::Invalid(_))));

    // A raise moves it up the order, a requeue does not move it.
    queue(&store, "R-1", Urgency::Now).await;
    assert_eq!(store.queued(SYS, true, 0).iter().map(|(e, _)| e.ticket.as_str()).collect::<Vec<_>>(), ["Q-5", "Q-3", "R-1", "Q-2"]);
    queue(&store, "Q-2", Urgency::Whenever).await;
    assert_eq!(store.queued(SYS, true, 0).iter().map(|(e, _)| e.ticket.as_str()).collect::<Vec<_>>(), ["Q-5", "Q-3", "R-1", "Q-2"]);
}

#[tokio::test]
async fn a_snooze_is_counted_and_reach_holds_in_the_rows_and_the_counts() {
    let store = store().await;
    for id in ["Q-2", "Q-3", "R-1"] {
        queue(&store, id, Urgency::Now).await;
    }
    store.snooze_by(SYS.permit("Q-3"), 60, act("idp:ada")).await.unwrap();
    store.snooze_by(SYS.permit("R-1"), 60, act("idp:ada")).await.unwrap();
    let now = chrono::Utc::now().timestamp();
    let all = store.queued_page(SYS, QueuedAsk { snoozed: true, ..QueuedAsk::default() }, now).unwrap();
    assert_eq!((all.count, all.snoozed), (3, 2));
    let awake = store.queued_page(SYS, QueuedAsk::default(), now).unwrap();
    assert_eq!((tickets(&awake), awake.count), (vec!["Q-2"], 3), "the count is the queue's; the rows, what was asked");
    let q = viewer_of(&["Q"]);
    let page = store.queued_page(&q, QueuedAsk { snoozed: true, ..QueuedAsk::default() }, now).unwrap();
    assert_eq!((tickets(&page), page.count, page.snoozed, page.remaining), (vec!["Q-2", "Q-3"], 2, 1, 0));
    // A snooze ended is off the count; a requeue ends it.
    let later = store.queued_page(SYS, QueuedAsk { snoozed: true, ..QueuedAsk::default() }, now + 4000).unwrap();
    assert_eq!(later.snoozed, 0);
    queue(&store, "Q-3", Urgency::Now).await;
    assert_eq!(store.queued_page(SYS, QueuedAsk { snoozed: true, ..QueuedAsk::default() }, now).unwrap().snoozed, 1);
}

/// A deleted ticket is off every read and the count, its entry in the file;
/// its undo brings it back.
#[tokio::test]
async fn a_deleted_ticket_leaves_the_queue_until_its_undo() {
    let store = store().await;
    store.create(SYS, new("Q", Kind::Build, "")).await.unwrap();
    queue(&store, "Q-14", Urgency::Now).await;
    let sys = store.system().unwrap();
    store.delete(&sys, Selection::Ids(vec!["Q-14".to_string()]), Deletion::default(), None).await.unwrap();
    let page = store.queued_page(SYS, QueuedAsk::default(), 0).unwrap();
    assert_eq!((page.rows.len(), page.count), (0, 0));
    store.undelete(&sys, &["Q-14".to_string()]).await.unwrap();
    assert_eq!(store.queued_page(SYS, QueuedAsk::default(), 0).unwrap().count, 1);
}

#[tokio::test]
async fn pages_of_the_inputs_walk_the_stores_order() {
    let store = store().await;
    store.answer(SYS.permit("Q-2"), "a", crate::types::InputValue::Text { text: "yes".into() }, None, AnswerSource::Api).await.unwrap();
    let ask = |state, after: Option<String>| InputsAsk { state, limit: Some(5), after, ..InputsAsk::default() };
    let ids = |p: &InputsPage| p.inputs.iter().map(|i| format!("{}/{}", i.ticket, i.id)).collect::<Vec<_>>();
    let first = store.inputs_page(SYS, &ask(Some(InputState::Open), None), None).unwrap();
    assert_eq!(ids(&first), ["Q-2/b", "Q-3/a", "Q-3/b", "Q-4/a", "Q-4/b"]);
    assert_eq!(first.remaining, 18, "{first:?}");
    // A page may end inside a ticket's list.
    let two = store.inputs_page(SYS, &InputsAsk { state: Some(InputState::Open), limit: Some(2), ..InputsAsk::default() }, None).unwrap();
    assert_eq!(ids(&two), ["Q-2/b", "Q-3/a"]);
    let on = store.inputs_page(SYS, &InputsAsk { state: Some(InputState::Open), limit: Some(1), after: two.next.clone(), ..InputsAsk::default() }, None).unwrap();
    assert_eq!((ids(&on), on.remaining), (vec!["Q-3/b".to_string()], 20));
    // Every page, joined, is the whole; with no state too.
    let mut walked = Vec::new();
    let mut after = None;
    loop {
        let p = store.inputs_page(SYS, &ask(None, after), None).unwrap();
        walked.extend(ids(&p));
        match p.next {
            Some(n) => after = Some(n),
            None => break,
        }
    }
    assert_eq!(walked.len(), 24);
    assert_eq!((walked[0].as_str(), walked[23].as_str()), ("Q-2/a", "Q-13/b"), "by number, not as text");
    assert!(matches!(store.inputs_page(SYS, &ask(None, Some("nowhere".into())), None), Err(StoreError::Invalid(_))));
    // Reach, and a shortcode.
    assert!(store.inputs_page(&viewer_of(&["R"]), &ask(None, None), None).unwrap().inputs.is_empty());
    assert!(store.inputs_page(SYS, &InputsAsk { epic: Some("R".into()), ..InputsAsk::default() }, None).unwrap().inputs.is_empty());
}

#[tokio::test]
async fn a_reshortcode_takes_the_queue_and_the_inputs_with_it() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(&dir.path().join("t.db")).unwrap();
    let sys = store.system().unwrap();
    store.create(&sys, new("Q", Kind::Scope, "root")).await.unwrap();
    store.create(&sys, new("Q", Kind::Build, &text("a"))).await.unwrap();
    store.create(&sys, new("Z", Kind::Scope, "root")).await.unwrap();
    store.queue_by(sys.permit("Q-2"), Urgency::Blocking, "idle", Some("a"), act("idp:ada")).await.unwrap();
    store.reshortcode(&sys, Reshortcode { ids: vec!["Q-2".into()], to: "Z".into(), ..Reshortcode::default() }).await.unwrap();
    let rows = store.queued(SYS, true, 0);
    assert_eq!(rows.iter().map(|(e, _)| e.ticket.as_str()).collect::<Vec<_>>(), ["Z-2"]);
    assert_eq!(store.inputs(SYS, Some("Z-2"), None, None).unwrap().len(), 1);
    assert_eq!(store.queue_history(SYS, "Z-2").first().map(|r| r.action), Some(QueueAction::Queued));
    drop(store);
}

/// A form-16 file: its tables, v1's or none, say nothing of the bodies v2
/// wrote. Form 17's one build reads them: a body's input gained, one no body
/// declares removed, its history kept, `asking` as they say.
#[tokio::test]
async fn a_form_16_file_gains_the_inputs_its_bodies_declare_and_keeps_its_records() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("t.db");
    {
        let store = Store::open(&path).unwrap();
        let sys = store.system().unwrap();
        store.create(&sys, new("Q", Kind::Scope, "root")).await.unwrap();
        store.create(&sys, new("Q", Kind::Build, &text("v2"))).await.unwrap();
        store.create(&sys, new("Q", Kind::Build, "no inputs now")).await.unwrap();
    }
    let conn = rusqlite::Connection::open(&path).unwrap();
    conn.execute_batch(
        "DELETE FROM inputs WHERE ticket = 'Q-2';
         INSERT INTO inputs (ticket, id, position, kind, options, state) VALUES ('Q-3', 'v1', 0, 'text', '[]', 'open');
         INSERT INTO answers (ticket, input, action, value, source, at_unix) VALUES ('Q-3', 'v1', 'answered', '{\"type\":\"text\",\"text\":\"kept\"}', 'api', 1);
         ALTER TABLE cells DROP COLUMN asking;
         UPDATE derived_state SET form = 16;",
    )
    .unwrap();
    let answers: i64 = conn.query_row("SELECT count(*) FROM answers", [], |r| r.get(0)).unwrap();
    drop(conn);
    let store = Store::open(&path).unwrap();
    assert_eq!(store.inputs(SYS, Some("Q-2"), None, None).unwrap().iter().map(|i| i.state).collect::<Vec<_>>(), [InputState::Open], "a body v2 wrote");
    let v1 = store.inputs(SYS, Some("Q-3"), None, None).unwrap();
    assert_eq!((v1[0].state, v1[0].history.len()), (InputState::Removed, 1), "its history kept");
    let q = Filtered { lens: Lens::parse("asking").unwrap(), epic: None, status: None, parent: None, keep: None };
    let asking = store.filtered_page(SYS, &q, None, None, None).unwrap().tickets;
    assert_eq!(asking.iter().map(|t| t.id.as_str()).collect::<Vec<_>>(), ["Q-2"]);
    drop(store);
    let conn = rusqlite::Connection::open(&path).unwrap();
    assert_eq!(conn.query_row("SELECT count(*) FROM answers", [], |r| r.get::<_, i64>(0)).unwrap(), answers, "no record moved");
    assert_eq!(conn.query_row("SELECT form FROM derived_state", [], |r| r.get::<_, i64>(0)).unwrap(), stored::FORM);
    let verdict = Store::verify_derived(&path).unwrap();
    assert!(matches!(verdict, Verdict::Consistent { .. }), "{verdict:?}");
}
