//! UI-6's testing surface: `live()` — the folds and the shared cache.

use super::*;
use crate::transport::{FakeWire, WireItem};
use serde_json::json;

/// The hello page's collection: every ticket's summary, folded from
/// `tickets.watch`. `deleted` carries `ids` (a list) on the wire.
fn spec(params: Value) -> LiveSpec {
    LiveSpec::new(
        "tickets.list",
        params,
        "tickets.watch",
        "id",
        &[("created", "upsert:ticket"), ("amended", "upsert:ticket"), ("deleted", "remove:ids")],
    )
    .expect("a spec")
}

fn summary(id: &str, title: &str) -> Value {
    json!({ "type": "summary", "summary": { "id": id, "title": title } })
}

fn ids(live: &Live) -> Vec<String> {
    live.rows.iter().map(|r| r["id"].as_str().unwrap_or("?").to_string()).collect()
}

fn setup() -> (FakeWire, ManualClock, Rc<LiveCache<FakeWire, ManualClock>>) {
    let rt = Runtime::new();
    let (wire, clock) = (FakeWire::new(), ManualClock::new());
    let cache = LiveCache::new(&rt, wire.clone(), clock.clone());
    (wire, clock, cache)
}

/// A snapshot of three rows, then `created` (upsert), `amended` (replace in
/// place), `deleted` (remove), then `lagged`: `resyncing`, the snapshot
/// asked again, `live` on its rows. And a dropped socket: `resyncing`, then
/// both calls opened again on the reconnect, `live`.
#[test]
fn live_folds_a_snapshot_and_its_changes() {
    assert_eq!("upsert:ticket".parse::<FoldRule>(), Ok(FoldRule::Upsert("ticket".into())));
    assert_eq!("remove:ids".parse::<FoldRule>(), Ok(FoldRule::Remove("ids".into())));
    assert!("replace:ticket".parse::<FoldRule>().is_err(), "only upsert: and remove:");

    let (wire, _clock, cache) = setup();
    let params = json!({ "summary": true, "limit": 200 });
    let reader = cache.acquire(&spec(params.clone()));
    assert_eq!(wire.opened_of("tickets.list").len(), 1, "the snapshot asked once");
    assert_eq!(wire.opened_of("tickets.list")[0].params, params, "with the declared params");
    assert_eq!(wire.opened_of("tickets.watch").len(), 1, "the changes opened once");
    assert_eq!(reader.live().status, Status::Pending);
    assert!(reader.live().rows.is_empty());

    let snap = wire.last_of("tickets.list");
    wire.answer(snap, [summary("T-1", "a"), summary("T-2", "b"), summary("T-3", "c"), json!({ "type": "page", "next": null, "remaining": 0 })]);
    let live = reader.live();
    assert_eq!(live.status, Status::Live);
    assert_eq!(ids(&live), ["T-1", "T-2", "T-3"], "each summary a row; the page none");
    assert_eq!(live.rows[1]["title"], "b");
    assert_eq!(live.to_value()["status"], "live", "an expression reads status as text");

    let watch = wire.last_of("tickets.watch");
    wire.feed(watch, WireItem::Data(json!({ "type": "created", "ticket": { "id": "T-4", "title": "d" } })));
    assert_eq!(ids(&reader.live()), ["T-1", "T-2", "T-3", "T-4"], "created: upserted");
    wire.feed(watch, WireItem::Data(json!({ "type": "amended", "ticket": { "id": "T-2", "title": "b2" }, "changed": ["title"] })));
    let live = reader.live();
    assert_eq!(ids(&live), ["T-1", "T-2", "T-3", "T-4"], "amended: replaced in place");
    assert_eq!(live.rows[1]["title"], "b2");
    wire.feed(watch, WireItem::Data(json!({ "type": "deleted", "ids": ["T-3"], "tombstones": [] })));
    assert_eq!(ids(&reader.live()), ["T-1", "T-2", "T-4"], "deleted: removed");
    wire.feed(watch, WireItem::Data(json!({ "type": "linked", "id": "T-1", "added": [] })));
    assert_eq!(ids(&reader.live()), ["T-1", "T-2", "T-4"], "an event no rule names: nothing");

    wire.feed(watch, WireItem::Data(json!({ "type": "lagged", "missed": 3 })));
    assert_eq!(reader.live().status, Status::Resyncing, "lagged: resyncing");
    assert_eq!(wire.opened_of("tickets.list").len(), 2, "the snapshot asked again");
    assert_eq!(wire.opened_of("tickets.watch").len(), 1, "the changes stream kept");
    wire.answer(wire.last_of("tickets.list"), [summary("T-1", "a"), summary("T-5", "e")]);
    let live = reader.live();
    assert_eq!(live.status, Status::Live);
    assert_eq!(ids(&live), ["T-1", "T-5"], "the new snapshot's rows");

    wire.drop_socket();
    assert_eq!(reader.live().status, Status::Resyncing, "the socket dropped: resyncing");
    cache.reconnected();
    assert_eq!(wire.opened_of("tickets.watch").len(), 2, "the changes re-opened on the new socket");
    assert_eq!(wire.opened_of("tickets.list").len(), 3, "the snapshot asked again");
    wire.answer(wire.last_of("tickets.list"), [summary("T-5", "e")]);
    assert_eq!(reader.live().status, Status::Live);
    assert_eq!(ids(&reader.live()), ["T-5"]);
}

/// Two readers of one method and params open one stream; other params
/// another; both readers gone, a third within 5 s reuses the first; past
/// 5 s it is closed and a new reader opens anew.
#[test]
fn a_subscription_is_shared_and_kept_five_seconds() {
    let (wire, clock, cache) = setup();
    let params = json!({ "summary": true, "limit": 200 });
    let one = cache.acquire(&spec(params.clone()));
    let two = cache.acquire(&spec(params.clone()));
    assert_eq!(wire.opened_of("tickets.list").len(), 1, "one snapshot for two readers");
    assert_eq!(wire.opened_of("tickets.watch").len(), 1, "one changes stream for two readers");
    wire.answer(wire.last_of("tickets.list"), [summary("T-1", "a"), summary("T-2", "b"), summary("T-3", "c")]);
    assert_eq!(ids(&two.live()), ["T-1", "T-2", "T-3"], "both read the one collection");

    let other = cache.acquire(&spec(json!({ "parent": "T-1", "summary": true })));
    assert_eq!(wire.opened_of("tickets.list").len(), 2, "other params: another subscription");
    assert_eq!(cache.kept(), 2);
    drop(other);

    let first_watch = wire.opened_of("tickets.watch")[0].id;
    drop(one);
    drop(two);
    clock.advance(4_000);
    cache.sweep();
    assert!(!wire.closed().contains(&first_watch), "within 5 s of its last reader: kept");
    let three = cache.acquire(&spec(params.clone()));
    assert_eq!(wire.opened_of("tickets.list").len(), 2, "reused: no snapshot asked");
    assert_eq!(ids(&three.live()), ["T-1", "T-2", "T-3"], "the kept rows at once");
    assert_eq!(three.live().status, Status::Live);

    drop(three);
    clock.advance(5_001);
    cache.sweep();
    assert!(wire.closed().contains(&first_watch), "past 5 s: its changes stream closed");
    assert_eq!(cache.kept(), 0, "the other params' subscription went too");
    let four = cache.acquire(&spec(params));
    assert_eq!(wire.opened_of("tickets.list").len(), 3, "a new reader: a new snapshot");
    assert_eq!(wire.opened_of("tickets.watch").len(), 3, "and a new changes stream");
    assert_eq!(four.live().status, Status::Pending);
}
