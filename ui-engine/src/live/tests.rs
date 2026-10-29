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
        json!({}),
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
    wire.feed(watch, WireItem::Data(json!({ "type": "created", "ticket": { "id": "T-9", "title": "z" } })));
    wire.feed(watch, WireItem::Data(json!({ "type": "deleted", "ids": "T-9", "tombstones": [] })));
    assert_eq!(ids(&reader.live()), ["T-1", "T-2", "T-4"], "deleted with a single id: removed");
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
    let other_watch = wire.opened_of("tickets.watch")[1].id;
    drop(one);
    drop(two);
    // First leg: 4,999 ms after the last reader went, kept; a reader then
    // reuses it.
    clock.advance(4_999);
    cache.sweep();
    assert!(!wire.closed().contains(&first_watch), "4,999 ms after its last reader: kept");
    assert!(!wire.closed().contains(&other_watch), "the other params' too");
    let three = cache.acquire(&spec(params.clone()));
    assert_eq!(wire.opened_of("tickets.list").len(), 2, "reused: no snapshot asked");
    assert_eq!(ids(&three.live()), ["T-1", "T-2", "T-3"], "the kept rows at once");
    assert_eq!(three.live().status, Status::Live);
    clock.advance(2);
    cache.sweep();
    assert!(wire.closed().contains(&other_watch), "the other params' subscription, 5,001 ms without a reader: closed");
    assert!(!wire.closed().contains(&first_watch), "the one with a reader stays");

    // Second leg: the third reader goes; kept at 4,999 ms, closed at 5,001.
    drop(three);
    clock.advance(4_999);
    cache.sweep();
    assert!(!wire.closed().contains(&first_watch), "4,999 ms after its last reader: kept");
    clock.advance(2);
    cache.sweep();
    assert!(wire.closed().contains(&first_watch), "5,001 ms: its changes stream closed");
    assert_eq!(cache.kept(), 0);
    let four = cache.acquire(&spec(params));
    assert_eq!(wire.opened_of("tickets.list").len(), 3, "a new reader: a new snapshot");
    assert_eq!(wire.opened_of("tickets.watch").len(), 3, "and a new changes stream");
    assert_eq!(four.live().status, Status::Pending);
}

/// A filtered collection keeps only what is its own: with params
/// `{ parent: "T-1", summary, limit }` a created ticket under T-2 is
/// ignored, one under T-1 upserted (`summary` and `limit` name no field of
/// a ticket, so they filter nothing); an amended item that leaves the
/// filter is removed. The changes stream opens with its own params.
#[test]
fn live_keeps_only_what_matches_its_filter() {
    let (wire, _clock, cache) = setup();
    let params = json!({ "parent": "T-1", "summary": true, "limit": 200 });
    let spec = LiveSpec::new(
        "tickets.list",
        params.clone(),
        "tickets.watch",
        json!({ "epic": "KILN" }),
        "id",
        &[("created", "upsert:ticket"), ("amended", "upsert:ticket"), ("deleted", "remove:ids")],
    )
    .expect("a spec");
    let reader = cache.acquire(&spec);
    assert_eq!(wire.opened_of("tickets.watch")[0].params, json!({ "epic": "KILN" }), "the changes stream's own params");
    assert_eq!(wire.opened_of("tickets.list")[0].params, params);
    wire.answer(wire.last_of("tickets.list"), [json!({ "type": "summary", "summary": { "id": "T-2", "parent": "T-1", "title": "a" } })]);

    let watch = wire.last_of("tickets.watch");
    wire.feed(watch, WireItem::Data(json!({ "type": "created", "ticket": { "id": "T-7", "parent": "T-2", "title": "elsewhere" } })));
    assert_eq!(ids(&reader.live()), ["T-2"], "a created ticket under another parent: ignored");
    wire.feed(watch, WireItem::Data(json!({ "type": "created", "ticket": { "id": "T-8", "parent": "T-1", "title": "mine" } })));
    assert_eq!(ids(&reader.live()), ["T-2", "T-8"], "one under T-1: upserted");
    wire.feed(watch, WireItem::Data(json!({ "type": "amended", "ticket": { "id": "T-8", "parent": "T-2", "title": "moved" }, "changed": ["parent"] })));
    assert_eq!(ids(&reader.live()), ["T-2"], "amended out of the filter: removed");
    wire.feed(watch, WireItem::Data(json!({ "type": "amended", "ticket": { "id": "T-7", "parent": "T-2", "title": "still elsewhere" }, "changed": ["title"] })));
    assert_eq!(ids(&reader.live()), ["T-2"], "amended, never its own: nothing");
}
