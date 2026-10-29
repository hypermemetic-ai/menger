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
        Some("tickets.watch"),
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
        Some("tickets.watch"),
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

// ── UI-10: the second cut ─────────────────────────────────────────────────

fn query(changes: Option<&str>, debounce_ms: Option<u64>) -> LiveQuery {
    LiveQuery {
        snapshot: "tickets.list".into(),
        changes: changes.map(str::to_string),
        key: "id".into(),
        on: vec![("created".into(), "upsert:ticket".into())],
        debounce_ms,
    }
}

/// A `live` without `changes` is a one-shot read: the snapshot alone, no
/// watch opened; `pending → live`.
#[test]
fn a_live_without_changes_opens_no_watch() {
    let (wire, _clock, cache) = setup();
    let spec = LiveSpec::new("tickets.list", json!({ "limit": 5 }), None, json!({}), "id", &[]).expect("a spec");
    let reader = cache.acquire(&spec);
    assert_eq!(wire.opened_of("tickets.list").len(), 1, "one snapshot call");
    assert!(wire.opened_of("tickets.watch").is_empty(), "no watch");
    assert_eq!(wire.opened().len(), 1, "nothing else opened");
    assert_eq!(reader.live().status, Status::Pending);
    wire.answer(wire.last_of("tickets.list"), [summary("T-1", "a")]);
    assert_eq!(reader.live().status, Status::Live);
    assert_eq!(ids(&reader.live()), ["T-1"]);

    // Followed the same way: one snapshot, no watch.
    let rt = Runtime::new();
    let (wire, clock) = (FakeWire::new(), ManualClock::new());
    let cache = LiveCache::new(&rt, wire.clone(), clock);
    let follow = cache.follow(query(None, None), || (json!({ "limit": 5 }), json!({})));
    assert_eq!(wire.opened_of("tickets.list").len(), 1);
    assert!(wire.opened_of("tickets.watch").is_empty(), "a followed live without changes opens no watch either");
    wire.answer(wire.last_of("tickets.list"), [summary("T-1", "a")]);
    assert_eq!(follow.live().status, Status::Live);
}

/// A param's signal changes: the snapshot runs again, once, with the new
/// params (the changes stream reopened with the new changes params); the
/// old rows stay, `resyncing`, until the new snapshot lands; the follow's
/// signal is the same one throughout.
#[test]
fn a_live_re_runs_when_a_param_signal_changes() {
    let rt = Runtime::new();
    let (wire, clock) = (FakeWire::new(), ManualClock::new());
    let cache = LiveCache::new(&rt, wire.clone(), clock);
    let parent = rt.signal("T-1".to_string());
    let p = parent.clone();
    let follow = cache.follow(query(Some("tickets.watch"), None), move || (json!({ "parent": p.get() }), json!({ "parent": p.get() })));
    let signal = follow.signal();
    assert_eq!(wire.opened_of("tickets.list")[0].params, json!({ "parent": "T-1" }));
    assert_eq!(wire.opened_of("tickets.watch")[0].params, json!({ "parent": "T-1" }));
    wire.answer(wire.last_of("tickets.list"), [summary("T-2", "a")]);
    assert_eq!(ids(&follow.live()), ["T-2"]);

    parent.set("T-9".into());
    assert_eq!(wire.opened_of("tickets.list").len(), 2, "the snapshot ran again, once");
    assert_eq!(wire.opened_of("tickets.list")[1].params, json!({ "parent": "T-9" }), "with the new params");
    assert_eq!(wire.opened_of("tickets.watch").last().map(|o| o.params.clone()), Some(json!({ "parent": "T-9" })), "the changes reopened with the new changes params");
    let mid = follow.live();
    assert_eq!((ids(&mid), mid.status), (vec!["T-2".to_string()], Status::Resyncing), "the old rows kept, resyncing");
    wire.answer(wire.last_of("tickets.list"), [summary("T-10", "b"), summary("T-11", "c")]);
    let now = follow.live();
    assert_eq!((ids(&now), now.status), (vec!["T-10".to_string(), "T-11".to_string()], Status::Live), "the new snapshot's rows");
    assert_eq!(Live::from_value(&signal.peek()).map(|l| l.rows.len()), Some(2), "the same signal carries them");

    parent.set("T-9".into());
    assert_eq!(wire.opened_of("tickets.list").len(), 2, "the same value: nothing ran");
    drop(follow);
}

/// With `debounce_ms: 80`, two changes 30 ms apart run the snapshot once,
/// 80 ms after the second, with the last params.
#[test]
fn a_live_re_runs_when_a_param_signal_changes_and_waits_debounce_ms() {
    let rt = Runtime::new();
    let (wire, clock) = (FakeWire::new(), ManualClock::new());
    let cache = LiveCache::new(&rt, wire.clone(), clock.clone());
    let q = rt.signal("a".to_string());
    let p = q.clone();
    let follow = cache.follow(query(None, Some(80)), move || (json!({ "query": p.get() }), json!({})));
    assert_eq!(wire.opened_of("tickets.list").len(), 1, "the first snapshot at once");
    wire.answer(wire.last_of("tickets.list"), [summary("T-1", "a")]);

    q.set("ab".into());
    clock.advance(30);
    q.set("abc".into());
    clock.advance(79);
    assert_eq!(wire.opened_of("tickets.list").len(), 1, "79 ms after the second change: not yet");
    assert_eq!(follow.live().status, Status::Live, "nothing asked yet: the rows as they are");
    clock.advance(1);
    assert_eq!(wire.opened_of("tickets.list").len(), 2, "80 ms after the second change: once");
    assert_eq!(wire.opened_of("tickets.list")[1].params, json!({ "query": "abc" }), "with the last params");
    clock.advance(500);
    assert_eq!(wire.opened_of("tickets.list").len(), 2, "and only once");
}

/// `upsert:.` takes the event itself, less `type`, as the row: `extent`
/// events, from the snapshot and the changes, keyed by `epic`.
#[test]
fn a_flat_event_is_its_own_row() {
    let (wire, _clock, cache) = setup();
    let spec = LiveSpec::new("tickets.nav.extents", json!({}), Some("tickets.watch"), json!({}), "epic", &[("extent", "upsert:.")]).expect("a spec");
    let reader = cache.acquire(&spec);
    wire.answer(
        wire.last_of("tickets.nav.extents"),
        [
            json!({ "type": "extent", "epic": "KILN", "first": 1, "last": 9 }),
            json!({ "type": "extent", "epic": "GLAZE", "first": 1, "last": 4 }),
        ],
    );
    let live = reader.live();
    assert_eq!(live.rows, vec![json!({ "epic": "KILN", "first": 1, "last": 9 }), json!({ "epic": "GLAZE", "first": 1, "last": 4 })], "the events, less type");
    wire.feed(wire.last_of("tickets.watch"), WireItem::Data(json!({ "type": "extent", "epic": "KILN", "first": 1, "last": 12 })));
    wire.feed(wire.last_of("tickets.watch"), WireItem::Data(json!({ "type": "extent", "epic": "SLIP", "first": 1, "last": 1 })));
    let live = reader.live();
    let epics: Vec<&str> = live.rows.iter().map(|r| r["epic"].as_str().unwrap()).collect();
    assert_eq!(epics, ["KILN", "GLAZE", "SLIP"], "replaced by epic in place; a new one appended");
    assert_eq!(live.rows[0]["last"], 12);
    assert!(live.rows.iter().all(|r| r.get("type").is_none()), "no row keeps its type");
}
