//! V2-20's review: what a form-16 file left for form 17 to reconcile, `ask`
//! on a body that swallows it, subtasks in a shortcode's inputs, a queue that
//! takes no archived ticket, every answer a reply, and the watches' shortcuts
//! held equal to the walks they stand for.

use super::lineage::Compiled;
use super::*;
use crate::queue::Urgency;
use crate::types::{InputValue, TransitionSource};
use roaring::RoaringBitmap;

fn new(epic: &str, kind: Kind, body: &str) -> NewTicket {
    NewTicket { epic: epic.into(), title: "a ticket".into(), kind, severity: None, blocked_by: Vec::new(), parent: None, path: None, body: body.into() }
}

fn act(actor: &str) -> Act {
    Act { actor: actor.into(), name: None, source: TransitionSource::Api, message: None, cause: None }
}

fn text(id: &str) -> String {
    format!("<m-input id=\"{id}\" kind=\"text\"><m-question>{id}?</m-question></m-input>\n\n")
}

fn say(text: &str) -> InputValue {
    InputValue::Text { text: text.into() }
}

fn admitted(store: &Store, filter: &str) -> Vec<String> {
    let q = Filtered { lens: Lens::parse(filter).unwrap(), epic: None, status: None, parent: None, keep: None };
    store.filtered_page(SYS, &q, None, None, None).unwrap().tickets.into_iter().map(|t| t.id.into_string()).collect()
}

/// v2's reshortcode left inputs, answers and queue rows under the old id:
/// they follow the pin, an answer with its input, the entry with its ticket.
#[tokio::test]
async fn form_17_follows_a_ticket_v2_moved() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("t.db");
    {
        let store = Store::open(&path).unwrap();
        let sys = store.system().unwrap();
        store.create(&sys, new("KILN", Kind::Scope, "root")).await.unwrap();
        store.create(&sys, new("KILN", Kind::Build, &text("cone"))).await.unwrap();
        store.create(&sys, new("ZED", Kind::Scope, "root")).await.unwrap();
        store.answer(sys.permit("KILN-2"), "cone", say("six"), None, AnswerSource::Api).await.unwrap();
        store.queue_by(sys.permit("KILN-2"), Urgency::Now, "look", None, act("idp:ada")).await.unwrap();
        store.reshortcode(&sys, Reshortcode { ids: vec!["KILN-2".into()], to: "ZED".into(), ..Reshortcode::default() }).await.unwrap();
    }
    let conn = rusqlite::Connection::open(&path).unwrap();
    conn.execute_batch(
        "UPDATE inputs SET ticket = 'KILN-2' WHERE ticket = 'ZED-2';
         UPDATE answers SET ticket = 'KILN-2' WHERE ticket = 'ZED-2';
         UPDATE queue SET ticket = 'KILN-2' WHERE ticket = 'ZED-2';
         UPDATE queue_log SET ticket = 'KILN-2' WHERE ticket = 'ZED-2';
         UPDATE derived_state SET form = 16;",
    )
    .unwrap();
    drop(conn);
    let store = Store::open(&path).unwrap();
    let got = store.inputs(SYS, Some("ZED-2"), None, None).unwrap();
    assert_eq!((got.len(), got[0].state, got[0].answer.is_some()), (1, InputState::Answered, true), "{got:?}");
    assert!(admitted(&store, "asking").is_empty());
    assert_eq!(store.queued(SYS, true, 0).iter().map(|(e, _)| e.ticket.as_str()).collect::<Vec<_>>(), ["ZED-2"]);
    assert_eq!(store.queue_history(SYS, "ZED-2").len(), 1);
    drop(store);
    let conn = rusqlite::Connection::open(&path).unwrap();
    for t in ["inputs", "answers", "queue", "queue_log"] {
        let n: i64 = conn.query_row(&format!("SELECT count(*) FROM {t} WHERE ticket = 'KILN-2'"), [], |r| r.get(0)).unwrap();
        assert_eq!(n, 0, "{t} still names the old id");
    }
}

/// A ticket v2 canceled comes back withdrawn, and one v2 took out of
/// `Canceled` comes back restored.
#[tokio::test]
async fn form_17_withdraws_what_a_cancel_under_v2_left_open() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("t.db");
    {
        let store = Store::open(&path).unwrap();
        let sys = store.system().unwrap();
        store.create(&sys, new("Q", Kind::Scope, "root")).await.unwrap();
        store.create(&sys, new("Q", Kind::Build, &text("a"))).await.unwrap();
        store.create(&sys, new("Q", Kind::Build, &text("b"))).await.unwrap();
        store.set_status(sys.permit("Q-3"), Status::Canceled).await.unwrap();
        store.set_status(sys.permit("Q-3"), Status::Draft).await.unwrap();
        store.set_status(sys.permit("Q-3"), Status::Canceled).await.unwrap();
    }
    let conn = rusqlite::Connection::open(&path).unwrap();
    // Q-2 canceled, its input left open; Q-3 back from Canceled, its input left withdrawn.
    conn.execute_batch(
        "UPDATE tickets SET status = 'Canceled' WHERE id = 'Q-2';
         UPDATE tickets SET status = 'Draft' WHERE id = 'Q-3';
         UPDATE derived_state SET form = 16;",
    )
    .unwrap();
    drop(conn);
    let store = Store::open(&path).unwrap();
    let sys = store.system().unwrap();
    assert_eq!(store.inputs(SYS, Some("Q-2"), None, None).unwrap()[0].state, InputState::Withdrawn);
    assert!(matches!(store.answer(sys.permit("Q-2"), "a", say("no"), None, AnswerSource::Api).await, Err(StoreError::InputWithdrawn { .. })));
    assert_eq!(store.inputs(SYS, Some("Q-3"), None, None).unwrap()[0].state, InputState::Open);
    assert_eq!(admitted(&store, "asking"), ["Q-3"]);
}

/// `ask` declares its input or writes nothing: a body that ends inside a
/// fence, a comment or a `<pre>` swallows what is appended.
#[tokio::test]
async fn ask_declares_its_input_or_writes_nothing() {
    for tail in ["```\ncode", "<!-- open", "<pre>\nopen", "<textarea>open", "plain", "<code>open"] {
        let store = Store::new();
        store.create(SYS, new("Q", Kind::Scope, "root")).await.unwrap();
        store.create(SYS, new("Q", Kind::Build, tail)).await.unwrap();
        let revisions = store.revisions(SYS, "Q-2", None, None).await.map(|r| r.revisions.len()).unwrap_or(0);
        let asked = store.ask(SYS.permit("Q-2"), "Hold at cone 6?").await;
        let body = store.get(SYS, "Q-2").await.unwrap().body;
        match asked {
            Ok(_) => assert_eq!(store.inputs(SYS, Some("Q-2"), None, None).unwrap().len(), 1, "{tail}"),
            Err(e) => {
                assert_eq!(body, tail, "{tail}: refused, and nothing kept");
                assert_eq!(store.revisions(SYS, "Q-2", None, None).await.map(|r| r.revisions.len()).unwrap_or(0), revisions, "{tail}: no revision");
                assert!(e.to_string().contains("nothing"), "{e}");
            }
        }
    }
}

/// A subtask's inputs are its shortcode's, in the store's order.
#[tokio::test]
async fn a_shortcodes_inputs_hold_its_subtasks() {
    let store = Store::new();
    store.create(SYS, new("Q", Kind::Scope, "root")).await.unwrap();
    store.create(SYS, new("Q", Kind::Build, &text("a"))).await.unwrap();
    store.create(SYS, NewTicket { parent: Some("Q-2".into()), ..new("Q", Kind::Build, &text("c")) }).await.unwrap();
    store.create(SYS, new("Q", Kind::Build, "")).await.unwrap();
    let sys = store.system().unwrap();
    store.rename(&sys, Rename { id: "Q-3".into(), to: "Q-2.1".into(), ..Rename::default() }).await.unwrap_or_else(|e| panic!("{e}"));
    let got = store.inputs(SYS, None, Some("Q"), None).unwrap();
    assert_eq!(got.iter().map(|i| i.id.as_str()).collect::<Vec<_>>(), ["a", "c"], "{got:?}");
}

/// An archived ticket asks nothing: it is not queued, and `count` is the rows.
#[tokio::test]
async fn an_archived_ticket_is_not_queued() {
    let store = Store::new();
    store.create(SYS, new("Q", Kind::Scope, "root")).await.unwrap();
    store.create(SYS, new("Q", Kind::Build, "")).await.unwrap();
    store.archive_by(Selection::Ids(vec!["Q-2".into()]), act("idp:ada"), false, |ids| Ok(SYS.permits(ids))).await.unwrap();
    assert!(matches!(store.queue_by(SYS.permit("Q-2"), Urgency::Now, "look", None, act("idp:ada")).await, Err(StoreError::Invalid(_))));
    let page = store.queued_page(SYS, QueuedAsk { snoozed: true, ..QueuedAsk::default() }, 0).unwrap();
    assert_eq!((page.rows.len() as u64, page.count), (0, 0));
}

/// Every answer since is a reply, not the input's first: a wait that set the
/// first aside hears the next, in the same second.
#[tokio::test]
async fn each_answer_is_a_reply_and_a_reader_is_asked() {
    let store = Store::new();
    store.create(SYS, new("Q", Kind::Scope, "root")).await.unwrap();
    store.create(SYS, new("Q", Kind::Build, &text("a"))).await.unwrap();
    store.create(SYS, new("R", Kind::Scope, "root")).await.unwrap();
    for _ in 0..2 {
        store.answer(SYS.permit("Q-2"), "a", say("same"), None, AnswerSource::Api).await.unwrap();
    }
    let w = Awaiting { ticket: "Q-2".into(), since: 0, input: None, me: None };
    assert_eq!(store.replies_since(SYS, &w).len(), 2);
    assert!(store.replies_since(&super::tests_reach::viewer_of(&["R"]), &w).is_empty(), "out of reach, nothing");
}

/// `matches_among` is `matches_in` over a few, and the kept set is the walk,
/// whatever the writes did.
#[tokio::test]
async fn the_shortcuts_equal_the_walks() {
    let store = Store::new();
    store.create(SYS, new("Q", Kind::Scope, "root")).await.unwrap();
    for n in 0..40 {
        let body = if n % 3 == 0 { text("a") } else { String::new() };
        store.create(SYS, new("Q", Kind::Build, &body)).await.unwrap();
    }
    let lenses = ["queued", "queued now", "not queued", "asking", "asked", "queued or asking", "kind build and queued", "under Q-1 and queued", "status Draft"];
    let check = |store: &Store, step: &str| {
        let cache = store.tickets.read().unwrap();
        for text in lenses {
            let lens = Lens::parse(text).unwrap();
            let c = Compiled::new(&cache, &lens, None);
            for (e, _) in cache.cells.shortcodes() {
                let whole = c.matches_in(e);
                let mut some = RoaringBitmap::new();
                some.extend([2u32, 3, 5, 7, 11, 13, 40]);
                assert_eq!(c.matches_among(e, Some(&some)), &whole & &some, "{step}: {text} among a few");
                assert_eq!(c.matches_among(e, None), whole, "{step}: {text} whole");
                let many: RoaringBitmap = (1..200u32).collect();
                assert_eq!(c.matches_among(e, Some(&many)), &whole & &many, "{step}: {text} among many");
            }
        }
        for u in [None, Some(Urgency::Now), Some(Urgency::Today)] {
            let kept = cache.queue.holds(u, &cache.at);
            assert_eq!(kept, cache.queue.holds_walked(u, &cache.at), "{step}: {u:?}");
        }
    };
    check(&store, "at the start");
    for (i, u) in [Urgency::Now, Urgency::Today, Urgency::Whenever, Urgency::Now].into_iter().enumerate() {
        store.queue_by(SYS.permit(&format!("Q-{}", i + 2)), u, "look", None, act("idp:ada")).await.unwrap();
        check(&store, "queued");
    }
    store.queue_by(SYS.permit("Q-3"), Urgency::Now, "sooner", None, act("idp:ada")).await.unwrap();
    check(&store, "raised");
    store.unqueue_by(SYS.permit("Q-2"), act("idp:ada")).await.unwrap();
    check(&store, "unqueued");
    store.set_status(SYS.permit("Q-3"), Status::Coding).await.unwrap();
    check(&store, "moved");
}

/// V2-20's re-review, N1: asks at once on one ticket all land, each with an
/// id of its own, however often the body moved under them.
#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn asks_at_once_on_one_ticket_all_land() {
    let dir = tempfile::tempdir().unwrap();
    let store = std::sync::Arc::new(Store::open(&dir.path().join("t.db")).unwrap());
    let sys = store.system().unwrap();
    store.create(&sys, new("Q", Kind::Scope, "root")).await.unwrap();
    store.create(&sys, new("Q", Kind::Build, "the body")).await.unwrap();
    const N: usize = 64;
    let mut asks = Vec::new();
    for n in 0..N {
        let (s, on) = (store.clone(), sys.permit("Q-2"));
        asks.push(tokio::spawn(async move { s.ask(on, &format!("Question {n}?")).await }));
    }
    let mut ids = std::collections::HashSet::new();
    for a in asks {
        match a.await.unwrap() {
            Ok(InputWrite::Changed(i)) => assert!(ids.insert(i.id.to_string()), "{} twice", i.id),
            other => panic!("{other:?}"),
        }
    }
    assert_eq!(store.inputs(SYS, Some("Q-2"), None, None).unwrap().len(), N);
    assert_eq!(ids.len(), N);
}
