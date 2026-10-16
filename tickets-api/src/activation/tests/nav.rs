//! ACCESS-16: `tickets nav`'s tests, moved from `tests/mod.rs` with its
//! methods — each called through the parent's `nav` accessor, as the hub
//! routes `tickets.nav.<method>`.

use super::*;

/// MNG-10: `groups` yields one `Group` per shortcode row and nothing
/// else — no trailing page; the stream's end is the answer's end — and
/// `watch { epic }` delivers a `GroupChanged` by the group's own
/// shortcode: a watcher of PLX hears PLX's count move when a ticket of
/// its sub-shortcode completes, without hearing the ticket.
#[tokio::test]
async fn groups_yields_the_rows_and_watch_filters_group_changes_by_their_shortcode() {
    let hub = hub();
    one(hub.create(anyone(), "PLX".into(), "host".into(), Some("scope".into()), None, None, None, None, None, None).await).await;
    one(hub.create(anyone(), "SUB".into(), "hosted".into(), Some("scope".into()), None, None, None, Some("PLX-1".into()), None, None).await).await;
    one(hub.create(anyone(), "SUB".into(), "work".into(), None, None, None, None, None, None, None).await).await;

    let evs: Vec<TicketsEvent> = Box::pin(hub.nav().groups(anyone()).await).collect().await;
    let groups: Vec<crate::types::Group> = evs
        .into_iter()
        .map(|ev| match ev {
            TicketsEvent::Group { group } => group,
            other => panic!("groups yields only `group` events, got {other:?}"),
        })
        .collect();
    assert_eq!(
        groups.iter().map(|g| (g.epic.as_str(), g.total, g.host.as_deref())).collect::<Vec<_>>(),
        [("PLX", 3, None), ("SUB", 2, Some("PLX-1"))]
    );
    assert_eq!(serde_json::to_value(&groups[0]).unwrap()["host"], serde_json::Value::Null, "null, not missing");

    let mut plx = Box::pin(hub.watch(anyone(), Some("plx".into()), None, None, None).await);
    one(hub.set_status(anyone(), "SUB-2".into(), "Complete".into(), None, None, None).await).await;
    match plx.next().await.expect("change") {
        TicketsEvent::GroupChanged { group } => {
            assert_eq!((group.epic.as_str(), group.total, group.done), ("PLX", 3, 1))
        }
        other => panic!("the SUB ticket's status_changed is filtered out; got {other:?}"),
    }
}

/// SCALE-16: `extents` yields one `Extent` per shortcode that holds a
/// ticket and nothing else — no trailing page — in shortcode order;
/// a refused create moves nothing, and the `last_seq`s add up to
/// `health`'s count.
#[tokio::test]
async fn extents_yields_each_shortcodes_last_seq_in_shortcode_order() {
    let hub = hub();
    for (epic, kind) in [("ZED", "scope"), ("AB", "scope"), ("ZED", "build"), ("B", "scope"), ("ZED", "gap")] {
        one(hub.create(anyone(), epic.into(), "t".into(), Some(kind.into()), None, None, None, None, None, None).await).await;
    }
    // Refused: AB-2 would be its own blocker. Nothing is allocated.
    let refused = one(hub.create(anyone(), "AB".into(), "t".into(), None, None, Some(vec!["AB-2".into()]), None, None, None, None).await).await;
    assert!(matches!(refused, TicketsEvent::Error { .. }), "{refused:?}");

    let evs: Vec<TicketsEvent> = Box::pin(hub.nav().extents(anyone()).await).collect().await;
    let extents: Vec<(String, u32)> = evs
        .into_iter()
        .map(|ev| match ev {
            TicketsEvent::Extent { epic, last_seq, .. } => (epic.to_string(), last_seq),
            other => panic!("extents yields only `extent` events, got {other:?}"),
        })
        .collect();
    assert_eq!(
        extents,
        [("AB".to_string(), 1), ("B".to_string(), 1), ("ZED".to_string(), 3)],
        "byte order on the epic: AB before B"
    );
    let total: u64 = extents.iter().map(|(_, n)| u64::from(*n)).sum();
    match one(hub.health(anyone()).await).await {
        TicketsEvent::Health { ticket_count, .. } => assert_eq!(ticket_count, total),
        other => panic!("expected Health, got {other:?}"),
    }
    let wire = serde_json::to_value(TicketsEvent::Extent { epic: "AB".into(), shortcode: "AB".into(), last_seq: 1, holes: Vec::new() }).unwrap();
    assert_eq!(wire, serde_json::json!({"type": "extent", "epic": "AB", "shortcode": "AB", "last_seq": 1}));
}
