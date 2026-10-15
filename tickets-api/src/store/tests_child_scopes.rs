//! ORG-5: a scope under a scope roots its own shortcode — a test per row of
//! the ticket's table: `create --kind scope --parent X` with X's shortcode
//! refused, naming why; with a new `--epic`, NEW-1 born hosted under X (and
//! NEW unique in X's namespace); `set_kind` to a scope below a root refused,
//! naming `reshortcode --new-scope`; `notes upgrade`, `update --kind` and
//! `update_many --kind` held to the same rule; a shortcode's root a scope or
//! a doc as before; and `reshortcode` moving a scope only to root a new
//! shortcode — the way the scopes from before ORG-5 come out.

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

/// `A`: A-1 its root, A-2 a build under it, A-3 a doc under it.
async fn store() -> Store {
    let store = Store::new();
    store.create(new("A", "the root", Kind::Scope, None)).await.unwrap();
    store.create(new("A", "two", Kind::Build, None)).await.unwrap();
    store.create(new("A", "a doc", Kind::Doc, None)).await.unwrap();
    store
}

#[tokio::test]
async fn a_scope_under_a_ticket_of_its_shortcode_is_refused_naming_why() {
    let store = store().await;
    let e = store.create(new("A", "an area", Kind::Scope, Some("A-1"))).await.unwrap_err();
    assert_eq!(e.to_string(), "a scope under A-1 roots its own shortcode; pass a new --epic (nothing was created)");
    let e = store.create(new("A", "an area", Kind::Scope, Some("A-2"))).await.unwrap_err();
    assert!(e.to_string().starts_with("a scope under A-2 roots its own shortcode"), "{e}");
    let e = store.create(new("A", "an area", Kind::Scope, None)).await.unwrap_err();
    assert!(e.to_string().contains("A is rooted by A-1 — pass a new --epic"), "{e}");
    assert_eq!(store.count(SYS), 3, "nothing was created");
    // A doc or any work under it is still filed in A.
    assert_eq!(store.create(new("A", "a finding", Kind::Doc, Some("A-2"))).await.unwrap().id, "A-4");
    assert_eq!(store.create(new("A", "work", Kind::Execution, Some("A-1"))).await.unwrap().id, "A-5");
}

#[tokio::test]
async fn a_scope_with_a_new_epic_is_born_new_1_hosted_where_it_hangs() {
    let store = store().await;
    let t = store.create(new("AREA", "an area", Kind::Scope, Some("A-2"))).await.unwrap();
    assert_eq!((t.id.as_str(), t.parent.as_deref(), t.address.as_str()), ("AREA-1", Some("A-2"), "hypermemetic:A/AREA-1"));
    // NEW is unique in A's namespace (ORG-3): a second AREA under A refused.
    let e = store.create(new("AREA", "again", Kind::Scope, Some("A-1"))).await.unwrap_err();
    assert!(e.to_string().contains("hypermemetic:A/AREA is already there"), "{e}");
    // Under AREA's own tickets a scope is again a new shortcode.
    let t = store.create(new("DEEP", "deeper", Kind::Scope, Some("AREA-1"))).await.unwrap();
    assert_eq!(t.address, "hypermemetic:A/AREA/DEEP-1");
}

#[tokio::test]
async fn set_kind_to_a_scope_below_a_root_is_refused_naming_reshortcode() {
    let store = store().await;
    let e = store.set_kind("A-2", Kind::Scope, None).await.unwrap_err();
    assert!(matches!(&e, StoreError::ScopeNotRoot { id, root, .. } if id == "A-2" && root == "A-1"), "{e:?}");
    assert!(e.to_string().contains("tickets reshortcode --ids A-2 --to NEW --new-scope TITLE --parent A-1"), "{e}");
    assert!(matches!(store.set_kind("A-3", Kind::Scope, None).await, Err(StoreError::ScopeNotRoot { .. })), "a doc below the root too");
    assert_eq!(store.get(SYS, "A-2").await.unwrap().kind, Kind::Build, "nothing changed");
    // A root goes between scope and doc as before (MNG-12, SDLC-11)…
    store.set_kind("A-1", Kind::Doc, None).await.unwrap();
    store.set_kind("A-1", Kind::Scope, None).await.unwrap();
    // …and is still refused any other kind.
    assert!(matches!(store.set_kind("A-1", Kind::Build, None).await, Err(StoreError::RootKind { .. })));
    // A ticket that would root its shortcode (none lower roots it) may.
    let store = Store::new();
    store.create(new("B", "a doc root", Kind::Doc, None)).await.unwrap();
    store.set_kind("B-1", Kind::Scope, None).await.unwrap();
}

#[tokio::test]
async fn notes_upgrade_and_update_follow_the_same_rule() {
    let store = store().await;
    let n = store.add_note_as("an area of its own".into(), None, None).await.unwrap();
    let e = store.upgrade_note(n.id.get(), new("A", "an area", Kind::Scope, Some("A-1"))).await.unwrap_err();
    assert!(e.to_string().starts_with("a scope under A-1 roots its own shortcode"), "{e}");
    let (t, note) = store.upgrade_note(n.id.get(), new("LOOK", "an area", Kind::Scope, Some("A-1"))).await.unwrap();
    assert_eq!((t.id.as_str(), note.ticket.as_deref()), ("LOOK-1", Some("LOOK-1")));
    let e = store.update("A-2", Update { kind: Some(Kind::Scope), ..Update::default() }).await.unwrap_err();
    assert!(e.to_string().contains("a scope roots its own shortcode (ORG-5)"), "{e}");
    let e = store.update_many(Selection::Ids(vec!["A-2".into(), "A-3".into()]), Update { kind: Some(Kind::Scope), ..Update::default() }).await.unwrap_err();
    assert!(e.to_string().contains("A-2") && e.to_string().contains("A-3"), "{e}");
    assert_eq!(store.get(SYS, "A-2").await.unwrap().kind, Kind::Build, "nothing changed");
}

#[tokio::test]
async fn reshortcode_moves_a_scope_only_to_root_a_new_shortcode() {
    let store = store().await;
    store.create(new("B", "b root", Kind::Scope, None)).await.unwrap();
    store.create(new("A", "under two", Kind::Build, Some("A-2"))).await.unwrap(); // A-4
    // The shape a file from before ORG-5 holds: A-2 a scope inside A.
    rules::LEGACY_SCOPES.with(|l| l.set(true));
    store.set_kind("A-2", Kind::Scope, None).await.unwrap();
    rules::LEGACY_SCOPES.with(|l| l.set(false));
    // Into a shortcode there is, it would sit below B-1: refused.
    let e = store.reshortcode(Reshortcode { ids: vec!["A-2".into()], to: "B".into(), ..Reshortcode::default() }).await.unwrap_err();
    assert!(e.to_string().contains("a scope roots its own shortcode (ORG-5)"), "{e}");
    let e = store
        .reshortcode(Reshortcode { ids: vec!["A-2".into()], to: "NEW".into(), new_scope: Some("n".into()), parent: Some("A-1".into()), ..Reshortcode::default() })
        .await
        .unwrap_err();
    assert!(e.to_string().contains("a scope roots its own shortcode (ORG-5)"), "{e}");
    // Into a new code it roots it, its tree in A with it.
    let r = store.reshortcode(Reshortcode { ids: vec!["A-2".into()], to: "TWO".into(), with_children: true, ..Reshortcode::default() }).await.unwrap();
    let pairs: Vec<(&str, &str)> = r.moves.iter().map(|m| (m.from.as_str(), m.to.as_str())).collect();
    assert_eq!(pairs, [("A-2", "TWO-1"), ("A-4", "TWO-2")]);
    let t = store.get(SYS, "A-2").await.unwrap();
    assert_eq!((t.id.as_str(), t.kind, t.parent.as_deref(), t.address.as_str()), ("TWO-1", Kind::Scope, Some("A-1"), "hypermemetic:A/TWO-1"));
    // No scope is left below a root of another.
    let cache = store.tickets.read().unwrap();
    for t in cache.order.values().filter(|t| t.kind == Kind::Scope) {
        assert_eq!(cache.rollups.root(&t.shortcode), Some(t.id.as_str()), "{} roots its shortcode", t.id);
    }
}
