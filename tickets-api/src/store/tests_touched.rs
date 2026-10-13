//! FILTER-30: `touched <window>` — a ticket's last touch: any write to its
//! own row, a comment left on it, an answer to or a clear of one of its
//! inputs. Each kind of edit touches; a roll-up, a child's count or a
//! placement moving does not; the answer comes from the kept index, equal
//! to the rule ticket by ticket, across a reopen and under
//! `--verify-derived`; and an open `watch { filter }` asking `touched` hears
//! a ticket leave `not touched within …` the moment it is touched — by a
//! body, a comment or an answer, none of which moves a cell.

use super::filtered::Membership;
use super::history::Clock;
use super::*;
use crate::comments::Anchor;
use crate::types::{AnswerSource, InputValue, Severity};
use std::collections::BTreeSet;

const DAY: i64 = 86_400;

fn build(title: &str, parent: Option<&str>, body: &str) -> NewTicket {
    NewTicket {
        epic: "T".into(),
        title: title.into(),
        kind: Kind::Build,
        severity: None,
        blocked_by: Vec::new(),
        parent: parent.map(String::from),
        path: None,
        body: body.into(),
    }
}

fn anchor(ticket: &str) -> Anchor {
    Anchor {
        route: format!("#/ticket/{ticket}"),
        path: "[data-tag=\"body\"]".into(),
        label: "Body".into(),
        excerpt: String::new(),
        ticket: Some(ticket.into()),
        version: None,
    }
}

const ASKS: &str = "<m-input id=\"notes\" kind=\"text\"><m-question>Anything else?</m-question></m-input>\n";

/// The ids `text` admits, read now.
fn ids(store: &Store, text: &str) -> BTreeSet<String> {
    let lens = Lens::parse(text).unwrap_or_else(|e| panic!("{text}: {e}")).with_clock(Clock::default());
    let q = Filtered { lens, epic: None, status: None, parent: None, keep: None };
    store.filtered_page(&q, None, None, Some(10_000)).unwrap().tickets.into_iter().map(|t| t.id).collect()
}

/// The rule, ticket by ticket, from the rows, the comments and the inputs'
/// histories — never the index: the later of a row's last write and its
/// last comment or answer, in the last day.
fn touched_by_rule(store: &Store) -> BTreeSet<String> {
    let since = chrono::Utc::now().timestamp() - DAY;
    let cache = store.tickets.read().unwrap();
    let mut last: HashMap<String, i64> = cache.order.values().map(|r| (r.id.clone(), r.updated_unix.max(r.created_unix))).collect();
    for (id, at) in cache.comments.activity().chain(cache.inputs.activity()) {
        if let Some(l) = last.get_mut(id) {
            *l = (*l).max(at);
        }
    }
    last.into_iter().filter(|(_, at)| *at >= since).map(|(id, _)| id).collect()
}

/// `n` tickets in a file — T-1 a scope, T-2 a build with T-3 and T-4 under
/// it, T-5 asking an input, the rest plain builds — every one last written
/// 30 days ago, reopened.
async fn aged(path: &std::path::Path, n: usize) -> Store {
    let store = Store::open(path).unwrap();
    store
        .create(NewTicket { kind: Kind::Scope, ..build("root", None, "") })
        .await
        .unwrap();
    store.create(build("parent", None, "")).await.unwrap();
    store.create(build("child a", Some("T-2"), "")).await.unwrap();
    store.create(build("child b", Some("T-2"), "")).await.unwrap();
    store.create(build("asks", None, ASKS)).await.unwrap();
    for i in 6..=n {
        store.create(build(&format!("plain {i}"), None, "")).await.unwrap();
    }
    drop(store);
    let then = chrono::Utc::now().timestamp() - 30 * DAY;
    {
        let conn = rusqlite::Connection::open(path).unwrap();
        conn.execute("UPDATE tickets SET created_unix = ?1 - 1000, updated_unix = ?1", [then]).unwrap();
        conn.execute("UPDATE transitions SET at_unix = ?1", [then]).unwrap();
    }
    Store::open(path).unwrap()
}

/// Every kind of edit the owner named (FILTER-30's decision, `activity`)
/// touches the ticket it is made to; what only moves a derived count — a
/// child's status rolling up, a child born or moved under a parent — does
/// not touch the parent; a cancel's cascade touches the descendants whose
/// rows it writes. At every step the index answers what the rule says, a
/// reopened file the same, and `--verify-derived` holds it.
#[tokio::test]
async fn each_kind_of_edit_touches_and_a_rollup_does_not() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("touched.db");
    let store = aged(&path, 20).await;
    let all: BTreeSet<String> = (1..=20).map(|i| format!("T-{i}")).collect();
    assert!(ids(&store, "touched within 1d").is_empty(), "nothing touched in a month");
    assert_eq!(ids(&store, "not touched within 1d"), all);
    assert_eq!(ids(&store, "touched before 2w ago"), all);

    let mut want: BTreeSet<String> = BTreeSet::new();
    let mut step = |store: &Store, label: &str, add: &[&str]| {
        want.extend(add.iter().map(|s| s.to_string()));
        assert_eq!(ids(store, "touched within 1d"), want, "after {label}");
        assert_eq!(touched_by_rule(store), want, "the rule, after {label}");
        let every: BTreeSet<String> = store.tickets.read().unwrap().order.values().map(|r| r.id.clone()).collect();
        let untouched = ids(store, "not touched within 1d");
        assert_eq!(untouched, every.difference(&want).cloned().collect(), "its negation, after {label}");
    };
    store.amend("T-6", None, Some("a new body".into())).await.unwrap();
    step(&store, "a body", &["T-6"]);
    store.amend("T-7", Some("a new title".into()), None).await.unwrap();
    step(&store, "a title", &["T-7"]);
    // A child's status moves its parent's roll-up, not its parent's row.
    store.set_status("T-3", Status::Complete).await.unwrap();
    step(&store, "a child's status", &["T-3"]);
    store.set_kind("T-8", Kind::Gap, None).await.unwrap();
    step(&store, "a kind", &["T-8"]);
    store.update("T-9", Update { severity: Some(Some(Severity::High)), ..Update::default() }).await.unwrap();
    step(&store, "a severity", &["T-9"]);
    store.label("T-10", vec!["stale".into()], Vec::new()).await.unwrap();
    step(&store, "a label", &["T-10"]);
    store.link_commits("T-11", vec!["abc1234".into()]).await.unwrap();
    step(&store, "a commit", &["T-11"]);
    // A blocker: the ticket, and the blocker whose `unlocks` mirrors it.
    store.update("T-12", Update { block: vec!["T-13".into()], ..Update::default() }).await.unwrap();
    step(&store, "a blocker", &["T-12", "T-13"]);
    // A move: the child's row; the parents' counts are derived.
    store.set_parent("T-14", "T-2").await.unwrap();
    step(&store, "a parent", &["T-14"]);
    // A child born: the newborn; its parent only counts it.
    store.create(build("newborn", Some("T-15"), "")).await.unwrap();
    step(&store, "a child born", &["T-21"]);
    // A comment and an answer, neither of which writes the row.
    let before = store.row("T-16").unwrap().0.updated_unix;
    store.add_comment(anchor("T-16"), "still needed?".into(), None).await.unwrap();
    assert_eq!(store.row("T-16").unwrap().0.updated_unix, before, "a comment writes no row");
    step(&store, "a comment", &["T-16"]);
    store.answer("T-5", "notes", InputValue::Text { text: "ship it".into() }, None, AnswerSource::Api).await.unwrap();
    step(&store, "an answer", &["T-5"]);
    // Closing an input is not the owner's word on it: no touch.
    store.close_input("T-5", "notes").await.unwrap();
    step(&store, "a close", &[]);
    // A cancel cascades: the parent's row and its open child's are written
    // (T-3 is done already and stays).
    store.set_status("T-2", Status::Canceled).await.unwrap();
    step(&store, "a cancel's cascade", &["T-2", "T-4"]);
    // A comment on no ticket touches none.
    store.add_comment(Anchor { ticket: None, route: "#/".into(), ..anchor("T-1") }, "the board".into(), None).await.unwrap();
    step(&store, "a comment on the board", &[]);

    let fresh = |store: &Store| {
        let cache = store.tickets.read().unwrap();
        cache.log.check_times(cache.order.values())
    };
    assert!(fresh(&store).is_empty(), "{:?}", fresh(&store));
    drop(store);
    let store = Store::open(&path).unwrap();
    assert_eq!(ids(&store, "touched within 1d"), want, "the activity read back at open");
    assert!(fresh(&store).is_empty(), "{:?}", fresh(&store));
    drop(store);
    match Store::verify_derived(&path).unwrap() {
        Verdict::Consistent { .. } => {}
        other => panic!("{other:?}"),
    }
}

/// An open `watch { filter: "kind build and not touched within 1d" }`
/// hears each ticket leave as it is touched — a body, a comment, an answer
/// (no cell moves for any of them), a label (one moves) — and only it.
#[tokio::test]
async fn a_watch_hears_a_touched_ticket_leave() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("watch.db");
    let store = aged(&path, 8).await;
    let mut rx = store.subscribe_filtered();
    let lens = Lens::parse("kind build and not touched within 1d").unwrap();
    let w = store.watcher(lens);
    let heard = |rx: &mut broadcast::Receiver<Change>| -> Vec<Membership> {
        let mut out = Vec::new();
        while let Ok(c) = rx.try_recv() {
            if let Change::UnderMoved { moves } = c {
                out.extend(w.under_membership(&moves));
            }
        }
        out
    };
    let left = |id: &str| Membership::Left { id: id.into(), epic: "T".into(), seq: id[2..].parse().unwrap() };
    store.amend("T-6", None, Some("rewritten".into())).await.unwrap();
    assert_eq!(heard(&mut rx), [left("T-6")], "a body");
    store.add_comment(anchor("T-7"), "is this still wanted?".into(), None).await.unwrap();
    assert_eq!(heard(&mut rx), [left("T-7")], "a comment");
    store.answer("T-5", "notes", InputValue::Text { text: "yes".into() }, None, AnswerSource::App).await.unwrap();
    assert_eq!(heard(&mut rx), [left("T-5")], "an answer");
    store.label("T-8", vec!["x".into()], Vec::new()).await.unwrap();
    assert_eq!(heard(&mut rx), [left("T-8")], "a label");
    store.amend("T-6", None, Some("again".into())).await.unwrap();
    assert!(heard(&mut rx).is_empty(), "gone already");
    // A child's status: the child leaves; the parent, whose roll-up moved,
    // stays.
    store.set_status("T-3", Status::Ready).await.unwrap();
    assert_eq!(heard(&mut rx), [left("T-3")], "the child alone");
}

/// The boundary a watch sleeps until: the first last touch inside a
/// relative window leaving it.
#[tokio::test]
async fn a_touch_ages_out_of_a_relative_window() {
    let store = Store::new();
    store.create(NewTicket { kind: Kind::Scope, ..build("root", None, "") }).await.unwrap();
    store.create(build("one", None, "")).await.unwrap();
    let at = store.row("T-2").unwrap().0.updated_unix;
    let clock = |t: i64| Clock { fixed: Some(t), ..Clock::default() };
    let lens = Lens::parse("kind build and not touched within 1h").unwrap().with_clock(clock(at));
    assert!(store.aged(&lens, at, at + 3_600, None).is_empty(), "not yet an hour");
    assert_eq!(store.aged(&lens, at, at + 3_601, None), [Membership::Entered { id: "T-2".into(), epic: "T".into(), seq: 2 }]);
    let next = store.next_boundary(&lens).unwrap();
    assert!(next > at && next <= at + 3_601, "{next} vs {at}");
}
