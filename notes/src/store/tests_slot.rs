//! NOTES-4: a note's slot in the store — empty until filled; a fill is kept
//! with every fill before it and said whole; an empty keeps the fills; the
//! props merge (a `null` takes a key away) and are held to 64 KiB; a build
//! is said, ended by a fill and by ten minutes of silence; a delete leaves
//! the slot as it was, so a note brought back has it; a file from before
//! NOTES-4 opens with every slot empty; the slot survives a reopen, the
//! build does not.

use super::*;
use crate::slot::{FillOf, Hand};
use serde_json::json;

fn open(dir: &tempfile::TempDir) -> Arc<NotesStore> {
    Arc::new(NotesStore::open(&dir.path().join("notes.db")).unwrap())
}

fn hand() -> Hand {
    Hand { name: "builder".into(), principal: "key:b".into() }
}

fn graph(version: &str) -> FillOf {
    FillOf { component: "graph-nodes".into(), version: version.into(), props: None, say: Some("nodes on a ring".into()) }
}

fn obj(v: serde_json::Value) -> serde_json::Map<String, serde_json::Value> {
    v.as_object().unwrap().clone()
}

fn building(component: &str, step: Option<crate::BuildStep>, failed: bool, say: Option<&str>) -> crate::Building {
    let at = now_ms();
    crate::Building { component: component.into(), step, say: say.map(str::to_string), failed, by: "builder".into(), principal: "key:b".into(), started: at, at }
}

#[tokio::test]
async fn a_slot_is_empty_until_filled_and_keeps_every_fill() {
    let dir = tempfile::tempdir().unwrap();
    let s = open(&dir);
    let n = s.create("acme", "The service map", "", "owner", "key:o").await.unwrap();
    let empty = s.slot(&n.id).await.unwrap();
    assert_eq!((empty.fill.is_none(), empty.fills.len(), empty.props.len()), (true, 0, 0), "{empty:?}");

    let mut rx = s.subscribe();
    let one = s.fill(&n.id, graph(&"a".repeat(64)), hand()).await.unwrap();
    assert_eq!(one.fill.as_ref().map(|f| (f.n, f.component.as_str())), Some((1, "graph-nodes")));
    match rx.try_recv().unwrap() {
        Change::SlotChanged { org, slot } => assert_eq!((org.as_str(), slot), ("acme", one.clone())),
        other => panic!("{other:?}"),
    }
    let two = s.fill(&n.id, FillOf { props: Some(obj(json!({"nodes": 6}))), ..graph(&"b".repeat(64)) }, hand()).await.unwrap();
    assert_eq!(two.fill.as_ref().map(|f| f.n), Some(2));
    assert_eq!(two.fills.iter().map(|f| f.n).collect::<Vec<_>>(), [2, 1], "newest first");
    assert_eq!(two.props, obj(json!({"nodes": 6})));
    // A fill naming no props keeps the slot's.
    let three = s.fill(&n.id, graph(&"c".repeat(64)), hand()).await.unwrap();
    assert_eq!(three.props, obj(json!({"nodes": 6})));

    let gone = s.empty(&n.id, hand()).await.unwrap();
    assert_eq!((gone.fill.is_none(), gone.fills.len(), gone.props.len()), (true, 3, 0), "the fills stay: {gone:?}");
    assert!(s.slot("zzzzzzzz").await.is_err(), "no such note");
}

#[tokio::test]
async fn props_merge_a_null_takes_a_key_and_64_kib_is_the_cap() {
    let dir = tempfile::tempdir().unwrap();
    let s = open(&dir);
    let n = s.create("acme", "Map", "", "owner", "key:o").await.unwrap();
    s.props(&n.id, obj(json!({"a": 1, "b": [1, 2]})), hand()).await.unwrap();
    let mut rx = s.subscribe();
    let slot = s.props(&n.id, obj(json!({"a": null, "c": "x"})), hand()).await.unwrap();
    assert_eq!(slot.props, obj(json!({"b": [1, 2], "c": "x"})));
    assert!(matches!(rx.try_recv().unwrap(), Change::PropsChanged { by, .. } if by == "builder"));
    let big = obj(json!({ "d": "y".repeat(crate::vocab::PROPS_MAX) }));
    assert!(matches!(s.props(&n.id, big, hand()).await, Err(StoreError::TooLarge(_))));
    assert_eq!(s.slot(&n.id).await.unwrap().props, obj(json!({"b": [1, 2], "c": "x"})), "nothing written");
}

#[tokio::test]
async fn a_build_is_said_ended_by_a_fill_and_by_ten_minutes_of_silence() {
    let dir = tempfile::tempdir().unwrap();
    let s = open(&dir);
    let n = s.create("acme", "Map", "", "owner", "key:o").await.unwrap();
    s.fill(&n.id, graph(&"a".repeat(64)), hand()).await.unwrap();
    let mut rx = s.subscribe();
    let b = s.building(&n.id, building("graph-nodes", Some(crate::BuildStep::Written), false, Some("drag a node"))).await.unwrap();
    assert_eq!(b.building.as_ref().and_then(|b| b.step), Some(crate::BuildStep::Written));
    assert_eq!(b.fill.as_ref().map(|f| f.n), Some(1), "the component that was there stays");
    assert!(matches!(rx.try_recv().unwrap(), Change::BuildingChanged { slot, .. } if slot.building.is_some()));

    let filled = s.fill(&n.id, graph(&"b".repeat(64)), hand()).await.unwrap();
    assert!(filled.building.is_none(), "a fill ends it");

    s.building(&n.id, building("graph-nodes", Some(crate::BuildStep::Compiled), true, Some("graph-nodes.svelte:9:54 `drag` is not defined"))).await.unwrap();
    s.sweep_builds();
    assert!(s.slot(&n.id).await.unwrap().building.is_some(), "not yet idle");
    let mut rx = s.subscribe();
    s.age_build(&n.id, crate::slot::BUILD_IDLE_MS + 1);
    s.sweep_builds();
    assert!(s.slot(&n.id).await.unwrap().building.is_none(), "ten minutes on, over");
    assert!(matches!(rx.try_recv().unwrap(), Change::BuildingChanged { slot, .. } if slot.building.is_none()));
}

#[tokio::test]
async fn a_deleted_note_keeps_its_slot_and_a_reopen_keeps_it_but_not_the_build() {
    let dir = tempfile::tempdir().unwrap();
    let s = open(&dir);
    let n = s.create("acme", "Map", "", "owner", "key:o").await.unwrap();
    s.fill(&n.id, FillOf { props: Some(obj(json!({"k": 1}))), ..graph(&"a".repeat(64)) }, hand()).await.unwrap();
    s.fill(&n.id, graph(&"b".repeat(64)), hand()).await.unwrap();
    let before = s.slot(&n.id).await.unwrap();
    s.building(&n.id, building("graph-nodes", None, false, None)).await.unwrap();

    s.delete(&n.id, "admin", "key:a").await.unwrap();
    assert!(matches!(s.slot(&n.id).await, Err(StoreError::NotFound(_))), "a deleted note's slot reads as none");
    // Brought back (the file's row live again — the service has no undelete yet): the slot as it was.
    s.lock().execute("UPDATE docs SET deleted_at = NULL WHERE id = ?1", [&n.id]).unwrap();
    let back = s.slot(&n.id).await.unwrap();
    assert_eq!((back.fill.clone(), back.fills.clone(), back.props.clone()), (before.fill.clone(), before.fills.clone(), before.props.clone()));

    drop(s);
    let s = open(&dir);
    let again = s.slot(&n.id).await.unwrap();
    assert_eq!((again.fill, again.fills, again.props), (before.fill, before.fills, before.props), "kept in the file");
    assert!(again.building.is_none(), "a build is memory's: a restart ends it");
}

#[tokio::test]
async fn a_file_from_before_opens_with_every_slot_empty() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("notes.db");
    {
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.execute_batch(TABLES).unwrap();
        conn.execute_batch(crate::threads::TABLES).unwrap();
    }
    let s = Arc::new(NotesStore::open(&path).unwrap());
    let n = s.create("acme", "Map", "", "owner", "key:o").await.unwrap();
    let slot = s.slot(&n.id).await.unwrap();
    assert_eq!((slot.fill, slot.fills.len()), (None, 0));
}
