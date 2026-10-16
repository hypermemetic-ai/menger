//! ACCESS-15: `tickets commits` over the activation — `link` and `unlink`,
//! reached as `tickets.commits.*` through the parent's child accessor.

use super::*;

/// SDLC-9 over the activation (criteria 1, 2, 4): two commits linked,
/// then one of them again with a third — three in link order, the
/// second call's `added` the third alone, `get` showing them; a call
/// with one malformed entry is one in-band `error` naming it and writes
/// nothing; `not_found` for a missing id; and `watch` carries one event
/// per call that changed something — `commits_linked`,
/// `commits_unlinked` — and none for the no-ops or the refusal.
#[tokio::test]
async fn link_yields_commits_linked_and_watch_carries_only_what_changed() {
    let hub = hub();
    one(hub.create(anyone(), "LC".into(), "root".into(), Some("scope".into()), None, None, None, None, None, None).await).await;
    let mut watch = Box::pin(hub.watch(anyone(), None, None, None, None).await);
    let commits = |v: &[&str]| v.iter().map(|s| (*s).into()).collect::<Vec<crate::vocab::CommitRef>>();

    let ev = one(hub.commits().link(anyone(), Some("lc-1".into()), None, commits(&["77df9a0", "plexus-macros@4F7114A"])).await).await;
    let wire = serde_json::to_value(&ev).unwrap();
    assert_eq!(wire["type"], "commits_linked");
    assert_eq!(wire["added"], serde_json::json!(["77df9a0", "plexus-macros@4f7114a"]));
    assert_eq!(wire["ticket"]["id"], "LC-1");
    assert_eq!(wire["ticket"]["commits"], wire["added"]);
    let ev = one(hub.commits().link(anyone(), Some("LC-1".into()), None, commits(&["77df9a0", "0bb28b2"])).await).await;
    let wire = serde_json::to_value(&ev).unwrap();
    assert_eq!(wire["added"], serde_json::json!(["0bb28b2"]), "the third alone");
    let three = serde_json::json!(["77df9a0", "plexus-macros@4f7114a", "0bb28b2"]);
    assert_eq!(wire["ticket"]["commits"], three);
    let got = serde_json::to_value(one(hub.get(anyone(), "LC-1".into()).await).await).unwrap();
    assert_eq!(got["ticket"]["commits"], three, "get shows them, in link order");
    let (rows, _) = ids_and_page(hub.list(anyone(), None, None, None, None, None, Some(true), None, None, None, None).await).await;
    assert_eq!(rows, ["LC-1"]);
    let evs: Vec<TicketsEvent> = Box::pin(hub.list(anyone(), None, None, None, None, None, Some(true), None, None, None, None).await).collect().await;
    assert_eq!(serde_json::to_value(&evs[0]).unwrap()["summary"]["commits"], three, "and the summary row");

    for (i, added) in [&["77df9a0"], &["0bb28b2"]].into_iter().enumerate() {
        match watch.next().await.expect("change") {
            TicketsEvent::CommitsLinked { ticket, added: got } => {
                assert_eq!((ticket.id.as_str(), got.len()), ("LC-1", if i == 0 { 2 } else { 1 }));
                assert_eq!(got[0], added[0]);
            }
            other => panic!("expected CommitsLinked, got {other:?}"),
        }
    }

    // The no-op, the refusal, the miss: nothing on `watch`.
    match one(hub.commits().link(anyone(), Some("LC-1".into()), None, commits(&["0BB28B2"])).await).await {
        TicketsEvent::CommitsLinked { added, ticket } => {
            assert!(added.is_empty(), "{added:?}");
            assert_eq!(ticket.commits.len(), 3);
        }
        other => panic!("expected CommitsLinked, got {other:?}"),
    }
    // ACCESS-8: a malformed entry is no CommitRef — refused at dispatch,
    // naming it (`invalid_params`, tests/wire.rs), before the method runs.
    let refused = "77df9a0;rm".parse::<crate::vocab::CommitRef>().unwrap_err();
    assert!(refused.contains("`77df9a0;rm`"), "names the entry: {refused}");
    assert!(matches!(
        one(hub.commits().link(anyone(), Some("LC-9".into()), None, commits(&["c8c5678"])).await).await,
        TicketsEvent::NotFound { id } if id == "LC-9"
    ));
    let got = serde_json::to_value(one(hub.get(anyone(), "LC-1".into()).await).await).unwrap();
    assert_eq!(got["ticket"]["commits"], three, "the refused call wrote nothing");

    let ev = one(hub.commits().unlink(anyone(), "LC-1".into(), commits(&["plexus-macros@4f7114a", "c8c5678"])).await).await;
    let wire = serde_json::to_value(&ev).unwrap();
    assert_eq!(wire["type"], "commits_unlinked");
    assert_eq!(wire["removed"], serde_json::json!(["plexus-macros@4f7114a"]));
    assert_eq!(wire["ticket"]["commits"], serde_json::json!(["77df9a0", "0bb28b2"]), "exactly what it named");
    match one(hub.commits().unlink(anyone(), "LC-1".into(), commits(&["c8c5678"])).await).await {
        TicketsEvent::CommitsUnlinked { removed, .. } => assert!(removed.is_empty()),
        other => panic!("expected CommitsUnlinked, got {other:?}"),
    }
    match watch.next().await.expect("change") {
        TicketsEvent::CommitsUnlinked { ticket, removed } => {
            assert_eq!(removed, ["plexus-macros@4f7114a"], "the no-op, the refusal and the miss said nothing");
            assert_eq!(ticket.commits, ["77df9a0", "0bb28b2"]);
        }
        other => panic!("expected CommitsUnlinked, got {other:?}"),
    }
    assert!(futures::FutureExt::now_or_never(watch.next()).is_none(), "and nothing more");
}
