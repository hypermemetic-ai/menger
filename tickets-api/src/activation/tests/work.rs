//! ACCESS-17: `tickets work`'s tests — `ready` (was `ready_set`), `waves`,
//! `waves_under`, `waves_to`, `blockers` — moved here from `tests/mod.rs`,
//! called through the child as the hub routes them (`hub.work()`).

use super::*;

#[tokio::test]
async fn ready_set_gates_on_blockers() {
    let hub = hub();
    one(hub.create(anyone(), "DAG".into(), "a".into(), Some("scope".into()), None, None, None, None, None, None).await).await;
    one(hub
        .create(anyone(), "DAG".into(), "b".into(), None, None, Some(vec!["DAG-1".into()]), None, None, None, None)
        .await)
    .await;
    one(hub.set_status(anyone(), "DAG-1".into(), "Ready".into(), None, None, None).await).await;
    one(hub.set_status(anyone(), "DAG-2".into(), "Ready".into(), None, None, None).await).await;

    assert_eq!(ticket_ids(hub.work().ready(anyone(), None, None, None, None, None, None).await).await, vec!["DAG-1"]);

    one(hub.set_status(anyone(), "DAG-1".into(), "Complete".into(), None, None, None).await).await;
    assert_eq!(ticket_ids(hub.work().ready(anyone(), None, None, None, None, None, None).await).await, vec!["DAG-2"]);
}

#[tokio::test]
async fn ready_set_accepts_superseded_blockers_and_filters_by_epic() {
    let hub = hub();
    one(hub.create(anyone(), "ONE".into(), "a".into(), Some("scope".into()), None, None, None, None, None, None).await).await;
    one(hub
        .create(anyone(), "ONE".into(), "b".into(), None, None, Some(vec!["ONE-1".into()]), None, None, None, None)
        .await)
    .await;
    one(hub.create(anyone(), "TWO".into(), "c".into(), Some("scope".into()), None, None, None, None, None, None).await).await;
    one(hub.set_status(anyone(), "ONE-1".into(), "Superseded".into(), None, None, None).await).await;
    one(hub.set_status(anyone(), "ONE-2".into(), "Ready".into(), None, None, None).await).await;
    one(hub.set_status(anyone(), "TWO-1".into(), "Ready".into(), None, None, None).await).await;

    assert_eq!(
        ticket_ids(hub.work().ready(anyone(), None, None, None, None, None, None).await).await,
        vec!["ONE-2", "TWO-1"]
    );
    assert_eq!(
        ticket_ids(hub.work().ready(anyone(), Some("one".into()), None, None, None, None, None).await).await,
        vec!["ONE-2"]
    );
}

#[tokio::test]
async fn dangling_blocker_excludes_from_ready_set_and_is_marked() {
    let hub = hub();
    one(hub
        .create(anyone(), "DAG".into(), "c".into(), Some("scope".into()), None, Some(vec!["DAG-99".into()]), None, None, None, None)
        .await)
    .await;
    one(hub.set_status(anyone(), "DAG-1".into(), "Ready".into(), None, None, None).await).await;

    assert!(ticket_ids(hub.work().ready(anyone(), None, None, None, None, None, None).await).await.is_empty());

    let evs: Vec<_> = Box::pin(hub.work().blockers(anyone(), "DAG-1".into()).await).collect().await;
    assert_eq!(evs.len(), 1);
    match &evs[0] {
        TicketsEvent::DanglingBlocker { id } => assert_eq!(id, "DAG-99"),
        other => panic!("expected DanglingBlocker, got {other:?}"),
    }
}

#[tokio::test]
async fn blockers_resolve_to_current_statuses_in_order() {
    let hub = hub();
    one(hub.create(anyone(), "DAG".into(), "a".into(), Some("scope".into()), None, None, None, None, None, None).await).await;
    one(hub.create(anyone(), "DAG".into(), "b".into(), None, None, None, None, None, None, None).await).await;
    one(hub
        .create(anyone(), 
            "DAG".into(),
            "c".into(),
            None,
            None,
            Some(vec!["DAG-1".into(), "DAG-2".into(), "DAG-99".into()]),
            None, None, None,
            None,
        )
        .await)
    .await;
    one(hub.set_status(anyone(), "DAG-1".into(), "Complete".into(), None, None, None).await).await;

    let evs: Vec<_> = Box::pin(hub.work().blockers(anyone(), "DAG-3".into()).await).collect().await;
    assert_eq!(evs.len(), 3);
    match &evs[0] {
        TicketsEvent::Blocker { ticket } => {
            assert_eq!(ticket.id, "DAG-1");
            assert_eq!(ticket.status, Status::Complete);
        }
        other => panic!("expected Blocker, got {other:?}"),
    }
    match &evs[1] {
        TicketsEvent::Blocker { ticket } => {
            assert_eq!(ticket.id, "DAG-2");
            assert_eq!(ticket.status, Status::Pending);
        }
        other => panic!("expected Blocker, got {other:?}"),
    }
    match &evs[2] {
        TicketsEvent::DanglingBlocker { id } => assert_eq!(id, "DAG-99"),
        other => panic!("expected DanglingBlocker, got {other:?}"),
    }

    assert!(matches!(
        one(hub.work().blockers(anyone(), "DAG-42".into()).await).await,
        TicketsEvent::NotFound { .. }
    ));
}

/// MNG-9: `ready_set` takes the same three params, and its page event
/// counts the ready set — not the store.
#[tokio::test]
async fn ready_set_takes_the_same_page_params() {
    let hub = hub();
    for i in 0..3 {
        let kind = (i == 0).then_some(crate::types::Kind::Scope); // MNG-12: the root first
        one(hub.create(anyone(), "RS".into(), "r".into(), kind, None, None, None, None, None, None).await).await;
    }
    one(hub.create(anyone(), "RS".into(), "still pending".into(), None, None, None, None, None, None, None).await).await;
    for id in ["RS-1", "RS-2", "RS-3"] {
        one(hub.set_status(anyone(), id.into(), "Ready".into(), None, None, None).await).await;
    }

    let (ids, page) = ids_and_page(hub.work().ready(anyone(), None, None, Some(2), Some(true), None, None).await).await;
    assert_eq!(ids, vec!["RS-1", "RS-2"]);
    assert_eq!(
        page,
        Some((Some("RS-2".into()), 1)),
        "one ready ticket left, not two store rows"
    );

    let (ids, page) =
        ids_and_page(hub.work().ready(anyone(), None, Some("RS-2".into()), Some(2), Some(true), None, None).await).await;
    assert_eq!(ids, vec!["RS-3"]);
    assert_eq!(page, Some((None, 0)));
}

/// The `waves` stream, split the way the contract reads it: one
/// `(id, placement, waiting_on)` per `placed`, then the one closing
/// `wave_counts` — asserted last and alone.
#[allow(clippy::type_complexity)]
async fn placed_and_counts(
    s: impl Stream<Item = TicketsEvent> + Send + 'static,
) -> (Vec<(String, crate::types::Placement, Vec<crate::TicketName>)>, (Vec<u64>, u64, u64)) {
    let evs: Vec<TicketsEvent> = Box::pin(s).collect().await;
    let mut placed = Vec::new();
    let mut counts = None;
    for ev in evs {
        assert!(counts.is_none(), "wave_counts is last: {ev:?}");
        match ev {
            TicketsEvent::Placed { summary, placement, waiting_on } => {
                placed.push((summary.id.into_string(), placement, waiting_on))
            }
            TicketsEvent::WaveCounts { waves, missing, done } => counts = Some((waves, missing, done)),
            other => panic!("expected placed or wave_counts, got {other:?}"),
        }
    }
    (placed, counts.expect("every waves answer ends with wave_counts"))
}

/// WAVE-4 acceptance 2: the yield order exactly as specified — waves
/// ascending; inside a wave Coding, In Review, Ready, Pending, Blocked,
/// Idea, Epic, then (epic, seq); then the missing lane; then the done,
/// each in (epic, seq) — with the default kinds (the scope roots are not
/// work), a blocker followed into a shortcode not asked for, and the
/// tally after the rows, exactly once.
#[tokio::test]
async fn waves_yields_placements_in_the_specified_order_then_one_wave_counts() {
    use crate::types::Placement::{Done, MissingBlocker, Wave};
    let hub = hub();
    let create = |epic: &str, kind: Option<&str>, blocked_by: &[&str]| {
        let blocked_by = (!blocked_by.is_empty()).then(|| blocked_by.iter().map(|b| (*b).into()).collect());
        let (epic, kind) = (epic.into(), kind.map(Into::into));
        let hub = hub.clone();
        async move { one(hub.create(anyone(), epic, "t".into(), kind, None, blocked_by, None, None, None, None).await).await }
    };
    let status = |id: &str, s: &str| {
        let (id, s, hub) = (id.into(), s.into(), hub.clone());
        async move { one(hub.set_status(anyone(), id, s, None, None, None).await).await }
    };
    for epic in ["WA", "WB", "ELSE"] {
        create(epic, Some("scope"), &[]).await; // X-1, each shortcode's root
    }
    create("ELSE", None, &[]).await; // ELSE-2, wave 1 — not asked for
    create("WA", None, &["ELSE-2"]).await; // WA-2: wave 2, Pending
    create("WB", Some("spike"), &[]).await; // WB-2: wave 1, Pending
    create("WA", Some("gap"), &[]).await; // WA-3: wave 1, Ready
    status("WA-3", "Ready").await;
    create("WB", None, &[]).await; // WB-3: wave 1, Coding
    status("WB-3", "active").await;
    create("WA", None, &["WB-3"]).await; // WA-4: wave 2, In Review
    status("WA-4", "in review").await;
    create("WB", None, &["WB-9"]).await; // WB-4: missing (WB-9 is no ticket)
    create("WA", None, &["WB-4"]).await; // WA-5: missing, behind WB-4
    create("WA", None, &[]).await; // WA-6: done
    status("WA-6", "Complete").await;
    create("WA", Some("execution"), &[]).await; // WA-7: not work, not yielded
    create("WB", None, &[]).await; // WB-5: wave 1, Blocked (a marker, not an edge)
    status("WB-5", "Blocked").await;

    let (placed, counts) = placed_and_counts(hub.work().waves(anyone(), vec!["wa".into(), "WB".into()], None).await).await;
    let want: Vec<(&str, crate::types::Placement, Vec<&str>)> = vec![
        ("WB-3", Wave { n: 1 }, vec![]),
        ("WA-3", Wave { n: 1 }, vec![]),
        ("WB-2", Wave { n: 1 }, vec![]),
        ("WB-5", Wave { n: 1 }, vec![]),
        ("WA-4", Wave { n: 2 }, vec!["WB-3"]),
        ("WA-2", Wave { n: 2 }, vec!["ELSE-2"]),
        ("WA-5", MissingBlocker, vec!["WB-4"]),
        ("WB-4", MissingBlocker, vec!["WB-9"]),
        ("WA-6", Done, vec![]),
    ];
    let got: Vec<(&str, crate::types::Placement, Vec<&str>)> = placed
        .iter()
        .map(|(id, p, w)| (id.as_str(), *p, w.iter().map(crate::TicketName::as_str).collect()))
        .collect();
    assert_eq!(got, want);
    assert_eq!(counts, (vec![4, 2], 2, 1));

    // `kinds` names what is yielded: the scopes and the execution too.
    let (placed, counts) = placed_and_counts(
        hub.work().waves(anyone(), vec!["WA".into()], Some(vec!["scope".into(), "Execution".into()])).await,
    )
    .await;
    let ids: Vec<&str> = placed.iter().map(|(id, _, _)| id.as_str()).collect();
    assert_eq!(ids, ["WA-1", "WA-7"]);
    assert_eq!(counts, (vec![2], 0, 0));

    // A shortcode with no tickets: nothing placed, the tally all zero.
    let (placed, counts) = placed_and_counts(hub.work().waves(anyone(), vec!["NONE".into()], None).await).await;
    assert!(placed.is_empty());
    assert_eq!(counts, (vec![], 0, 0));
}

/// WAVE-4 acceptance 2: the error cases are in-band — `epics` empty, and
/// a kind outside the six (the error `create` gives, naming them) — one
/// `Error` and nothing else, no tally.
#[tokio::test]
async fn waves_errors_are_in_band_and_alone() {
    let hub = hub();
    one(hub.create(anyone(), "ER".into(), "root".into(), Some("scope".into()), None, None, None, None, None, None).await).await;

    let evs: Vec<TicketsEvent> = Box::pin(hub.work().waves(anyone(), vec![], None).await).collect().await;
    assert_eq!(evs.len(), 1, "{evs:?}");
    match &evs[0] {
        TicketsEvent::Error { message } => assert!(message.contains("epics"), "{message}"),
        other => panic!("expected Error, got {other:?}"),
    }

    // ACCESS-8: a kind outside the six is refused at dispatch, naming them.
    let message = "bug".parse::<Kind>().unwrap_err();
    assert!(message.contains("'bug'"), "{message}");
    for k in Kind::ALL {
        assert!(message.contains(k.as_str()), "names {k}: {message}");
    }
}

/// MENGER-31: `waves_to` and `waves_under` on the wire's terms — the placed
/// rows in order, the ticket last, then the tally; an unknown id is
/// `NotFound`, a doc and a bad kind one in-band `Error`, alone.
#[tokio::test]
async fn waves_to_and_under_answer_as_waves_does() {
    use crate::types::Placement::Wave;
    let hub = hub();
    let create = |kind: &str, blocked_by: &[&str], parent: Option<&str>| {
        let blocked_by = (!blocked_by.is_empty()).then(|| blocked_by.iter().map(|b| (*b).into()).collect());
        let (kind, parent, hub) = (kind.into(), parent.map(Into::into), hub.clone());
        async move { one(hub.create(anyone(), "WT".into(), "t".into(), Some(kind), None, blocked_by, parent, None, None, None).await).await }
    };
    create("scope", &[], None).await; // WT-1
    create("build", &[], Some("WT-1")).await; // WT-2
    create("build", &["WT-2"], Some("WT-1")).await; // WT-3
    create("build", &["WT-2"], None).await; // WT-4
    create("build", &["WT-3", "WT-4"], None).await; // WT-5
    create("doc", &[], None).await; // WT-6

    let (placed, counts) = placed_and_counts(hub.work().waves_to(anyone(), "WT-5".into(), None).await).await;
    let got: Vec<(&str, crate::types::Placement)> = placed.iter().map(|(id, p, _)| (id.as_str(), *p)).collect();
    assert_eq!(got, [("WT-2", Wave { n: 1 }), ("WT-3", Wave { n: 2 }), ("WT-4", Wave { n: 2 }), ("WT-5", Wave { n: 3 })]);
    assert_eq!(counts, (vec![1, 2, 1], 0, 0));

    let (placed, _) = placed_and_counts(hub.work().waves_under(anyone(), "WT-1".into(), None).await).await;
    assert_eq!(placed.iter().map(|(id, ..)| id.as_str()).collect::<Vec<_>>(), ["WT-2", "WT-3", "WT-4", "WT-5"], "a root's subtree is its shortcode's work");

    for evs in [
        Box::pin(hub.work().waves_to(anyone(), "WT-99".into(), None).await).collect::<Vec<_>>().await,
        Box::pin(hub.work().waves_under(anyone(), "WT-99".into(), None).await).collect::<Vec<_>>().await,
    ] {
        assert!(matches!(evs.as_slice(), [TicketsEvent::NotFound { id }] if id == "WT-99"), "{evs:?}");
    }
    let evs = Box::pin(hub.work().waves_to(anyone(), "WT-6".into(), None).await).collect::<Vec<_>>().await;
    assert!(matches!(evs.as_slice(), [TicketsEvent::Error { .. }]), "{evs:?}");
}
