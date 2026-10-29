//! UI-9: sections over the socket — `ui.slots.list|set|clear` and
//! `ui.watch` on a spawned `menger` under the fixture operator (`--admin`)
//! with its own ui file and the test signer. Each bundle's manifest (the
//! push's param and the bundle's `manifest.json`, the contract's JSON)
//! declares its components and its `slots` (`support::slotted_bundle`).
//! A watch opens with `live` for the pointer as it stands: the tests wait
//! on that item, not on a clock. Servers on ports the OS hands out, their
//! files under /tmp/claude-1000.

mod support;

use jsonrpsee::core::client::Subscription;
use jsonrpsee::ws_client::WsClient;
use serde_json::{json, Value};
use std::time::{Duration, Instant};
use support::{call, connect, connect_as, default_slots, free_port, next_item, sha_hex, slotted_bundle, slotted_push_params, spawn_menger, subscribe, ui_args, LAYOUT, SLOT};

/// A key that holds editor on `*` and is no admin.
const EDITOR: [u8; 32] = [0xed; 32];

/// v1: the layout and the default that fills its section.
const V1: &[&str] = &["board-layout", "ticket-list"];
/// v2: v1 and one more component that may fill it.
const V2: &[&str] = &["board-layout", "ticket-list", "ticket-grid"];

/// How long "nothing more" is waited for.
const QUIET: Duration = Duration::from_millis(400);

struct Server {
    s: support::Spawned,
    shell: Option<u16>,
    _dir: tempfile::TempDir,
}

async fn start_with(shell: bool) -> Server {
    let root = std::path::Path::new("/tmp/claude-1000");
    std::fs::create_dir_all(root).unwrap();
    let dir = tempfile::Builder::new().prefix("menger-ui-slots-").tempdir_in(root).unwrap();
    let shell_port = if shell { Some(free_port().await) } else { None };
    let mut extra = match shell_port {
        Some(p) => {
            let empty = dir.path().join("no-dist");
            std::fs::create_dir_all(&empty).unwrap();
            vec!["--shell-port".to_string(), p.to_string(), "--app-dist".into(), empty.display().to_string()]
        }
        None => vec!["--no-shell".to_string()],
    };
    extra.extend(ui_args(&dir.path().join("ui.db")));
    let s = spawn_menger(free_port().await, &dir.path().join("t.db"), &extra).await;
    if let Some(p) = shell_port {
        for _ in 0..100 {
            if tokio::net::TcpStream::connect(("127.0.0.1", p)).await.is_ok() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }
    Server { s, shell: shell_port, _dir: dir }
}

async fn start() -> Server {
    start_with(false).await
}

/// Pushes version `n` of `components` with `slots`; its hash.
async fn push_slots(c: &WsClient, n: u32, components: &[&str], slots: &Value) -> String {
    let pushed = call(c, "ui.push", slotted_push_params(n, components, slots)).await;
    assert_eq!(pushed.first().and_then(|e| e["type"].as_str()), Some("pushed"), "{pushed:?}");
    let hash = sha_hex(&slotted_bundle(n, components, slots));
    assert_eq!(pushed[0]["version"], hash.as_str(), "{pushed:?}");
    hash
}

async fn push(c: &WsClient, n: u32, components: &[&str]) -> String {
    push_slots(c, n, components, &default_slots()).await
}

/// `ui.watch` opened, its first item — `live` for the pointer as it stands
/// — read and checked against `live`: from here on, it hears every change.
async fn open_watch(c: &WsClient, live: Option<&str>) -> Subscription<Value> {
    let mut watch = subscribe(c, "ui.watch", json!({})).await;
    let first = next_item(&mut watch, Duration::from_secs(5)).await.expect("the watch opens with live");
    assert_eq!(first["type"], "live", "the first item: {first}");
    assert_eq!(first["version"].as_str(), live, "the pointer as it stands: {first}");
    watch
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

async fn grant_editor(admin: &WsClient) {
    let granted = call(admin, "auth.grant", json!({ "principal": support::principal_of(&EDITOR), "role": "editor", "scope": "*" })).await;
    assert!(!granted.is_empty() && granted[0].get("error_kind").is_none(), "editor on *: {granted:?}");
}

#[tokio::test]
async fn an_admin_sets_a_section_and_the_watch_says_so() {
    let m = start().await;
    let admin = connect(m.s.port).await;
    let v2 = push(&admin, 2, V2).await;
    let watcher = connect(m.s.port).await;
    let mut watch = open_watch(&watcher, Some(&v2)).await;

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

    // A component the version lacks: refused in band, naming it; nothing written or said.
    let missing = call(&admin, "ui.slots.set", json!({ "layout": LAYOUT, "slot": SLOT, "component": "ticket-board" })).await;
    assert_eq!(missing.len(), 1, "{missing:?}");
    assert_eq!((missing[0]["type"].as_str(), missing[0]["reason"].as_str()), (Some("refused"), Some("unknown_component")), "{missing:?}");
    assert!(missing[0]["message"].as_str().is_some_and(|m| m.contains("ticket-board")), "it names it: {missing:?}");
    assert_eq!(assignments(&admin, json!({})).await, listed, "nothing written");

    // A clear: the watch says the section's default.
    let cleared = call(&admin, "ui.slots.clear", json!({ "layout": LAYOUT, "slot": SLOT })).await;
    assert_eq!(cleared.len(), 1, "{cleared:?}");
    assert_eq!(cleared[0]["type"], "cleared", "{cleared:?}");
    assert_eq!(section(&cleared[0]), [Some(LAYOUT), Some(SLOT), Some("ticket-list"), Some(v2.as_str())], "{cleared:?}");
    let said = next_item(&mut watch, Duration::from_secs(2)).await.expect("the watch says the clear");
    assert_eq!(said["type"], "slot_changed", "{said}");
    assert_eq!(section(&said), [Some(LAYOUT), Some(SLOT), Some("ticket-list"), Some(v2.as_str())], "the manifest's default: {said}");
    assert!(assignments(&admin, json!({})).await.is_empty(), "the row removed");

    // A clear on a section with no row: the default answered, nothing said.
    let again = call(&admin, "ui.slots.clear", json!({ "layout": LAYOUT, "slot": SLOT })).await;
    assert_eq!(again, cleared, "cleared, with the default");
    assert!(next_item(&mut watch, QUIET).await.is_none(), "nothing on the watch");
    assert!(assignments(&admin, json!({})).await.is_empty(), "nothing written");
}

#[tokio::test]
async fn a_non_admin_set_is_refused_by_the_gate() {
    let m = start().await;
    let admin = connect(m.s.port).await;
    let v2 = push(&admin, 2, V2).await;
    let set = call(&admin, "ui.slots.set", json!({ "layout": LAYOUT, "slot": SLOT, "component": "ticket-grid", "version": v2 })).await;
    assert_eq!(set.first().and_then(|e| e["type"].as_str()), Some("set"), "{set:?}");
    let before = assignments(&admin, json!({})).await;
    assert_eq!(before.len(), 1, "{before:?}");

    grant_editor(&admin).await;
    let editor = connect_as(&EDITOR, m.s.port).await;
    forbidden(&call(&editor, "ui.slots.set", json!({ "layout": LAYOUT, "slot": SLOT, "component": "ticket-list" })).await, "ui.slots.set");
    forbidden(&call(&editor, "ui.slots.clear", json!({ "layout": LAYOUT, "slot": SLOT })).await, "ui.slots.clear");
    assert_eq!(assignments(&admin, json!({})).await, before, "list unchanged");
    assert_eq!(assignments(&editor, json!({})).await, before, "and list is the editor's to read");
}

#[tokio::test]
async fn the_watch_says_live_on_a_push_and_a_rollback() {
    let m = start().await;
    let admin = connect(m.s.port).await;
    let watcher = connect(m.s.port).await;
    // Nothing pushed yet: the watch opens with live { version: null }.
    let mut watch = open_watch(&watcher, None).await;
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
    eprintln!("UI-9 acceptance 2: live came {lag:?} after pushed");
    assert!(lag <= Duration::from_millis(100), "live came {lag:?} after pushed");
    assert!(tokio::time::timeout(QUIET, rx.recv()).await.is_err(), "and nothing more");
}

#[tokio::test]
async fn a_rollback_keeps_an_assignment_to_a_kept_version() {
    let m = start_with(true).await;
    let admin = connect(m.s.port).await;
    let v1 = push(&admin, 1, V1).await;
    let v2 = push(&admin, 2, V2).await;
    let set = call(&admin, "ui.slots.set", json!({ "layout": LAYOUT, "slot": SLOT, "component": "ticket-grid", "version": v2 })).await;
    assert_eq!(set.first().and_then(|e| e["type"].as_str()), Some("set"), "{set:?}");
    let watcher = connect(m.s.port).await;
    let mut watch = open_watch(&watcher, Some(&v2)).await;

    // v1 holds no ticket-grid: the assignment names v2's hash, which is kept.
    let rolled = call(&admin, "ui.rollback", json!({ "to": v1 })).await;
    assert_eq!((rolled.first().and_then(|e| e["type"].as_str()), rolled.first().and_then(|e| e["version"].as_str())), (Some("live"), Some(v1.as_str())), "{rolled:?}");
    let said = next_item(&mut watch, Duration::from_secs(2)).await.expect("the rollback is said");
    assert_eq!((said["type"].as_str(), said["version"].as_str()), (Some("live"), Some(v1.as_str())), "{said}");
    assert!(next_item(&mut watch, QUIET).await.is_none(), "live alone: no slot_changed on a rollback");

    let listed = assignments(&admin, json!({})).await;
    assert_eq!(listed.len(), 1, "the assignment stays: {listed:?}");
    assert_eq!(section(&listed[0]), [Some(LAYOUT), Some(SLOT), Some("ticket-grid"), Some(v2.as_str())], "still names v2's hash: {listed:?}");
    assert_eq!(listed[0]["default"], "ticket-list", "the default v1 names: {listed:?}");

    // And it still draws: v2's component is served at its hashed path.
    let shell = m.shell.unwrap();
    let token = support::mint_session(m.s.port).await;
    let cookie = format!("{}={token}", menger::login_names().session_cookie_for(&format!("http://127.0.0.1:{shell}")));
    let http = reqwest::Client::builder().redirect(reqwest::redirect::Policy::none()).build().unwrap();
    let file = http.get(format!("http://127.0.0.1:{shell}/app/{v2}/components/ticket-grid.json")).header("cookie", cookie).send().await.unwrap();
    assert_eq!(file.status(), 200, "v2's ticket-grid is served");
    assert_eq!(file.text().await.unwrap(), support::component_json(2, "ticket-grid"), "as pushed");
}

#[tokio::test]
async fn a_watch_is_held_to_its_caller() {
    let m = start().await;
    let admin = connect(m.s.port).await;
    let v2 = push(&admin, 2, V2).await;
    grant_editor(&admin).await;
    let editor = connect_as(&EDITOR, m.s.port).await;
    let mut watch = open_watch(&editor, Some(&v2)).await;

    call(&admin, "ui.slots.set", json!({ "layout": LAYOUT, "slot": SLOT, "component": "ticket-grid" })).await;
    let said = next_item(&mut watch, Duration::from_secs(2)).await.expect("the editor hears the set");
    assert_eq!(said["type"], "slot_changed", "{said}");

    let revoked = call(&admin, "auth.revoke", json!({ "principal": support::principal_of(&EDITOR), "role": "editor", "scope": "*" })).await;
    assert!(!revoked.is_empty() && revoked[0].get("error_kind").is_none(), "revoked: {revoked:?}");
    let closed = next_item(&mut watch, Duration::from_secs(5)).await.expect("the watch ends in band");
    assert_eq!((closed["type"].as_str(), closed["reason"].as_str()), (Some("watch_closed"), Some("revoked")), "{closed}");
    assert!(next_item(&mut watch, Duration::from_secs(2)).await.is_none(), "and then the end");
}
