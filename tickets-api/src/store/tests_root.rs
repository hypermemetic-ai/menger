//! TICKETS-40: a shortcode's root made a root of its organization — its
//! parent removed (`set_parent --parent none`, `update --parent none`). The
//! shortcode rehomes to the organization's top namespace, every ticket in it
//! and every shortcode hosted under it taking its new address; a code the
//! top holds is refused, naming the one there; any other ticket is refused
//! (MNG-14); a root with no parent writes nothing; the old parent hears
//! `child_removed`; and `--verify-derived` finds the file consistent.

use super::*;
use super::tests::Changes;
use std::path::Path;

fn new(epic: &str, kind: Kind, parent: Option<&str>) -> NewTicket {
    NewTicket {
        epic: epic.into(),
        title: format!("a {kind} in {epic}"),
        kind,
        severity: None,
        blocked_by: Vec::new(),
        parent: parent.map(str::to_string),
        path: None,
        body: String::new(),
    }
}

/// PLEXUS and MENGER at the top; KAYOKEN under PLEXUS-1 (a scope, a build
/// under it, and DEEP hosted under KAYOKEN-2); THEME at the top and a
/// namesake THEME under PLEXUS-1.
async fn plexus(path: &Path) -> Store {
    let store = Store::open(path).unwrap();
    for top in ["PLEXUS", "MENGER", "THEME"] {
        store.create(SYS, new(top, Kind::Scope, None)).await.unwrap();
    }
    store.create(SYS, new("PLEXUS", Kind::Build, Some("PLEXUS-1"))).await.unwrap();
    let k = store.create(SYS, new("KAYOKEN", Kind::Scope, Some("PLEXUS-1"))).await.unwrap();
    assert_eq!(k.address, "hypermemetic:PLEXUS/KAYOKEN-1");
    store.create(SYS, new("KAYOKEN", Kind::Build, Some("KAYOKEN-1"))).await.unwrap();
    store.create(SYS, new("KAYOKEN", Kind::Build, Some("KAYOKEN-2"))).await.unwrap();
    let deep = store.create(SYS, new("DEEP", Kind::Scope, Some("KAYOKEN-2"))).await.unwrap();
    assert_eq!(deep.address, "hypermemetic:PLEXUS/KAYOKEN/DEEP-1");
    store.create(SYS, new("DEEP", Kind::Build, None)).await.unwrap();
    let theme = store.create(SYS, new("THEME", Kind::Scope, Some("PLEXUS-1"))).await.unwrap();
    assert_eq!(theme.address, "hypermemetic:PLEXUS/THEME-1");
    store
}

fn consistent(path: &Path) {
    match Store::verify_derived(path).unwrap() {
        Verdict::Consistent { .. } => {}
        other => panic!("the derived state is not a rebuild's: {other:?}"),
    }
}

/// The group of a shortcode, by its letters' address.
fn group(store: &Store, address: &str) -> Option<Group> {
    store.groups(SYS).into_iter().find(|g| g.address == address)
}

#[tokio::test]
async fn a_root_made_a_root_rehomes_its_shortcode_and_all_it_hosts() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("t.db");
    let store = plexus(&path).await;
    let before = group(&store, "hypermemetic:PLEXUS").unwrap();
    assert_eq!(group(&store, "hypermemetic:PLEXUS/KAYOKEN").unwrap().host.as_deref(), Some("PLEXUS-1"));
    let mut rx = store.subscribe();
    let id = store.get(SYS, "KAYOKEN-1").await.unwrap().id;

    let answer = store.set_parent(SYS.rooting("PLEXUS/KAYOKEN-1")).await.unwrap();
    match &answer {
        Change::ParentChanged { ticket, previous } => {
            assert_eq!((ticket.id.as_str(), previous.as_deref(), ticket.parent.as_ref()), (id.as_str(), Some("PLEXUS-1"), None));
            assert_eq!(ticket.address, "hypermemetic:KAYOKEN-1", "the answer carries its new address");
        }
        other => panic!("expected parent_changed, got {other:?}"),
    }
    let changes: Vec<Change> = std::iter::from_fn(|| rx.try_change().ok()).collect();
    assert!(matches!(&changes[0], Change::ParentChanged { .. }), "{changes:?}");
    assert!(!changes.iter().any(|c| matches!(c, Change::ChildrenChanged { .. })), "nothing joins: {changes:?}");
    let removed: Vec<(String, u64)> = changes
        .iter()
        .filter_map(|c| match c {
            Change::ChildRemoved { id, removed, child_count, .. } if removed == "KAYOKEN-1" => Some((id.clone(), *child_count)),
            _ => None,
        })
        .collect();
    assert_eq!(removed, [("PLEXUS-1".to_string(), 2)], "PLEXUS-1 keeps PLEXUS-2 and THEME's root");

    // Every ticket of it and of what it hosts takes its new address.
    for (named, address) in [
        ("KAYOKEN-1", "hypermemetic:KAYOKEN-1"),
        ("KAYOKEN-3", "hypermemetic:KAYOKEN-3"),
        ("DEEP-2", "hypermemetic:KAYOKEN/DEEP-2"),
    ] {
        assert_eq!(store.get(SYS, named).await.unwrap().address, address, "{named}");
    }
    assert_eq!(store.get(SYS, "PLEXUS-1").await.unwrap().child_count, 2);
    // Its row is a top-level group now; PLEXUS's no longer counts it.
    let kayoken = group(&store, "hypermemetic:KAYOKEN").unwrap();
    assert_eq!((kayoken.host, kayoken.total), (None, 5), "KAYOKEN's three and DEEP's two");
    let after = group(&store, "hypermemetic:PLEXUS").unwrap();
    assert_eq!(after.total, before.total - 5);
    assert_eq!(group(&store, "hypermemetic:KAYOKEN/DEEP").unwrap().host.as_deref(), Some("KAYOKEN-2"));
    consistent(&path);

    // Kept: a reopen — the roll-ups built from the rows — reads the same.
    let live = store.groups(SYS);
    drop(store);
    let store = Store::open(&path).unwrap();
    assert_eq!(store.groups(SYS), live, "the cache's groups are a rebuild's");
    assert_eq!(store.get(SYS, "KAYOKEN-2").await.unwrap().address, "hypermemetic:KAYOKEN-2");
    assert_eq!(group(&store, "hypermemetic:KAYOKEN").unwrap().total, 5);

    // A root with no parent, made one, writes nothing.
    let mut rx = store.subscribe();
    match store.set_parent(SYS.rooting("KAYOKEN-1")).await.unwrap() {
        Change::ParentChanged { previous, .. } => assert_eq!(previous, None),
        other => panic!("expected parent_changed, got {other:?}"),
    }
    assert!(rx.try_change().is_err(), "a no-op sends nothing");

    // And back under PLEXUS-1: the move it was, undone.
    store.set_parent(SYS.moving("KAYOKEN-1", "PLEXUS-1")).await.unwrap();
    assert_eq!(store.get(SYS, "KAYOKEN-3").await.unwrap().address, "hypermemetic:PLEXUS/KAYOKEN-3");
    assert_eq!(group(&store, "hypermemetic:PLEXUS").unwrap().total, before.total);
    consistent(&path);
}

#[tokio::test]
async fn a_code_the_top_holds_is_refused_naming_the_one_there() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("t.db");
    let store = plexus(&path).await;
    let refused = store.set_parent(SYS.rooting("PLEXUS/THEME-1")).await.unwrap_err();
    match &refused {
        StoreError::ShortcodeTaken { namespace, there, .. } => assert_eq!((namespace.as_str(), there.as_str()), ("hypermemetic:", "hypermemetic:THEME")),
        other => panic!("expected ShortcodeTaken, got {other:?}"),
    }
    assert_eq!(store.get(SYS, "PLEXUS/THEME-1").await.unwrap().parent.as_deref(), Some("PLEXUS-1"), "nothing written");
    consistent(&path);
}

#[tokio::test]
async fn only_a_shortcode_s_root_is_made_a_root() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("t.db");
    let store = plexus(&path).await;
    let refused = store.set_parent(SYS.rooting("KAYOKEN-3")).await.unwrap_err().to_string();
    assert!(refused.contains("KAYOKEN-3 is not KAYOKEN's root") && refused.contains("its root is KAYOKEN-1") && refused.contains("MNG-14"), "{refused}");
    assert_eq!(store.get(SYS, "KAYOKEN-3").await.unwrap().parent.as_deref(), Some("KAYOKEN-2"), "nothing written");
    let up = Update { unparent: true, ..Update::default() };
    let refused = store.update(SYS.updating("KAYOKEN-2"), up).await.unwrap_err().to_string();
    assert!(refused.contains("KAYOKEN-2 is not KAYOKEN's root"), "{refused}");
    consistent(&path);
}

#[tokio::test]
async fn update_makes_a_root_a_root_in_the_same_write_as_the_rest() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("t.db");
    let store = plexus(&path).await;
    let up = Update { unparent: true, title: Some("kayoken, a root".into()), ..Update::default() };
    let done = store.update(SYS.updating("KAYOKEN-1"), up).await.unwrap();
    assert_eq!(done.changed, ["title", "parent"]);
    assert_eq!((done.ticket.parent.as_ref(), done.ticket.address.as_str()), (None, "hypermemetic:KAYOKEN-1"));
    assert_eq!(store.get(SYS, "DEEP-1").await.unwrap().address, "hypermemetic:KAYOKEN/DEEP-1");
    assert_eq!(group(&store, "hypermemetic:KAYOKEN").unwrap().host, None);
    // A root already, nothing moves: `unparent` asks for no change.
    let again = store.update(SYS.updating("KAYOKEN-1"), Update { unparent: true, ..Update::default() }).await.unwrap();
    assert!(again.changed.is_empty(), "{:?}", again.changed);
    // The clash, through update: refused, nothing written.
    let refused = store.update(SYS.updating("PLEXUS/THEME-1"), Update { unparent: true, ..Update::default() }).await.unwrap_err();
    assert!(format!("{refused:?}").contains("ShortcodeTaken"), "{refused:?}");
    consistent(&path);
}
