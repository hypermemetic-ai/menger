//! SDLC-7: the worker states through the store. Statuses are stored as
//! their wire text, so `Coding` and `In Review` need no migration — what
//! is pinned here is that all nine go through the system of record and
//! back unchanged, that the stored text *is* the wire spelling (space
//! included), and that the two new states are what they claim to be and
//! nothing more: not ready, not done, not a satisfied blocker.

use super::*;

fn ticket(epic: &str, kind: Kind, blocked_by: &[&str]) -> NewTicket {
    NewTicket {
        epic: epic.into(),
        title: "t".into(),
        kind,
        severity: None,
        blocked_by: blocked_by.iter().map(|b| b.to_string()).collect(),
        parent: None,
        body: String::new(),
    }
}

/// Criterion 5 at the store: one ticket per status, set through
/// `set_status`, reopened from the file — each reads back as it was set,
/// `list { status }` finds exactly the tickets in it, and the column holds
/// the wire spelling.
#[tokio::test]
async fn all_nine_statuses_round_trip_through_the_database_and_filter() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("statuses.db");
    {
        let store = Store::open(&path).unwrap();
        store.create(ticket("ST", Kind::Scope, &[])).await.unwrap(); // ST-1, the root
        for s in Status::ALL {
            let id = store.create(ticket("ST", Kind::Build, &[])).await.unwrap().id;
            match store.set_status(&id, s).await.unwrap() {
                Change::StatusChanged { ticket, previous } => {
                    assert_eq!((ticket.status, previous), (s, Status::Pending), "{id}");
                }
                other => panic!("expected StatusChanged, got {other:?}"),
            }
        }
        store.set_status("ST-1", Status::Epic).await.unwrap();
    }

    let stored: Vec<String> = Connection::open(&path)
        .unwrap()
        .prepare("SELECT status FROM tickets WHERE seq > 1 ORDER BY seq")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(stored, Status::ALL.map(|s| s.as_str().to_string()), "the column holds the wire text");
    assert_eq!(stored[3], "In Review");

    let store = Store::open(&path).unwrap();
    for (i, s) in Status::ALL.into_iter().enumerate() {
        let id = format!("ST-{}", i + 2);
        assert_eq!(store.get(&id).unwrap().status, s, "{id} after a reopen");
        let found: Vec<String> = store
            .list(Some("ST"), Some(s), None)
            .into_iter()
            .map(|t| t.id)
            .collect();
        let want: Vec<String> = if s == Status::Epic {
            vec!["ST-1".into(), id]
        } else {
            vec![id]
        };
        assert_eq!(found, want, "list {{ status: {s} }}");
    }
}

/// The worker states change nothing they should not: a `Coding` ticket is
/// not in the ready set, an `In Review` blocker does not free what it
/// blocks, and neither moves a roll-up's `done` — only the merge's
/// `Complete` does (MNG-10's rule, unchanged).
#[tokio::test]
async fn the_worker_states_are_neither_ready_nor_done() {
    let store = Store::new();
    store.create(ticket("WS", Kind::Scope, &[])).await.unwrap(); // WS-1
    store.create(ticket("WS", Kind::Build, &[])).await.unwrap(); // WS-2
    store.create(ticket("WS", Kind::Build, &["WS-2"])).await.unwrap(); // WS-3, blocked by WS-2
    store.set_status("WS-3", Status::Ready).await.unwrap();
    let done = |store: &Store| store.groups().into_iter().find(|g| g.epic == "WS").unwrap().done;

    let mut rx = store.subscribe();
    for s in [Status::Ready, Status::Coding, Status::InReview] {
        store.set_status("WS-2", s).await.unwrap();
        assert!(matches!(rx.try_recv(), Ok(Change::StatusChanged { .. })));
        assert!(rx.try_recv().is_err(), "{s}: no group_changed — nothing crossed Complete");
        assert_eq!(done(&store), 0, "{s}");
        let ready: Vec<String> = store.ready_set(None).into_iter().map(|t| t.id).collect();
        let want: &[&str] = if s == Status::Ready { &["WS-2"] } else { &[] };
        assert_eq!(ready, want, "{s}: WS-3 waits on its blocker; WS-2 is ready only while Ready");
    }
    store.set_status("WS-2", Status::Complete).await.unwrap();
    assert_eq!(done(&store), 1, "the merge's Complete is what counts");
    let ready: Vec<String> = store.ready_set(None).into_iter().map(|t| t.id).collect();
    assert_eq!(ready, ["WS-3"]);
}
