//! UI-9: the sections' store tests — the testing surface's first two rows.
//! Each opens a throwaway `ui.db`, pushes bundles whose `manifest.json`
//! declares `components` and `slots` (UI-4), and reads `ui_slots` back
//! through a connection of its own, so what is asserted is what the file
//! holds.

use super::*;
use rusqlite::types::Value;
use tokio::sync::broadcast::error::TryRecvError;

const LAYOUT: &str = "board-layout";
const SLOT: &str = "main";

/// The slots most versions declare: the layout's one section, defaulting to `ticket-list`.
fn default_slots() -> serde_json::Value {
    serde_json::json!({ LAYOUT: { SLOT: "ticket-list" } })
}

/// Version `n`'s manifest (UI-4's JSON): `components` each named, `slots` as given.
fn manifest_json(n: u32, components: &[&str], slots: &serde_json::Value) -> String {
    let components: serde_json::Map<String, serde_json::Value> = components
        .iter()
        .map(|c| (c.to_string(), serde_json::json!({ "tree": format!("{n}-{c}-tree"), "handlers": format!("{n}-{c}-rhai"), "uses": [] })))
        .collect();
    serde_json::json!({
        "abi": "engine/1", "contract": format!("c{n}"), "commit": format!("menger@{n:07x}"),
        "routes": { "#/": LAYOUT },
        "components": components,
        "slots": slots,
    })
    .to_string()
}

/// A bundle holding `index.html`, `manifest.json` and each component's tree.
fn bundle(n: u32, components: &[&str], slots: &serde_json::Value) -> Vec<u8> {
    let mut files = vec![("index.html".to_string(), format!("<!doctype html><title>v{n}</title>")), ("manifest.json".to_string(), manifest_json(n, components, slots))];
    files.extend(components.iter().map(|c| (format!("components/{c}.json"), format!("{{\"name\":\"{c}\",\"v\":{n}}}"))));
    let mut b = tar::Builder::new(Vec::new());
    for (name, body) in files {
        let mut h = tar::Header::new_gnu();
        h.set_size(body.len() as u64);
        h.set_mode(0o644);
        h.set_cksum();
        b.append_data(&mut h, name, body.as_bytes()).unwrap();
    }
    b.into_inner().unwrap()
}

fn version_with(n: u32, components: &[&str], slots: serde_json::Value) -> NewVersion {
    NewVersion {
        bundle: bundle(n, components, &slots),
        // The contract's JSON (UI-4), as the push's param and the bundle hold it.
        manifest: manifest_json(n, components, &slots),
        commit: Some(format!("{n:07x}")),
        pushed_by: "key:admin".into(),
        pushed_at: 100 * n as i64,
    }
}

fn version(n: u32, components: &[&str]) -> NewVersion {
    version_with(n, components, default_slots())
}

/// v1: the layout and two components that may fill its section.
const V1: &[&str] = &["board-layout", "ticket-list", "ticket-grid"];

/// Every row of `ui_slots`, as the file holds it.
fn slot_rows(path: &Path) -> Vec<Vec<Value>> {
    let c = Connection::open(path).unwrap();
    let mut q = c.prepare("SELECT layout, slot, component, version_hash FROM ui_slots ORDER BY layout, slot").unwrap();
    q.query_map([], |r| (0..4).map(|i| r.get::<_, Value>(i)).collect()).unwrap().map(Result::unwrap).collect()
}

fn row(layout: &str, slot: &str, component: &str, hash: &str) -> Vec<Value> {
    [layout, slot, component, hash].iter().map(|s| Value::Text(s.to_string())).collect()
}

fn section(component: Option<&str>, version: Option<&str>) -> Section {
    Section { layout: LAYOUT.into(), slot: SLOT.into(), component: component.map(str::to_string), version: version.map(str::to_string) }
}

#[test]
fn an_assignment_is_a_row_and_clear_removes_it() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(UI_DB_FILE);
    let s = UiStore::open(&path).unwrap();
    let v1 = s.push(version(1, V1)).unwrap().version;
    let v2 = s.push(version(2, &["board-layout", "ticket-list"])).unwrap().version;
    let mut changes = s.subscribe();

    // v2 is live and holds no ticket-grid: name v1.
    let set = s.slots_set(LAYOUT, SLOT, "ticket-grid", Some(&v1)).unwrap();
    let want = Assignment { layout: LAYOUT.into(), slot: SLOT.into(), component: "ticket-grid".into(), version: v1.clone(), default: Some("ticket-list".into()) };
    assert_eq!(set, want, "the set answers the row, with the default it overrides");
    assert_eq!(slot_rows(&path), vec![row(LAYOUT, SLOT, "ticket-grid", &v1)], "one row, naming v1's hash");
    assert_eq!(s.slots_list(None).unwrap(), vec![want.clone()], "list shows it");
    assert_eq!(s.slots_list(Some(LAYOUT)).unwrap(), vec![want], "and under its layout");
    assert!(s.slots_list(Some("ticket-page")).unwrap().is_empty(), "and under no other");
    assert_eq!(changes.try_recv().expect("the set is said on the broadcast"), UiChange::SlotChanged(section(Some("ticket-grid"), Some(&v1))));

    // Set again, no version named: the live hash at that moment, pinned; still one row per section.
    let again = s.slots_set(LAYOUT, SLOT, "ticket-list", None).unwrap();
    assert_eq!((again.component.as_str(), again.version.as_str()), ("ticket-list", v2.as_str()), "no version: the live one's hash");
    assert_eq!(slot_rows(&path), vec![row(LAYOUT, SLOT, "ticket-list", &v2)], "the row replaced, not added");
    assert_eq!(changes.try_recv().expect("said too"), UiChange::SlotChanged(section(Some("ticket-list"), Some(&v2))));

    // A later push never moves it; the default follows the live manifest (v3's is ticket-grid).
    let v3 = s.push(version_with(3, &["board-layout", "ticket-grid"], serde_json::json!({ LAYOUT: { SLOT: "ticket-grid" } }))).unwrap().version;
    assert_eq!(changes.try_recv().expect("the push is said"), UiChange::Live { version: v3.clone() });
    assert_eq!(slot_rows(&path), vec![row(LAYOUT, SLOT, "ticket-list", &v2)], "the row still names v2");
    let listed = s.slots_list(None).unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!((listed[0].version.as_str(), listed[0].default.as_deref()), (v2.as_str(), Some("ticket-grid")), "v2 pinned; v3's default");

    // Clear: the row gone, and the live manifest's default said, in the live version.
    let cleared = s.slots_clear(LAYOUT, SLOT).unwrap();
    assert_eq!(cleared, section(Some("ticket-grid"), Some(&v3)), "the section shows the live manifest's default");
    assert!(slot_rows(&path).is_empty(), "the row removed");
    assert!(s.slots_list(None).unwrap().is_empty(), "list shows none");
    assert_eq!(changes.try_recv().expect("the clear is said"), UiChange::SlotChanged(cleared.clone()));

    // A clear on a section with no row: the default answered, nothing written, nothing said.
    assert_eq!(s.slots_clear(LAYOUT, SLOT).unwrap(), cleared, "the default, again");
    assert!(slot_rows(&path).is_empty());
    assert!(matches!(changes.try_recv(), Err(TryRecvError::Empty)), "nothing said");

    // A live manifest without the layout: the default is null, and a clear's too.
    s.slots_set(LAYOUT, SLOT, "ticket-list", Some(&v2)).unwrap();
    let _ = changes.try_recv();
    let v4 = s.push(version_with(4, &["ticket-page"], serde_json::json!({}))).unwrap().version;
    assert_eq!(changes.try_recv().unwrap(), UiChange::Live { version: v4 });
    assert_eq!(s.slots_list(None).unwrap()[0].default, None, "v4 declares no such layout");
    assert_eq!(s.slots_clear(LAYOUT, SLOT).unwrap(), section(None, None), "nulls");
    assert_eq!(changes.try_recv().unwrap(), UiChange::SlotChanged(section(None, None)));
    assert!(matches!(changes.try_recv(), Err(TryRecvError::Empty)), "and nothing else");
}

#[test]
fn a_set_names_only_what_is_stored() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(UI_DB_FILE);
    let s = UiStore::open(&path).unwrap();
    // Nothing live: a set naming no version names nothing stored.
    assert!(matches!(s.slots_set(LAYOUT, SLOT, "ticket-list", None), Err(UiError::UnknownVersion(_))), "nothing is live");
    let v1 = s.push(version(1, V1)).unwrap().version;
    let mut changes = s.subscribe();
    let versions = s.versions().unwrap();

    let unknown = "f".repeat(64);
    match s.slots_set(LAYOUT, SLOT, "ticket-list", Some(&unknown)) {
        Err(e @ UiError::UnknownVersion(_)) => assert!(e.to_string().contains(&unknown), "it names the hash: {e}"),
        other => panic!("an unknown hash: {other:?}"),
    }
    match s.slots_set(LAYOUT, SLOT, "ticket-board", Some(&v1)) {
        Err(UiError::UnknownComponent { component, version }) => assert_eq!((component.as_str(), version.as_str()), ("ticket-board", v1.as_str())),
        other => panic!("a component v1 lacks: {other:?}"),
    }
    match s.slots_set(LAYOUT, "sidebar", "ticket-list", None) {
        Err(UiError::UnknownSlot { layout, slot }) => assert_eq!((layout.as_str(), slot.as_str()), (LAYOUT, "sidebar")),
        other => panic!("a slot the layout does not declare: {other:?}"),
    }
    match s.slots_set("ticket-list", SLOT, "ticket-grid", None) {
        Err(UiError::UnknownSlot { layout, slot }) => assert_eq!((layout.as_str(), slot.as_str()), ("ticket-list", SLOT)),
        other => panic!("a component that is no layout: {other:?}"),
    }

    assert!(slot_rows(&path).is_empty(), "nothing written");
    assert!(s.slots_list(None).unwrap().is_empty());
    assert_eq!(s.versions().unwrap(), versions, "no version touched");
    assert!(matches!(changes.try_recv(), Err(TryRecvError::Empty)), "nothing said");
    // The file itself refuses an assignment to a hash it does not store.
    let c = Connection::open(&path).unwrap();
    c.pragma_update(None, "foreign_keys", true).unwrap();
    let dangling = c.execute("INSERT INTO ui_slots (layout, slot, component, version_hash) VALUES (?1, ?2, 'ticket-list', ?3)", [LAYOUT, SLOT, &unknown]);
    assert!(dangling.is_err(), "a foreign key");
}
