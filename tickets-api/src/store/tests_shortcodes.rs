//! ORG-3: a shortcode is unique only within its namespace. Two shortcodes
//! named THEME under different hosts both exist; the same name twice under
//! one host is refused — by every write that can make or move one (`create`,
//! `set_parent` / `update --parent` of a root, `notes upgrade`, `undelete`);
//! the resolver names a ticket by any short form of its address and refuses
//! one naming several, each candidate by its full address; `in` matches the
//! letters wherever they are hosted, `under` binds one; `--verify-derived`
//! checks the index; a file from before ORG-3 gains it once, its ids pinned.

use super::*;
use crate::address::{Address, ShortcodeId};
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

/// MENGER and PLEXUS at the top, each with a second ticket; THEME under
/// MENGER-1 and a namesake THEME under PLEXUS-1, each with a build. Answers
/// the namesake's root.
async fn two_themes(path: &Path) -> (Store, Ticket) {
    let store = Store::open(path).unwrap();
    for top in ["MENGER", "PLEXUS", "OUGHT"] {
        store.create(SYS, new(top, Kind::Scope, None)).await.unwrap();
        store.create(SYS, new(top, Kind::Build, None)).await.unwrap();
    }
    let first = store.create(SYS, new("THEME", Kind::Scope, Some("MENGER-1"))).await.unwrap();
    assert_eq!((first.id.as_str(), first.address.as_str()), ("THEME-1", "hypermemetic:MENGER/THEME-1"));
    store.create(SYS, new("THEME", Kind::Build, Some("THEME-1"))).await.unwrap();
    let second = store.create(SYS, new("THEME", Kind::Scope, Some("PLEXUS-1"))).await.unwrap();
    assert_eq!(second.epic, "THEME", "its letters are THEME");
    assert_ne!(second.shortcode, first.shortcode, "and it is another shortcode");
    assert_eq!(second.address, "hypermemetic:PLEXUS/THEME-1");
    store.create(SYS, new(&format!("PLEXUS/THEME"), Kind::Build, Some(&second.id))).await.unwrap();
    (store, second)
}

fn taken(e: StoreError) -> (String, String) {
    match e {
        StoreError::ShortcodeTaken { namespace, there, .. } => (namespace, there),
        other => panic!("expected ShortcodeTaken, got {other:?}"),
    }
}

#[tokio::test]
async fn create_makes_two_themes_under_two_hosts_and_refuses_a_second_under_one() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("t.db");
    let (store, second) = two_themes(&path).await;
    let themes: Vec<String> = store.shortcodes().into_iter().filter(|(s, _)| s.code == "THEME").map(|(_, a)| a).collect();
    assert_eq!(themes, ["hypermemetic:MENGER/THEME", "hypermemetic:PLEXUS/THEME"]);
    // Each numbers its own tickets.
    assert_eq!(store.get(SYS, "hypermemetic:PLEXUS/THEME-2").await.unwrap().title, "a build in PLEXUS/THEME");
    assert_eq!(store.get(SYS, "MENGER/THEME-2").await.unwrap().title, "a build in THEME");
    assert_eq!(store.get(SYS, &second.id).await.unwrap().address, second.address, "its key names it");
    // The same name twice under one host: a scope under MENGER-1 or under
    // MENGER-2 (the same namespace — MENGER's) is refused, naming the one
    // there; nothing was written.
    let before = store.page(SYS, PageQuery::default()).unwrap().tickets.len();
    for parent in ["MENGER-1", "MENGER-2"] {
        let (namespace, there) = taken(store.create(SYS, new("THEME", Kind::Scope, Some(parent))).await.unwrap_err());
        assert_eq!((namespace.as_str(), there.as_str()), ("hypermemetic:MENGER", "hypermemetic:MENGER/THEME"), "{parent}");
    }
    // A doc roots a shortcode as a scope does.
    assert!(matches!(store.create(SYS, new("THEME", Kind::Doc, Some("PLEXUS-2"))).await, Err(StoreError::ShortcodeTaken { .. })));
    assert_eq!(store.page(SYS, PageQuery::default()).unwrap().tickets.len(), before);
    // A third, under OUGHT: a namespace that holds none.
    let third = store.create(SYS, new("THEME", Kind::Scope, Some("OUGHT-2"))).await.unwrap();
    assert_eq!(third.address, "hypermemetic:OUGHT/THEME-1");
    // A build names its shortcode by any short form; a bare name for two is
    // refused, each by its full address.
    match store.create(SYS, new("THEME", Kind::Build, None)).await {
        Err(StoreError::Ambiguous { candidates, .. }) => assert_eq!(candidates.len(), 3, "{candidates:?}"),
        other => panic!("expected Ambiguous, got {other:?}"),
    }
    assert_eq!(store.create(SYS, new("OUGHT/THEME", Kind::Build, None)).await.unwrap().address, "hypermemetic:OUGHT/THEME-2");
    drop(store);
    assert!(matches!(Store::verify_derived(&path).unwrap(), Verdict::Consistent { shortcodes: Some(6), .. }));
}

#[tokio::test]
async fn rehoming_a_root_into_a_namespace_that_holds_its_code_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("t.db");
    let (store, second) = two_themes(&path).await;
    // set_parent: PLEXUS's THEME under MENGER-2 — MENGER holds a THEME.
    let (namespace, there) = taken(store.set_parent(SYS.moving(&second.id, "MENGER-2")).await.unwrap_err());
    assert_eq!((namespace.as_str(), there.as_str()), ("hypermemetic:MENGER", "hypermemetic:MENGER/THEME"));
    assert_eq!(store.get(SYS, &second.id).await.unwrap().parent.as_deref(), Some("PLEXUS-1"), "nothing moved");
    // update --parent: the same write, the same refusal.
    let up = Update { parent: Some("MENGER-1".into()), ..Update::default() };
    let refused = store.update(SYS.updating(&second.id), up).await.unwrap_err();
    assert!(format!("{refused:?}").contains("ShortcodeTaken"), "{refused:?}");
    // Within its own namespace, and into one free of THEME, it moves — and
    // its address and its tickets' move with it.
    store.set_parent(SYS.moving(&second.id, "PLEXUS-2")).await.unwrap();
    assert_eq!(store.get(SYS, &second.id).await.unwrap().address, "hypermemetic:PLEXUS/THEME-1");
    store.set_parent(SYS.moving(&second.id, "OUGHT-1")).await.unwrap();
    assert_eq!(store.get(SYS, &second.id).await.unwrap().address, "hypermemetic:OUGHT/THEME-1");
    assert_eq!(store.get(SYS, "OUGHT/THEME-2").await.unwrap().title, "a build in PLEXUS/THEME");
    assert!(matches!(store.get(SYS, "PLEXUS/THEME-2").await, Err(StoreError::NotFound(_))));
    // A ticket that becomes a scope (set_kind) inside a shortcode another
    // roots is refused (ORG-5): it moves into one of its own first.
    assert!(matches!(store.set_kind(SYS.permit("OUGHT/THEME-2"), Kind::Scope, None).await, Err(StoreError::ScopeNotRoot { .. })));
    assert_eq!(store.get(SYS, "OUGHT/THEME-2").await.unwrap().address, "hypermemetic:OUGHT/THEME-2");
    drop(store);
    assert!(matches!(Store::verify_derived(&path).unwrap(), Verdict::Consistent { .. }));
}

#[tokio::test]
async fn a_note_upgraded_and_a_ticket_undeleted_are_held_to_the_namespace() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("t.db");
    let (store, second) = two_themes(&path).await;
    // notes upgrade: a scope THEME under PLEXUS-2 — PLEXUS holds one.
    let note = store.add_note_as("a theme for the platform".into(), None, None).await.unwrap();
    let refused = store.upgrade_note(SYS, note.id.get(), new("THEME", Kind::Scope, Some("PLEXUS-2"))).await.unwrap_err();
    taken(refused);
    let (made, _) = store.upgrade_note(SYS, note.id.get(), new("THEME", Kind::Scope, Some("OUGHT-1"))).await.unwrap();
    assert_eq!(made.address, "hypermemetic:OUGHT/THEME-1");
    // delete and undelete: the namesake's root and its build go and come
    // back with their addresses.
    let ids = vec![second.id.to_string(), format!("{}-2", second.shortcode.as_str())];
    store.delete(SYS, Selection::Ids(ids.clone()), Deletion { children: Some(Children::Delete), unblock: true }, None).await.unwrap();
    assert!(matches!(store.get(SYS, "PLEXUS/THEME-1").await, Err(StoreError::NotFound(_))));
    store.undelete(SYS, &ids).await.unwrap();
    assert_eq!(store.get(SYS, "PLEXUS/THEME-1").await.unwrap().address, "hypermemetic:PLEXUS/THEME-1");
    drop(store);
    assert!(matches!(Store::verify_derived(&path).unwrap(), Verdict::Consistent { .. }));
}

#[tokio::test]
async fn the_resolver_names_a_ticket_by_any_short_form_and_refuses_one_naming_two() {
    let dir = tempfile::tempdir().unwrap();
    let (store, second) = two_themes(&dir.path().join("t.db")).await;
    // Every form of the PLEXUS one.
    for text in ["hypermemetic:PLEXUS/THEME-1", "PLEXUS/THEME-1", "plexus/theme-1", " PLEXUS/THEME-1 ", second.id.as_str()] {
        assert_eq!(store.resolve(text).unwrap(), second.id, "{text}");
    }
    // A bare name for two tickets: each by its full address.
    match store.get(SYS, "THEME-1").await {
        Err(StoreError::Ambiguous { text, candidates }) => {
            assert_eq!(text, "THEME-1");
            assert_eq!(candidates, ["hypermemetic:MENGER/THEME-1", "hypermemetic:PLEXUS/THEME-1"]);
        }
        other => panic!("expected Ambiguous, got {other:?}"),
    }
    assert!(matches!(store.shortcode("THEME"), Err(StoreError::Ambiguous { .. })));
    assert_eq!(store.shortcode("MENGER/THEME").unwrap().unwrap().id.as_str(), "THEME");
    assert!(store.shortcode("NOPE").unwrap().is_none());
    assert!(matches!(store.get(SYS, "NOPE/THEME-1").await, Err(StoreError::NotFound(t)) if t == "NOPE/THEME-1"));
    // A blocker by short form is stored by its key.
    let mut b = new("MENGER", Kind::Build, None);
    b.blocked_by = vec!["PLEXUS/THEME-2".into()];
    let blocked = store.create(SYS, b).await.unwrap();
    assert_eq!(blocked.blocked_by, [format!("{}-2", second.shortcode.as_str())]);
    let mut b = new("MENGER", Kind::Build, None);
    b.blocked_by = vec!["THEME-2".into()];
    assert!(matches!(store.create(SYS, b).await, Err(StoreError::Ambiguous { .. })));
    // A parent too.
    assert!(matches!(store.create(SYS, new("THEME", Kind::Build, Some("THEME-1"))).await, Err(StoreError::Ambiguous { .. })));
    assert_eq!(store.create(SYS, new("PLEXUS/THEME", Kind::Build, Some("PLEXUS/THEME-1"))).await.unwrap().address, "hypermemetic:PLEXUS/THEME-3");
}

#[tokio::test]
async fn in_matches_the_letters_wherever_hosted_and_under_binds_one() {
    let dir = tempfile::tempdir().unwrap();
    let (store, second) = two_themes(&dir.path().join("t.db")).await;
    let ids: Vec<ShortcodeId> = store.shortcodes_in("theme");
    assert_eq!(ids.len(), 2, "`in` reads the letters, whatever their case");
    let counts = store.counts(SYS, &Lens::parse("in THEME").unwrap());
    assert_eq!(counts.len(), 2, "{counts:?}");
    assert!(counts.iter().all(|(_, n)| *n == 2), "{counts:?}");
    // `under` binds one: a full address, or a short form only it has.
    let under = store.counts_read(SYS, &Lens::parse("under PLEXUS/THEME").unwrap(), None).unwrap();
    assert_eq!(under, [(second.shortcode.clone(), 2)]);
    // A name that is a shortcode's identity binds that one first — a filter
    // written before the namesake keeps its meaning (MENGER's THEME here);
    // a short form naming several and no identity is refused.
    let first = store.counts_read(SYS, &Lens::parse("under THEME").unwrap(), None).unwrap();
    assert_eq!(first, [(ShortcodeId::new("THEME"), 2)]);
    store.create(SYS, new("CSS", Kind::Scope, Some("MENGER/THEME-1"))).await.unwrap();
    store.create(SYS, new("CSS", Kind::Scope, Some(&second.id))).await.unwrap();
    assert_eq!(store.counts_read(SYS, &Lens::parse("under CSS").unwrap(), None).unwrap(), [(ShortcodeId::new("CSS"), 1)]);
    assert_eq!(store.counts(SYS, &Lens::parse("in CSS").unwrap()).len(), 2);
    let refused = store.counts_read(SYS, &Lens::parse("under THEME/CSS").unwrap(), None).unwrap_err();
    assert!(refused.to_string().contains("names more than one"), "{refused}");
    // find, by the letters alone, partial: both, each by its full address.
    let found = store.find_page(SYS, "the", None, Some(50)).unwrap();
    let addresses: Vec<&str> = found.tickets.iter().map(|t| t.address.as_str()).collect();
    for want in ["hypermemetic:MENGER/THEME-1", "hypermemetic:PLEXUS/THEME-1"] {
        assert!(addresses.contains(&want), "{want} in {addresses:?}");
    }
}

#[tokio::test]
async fn verify_names_a_stored_host_that_is_not_where_the_root_hangs() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("t.db");
    let (store, second) = two_themes(&path).await;
    drop(store);
    rusqlite::Connection::open(&path)
        .unwrap()
        .execute("UPDATE shortcodes SET host = NULL WHERE id = ?1", [second.shortcode.as_str()])
        .unwrap();
    match Store::verify_derived(&path).unwrap() {
        Verdict::Differs(why) => assert!(why.iter().any(|w| w.contains("its stored host")), "{why:?}"),
        other => panic!("expected Differs, got {other:?}"),
    }
}

/// A file from before ORG-3 (stored form 10, no shortcodes table) gains
/// the shortcodes once, at its first open: every one its code, in
/// hypermemetic, its namespace where its root hangs, every id pinned.
#[tokio::test]
async fn a_file_from_before_org_3_gains_its_shortcodes_once_and_answers_as_before() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("t.db");
    let store = Store::open(&path).unwrap();
    store.create(SYS, new("MENGER", Kind::Scope, None)).await.unwrap();
    store.create(SYS, new("THEME", Kind::Scope, Some("MENGER-1"))).await.unwrap();
    store.create(SYS, new("THEME", Kind::Build, None)).await.unwrap();
    let before = store.page(SYS, PageQuery::default()).unwrap().tickets;
    drop(store);
    rusqlite::Connection::open(&path)
        .unwrap()
        .execute_batch("DROP TABLE shortcodes; DROP TABLE pins; DROP TABLE orgs; UPDATE derived_state SET form = 10;")
        .unwrap();
    let store = Store::open(&path).unwrap();
    assert_eq!(store.page(SYS, PageQuery::default()).unwrap().tickets, before, "every ticket as before");
    assert_eq!(
        store.shortcodes().into_iter().map(|(_, a)| a).collect::<Vec<_>>(),
        ["hypermemetic:MENGER", "hypermemetic:MENGER/THEME"]
    );
    // Today's ids are pinned: THEME-2 stays THEME-2 whatever is filed later.
    store.create(SYS, new("THEME", Kind::Scope, Some("MENGER-1"))).await.unwrap_err();
    assert_eq!(store.get(SYS, "theme-2").await.unwrap().address, "hypermemetic:MENGER/THEME-2");
    drop(store);
    assert!(matches!(Store::verify_derived(&path).unwrap(), Verdict::Consistent { shortcodes: Some(2), .. }));
    let _ = Address::parse("MENGER/THEME-2").unwrap();
}
