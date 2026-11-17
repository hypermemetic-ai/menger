//! LOGS-2: the logs service over a stand-in for the tickets service (a
//! [`Reach`] whose tickets, grants and moves are set here) and an allowance
//! that records — one test per scenario of LOGS-2's requirements, and the
//! refusals around them. `tests/wire_logs.rs` (menger's) is the same over a
//! real server and the real tickets service.

use async_trait::async_trait;
use menger_logs::allowance::{Allowance, Denied};
use menger_logs::{Answer, Caller, LogsEvent, LogsService, LogsStore, Reach, Seen};
use menger_tickets_api::members::Role;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

const ADMIN: &str = "key:admin";
const EDITOR: &str = "key:editor";
const VIEWER: &str = "key:viewer";
const OUTSIDER: &str = "key:outsider";
/// The ticket's uid, and the ticket it is tied to.
const UID: &str = "7k3mq2xa";
const OTHER_UID: &str = "9qxd1gn8";

const PERF: &str = r#"{
  "type": "object",
  "required": ["app", "scenario", "metric", "value", "unit", "commit"],
  "properties": {
    "app":       { "type": "string", "description": "what was measured: plexus, plexus-proto, menger" },
    "scenario":  { "type": "string" },
    "transport": { "type": "string", "enum": ["memory", "websocket", "stdio"] },
    "metric":    { "type": "string" },
    "value":     { "type": "number" },
    "unit":      { "type": "string" },
    "commit":    { "type": "string", "description": "repo@hash" },
    "machine":   { "type": "string" },
    "load":      { "type": "number" },
    "config":    { "type": "object" }
  }
}"#;

/// The tickets service as this test sets it: each ticket's id now by uid,
/// and each principal's role on each uid.
#[derive(Debug, Default)]
struct Tickets {
    ids: Mutex<HashMap<String, String>>,
    roles: Mutex<HashMap<(String, String), Role>>,
}

impl Tickets {
    fn new() -> Arc<Self> {
        let t = Arc::new(Self::default());
        t.ticket(UID, "LAYERS-9");
        t.ticket(OTHER_UID, "LAYERS-23");
        for uid in [UID, OTHER_UID] {
            t.grant(ADMIN, uid, Role::Admin);
            t.grant(EDITOR, uid, Role::Editor);
            t.grant(VIEWER, uid, Role::Viewer);
        }
        t
    }
    fn ticket(&self, uid: &str, id: &str) {
        self.ids.lock().unwrap().insert(uid.into(), id.into());
    }
    fn grant(&self, who: &str, uid: &str, role: Role) {
        self.roles.lock().unwrap().insert((who.into(), uid.into()), role);
    }
    /// The uid `name` (an id or a uid) names, if the ticket is there.
    fn uid_of(&self, name: &str) -> Option<String> {
        let ids = self.ids.lock().unwrap();
        ids.iter().find(|(u, i)| *u == name || *i == name).map(|(u, _)| u.clone())
    }
}

fn rank(r: Role) -> u8 {
    match r {
        Role::Chatter => 0,
        Role::Viewer => 1,
        Role::Deployer => 2,
        Role::Editor => 3,
        Role::Admin => 4,
    }
}

#[async_trait]
impl Reach for Tickets {
    async fn seen(&self, caller: &Caller, ticket: &str) -> Result<Option<Seen>, String> {
        let Some(uid) = self.uid_of(ticket) else { return Ok(None) };
        match self.roles.lock().unwrap().get(&(caller.principal().to_string(), uid.clone())) {
            Some(_) => Ok(Some(Seen { id: self.ids.lock().unwrap()[&uid].clone(), uid })),
            None => Ok(None),
        }
    }
    async fn may(&self, caller: &Caller, ticket: &str, role: Role) -> Result<Answer, String> {
        let Some(uid) = self.uid_of(ticket) else { return Ok(Answer::NotFound) };
        let held = self.roles.lock().unwrap().get(&(caller.principal().to_string(), uid.clone())).copied();
        Ok(match held {
            None => Answer::NotFound,
            Some(h) if rank(h) >= rank(role) => Answer::Held(self.ids.lock().unwrap()[&uid].clone()),
            Some(_) => Answer::NotHeld,
        })
    }
}

/// An allowance that records every hold, and refuses past `cap` bytes.
#[derive(Debug, Default)]
struct Meter {
    cap: Option<u64>,
    held: Mutex<Vec<(String, u64, &'static str)>>,
}

#[async_trait]
impl Allowance for Meter {
    async fn reserve(&self, principal: &str, amount: u64) -> Result<String, Denied> {
        if self.cap.is_some_and(|c| amount > c) {
            return Err(Denied { reason: "over_allowance".into(), message: format!("over_allowance: {amount} bytes asked") });
        }
        let mut h = self.held.lock().unwrap();
        h.push((principal.to_string(), amount, "held"));
        Ok(format!("{}", h.len() - 1))
    }
    async fn commit(&self, id: &str, _: u64) {
        self.held.lock().unwrap()[id.parse::<usize>().unwrap()].2 = "committed";
    }
    async fn release(&self, id: &str) {
        self.held.lock().unwrap()[id.parse::<usize>().unwrap()].2 = "released";
    }
}

struct World {
    _dir: tempfile::TempDir,
    logs: LogsService,
    tickets: Arc<Tickets>,
    meter: Arc<Meter>,
}

fn world_with(meter: Meter) -> World {
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(LogsStore::open(&dir.path().join("logs.db")).unwrap());
    let tickets = Tickets::new();
    let meter = Arc::new(meter);
    let logs = LogsService::new(store, tickets.clone()).with_allowance(meter.clone()).with_clock(Arc::new(|| 1_794_872_000_000));
    World { _dir: dir, logs, tickets, meter }
}

fn world() -> World {
    world_with(Meter::default())
}

fn me(p: &str) -> Caller {
    Caller::of(p)
}

fn result(app: &str, value: Value) -> Value {
    json!({"app": app, "scenario": "stream.10k.64B", "transport": "websocket", "metric": "items_per_s", "value": value, "unit": "1/s", "commit": "plexus@abc1234", "machine": "box", "config": {"window": 64}})
}

async fn defined(w: &World) {
    let d = w.logs.define(&me(ADMIN), "perf", "LAYERS-9", PERF).await;
    assert!(matches!(d, LogsEvent::Defined { .. }), "{d:?}");
}

/// Scenario "the perf log": defined { log, columns }, and describe answers
/// the schema and the columns.
#[tokio::test]
async fn the_perf_log_is_defined_and_described() {
    let w = world();
    let LogsEvent::Defined { log, ticket, uid, columns } = w.logs.define(&me(ADMIN), "perf", "LAYERS-9", PERF).await else { panic!() };
    assert_eq!((log.as_str(), ticket.as_str(), uid.as_str()), ("perf", "LAYERS-9", UID));
    let names: Vec<(&str, &str, bool)> = columns.iter().map(|c| (c.name.as_str(), c.column.as_str(), c.envelope)).collect();
    assert_eq!(
        names,
        [
            ("seq", "integer", true), ("at", "integer", true), ("by", "text", true), ("tags", "json", true), ("ticket", "text", true),
            ("app", "text", false), ("scenario", "text", false), ("transport", "text", false), ("metric", "text", false), ("value", "real", false),
            ("unit", "text", false), ("commit", "text", false), ("machine", "text", false), ("load", "real", false), ("config", "json", false),
        ]
    );
    let LogsEvent::Described { schema, columns: c2, entries, created_by, deprecated, .. } = w.logs.describe(&me(VIEWER), "perf").await else { panic!() };
    assert_eq!(schema, serde_json::from_str::<Value>(PERF).unwrap());
    assert_eq!((c2, entries, created_by.as_str(), deprecated), (columns, 0, ADMIN, None));
}

/// Scenario "a schema it cannot hold": refused naming the place; nothing made.
#[tokio::test]
async fn a_schema_it_cannot_hold_makes_nothing() {
    let w = world();
    for (schema, place) in [(r#"{"oneOf": []}"#, "/oneOf"), (r##"{"type": "object", "properties": {"a": {"$ref": "#/x"}}}"##, "/properties/a/$ref"), (r#"{"type": "string"}"#, "/type")] {
        match w.logs.define(&me(ADMIN), "perf", "LAYERS-9", schema).await {
            LogsEvent::Refused { reason, place: Some(p), message, .. } => {
                assert_eq!((reason.as_str(), p.as_str()), ("bad_schema", place), "{message}");
                assert!(message.contains("nothing was made"), "{message}");
            }
            other => panic!("{schema}: {other:?}"),
        }
    }
    assert_eq!(w.logs.store().count(), 0);
    assert_eq!(w.logs.describe(&me(ADMIN), "perf").await, LogsEvent::no_log("perf"));
}

/// Scenario "the name is taken".
#[tokio::test]
async fn a_name_is_one_logs() {
    let w = world();
    defined(&w).await;
    match w.logs.define(&me(ADMIN), "perf", "LAYERS-23", PERF).await {
        LogsEvent::Refused { reason, message, .. } => assert_eq!((reason.as_str(), message.as_str()), ("name_taken", "the log `perf` exists: a log's name is its own")),
        other => panic!("{other:?}"),
    }
}

/// Scenario "a perf result": appended { log, seq }, and the entry is kept
/// with its time, its writer, its tags and its ticket (by uid); its bytes
/// held and committed for the writer.
#[tokio::test]
async fn a_perf_result_is_kept_with_its_envelope() {
    let w = world();
    defined(&w).await;
    let a = w.logs.append(&me(EDITOR), "perf", vec![result("plexus", json!(812_000.5))], vec!["plexus".into(), "websocket".into(), "plexus".into()], Some("LAYERS-23")).await;
    let LogsEvent::Appended { log, seq, last, count, bytes } = a else { panic!("{a:?}") };
    assert_eq!((log.as_str(), seq, last, count), ("perf", 1, 1, 1));
    let rows = w.logs.store().entries("perf", None, 10).unwrap();
    assert_eq!((rows[0].at, rows[0].by.as_str(), rows[0].tags.clone(), rows[0].ticket.as_deref()), (1_794_872_000_000, EDITOR, vec!["plexus".to_string(), "websocket".into()], Some(OTHER_UID)));
    assert_eq!(Value::Object(rows[0].entry.clone()), result("plexus", json!(812_000.5)));
    assert_eq!(*w.meter.held.lock().unwrap(), vec![(EDITOR.to_string(), bytes, "committed")]);
    // A second call numbers on.
    let LogsEvent::Appended { seq, .. } = w.logs.append(&me(EDITOR), "perf", vec![result("menger", json!(1))], vec![], None).await else { panic!() };
    assert_eq!(seq, 2);
    // A ticket the writer cannot see is not found, and nothing kept.
    w.tickets.ticket("zzzzzzzz", "SECRET-1");
    assert_eq!(w.logs.append(&me(EDITOR), "perf", vec![result("x", json!(1))], vec![], Some("SECRET-1")).await, LogsEvent::no_ticket("SECRET-1"));
    assert_eq!(w.logs.store().entries("perf", None, 10).unwrap().len(), 2);
}

/// Scenario "many at once": twenty, the ninth bad — none kept, the refusal
/// names the ninth (index 8) and its field; nothing held.
#[tokio::test]
async fn one_bad_entry_of_many_keeps_none() {
    let w = world();
    defined(&w).await;
    let mut many: Vec<Value> = (0..20).map(|i| result("plexus", json!(i))).collect();
    many[8]["load"] = json!("high");
    let lines = many.iter().map(Value::to_string).collect::<Vec<_>>().join("\n");
    let entries = menger_logs::service::entries_of(None, Some(&lines)).unwrap();
    match w.logs.append(&me(EDITOR), "perf", entries, vec![], None).await {
        LogsEvent::Refused { reason, index, field, message, .. } => {
            assert_eq!((reason.as_str(), index, field.as_deref()), ("bad_entry", Some(8), Some("load")));
            assert!(message.starts_with("entry 8: `load` is number, not string") && message.contains("nothing of this call was kept"), "{message}");
        }
        other => panic!("{other:?}"),
    }
    assert!(w.logs.store().entries("perf", None, 50).unwrap().is_empty());
    assert!(w.meter.held.lock().unwrap().is_empty(), "checked before anything is held");
    // A line that is not JSON is named by its index too.
    let e = menger_logs::service::entries_of(None, Some("{\"app\": 1}\nnope")).unwrap_err();
    assert!(matches!(e, LogsEvent::Refused { index: Some(1), .. }), "{e:?}");
    // And an array is many entries.
    assert_eq!(menger_logs::service::entries_of(None, Some("[{}, {}]")).unwrap().len(), 2);
}

/// Scenario "a field of the wrong type": `value` a string where the schema says number.
#[tokio::test]
async fn a_field_of_the_wrong_type_is_named() {
    let w = world();
    defined(&w).await;
    match w.logs.append(&me(EDITOR), "perf", vec![result("plexus", json!("fast"))], vec![], None).await {
        LogsEvent::Refused { reason, field, message, .. } => {
            assert_eq!((reason.as_str(), field.as_deref()), ("bad_entry", Some("value")));
            assert!(message.contains("`value` is number, not string"), "{message}");
        }
        other => panic!("{other:?}"),
    }
}

/// Scenarios "a new field" and "a field taken away".
#[tokio::test]
async fn a_log_grows_by_optional_fields_only() {
    let w = world();
    defined(&w).await;
    w.logs.append(&me(EDITOR), "perf", vec![result("plexus", json!(1))], vec![], None).await;
    let mut next: Value = serde_json::from_str(PERF).unwrap();
    next["properties"]["p99"] = json!({"type": "number"});
    assert!(matches!(w.logs.extend(&me(EDITOR), "perf", &next.to_string()).await, LogsEvent::Refused { ref reason, .. } if reason == "forbidden"), "the admin's");
    let LogsEvent::Extended { added, columns, .. } = w.logs.extend(&me(ADMIN), "perf", &next.to_string()).await else { panic!() };
    assert_eq!((added.len(), added[0].name.as_str(), added[0].required, columns.last().unwrap().name.as_str()), (1, "p99", false, "p99"));
    let mut with = result("plexus", json!(2));
    with["p99"] = json!(41.5);
    assert!(matches!(w.logs.append(&me(EDITOR), "perf", vec![with], vec![], None).await, LogsEvent::Appended { seq: 2, .. }));
    let rows = w.logs.store().entries("perf", None, 10).unwrap();
    assert_eq!((rows[0].entry.get("p99"), rows[1].entry.get("p99")), (Some(&json!(41.5)), None), "old entries read it as null");
    for (edit, field) in [("drop", "machine"), ("retype", "load")] {
        let mut s = next.clone();
        match edit {
            "drop" => {
                s["properties"].as_object_mut().unwrap().remove("machine");
            }
            _ => s["properties"]["load"] = json!({"type": "string"}),
        }
        match w.logs.extend(&me(ADMIN), "perf", &s.to_string()).await {
            LogsEvent::Refused { reason, field: f, message, .. } => assert_eq!((reason.as_str(), f.as_deref()), ("not_grown", Some(field)), "{message}"),
            other => panic!("{edit}: {other:?}"),
        }
    }
    let LogsEvent::Described { columns: now, .. } = w.logs.describe(&me(VIEWER), "perf").await else { panic!() };
    assert_eq!(now.len(), 16, "nothing changed by a refusal");
}

/// Scenario "a viewer": append refused `forbidden`; it may read.
#[tokio::test]
async fn a_viewer_reads_and_may_not_append() {
    let w = world();
    defined(&w).await;
    match w.logs.append(&me(VIEWER), "perf", vec![result("plexus", json!(1))], vec![], None).await {
        LogsEvent::Refused { reason, message, .. } => assert_eq!((reason.as_str(), message.as_str()), ("forbidden", "appending to a log is the editor's on LAYERS-9, the log's ticket")),
        other => panic!("{other:?}"),
    }
    assert!(matches!(w.logs.describe(&me(VIEWER), "perf").await, LogsEvent::Described { .. }));
    assert_eq!(w.logs.list(&me(VIEWER)).await.len(), 1);
    assert!(matches!(w.logs.define(&me(EDITOR), "x", "LAYERS-9", PERF).await, LogsEvent::Refused { ref reason, .. } if reason == "forbidden"), "define is the admin's");
}

/// Scenario "outside the reach": a principal who cannot see the ticket is
/// not shown the log, and is answered as if it were none.
#[tokio::test]
async fn an_outsider_sees_no_log() {
    let w = world();
    defined(&w).await;
    assert!(w.logs.list(&me(OUTSIDER)).await.is_empty());
    assert_eq!(w.logs.describe(&me(OUTSIDER), "perf").await, LogsEvent::no_log("perf"));
    assert_eq!(w.logs.append(&me(OUTSIDER), "perf", vec![result("x", json!(1))], vec![], None).await, LogsEvent::no_log("perf"));
    assert_eq!(w.logs.define(&me(OUTSIDER), "mine", "LAYERS-9", PERF).await, LogsEvent::no_ticket("LAYERS-9"));
    assert_eq!(w.logs.deprecate(&me(OUTSIDER), "perf", None, "x").await, LogsEvent::no_log("perf"));
}

/// Scenario "the ticket moves": the log is still tied to it, and describe
/// answers its new id.
#[tokio::test]
async fn a_log_follows_its_ticket() {
    let w = world();
    defined(&w).await;
    w.tickets.ticket(UID, "PERF-1");
    let LogsEvent::Described { ticket, uid, .. } = w.logs.describe(&me(VIEWER), "perf").await else { panic!() };
    assert_eq!((ticket.as_str(), uid.as_str()), ("PERF-1", UID));
    let listed = w.logs.list(&me(VIEWER)).await;
    assert!(matches!(&listed[..], [LogsEvent::Log { ticket, .. }] if ticket == "PERF-1"), "{listed:?}");
}

/// Scenarios "a log", "a field" and "listed" of deprecate.
#[tokio::test]
async fn a_deprecated_log_or_field_takes_nothing_new() {
    let w = world();
    defined(&w).await;
    w.logs.append(&me(EDITOR), "perf", vec![result("plexus", json!(1))], vec![], None).await;
    // A field.
    assert_eq!(w.logs.deprecate(&me(ADMIN), "perf", Some("machine"), "one machine now").await, LogsEvent::Deprecated { log: "perf".into(), field: Some("machine".into()), message: "one machine now".into() });
    match w.logs.append(&me(EDITOR), "perf", vec![result("plexus", json!(2))], vec![], None).await {
        LogsEvent::Refused { reason, field, message, .. } => {
            assert_eq!((reason.as_str(), field.as_deref()), ("bad_entry", Some("machine")));
            assert!(message.contains("`machine` is deprecated: one machine now"), "{message}");
        }
        other => panic!("{other:?}"),
    }
    let mut without = result("plexus", json!(3));
    without.as_object_mut().unwrap().remove("machine");
    assert!(matches!(w.logs.append(&me(EDITOR), "perf", vec![without.clone()], vec![], None).await, LogsEvent::Appended { seq: 2, .. }));
    assert!(matches!(w.logs.deprecate(&me(EDITOR), "perf", None, "x").await, LogsEvent::Refused { ref reason, .. } if reason == "forbidden"), "the admin's");
    assert!(matches!(w.logs.deprecate(&me(ADMIN), "perf", Some("nope"), "x").await, LogsEvent::Refused { ref reason, .. } if reason == "no_field"));
    // The log.
    w.logs.deprecate(&me(ADMIN), "perf", None, "replaced by perf2").await;
    match w.logs.append(&me(EDITOR), "perf", vec![without], vec![], None).await {
        LogsEvent::Refused { reason, message, .. } => {
            assert_eq!(reason, "deprecated");
            assert!(message.contains("the log `perf` is deprecated: replaced by perf2"), "{message}");
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(w.logs.store().entries("perf", None, 10).unwrap().len(), 2, "what was written stays readable");
    // Listed and described as such.
    let LogsEvent::Described { deprecated, columns, .. } = w.logs.describe(&me(VIEWER), "perf").await else { panic!() };
    assert_eq!(deprecated.as_deref(), Some("replaced by perf2"));
    assert_eq!(columns.iter().find(|c| c.name == "machine").unwrap().deprecated.as_deref(), Some("one machine now"));
    let listed = w.logs.list(&me(VIEWER)).await;
    assert!(matches!(&listed[..], [LogsEvent::Log { deprecated: Some(d), deprecated_fields, .. }] if d == "replaced by perf2" && deprecated_fields == &["machine".to_string()]), "{listed:?}");
    // Deprecated twice: said, the first message kept.
    assert!(matches!(w.logs.deprecate(&me(ADMIN), "perf", None, "again").await, LogsEvent::Refused { ref message, .. } if message.contains("replaced by perf2")));
}

/// Every kept byte is the quota service's to allow: past the cap, refused
/// in its words, nothing kept.
#[tokio::test]
async fn an_append_past_the_allowance_keeps_nothing() {
    let w = world_with(Meter { cap: Some(10), ..Default::default() });
    defined(&w).await;
    match w.logs.append(&me(EDITOR), "perf", vec![result("plexus", json!(1))], vec![], None).await {
        LogsEvent::Refused { reason, message, .. } => assert!(reason == "over_allowance" && message.ends_with("(nothing was kept)"), "{message}"),
        other => panic!("{other:?}"),
    }
    assert!(w.logs.store().entries("perf", None, 10).unwrap().is_empty());
}

// ── LOGS-3: read and watch ───────────────────────────────────────────────

use futures::StreamExt;
use menger_logs::ReadArgs;
use std::sync::atomic::{AtomicI64, Ordering};

/// A world whose clock the test moves: (the world, its clock in ms).
fn timed() -> (World, Arc<AtomicI64>) {
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(LogsStore::open(&dir.path().join("logs.db")).unwrap());
    let tickets = Tickets::new();
    let meter = Arc::new(Meter::default());
    let now = Arc::new(AtomicI64::new(1_794_000_000_000));
    let clock = now.clone();
    let logs = LogsService::new(store, tickets.clone()).with_allowance(meter.clone()).with_clock(Arc::new(move || clock.load(Ordering::SeqCst)));
    (World { _dir: dir, logs, tickets, meter }, now)
}

fn read(log: Option<&str>) -> ReadArgs {
    ReadArgs { log: log.map(str::to_string), ..Default::default() }
}

fn entries(events: &[LogsEvent]) -> Vec<(String, u64, Value)> {
    events.iter().filter_map(|e| match e {
        LogsEvent::Entry { log, seq, entry, .. } => Some((log.clone(), *seq, entry.clone())),
        _ => None,
    }).collect()
}

fn page(events: &[LogsEvent]) -> (u64, Option<String>) {
    match events.last() {
        Some(LogsEvent::Page { count, next }) => (*count, next.clone()),
        other => panic!("a page last: {other:?}"),
    }
}

/// Scenario "one scenario over time": the last twenty such results, newest
/// first, filtered on three fields; the page after them by `next`.
#[tokio::test]
async fn one_scenario_over_time() {
    let w = world();
    defined(&w).await;
    for i in 0..60u64 {
        let mut r = result(if i % 3 == 0 { "menger" } else { "plexus" }, json!(i));
        r["transport"] = json!(if i % 2 == 0 { "websocket" } else { "memory" });
        w.logs.append(&me(EDITOR), "perf", vec![r], vec![], None).await;
    }
    let ask = |after: Option<String>| ReadArgs { filter: Some(r#"app = "plexus" and scenario = "stream.10k.64B" and transport = "websocket""#.into()), limit: Some(20), after, ..read(Some("perf")) };
    let first = w.logs.read(&me(VIEWER), ask(None)).await;
    let got = entries(&first);
    // seq i+1 holds value i; plexus is i % 3 != 0, websocket is i even.
    let want: Vec<u64> = (0..60u64).rev().filter(|i| i % 3 != 0 && i % 2 == 0).map(|i| i + 1).collect();
    assert_eq!(got.iter().map(|(_, s, _)| *s).collect::<Vec<_>>(), want[..want.len().min(20)].to_vec(), "newest first");
    assert!(got.iter().all(|(l, _, e)| l == "perf" && e["app"] == "plexus" && e["transport"] == "websocket"));
    assert_eq!(want.len(), 20, "exactly twenty such");
    let (count, next) = page(&first);
    assert_eq!((count, next.as_deref()), (20, Some(want[19].to_string().as_str())));
    assert_eq!(page(&w.logs.read(&me(VIEWER), ask(next)).await), (0, None), "nothing after the twentieth");
    // The envelope too: by, seq.
    let mine = w.logs.read(&me(VIEWER), ReadArgs { by: Some(EDITOR.into()), filter: Some("seq <= 3".into()), ..read(Some("perf")) }).await;
    assert_eq!(entries(&mine).iter().map(|(_, s, _)| *s).collect::<Vec<_>>(), vec![3, 2, 1]);
    assert_eq!(page(&w.logs.read(&me(VIEWER), ReadArgs { by: Some("key:nobody".into()), ..read(Some("perf")) }).await).0, 0);
}

/// Scenario "across logs": every log's entries with the tag within the
/// week, each with its log's name and its entry as JSON, newest first;
/// paged across logs.
#[tokio::test]
async fn across_logs_by_tag_and_time() {
    let (w, now) = timed();
    defined(&w).await;
    let sizes = r#"{"type": "object", "required": ["bytes"], "properties": {"bytes": {"type": "integer"}, "what": {"type": "string"}}}"#;
    assert!(matches!(w.logs.define(&me(ADMIN), "sizes", "LAYERS-23", sizes).await, LogsEvent::Defined { .. }));
    let day = 86_400_000;
    // Ten days ago, tagged: out of the week.
    w.logs.append(&me(EDITOR), "perf", vec![result("plexus", json!(1))], vec!["plexus".into()], None).await;
    now.fetch_add(4 * day, Ordering::SeqCst);
    w.logs.append(&me(EDITOR), "sizes", vec![json!({"bytes": 1024, "what": "engine"})], vec!["plexus".into()], None).await;
    now.fetch_add(day, Ordering::SeqCst);
    w.logs.append(&me(EDITOR), "perf", vec![result("plexus", json!(2)), result("plexus", json!(3))], vec!["plexus".into(), "websocket".into()], None).await;
    w.logs.append(&me(EDITOR), "sizes", vec![json!({"bytes": 2048})], vec!["menger".into()], None).await;
    now.fetch_add(5 * day, Ordering::SeqCst);
    let week = w.logs.read(&me(VIEWER), ReadArgs { tag: Some("plexus".into()), since: Some("7d".into()), ..read(None) }).await;
    let got = entries(&week);
    assert_eq!(got.iter().map(|(l, s, _)| (l.as_str(), *s)).collect::<Vec<_>>(), [("perf", 3), ("perf", 2), ("sizes", 1)], "newest first, the tag's alone, the week's alone");
    assert_eq!(got[2].2, json!({"bytes": 1024, "what": "engine"}), "its entry as JSON");
    assert!(matches!(&week[0], LogsEvent::Entry { tags, by, .. } if tags == &["plexus".to_string(), "websocket".into()] && by == EDITOR));
    // A page at a time, across logs.
    let p1 = w.logs.read(&me(VIEWER), ReadArgs { tag: Some("plexus".into()), limit: Some(2), ..read(None) }).await;
    let (_, next) = page(&p1);
    let p2 = w.logs.read(&me(VIEWER), ReadArgs { tag: Some("plexus".into()), limit: Some(2), after: next, ..read(None) }).await;
    let both: Vec<(String, u64)> = entries(&p1).into_iter().chain(entries(&p2)).map(|(l, s, _)| (l, s)).collect();
    assert_eq!(both, [("perf".into(), 3), ("perf".into(), 2), ("sizes".into(), 1), ("perf".into(), 1)]);
    // Across logs the filter is the envelope's alone.
    match &w.logs.read(&me(VIEWER), ReadArgs { filter: Some(r#"app = "plexus""#.into()), ..read(None) }).await[..] {
        [LogsEvent::Refused { reason, message, .. }] => assert!(reason == "bad_filter" && message.contains("`app` is no field of the envelope"), "{message}"),
        other => panic!("{other:?}"),
    }
    let by_seq = w.logs.read(&me(VIEWER), ReadArgs { filter: Some("seq = 1".into()), ..read(None) }).await;
    assert_eq!(entries(&by_seq).len(), 2, "seq 1 of each log");
    // A ticket, by any name: entries naming it.
    w.logs.append(&me(EDITOR), "sizes", vec![json!({"bytes": 1})], vec![], Some("LAYERS-9")).await;
    let named = w.logs.read(&me(VIEWER), ReadArgs { ticket: Some(UID.into()), ..read(None) }).await;
    assert_eq!(entries(&named).iter().map(|(l, s, _)| (l.as_str(), *s)).collect::<Vec<_>>(), [("sizes", 3)]);
}

/// A viewer may read; an outsider is answered as if there were no log, and
/// sees nothing across logs.
#[tokio::test]
async fn a_viewer_reads_and_an_outsider_sees_nothing() {
    let w = world();
    defined(&w).await;
    w.logs.append(&me(EDITOR), "perf", vec![result("plexus", json!(1))], vec!["plexus".into()], None).await;
    assert_eq!(entries(&w.logs.read(&me(VIEWER), read(Some("perf"))).await).len(), 1);
    assert_eq!(w.logs.read(&me(OUTSIDER), read(Some("perf"))).await, vec![LogsEvent::no_log("perf")]);
    assert_eq!(w.logs.read(&me(OUTSIDER), ReadArgs { tag: Some("plexus".into()), ..read(None) }).await, vec![LogsEvent::Page { count: 0, next: None }]);
    // A deprecated log is still read.
    w.logs.deprecate(&me(ADMIN), "perf", None, "replaced").await;
    assert_eq!(entries(&w.logs.read(&me(VIEWER), read(Some("perf"))).await).len(), 1);
}

/// An unknown field in the filter is refused naming the log's fields;
/// nothing is read.
#[tokio::test]
async fn an_unknown_field_is_named_with_the_fields_there_are() {
    let w = world();
    defined(&w).await;
    match &w.logs.read(&me(VIEWER), ReadArgs { filter: Some(r#"aap = "plexus""#.into()), ..read(Some("perf")) }).await[..] {
        [LogsEvent::Refused { reason, message, .. }] => {
            assert_eq!(reason, "bad_filter");
            assert_eq!(message, "--filter: `aap` is no field of the log `perf` (its fields: seq, at, by, ticket, app, scenario, transport, metric, value, unit, commit, machine, load, config)");
        }
        other => panic!("{other:?}"),
    }
    assert!(matches!(&w.logs.read(&me(VIEWER), ReadArgs { filter: Some("value = \"fast\"".into()), ..read(Some("perf")) }).await[..], [LogsEvent::Refused { message, .. }] if message.contains("`value` is number, compared with a string")));
    assert!(matches!(&w.logs.read(&me(VIEWER), ReadArgs { after: Some("1.2.perf".into()), ..read(Some("perf")) }).await[..], [LogsEvent::Refused { reason, .. }] if reason == "bad_cursor"));
}

/// Scenario "watching": a watcher of the log receives each entry as it is
/// appended; one watching by tag only those carrying it; an outsider's
/// watch of every log is told nothing; a watch of a log it cannot see is
/// not found.
#[tokio::test]
async fn a_watcher_receives_an_append() {
    let w = world();
    defined(&w).await;
    let logs = Arc::new(w.logs);
    let mut perf = Box::pin(logs.clone().watch(me(VIEWER), Some("perf".into()), None));
    assert_eq!(perf.next().await, Some(LogsEvent::Watching { log: Some("perf".into()), tag: None }));
    let mut tagged = Box::pin(logs.clone().watch(me(VIEWER), None, Some("websocket".into())));
    assert!(matches!(tagged.next().await, Some(LogsEvent::Watching { .. })));
    let mut outsider = Box::pin(logs.clone().watch(me(OUTSIDER), None, None));
    assert!(matches!(outsider.next().await, Some(LogsEvent::Watching { .. })));
    let mut no = Box::pin(logs.clone().watch(me(OUTSIDER), Some("perf".into()), None));
    assert_eq!(no.next().await, Some(LogsEvent::no_log("perf")));
    assert_eq!(no.next().await, None, "and ends");

    logs.append(&me(EDITOR), "perf", vec![result("plexus", json!(1))], vec!["plexus".into()], None).await;
    logs.append(&me(EDITOR), "perf", vec![result("plexus", json!(2)), result("plexus", json!(3))], vec!["websocket".into()], None).await;
    let two = std::time::Duration::from_secs(2);
    for want in 1..=3u64 {
        match tokio::time::timeout(two, perf.next()).await.expect("an entry").unwrap() {
            LogsEvent::Entry { log, seq, entry, by, .. } => assert_eq!((log.as_str(), seq, entry["value"].clone(), by.as_str()), ("perf", want, json!(want as f64), EDITOR)),
            other => panic!("{other:?}"),
        }
    }
    for want in 2..=3u64 {
        assert!(matches!(tokio::time::timeout(two, tagged.next()).await.unwrap(), Some(LogsEvent::Entry { seq, .. }) if seq == want));
    }
    assert!(tokio::time::timeout(std::time::Duration::from_millis(300), outsider.next()).await.is_err(), "an outsider is told nothing");
}
