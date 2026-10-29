//! UI-9: sections over the socket — `ui.slots.list|set|clear` and
//! `ui.watch` on a spawned `menger` under the fixture operator (`--admin`)
//! with its own ui file and the test signer. Each bundle's `manifest.json`
//! declares its components and `slots: { board-layout: { main:
//! "ticket-list" } }` (`support::slotted_bundle`). Servers on ports the OS
//! hands out, their files under /tmp/claude-1000.

mod support;

use jsonrpsee::ws_client::WsClient;
use serde_json::{json, Value};
use std::time::{Duration, Instant};
use support::{call, connect, connect_as, free_port, next_item, sha_hex, slotted_bundle, slotted_push_params, spawn_menger, subscribe, ui_args, LAYOUT, SLOT};

/// A key that holds editor on `*` and is no admin.
const EDITOR: [u8; 32] = [0xed; 32];

/// v1: the layout and the default that fills its section.
const V1: &[&str] = &["board-layout", "ticket-list"];
/// v2: v1 and one more component that may fill it.
const V2: &[&str] = &["board-layout", "ticket-list", "ticket-grid"];

/// Long enough for a watch opened to be subscribed before the write after it.
const SETTLE: Duration = Duration::from_millis(300);

async fn start() -> (support::Spawned, tempfile::TempDir) {
    let root = std::path::Path::new("/tmp/claude-1000");
    std::fs::create_dir_all(root).unwrap();
    let dir = tempfile::Builder::new().prefix("menger-ui-slots-").tempdir_in(root).unwrap();
    let mut extra = vec!["--no-shell".to_string()];
    extra.extend(ui_args(&dir.path().join("ui.db")));
    let s = spawn_menger(free_port().await, &dir.path().join("t.db"), &extra).await;
    (s, dir)
}

/// Pushes version `n` of `components`; its hash.
async fn push(c: &WsClient, n: u32, components: &[&str]) -> String {
    let pushed = call(c, "ui.push", slotted_push_params(n, components)).await;
    assert_eq!(pushed.first().and_then(|e| e["type"].as_str()), Some("pushed"), "{pushed:?}");
    let hash = sha_hex(&slotted_bundle(n, components));
    assert_eq!(pushed[0]["version"], hash.as_str(), "{pushed:?}");
    hash
}

/// The `assignment` events `ui.slots.list` answers; panics on anything else.
async fn assignments(c: &WsClient, params: Value) -> Vec<Value> {
    let evs = call(c, "ui.slots.list", params).await;
    assert!(evs.iter().all(|e| e["type"] == "assignment"), "list answers only assignment events: {evs:?}");
    evs
}

/// The gate's refusal of `grant`: one error item, forbidden, naming it.
fn forbidden(evs: &[Value], grant: &str) {
    assert_eq!(evs.len(), 1, "the gate's refusal alone: {evs:?}");
    let e = &evs[0];
    assert_eq!(e["type"], "error", "the envelope is the gate's error: {e}");
    assert_eq!((e["error_kind"].as_str(), e["grant"].as_str()), (Some("forbidden"), Some(grant)), "{e}");
}

/// An item's four section fields, as text.
fn section(e: &Value) -> [Option<&str>; 4] {
    ["layout", "slot", "component", "version"].map(|k| e[k].as_str())
}

#[tokio::test]
async fn an_admin_sets_a_section_and_the_watch_says_so() {
    let (s, _dir) = start().await;
    let admin = connect(s.port).await;
    let v2 = push(&admin, 2, V2).await;
    let watcher = connect(s.port).await;
    let mut watch = subscribe(&watcher, "ui.watch", json!({})).await;
    tokio::time::sleep(SETTLE).await;

    // No version named: the live one's.
    let set = call(&admin, "ui.slots.set", json!({ "layout": LAYOUT, "slot": SLOT, "component": "ticket-grid" })).await;
    assert_eq!(set.len(), 1, "{set:?}");
    assert_eq!(set[0]["type"], "set", "{set:?}");
    assert_eq!(section(&set[0]), [Some(LAYOUT), Some(SLOT), Some("ticket-grid"), Some(v2.as_str())], "{set:?}");

    let said = next_item(&mut watch, Duration::from_secs(2)).await.expect("the watch says the set");
    assert_eq!(said["type"], "slot_changed", "{said}");
    assert_eq!(section(&said), [Some(LAYOUT), Some(SLOT), Some("ticket-grid"), Some(v2.as_str())], "the four fields: {said}");

    let listed = assignments(&admin, json!({})).await;
    assert_eq!(listed.len(), 1, "{listed:?}");
    assert_eq!(section(&listed[0]), [Some(LAYOUT), Some(SLOT), Some("ticket-grid"), Some(v2.as_str())], "list agrees: {listed:?}");
    assert_eq!(listed[0]["default"], "ticket-list", "with the default it overrides: {listed:?}");
    assert_eq!(assignments(&admin, json!({ "layout": LAYOUT })).await, listed, "and under its layout");

    // A clear: the watch says the section's default.
    let cleared = call(&admin, "ui.slots.clear", json!({ "layout": LAYOUT, "slot": SLOT })).await;
    assert_eq!(cleared.first().and_then(|e| e["type"].as_str()), Some("cleared"), "{cleared:?}");
    let said = next_item(&mut watch, Duration::from_secs(2)).await.expect("the watch says the clear");
    assert_eq!(said["type"], "slot_changed", "{said}");
    assert_eq!(section(&said), [Some(LAYOUT), Some(SLOT), Some("ticket-list"), Some(v2.as_str())], "the manifest's default: {said}");
    assert!(assignments(&admin, json!({})).await.is_empty(), "the row removed");
}

#[tokio::test]
async fn a_non_admin_set_is_refused_by_the_gate() {
    let (s, _dir) = start().await;
    let admin = connect(s.port).await;
    let v2 = push(&admin, 2, V2).await;
    let set = call(&admin, "ui.slots.set", json!({ "layout": LAYOUT, "slot": SLOT, "component": "ticket-grid", "version": v2 })).await;
    assert_eq!(set.first().and_then(|e| e["type"].as_str()), Some("set"), "{set:?}");
    let before = assignments(&admin, json!({})).await;
    assert_eq!(before.len(), 1, "{before:?}");

    let granted = call(&admin, "auth.grant", json!({ "principal": support::principal_of(&EDITOR), "role": "editor", "scope": "*" })).await;
    assert!(!granted.is_empty() && granted[0].get("error_kind").is_none(), "editor on *: {granted:?}");
    let editor = connect_as(&EDITOR, s.port).await;
    forbidden(&call(&editor, "ui.slots.set", json!({ "layout": LAYOUT, "slot": SLOT, "component": "ticket-list" })).await, "ui.slots.set");
    forbidden(&call(&editor, "ui.slots.clear", json!({ "layout": LAYOUT, "slot": SLOT })).await, "ui.slots.clear");
    assert_eq!(assignments(&admin, json!({})).await, before, "list unchanged");
    assert_eq!(assignments(&editor, json!({})).await, before, "and list is the editor's to read");
}

#[tokio::test]
async fn the_watch_says_live_on_a_push_and_a_rollback() {
    let (s, _dir) = start().await;
    let admin = connect(s.port).await;
    let watcher = connect(s.port).await;
    let mut watch = subscribe(&watcher, "ui.watch", json!({})).await;
    // Every item timestamped as it arrives, so a `live` is measured against
    // the `pushed` it follows.
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<(Instant, Value)>();
    tokio::spawn(async move {
        while let Some(item) = next_item(&mut watch, Duration::from_secs(30)).await {
            if tx.send((Instant::now(), item)).is_err() {
                break;
            }
        }
    });
    tokio::time::sleep(SETTLE).await;

    let v1 = push(&admin, 1, V1).await;
    let pushed_at = Instant::now();
    let v2 = push(&admin, 2, V2).await;
    let rolled = call(&admin, "ui.rollback", json!({ "to": v1 })).await;
    assert_eq!(rolled.first().and_then(|e| e["type"].as_str()), Some("live"), "{rolled:?}");

    let mut said = Vec::new();
    for _ in 0..3 {
        let (at, item) = tokio::time::timeout(Duration::from_secs(2), rx.recv()).await.ok().flatten().expect("a live item on the watch");
        said.push((at, item));
    }
    let lives: Vec<(Option<&str>, Option<&str>)> = said.iter().map(|(_, e)| (e["type"].as_str(), e["version"].as_str())).collect();
    assert_eq!(lives, vec![(Some("live"), Some(v1.as_str())), (Some("live"), Some(v2.as_str())), (Some("live"), Some(v1.as_str()))], "three live items, in order");
    // Acceptance 2: within 100 ms of `pushed`.
    let lag = said[0].0.saturating_duration_since(pushed_at);
    assert!(lag <= Duration::from_millis(100), "live came {lag:?} after pushed");
    assert!(tokio::time::timeout(Duration::from_millis(300), rx.recv()).await.is_err(), "and nothing more");
}

#[tokio::test]
async fn a_rollback_keeps_an_assignment_to_a_kept_version() {
    let (s, _dir) = start().await;
    let admin = connect(s.port).await;
    let v1 = push(&admin, 1, V1).await;
    let v2 = push(&admin, 2, V2).await;
    let set = call(&admin, "ui.slots.set", json!({ "layout": LAYOUT, "slot": SLOT, "component": "ticket-grid", "version": v2 })).await;
    assert_eq!(set.first().and_then(|e| e["type"].as_str()), Some("set"), "{set:?}");

    // v1 holds no ticket-grid: the assignment names v2's hash, which is kept.
    let rolled = call(&admin, "ui.rollback", json!({ "to": v1 })).await;
    assert_eq!((rolled.first().and_then(|e| e["type"].as_str()), rolled.first().and_then(|e| e["version"].as_str())), (Some("live"), Some(v1.as_str())), "{rolled:?}");
    let listed = assignments(&admin, json!({})).await;
    assert_eq!(listed.len(), 1, "the assignment stays: {listed:?}");
    assert_eq!(section(&listed[0]), [Some(LAYOUT), Some(SLOT), Some("ticket-grid"), Some(v2.as_str())], "still names v2's hash: {listed:?}");
    assert_eq!(listed[0]["default"], "ticket-list", "the default v1 names: {listed:?}");
}
