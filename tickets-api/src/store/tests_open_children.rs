//! TICKETS-30: a ticket comes to `Complete` only when every child is —
//! refused in band, naming each open child, on `set_status`, `update` and
//! `update_many` (the deploy's `record --complete` is an `update_many`);
//! Canceled and Superseded children do not hold it, a doc is not counted,
//! and a ticket already Complete is not asked again.

use super::*;

fn new(title: &str, kind: Kind, parent: Option<&str>) -> NewTicket {
    NewTicket {
        path: None,
        epic: "P".into(),
        title: title.into(),
        kind,
        severity: None,
        blocked_by: Vec::new(),
        parent: parent.map(Into::into),
        body: String::new(),
    }
}

/// P-1 the root; P-2 a build under it with children P-3 (Build), P-4
/// (Build), P-5 (Build), P-6 (Doc).
async fn store() -> Store {
    let store = Store::new();
    store.create(SYS, new("root", Kind::Scope, None)).await.unwrap();
    store.create(SYS, new("parent", Kind::Build, Some("P-1"))).await.unwrap();
    for t in ["a", "b", "c"] {
        store.create(SYS, new(t, Kind::Build, Some("P-2"))).await.unwrap();
    }
    store.create(SYS, new("a doc", Kind::Doc, Some("P-2"))).await.unwrap();
    store
}

async fn set(store: &Store, id: &str, s: Status) -> Result<Change, StoreError> {
    store.set_status(SYS.permit(id), s).await
}

async fn status_of(store: &Store, id: &str) -> Status {
    store.get(SYS, id).await.unwrap().status
}

fn open_of(e: StoreError) -> Vec<String> {
    match e {
        StoreError::OpenChildren { open, .. } => open,
        StoreError::Refused { refusals, .. } => refusals.into_iter().flat_map(open_of).collect(),
        StoreError::RefusedMany { refusals } => refusals.into_iter().flat_map(|(_, rs)| rs.into_iter().flat_map(open_of)).collect(),
        other => panic!("not an open-children refusal: {other:?}"),
    }
}

fn upd(s: Status) -> Update {
    Update { status: Some(s), ..Update::default() }
}

#[tokio::test]
async fn set_status_refuses_with_an_open_child_naming_each() {
    let store = store().await;
    set(&store, "P-3", Status::Complete).await.unwrap();
    let err = set(&store, "P-2", Status::Complete).await.unwrap_err();
    let said = err.to_string();
    assert_eq!(open_of(err), ["P-4", "P-5"]);
    assert!(said.contains("P-4, P-5 are not Complete"), "{said}");
    assert_eq!(status_of(&store, "P-2").await, Status::Draft, "nothing changed");
}

#[tokio::test]
async fn allowed_when_all_are_complete_canceled_or_superseded_and_docs_ignored() {
    let store = store().await;
    set(&store, "P-3", Status::Complete).await.unwrap();
    set(&store, "P-4", Status::Canceled).await.unwrap();
    set(&store, "P-5", Status::Superseded).await.unwrap();
    // P-6 is a doc, in the status Doc: not counted.
    set(&store, "P-2", Status::Complete).await.unwrap();
    assert_eq!(status_of(&store, "P-2").await, Status::Complete);
    // Already Complete: asked again, it is the no-op it was.
    set(&store, "P-2", Status::Complete).await.unwrap();
    // Other statuses are never held by children.
    set(&store, "P-1", Status::Coding).await.unwrap();
}

#[tokio::test]
async fn update_refuses_and_allows() {
    let store = store().await;
    let err = store.update(SYS.updating("P-2"), upd(Status::Complete)).await.unwrap_err();
    assert_eq!(open_of(err), ["P-3", "P-4", "P-5"]);
    assert_eq!(status_of(&store, "P-2").await, Status::Draft);
    for c in ["P-3", "P-4", "P-5"] {
        set(&store, c, Status::Complete).await.unwrap();
    }
    store.update(SYS.updating("P-2"), upd(Status::Complete)).await.unwrap();
    assert_eq!(status_of(&store, "P-2").await, Status::Complete);
}

#[tokio::test]
async fn update_many_refuses_all_or_none_and_allows() {
    let store = store().await;
    let sel = || Selection::Ids(vec!["P-2".to_string()]);
    let err = store.update_many(SYS, sel(), upd(Status::Complete)).await.unwrap_err();
    assert_eq!(open_of(err), ["P-3", "P-4", "P-5"]);
    // The children together with the parent in one write: the children are
    // not Complete when the parent is looked at, so the write is refused.
    // (The deploy completes the carried tickets in one update_many too.)
    let both = Selection::Ids(["P-3", "P-4", "P-5"].iter().map(|s| s.to_string()).collect());
    store.update_many(SYS, both, upd(Status::Complete)).await.unwrap();
    store.update_many(SYS, sel(), upd(Status::Complete)).await.unwrap();
    assert_eq!(status_of(&store, "P-2").await, Status::Complete);
}
