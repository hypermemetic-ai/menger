//! TICKETS-6: freezing a scope — a test per row of the ticket's table: a
//! frozen THEME-1 takes no new child (create, set_parent, update, notes
//! upgrade), no parentless THEME ticket, no shortcode hosted under it, no
//! new THEME number whatever its parent, no reshortcode into THEME, and no
//! kind but a scope; a shortcode hosted under THEME-4 grows, moving out is
//! allowed, and unfreezing lets it grow again. Only a scope freezes, both
//! ways are idempotent, `FrozenChanged` is broadcast only when it moved, the
//! mark is the cell `frozen`, durable, and the derived state a scan.

use super::update_many::Selection;
use super::*;

fn new(epic: &str, title: &str, kind: Kind, parent: Option<&str>) -> NewTicket {
    NewTicket {
        path: None,
        epic: epic.into(),
        title: title.into(),
        kind,
        severity: None,
        blocked_by: Vec::new(),
        parent: parent.map(Into::into),
        body: String::new(),
    }
}

/// `THEME`: THEME-1 its root, THEME-2 and THEME-3 builds under it, THEME-4
/// an execution under it. `OTHER`: OTHER-1 its root, OTHER-2 a build.
async fn fixture(store: &Store) {
    store.create(SYS, new("THEME", "the theme", Kind::Scope, None)).await.unwrap();
    store.create(SYS, new("THEME", "two", Kind::Build, None)).await.unwrap();
    store.create(SYS, new("THEME", "three", Kind::Build, None)).await.unwrap();
    store.create(SYS, new("THEME", "four", Kind::Execution, None)).await.unwrap();
    store.create(SYS, new("OTHER", "other", Kind::Scope, None)).await.unwrap();
    store.create(SYS, new("OTHER", "other two", Kind::Build, None)).await.unwrap();
}

/// The fixture, THEME-1 frozen.
async fn frozen() -> Store {
    let store = Store::new();
    fixture(&store).await;
    store.freeze(SYS.permit("THEME-1"), true).await.unwrap();
    store
}

fn scope_of(e: &StoreError) -> (&str, Option<&str>) {
    match e {
        StoreError::Frozen { scope, numbers } => (scope.as_str(), numbers.as_deref()),
        e => panic!("not a frozen refusal: {e:?}"),
    }
}

fn listed(store: &Store, filter: &str) -> Vec<String> {
    let q = Filtered { lens: Lens::parse(filter).unwrap(), epic: None, status: None, parent: None, keep: None };
    store.filtered_page(SYS, &q, None, None, None).unwrap().tickets.iter().map(|t| t.id.to_string()).collect()
}

fn assert_derived(store: &Store, after: &str) {
    let cache = store.tickets.read().unwrap();
    let scanned = Cache::new(cache.order.values().map(|t| (t.id.to_string(), t.clone())).collect(), cache.inputs.clone());
    assert_eq!(stored::compare(&cache.form(), &scanned.form()), Vec::<String>::new(), "after {after}");
}

#[tokio::test]
async fn only_a_scope_freezes_and_both_ways_are_idempotent_and_broadcast_once() {
    let store = Store::new();
    fixture(&store).await;
    let e = store.freeze(SYS.permit("THEME-2"), true).await.unwrap_err();
    assert_eq!(e.to_string(), "THEME-2 is a build, not a scope: only a scope freezes (nothing changed)");
    let mut rx = store.subscribe();
    match store.freeze(SYS.permit("THEME-1"), true).await.unwrap() {
        Change::FrozenChanged { ticket } => assert!(ticket.frozen),
        c => panic!("{c:?}"),
    }
    assert!(matches!(rx.try_recv(), Ok(Change::FrozenChanged { ticket }) if ticket.id == "THEME-1" && ticket.frozen));
    while rx.try_recv().is_ok() {}
    // Again: the ticket as it is, nothing written or sent.
    let written = store.tickets.read().unwrap().written;
    assert!(matches!(store.freeze(SYS.permit("THEME-1"), true).await.unwrap(), Change::FrozenChanged { ticket } if ticket.frozen));
    assert_eq!(store.tickets.read().unwrap().written, written);
    assert!(rx.try_recv().is_err(), "a no-op is not broadcast");
    assert!(store.get(SYS, "THEME-1").await.unwrap().frozen);
    // Unfreeze: once moved, then a no-op — on any ticket.
    assert!(matches!(store.freeze(SYS.permit("THEME-1"), false).await.unwrap(), Change::FrozenChanged { ticket } if !ticket.frozen));
    assert!(matches!(rx.try_recv(), Ok(Change::FrozenChanged { ticket }) if !ticket.frozen));
    while rx.try_recv().is_ok() {}
    assert!(matches!(store.freeze(SYS.permit("THEME-2"), false).await.unwrap(), Change::FrozenChanged { ticket } if !ticket.frozen));
    assert!(rx.try_recv().is_err());
}

#[tokio::test]
async fn a_new_child_of_the_frozen_scope_is_refused_naming_how_to_unfreeze() {
    let store = frozen().await;
    let e = store.create(SYS, new("THEME", "more", Kind::Build, Some("THEME-1"))).await.unwrap_err();
    assert_eq!(e.to_string(), "THEME-1 is frozen — unfreeze it first (tickets unfreeze --id THEME-1) (nothing changed)");
    // Parentless in THEME: it would sit at the scope's top.
    let e = store.create(SYS, new("THEME", "more", Kind::Build, None)).await.unwrap_err();
    assert_eq!(scope_of(&e), ("THEME-1", None));
    // A shortcode hosted under it.
    let e = store.create(SYS, new("NEW", "an area", Kind::Scope, Some("THEME-1"))).await.unwrap_err();
    assert_eq!(scope_of(&e), ("THEME-1", None));
    // A note upgraded under it.
    let n = store.add_note_as("more theme".into(), None, None).await.unwrap();
    let e = store.upgrade_note(SYS, n.id.get(), new("THEME", "more", Kind::Build, Some("THEME-1"))).await.unwrap_err();
    assert_eq!(scope_of(&e), ("THEME-1", None));
    assert_eq!(store.count(SYS), 6, "nothing was created");
}

#[tokio::test]
async fn the_frozen_shortcode_takes_no_new_numbers_whatever_the_parent() {
    let store = frozen().await;
    let e = store.create(SYS, new("THEME", "under four", Kind::Build, Some("THEME-4"))).await.unwrap_err();
    assert_eq!(e.to_string(), "THEME is frozen — no new THEME numbers (tickets unfreeze --id THEME-1) (nothing changed)");
    assert_eq!(scope_of(&e), ("THEME-1", Some("THEME")));
    let n = store.add_note_as("under four".into(), None, None).await.unwrap();
    let e = store.upgrade_note(SYS, n.id.get(), new("THEME", "under four", Kind::Build, Some("THEME-4"))).await.unwrap_err();
    assert_eq!(scope_of(&e), ("THEME-1", Some("THEME")));
    // Nor by a reshortcode into THEME — under its root, or anywhere in it.
    let e = store.reshortcode(SYS, Reshortcode { ids: vec!["OTHER-2".into()], to: "THEME".into(), ..Reshortcode::default() }).await.unwrap_err();
    assert_eq!(scope_of(&e), ("THEME-1", None));
    let e = store
        .reshortcode(SYS, Reshortcode { ids: vec!["OTHER-2".into()], to: "THEME".into(), parent: Some("THEME-4".into()), ..Reshortcode::default() })
        .await
        .unwrap_err();
    assert_eq!(scope_of(&e), ("THEME-1", Some("THEME")));
    // Nor a new shortcode a reshortcode mints under the frozen scope.
    let e = store
        .reshortcode(SYS, Reshortcode { ids: vec!["OTHER-2".into()], to: "NEW".into(), new_scope: Some("n".into()), parent: Some("THEME-1".into()), ..Reshortcode::default() })
        .await
        .unwrap_err();
    assert_eq!(scope_of(&e), ("THEME-1", None));
    assert_eq!(store.get(SYS, "OTHER-2").await.unwrap().id, "OTHER-2", "nothing moved");
}

#[tokio::test]
async fn a_shortcode_hosted_under_a_child_grows_and_is_not_frozen_by_it() {
    let store = frozen().await;
    let look = store.create(SYS, new("LOOK", "a look", Kind::Scope, Some("THEME-4"))).await.unwrap();
    assert_eq!((look.id.as_str(), look.parent.as_deref()), ("LOOK-1", Some("THEME-4")));
    assert_eq!(store.create(SYS, new("LOOK", "its work", Kind::Build, None)).await.unwrap().id, "LOOK-2");
    // Deeper too.
    assert_eq!(store.create(SYS, new("DEEP", "deeper", Kind::Scope, Some("LOOK-2"))).await.unwrap().id, "DEEP-1");
    // …and a reshortcode that mints one there.
    let r = store
        .reshortcode(SYS, Reshortcode { ids: vec!["OTHER-2".into()], to: "MORE".into(), new_scope: Some("m".into()), parent: Some("THEME-4".into()), ..Reshortcode::default() })
        .await
        .unwrap();
    assert_eq!(r.moves[0].to, "MORE-2");
}

#[tokio::test]
async fn a_move_under_the_frozen_scope_is_refused_and_out_of_it_allowed() {
    let store = Store::new();
    fixture(&store).await;
    store.set_parent(SYS.moving("THEME-3", "THEME-2")).await.unwrap();
    store.create(SYS, new("LOOK", "a look", Kind::Scope, Some("THEME-4"))).await.unwrap();
    store.freeze(SYS.permit("THEME-1"), true).await.unwrap();
    let e = store.set_parent(SYS.moving("THEME-3", "THEME-1")).await.unwrap_err();
    assert_eq!(scope_of(&e), ("THEME-1", None));
    let e = store.update(SYS.updating("THEME-3"), Update { parent: Some("THEME-1".into()), ..Update::default() }).await.unwrap_err();
    assert!(e.to_string().contains("THEME-1 is frozen — unfreeze it first"), "{e}");
    let e = store.update_many(SYS, Selection::Ids(vec!["THEME-3".into()]), Update { parent: Some("THEME-1".into()), ..Update::default() }).await.unwrap_err();
    assert!(e.to_string().contains("THEME-1 is frozen"), "{e}");
    // A hosted shortcode's root moved under it: a new shortcode there.
    let e = store.set_parent(SYS.moving("LOOK-1", "THEME-1")).await.unwrap_err();
    assert_eq!(scope_of(&e), ("THEME-1", None));
    assert_eq!(store.get(SYS, "THEME-3").await.unwrap().parent.as_deref(), Some("THEME-2"), "nothing moved");
    // Moving a child out, and within the shortcode under another: allowed.
    store.set_parent(SYS.moving("THEME-4", "THEME-2")).await.unwrap();
    store.set_parent(SYS.moving("LOOK-1", "OTHER-1")).await.unwrap();
    // The parent it already has is no move.
    store.set_parent(SYS.moving("THEME-2", "THEME-1")).await.unwrap();
}

#[tokio::test]
async fn the_frozen_scope_stays_a_scope() {
    let store = frozen().await;
    for kind in [Kind::Doc, Kind::Build] {
        let e = store.set_kind(SYS.permit("THEME-1"), kind, None).await.unwrap_err();
        assert_eq!(scope_of(&e), ("THEME-1", None));
    }
    let e = store.update(SYS.updating("THEME-1"), Update { kind: Some(Kind::Doc), ..Update::default() }).await.unwrap_err();
    assert!(e.to_string().contains("THEME-1 is frozen"), "{e}");
    assert_eq!(store.get(SYS, "THEME-1").await.unwrap().kind, Kind::Scope);
}

#[tokio::test]
async fn unfrozen_it_grows_again() {
    let store = frozen().await;
    store.freeze(SYS.permit("THEME-1"), false).await.unwrap();
    assert_eq!(store.create(SYS, new("THEME", "more", Kind::Build, Some("THEME-1"))).await.unwrap().id, "THEME-5");
    assert_eq!(store.create(SYS, new("THEME", "under four", Kind::Build, Some("THEME-4"))).await.unwrap().id, "THEME-6");
}

#[tokio::test]
async fn frozen_is_a_filter_cell_kept_with_the_derived_state() {
    let store = Store::new();
    fixture(&store).await;
    assert!(listed(&store, "frozen").is_empty());
    store.freeze(SYS.permit("THEME-1"), true).await.unwrap();
    assert_derived(&store, "a freeze");
    assert_eq!(listed(&store, "frozen"), ["THEME-1"]);
    assert_eq!(listed(&store, "kind scope and not frozen"), ["OTHER-1"]);
    store.freeze(SYS.permit("THEME-1"), false).await.unwrap();
    assert_derived(&store, "an unfreeze");
    assert!(listed(&store, "frozen").is_empty());
}

#[tokio::test]
async fn frozen_survives_a_reopen_and_verify_derived_holds() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("frozen.db");
    {
        let store = Store::open(&path).unwrap();
        fixture(&store).await;
        store.freeze(SYS.permit("THEME-1"), true).await.unwrap();
    }
    let store = Store::open(&path).unwrap();
    assert!(store.get(SYS, "THEME-1").await.unwrap().frozen);
    assert_eq!(listed(&store, "frozen"), ["THEME-1"]);
    assert!(matches!(store.create(SYS, new("THEME", "more", Kind::Build, Some("THEME-4"))).await, Err(StoreError::Frozen { .. })));
    drop(store);
    assert!(matches!(Store::verify_derived(&path).unwrap(), Verdict::Consistent { .. }), "{:?}", Store::verify_derived(&path).unwrap());
}

/// A form-13 file — no `frozen` columns — gains them in place on its first
/// open and builds nothing again.
#[test]
fn a_form_13_file_gains_frozen_in_place() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("form13.db");
    let rt = tokio::runtime::Runtime::new().unwrap();
    {
        let store = Store::open(&path).unwrap();
        rt.block_on(fixture(&store));
    }
    {
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.execute_batch("ALTER TABLE cells DROP COLUMN frozen; ALTER TABLE tickets DROP COLUMN frozen; UPDATE derived_state SET form = 13;").unwrap();
    }
    let state = |p: &std::path::Path| stored::state(&rusqlite::Connection::open_with_flags(p, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap()).unwrap();
    assert_eq!(state(&path), stored::State::Older(13));
    let store = Store::open(&path).unwrap();
    assert_eq!(state(&path), stored::State::Current);
    rt.block_on(store.freeze(SYS.permit("THEME-1"), true)).unwrap();
    assert_eq!(listed(&store, "frozen"), ["THEME-1"]);
    drop(store);
    assert!(matches!(Store::verify_derived(&path).unwrap(), Verdict::Consistent { .. }));
}

/// AGS-23: a shortcode root's row is its group's — its ❄ is on the group's
/// `root_row`, so a freeze of a root says its group, and the read carries it.
#[tokio::test]
async fn a_frozen_root_says_its_group_with_the_mark() {
    let store = Store::new();
    fixture(&store).await;
    let mut rx = store.subscribe();
    store.freeze(SYS.permit("THEME-1"), true).await.unwrap();
    let mut said = Vec::new();
    while let Ok(c) = rx.try_recv() {
        if let Change::GroupChanged { group } = c {
            said.push(group);
        }
    }
    assert_eq!(said.len(), 1, "one group: {said:?}");
    assert_eq!(said[0].shortcode.as_str(), "THEME");
    assert!(said[0].root_row.as_ref().unwrap().frozen);
    let theme = |s: &Store| s.groups(SYS).into_iter().find(|g| g.shortcode.as_str() == "THEME").unwrap();
    assert!(theme(&store).root_row.unwrap().frozen);
    store.freeze(SYS.permit("THEME-1"), false).await.unwrap();
    assert!(!theme(&store).root_row.unwrap().frozen);
}
