//! TICKETS-9: assignees — assign and unassign over one ticket and many,
//! the cap refused whole, the history records, the filter cells `assigned
//! <who>`, `assigned me`, `assigned` and `unassigned` (counts and a watch's
//! membership), `update_many { assign }`, and a reopen.

use super::tests::Changes;
use super::*;
use crate::assignees::AssignAction;
use crate::types::TransitionSource;
use crate::vocab::Principal;

const ADA: &str = "key:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const BOB: &str = "idp:0f8fad5b-d9cb-469f-a165-70867728950e";

fn p(s: &str) -> Principal {
    s.parse().expect("a principal")
}

fn ticket(kind: Kind) -> NewTicket {
    NewTicket { epic: "AS".into(), title: format!("a {}", kind.as_str()), kind, severity: None, blocked_by: Vec::new(), parent: None, path: None, body: String::new() }
}

fn act(actor: &str) -> Act {
    Act { actor: actor.into(), name: None, source: TransitionSource::Api, message: Some("handing over".into()), cause: None }
}

/// AS-1 a scope; AS-2..AS-4 builds.
async fn store() -> Store {
    let store = Store::new();
    store.create(SYS, ticket(Kind::Scope)).await.unwrap();
    for _ in 0..3 {
        store.create(SYS, ticket(Kind::Build)).await.unwrap();
    }
    store
}

fn moved(c: &Change) -> (String, Vec<String>, Vec<String>, Vec<String>) {
    match c {
        Change::AssigneesChanged { ticket, added, removed } => (ticket.id.to_string(), ticket.assignees.clone(), added.clone(), removed.clone()),
        other => panic!("expected assignees_changed, got {other:?}"),
    }
}

fn count(store: &Store, filter: &str, me: &str) -> u64 {
    let lens = Lens::parse(filter).unwrap().with_clock(Clock { me: me.into(), ..Clock::default() });
    store.counts(SYS, &lens).into_iter().map(|(_, n)| n).sum()
}

#[tokio::test]
async fn assign_and_unassign_move_the_set_and_keep_the_history() {
    let store = store().await;
    let c = store.assign_by(vec![SYS.permit("as-2")], vec![p(BOB), p(ADA)], vec![], act("idp:owner")).await.unwrap();
    assert_eq!(moved(&c[0]), ("AS-2".into(), vec![BOB.into(), ADA.into()], vec![BOB.into(), ADA.into()], vec![]));
    assert_eq!(store.get(SYS, "AS-2").await.unwrap().assignees, [BOB, ADA], "sorted on the row");
    // Nothing moves: no write, no record.
    let c = store.assign_by(vec![SYS.permit("AS-2")], vec![p(ADA)], vec![], act("idp:owner")).await.unwrap();
    assert_eq!(moved(&c[0]).2.len() + moved(&c[0]).3.len(), 0);
    assert_eq!(store.assign_history(SYS, "AS-2").len(), 2);
    let c = store.assign_by(vec![SYS.permit("AS-2")], vec![], vec![p(BOB)], act("idp:bob")).await.unwrap();
    assert_eq!(moved(&c[0]).3, [BOB]);
    let seen: Vec<(AssignAction, String, String)> =
        store.assign_history(SYS, "AS-2").into_iter().map(|r| (r.action, r.principal, r.actor)).collect();
    assert_eq!(
        seen,
        [
            (AssignAction::Assigned, BOB.into(), "idp:owner".into()),
            (AssignAction::Assigned, ADA.into(), "idp:owner".into()),
            (AssignAction::Unassigned, BOB.into(), "idp:bob".into()),
        ]
    );
    assert_eq!(store.assign_history(SYS, "AS-2")[0].message.as_deref(), Some("handing over"));
    assert!(store.assign_history(SYS, "AS-3").is_empty());
}

#[tokio::test]
async fn many_tickets_in_one_write_all_or_none() {
    let store = store().await;
    let c = store.assign_by(vec![SYS.permit("AS-3"), SYS.permit("AS-4")], vec![p(ADA)], vec![], act("a")).await.unwrap();
    assert_eq!(c.len(), 2);
    assert_eq!(count(&store, &format!("assigned \"{ADA}\""), "x"), 2);
    // Past the cap on one refuses the write for both, naming it.
    let many: Vec<Principal> = (0..crate::assignees::MAX_PER_TICKET).map(|i| p(&format!("key:{:064x}", i + 7))).collect();
    let e = store.assign_by(vec![SYS.permit("AS-2"), SYS.permit("AS-3")], many, vec![], act("a")).await.unwrap_err();
    assert!(e.to_string().contains("AS-3") && e.to_string().contains("at most 16"), "{e}");
    assert!(store.get(SYS, "AS-2").await.unwrap().assignees.is_empty(), "nothing written");
    let e = store.assign_by(vec![SYS.permit("AS-99")], vec![p(ADA)], vec![], act("a")).await.unwrap_err();
    assert!(matches!(e, StoreError::NotFound(_)), "{e:?}");
}

#[tokio::test]
async fn the_filter_cells_are_answered_from_the_assignments() {
    let store = store().await;
    store.assign_by(vec![SYS.permit("AS-2")], vec![p(ADA), p(BOB)], vec![], act("a")).await.unwrap();
    store.assign_by(vec![SYS.permit("AS-3")], vec![p(BOB)], vec![], act("a")).await.unwrap();
    assert_eq!(count(&store, "assigned", "x"), 2);
    assert_eq!(count(&store, "unassigned", "x"), 2, "AS-1 and AS-4");
    assert_eq!(count(&store, "not assigned", "x"), 2);
    assert_eq!(count(&store, &format!("assigned \"{BOB}\""), "x"), 2);
    assert_eq!(count(&store, "assigned me", ADA), 1);
    assert_eq!(count(&store, "assigned me", "key:nobody"), 0);
    assert_eq!(count(&store, "assigned me and kind build", BOB), 2);
    assert_eq!(count(&store, "unassigned and kind build", "x"), 1);
    assert_eq!(count(&store, "assigned me or unassigned", ADA), 3);
    for bad in ["assigned me today", "assigned by me", "unassigned me", "assigned system"] {
        assert!(Lens::parse(bad).is_err(), "{bad}");
    }
    assert_eq!(crate::filter::Filter::parse("assigned ME").unwrap().format(), "assigned me");
    assert_eq!(crate::filter::Filter::parse("Unassigned").unwrap().format(), "unassigned");
    // A watch of `unassigned` hears AS-4 leave, and AS-3 enter.
    let watcher = store.watcher(Lens::parse("unassigned").unwrap());
    let mut rx = store.subscribe_filtered();
    store.assign_by(vec![SYS.permit("AS-4")], vec![p(ADA)], vec![], act("a")).await.unwrap();
    store.assign_by(vec![SYS.permit("AS-3")], vec![], vec![p(BOB)], act("a")).await.unwrap();
    let mut heard = Vec::new();
    while let Ok(c) = rx.try_change() {
        if let Change::UnderMoved { moves } = c {
            heard.extend(watcher.under_membership(&moves));
        }
    }
    assert_eq!(
        heard,
        [Membership::Left { id: "AS-4".into(), shortcode: "AS".into(), seq: 4 }, Membership::Entered { id: "AS-3".into(), shortcode: "AS".into(), seq: 3 }]
    );
}

#[tokio::test]
async fn update_many_assigns_as_assign_does() {
    let store = store().await;
    let ask = Update { assign: vec![p(ADA)], ..Update::default() };
    let done = store.update_many(SYS, Selection::Ids(vec!["AS-2".into(), "AS-3".into()]), ask).await.unwrap();
    assert_eq!(done.changed, ["assignees"]);
    assert_eq!(count(&store, "assigned", "x"), 2);
    assert_eq!(store.assign_history(SYS, "AS-3").len(), 1);
    let ask = Update { unassign: vec![p(ADA)], status: Some(Status::Ready), ..Update::default() };
    let done = store.update_many(SYS, Selection::Ids(vec!["AS-2".into()]), ask).await.unwrap();
    assert_eq!(done.changed, ["status", "assignees"]);
    assert!(done.tickets[0].assignees.is_empty());
}

#[tokio::test]
async fn assignees_survive_a_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("as.db");
    {
        let store = Store::open(&path).unwrap();
        store.create(SYS, ticket(Kind::Scope)).await.unwrap();
        store.create(SYS, ticket(Kind::Build)).await.unwrap();
        store.assign_by(vec![SYS.permit("AS-2")], vec![p(ADA), p(BOB)], vec![], act("a")).await.unwrap();
        store.assign_by(vec![SYS.permit("AS-2")], vec![], vec![p(BOB)], act("b")).await.unwrap();
    }
    let store = Store::open(&path).unwrap();
    assert_eq!(store.get(SYS, "AS-2").await.unwrap().assignees, [ADA]);
    assert_eq!(count(&store, "assigned me", ADA), 1);
    assert_eq!(count(&store, &format!("assigned \"{BOB}\""), "x"), 0);
    assert_eq!(store.assign_history(SYS, "AS-2").len(), 3);
    store.assign_by(vec![SYS.permit("AS-2")], vec![p(BOB)], vec![], act("a")).await.unwrap();
    let ids: Vec<u64> = store.assign_history(SYS, "AS-2").iter().map(|r| r.id).collect();
    assert!(ids.windows(2).all(|w| w[0] < w[1]), "{ids:?}");
}

#[tokio::test]
async fn assignees_and_their_history_move_with_a_reshortcode() {
    let store = store().await;
    store.create(SYS, NewTicket { epic: "MV".into(), title: "the root".into(), kind: Kind::Scope, severity: None, blocked_by: Vec::new(), parent: None, path: None, body: String::new() }).await.unwrap();
    store.assign_by(vec![SYS.permit("AS-3")], vec![p(ADA)], vec![], act("a")).await.unwrap();
    let r = store.reshortcode(SYS, Reshortcode { ids: vec!["AS-3".into()], to: "MV".into(), ..Reshortcode::default() }).await.unwrap();
    let moved = r.tickets.iter().find(|t| t.id.as_str() != "MV-1").expect("the moved ticket");
    assert_eq!(moved.assignees, [ADA]);
    assert_eq!(store.assign_history(SYS, moved.id.as_str()).len(), 1, "its history moved with it");
    assert_eq!(count(&store, "assigned me", ADA), 1);
}
