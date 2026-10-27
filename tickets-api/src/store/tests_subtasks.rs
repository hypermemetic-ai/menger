//! TICKETS-26: a subtask's id, `<parent>.N` — a test per row: accepted for a
//! ticket under its own parent, refused (naming the rule) anywhere else; read
//! everywhere an id is (get, the resolver, blockers, parents, the filter's
//! `under`, the address); the old id pinned by the rename; nested; and the file
//! holding what the cache holds.

use super::tests_update::assert_kept;
use super::*;

fn new(title: &str, kind: Kind, parent: Option<&str>) -> NewTicket {
    NewTicket {
        path: None,
        epic: "M".into(),
        title: title.into(),
        kind,
        severity: None,
        blocked_by: Vec::new(),
        parent: parent.map(Into::into),
        body: format!("the body of {title}"),
    }
}

/// `M`: M-1 its root; M-2 an area under it; M-3 and M-4 builds under M-2;
/// M-5 a build under the root; M-6 a build under M-3 that waits on M-4.
async fn store() -> Store {
    let store = Store::new();
    fill(&store).await;
    store
}

async fn fill(store: &Store) {
    store.create(SYS, new("root", Kind::Scope, None)).await.unwrap();
    store.create(SYS, new("area", Kind::Execution, Some("M-1"))).await.unwrap();
    store.create(SYS, new("three", Kind::Build, Some("M-2"))).await.unwrap();
    store.create(SYS, new("four", Kind::Build, Some("M-2"))).await.unwrap();
    store.create(SYS, new("five", Kind::Build, Some("M-1"))).await.unwrap();
    store.create(SYS, NewTicket { blocked_by: vec!["M-4".into()], ..new("six", Kind::Build, Some("M-3")) }).await.unwrap();
}

fn rename(id: &str, to: &str) -> Rename {
    Rename { id: id.into(), to: to.into(), ..Rename::default() }
}

fn under(store: &Store, id: &str) -> u64 {
    store.counts_text(&Lens::parse(&format!("under {id}")).unwrap()).into_iter().map(|(_, n)| n).sum()
}

#[tokio::test]
async fn a_ticket_under_its_parent_is_renamed_into_dot_n_and_both_ids_name_it() {
    let store = store().await;
    let r = store.rename(SYS, rename("M-3", "M-2.1")).await.unwrap();
    assert_eq!(r.moves.iter().map(|m| (m.from.as_str(), m.to.as_str())).collect::<Vec<_>>(), [("M-3", "M-2.1")]);
    let t = store.get(SYS, "M-2.1").await.unwrap();
    assert_eq!((t.id.as_str(), t.title.as_str(), t.parent.as_deref(), t.seq), ("M-2.1", "three", Some("M-2"), 3));
    assert!(t.address.ends_with("M-2.1"), "{}", t.address);
    assert_eq!(t.moved_from, ["M-3"]);
    // Every way to write the id.
    for text in ["m-2.1", "M-2.1", "hypermemetic:M-2.1", "M-3", "m-3", &t.address] {
        assert_eq!(store.get(SYS, text).await.unwrap().id, "M-2.1", "{text}");
    }
    assert_eq!(store.resolve("M-2.1").unwrap(), "M-2.1");
    assert!(store.get(SYS, "M-2.2").await.is_err());
    // Its child and the ticket that waits on its sibling keep their places.
    assert_eq!(store.get(SYS, "M-6").await.unwrap().parent.as_deref(), Some("M-2.1"));
    // The filter: `under` reads either id, and the parent's subtree holds it.
    assert_eq!((under(&store, "M-2.1"), under(&store, "M-3"), under(&store, "M-2")), (2, 2, 4));
    // A new ticket names it as its parent and as a blocker, by either id.
    let child = store.create(SYS, new("seven", Kind::Build, Some("M-2.1"))).await.unwrap();
    assert_eq!(child.parent.as_deref(), Some("M-2.1"));
    let waits = store.create(SYS, NewTicket { blocked_by: vec!["M-3".into()], ..new("eight", Kind::Build, Some("M-1")) }).await.unwrap();
    assert_eq!(waits.blocked_by, ["M-2.1"]);
    store.update(SYS.updating("M-4"), Update { block: vec!["M-2.1".parse().unwrap()], ..Update::default() }).await.unwrap();
    assert_eq!(store.get(SYS, "M-4").await.unwrap().blocked_by, ["M-2.1"]);
    // It is a normal ticket otherwise.
    store.set_status(SYS.permit("M-2.1"), Status::Ready).await.unwrap();
    assert_eq!(store.get(SYS, "M-3").await.unwrap().status, Status::Ready);
    // No number is given out twice: the shortcode's next is past the renamed one.
    assert_eq!(store.create(SYS, new("after", Kind::Build, Some("M-1"))).await.unwrap().id, "M-9");
    assert_kept(&store, "the subtask");
}

#[tokio::test]
async fn it_is_refused_anywhere_but_under_the_parent_naming_the_rule() {
    let store = store().await;
    for (id, to, why) in [
        ("M-5", "M-2.1", "is not M-1.N"),        // its parent is M-1
        ("M-3", "M-3.1", "is not M-2.N"),        // the stem is itself
        ("M-3", "M-1.1", "is not M-2.N"),        // another ticket's stem
        ("M-3", "M-2", "is not M-2.N"),          // no number
        ("M-3", "M-2.0", "is not M-2.N"),        // from 1
        ("M-3", "M-2.01", "is not M-2.N"),       // no leading zero
        ("M-3", "M-2.x", "is not M-2.N"),
        ("M-3", "NOPE-1.1", "is not M-2.N"),     // no such parent
        ("M-1", "M-0.1", "has no parent"),       // a root
    ] {
        let e = store.rename(SYS, rename(id, to)).await.unwrap_err().to_string();
        assert!(e.contains(why) && e.contains("a subtask id is `<parent>.N`") && e.contains("nothing written"), "{id} -> {to}: {e}");
    }
    // Nothing moved.
    assert_eq!(store.get(SYS, "M-3").await.unwrap().id, "M-3");
    // A taken name.
    store.rename(SYS, rename("M-3", "M-2.1")).await.unwrap();
    let e = store.rename(SYS, rename("M-4", "M-2.1")).await.unwrap_err().to_string();
    assert!(e.contains("M-2.1 is taken"), "{e}");
    let e = store.rename(SYS, rename("M-2.1", "M-2.1")).await.unwrap_err().to_string();
    assert!(e.contains("already"), "{e}");
    // A dry run answers and writes nothing.
    let r = store.rename(SYS, Rename { dry_run: true, ..rename("M-4", "M-2.2") }).await.unwrap();
    assert_eq!(r.moves[0].to, "M-2.2");
    assert_eq!(store.get(SYS, "M-4").await.unwrap().id, "M-4");
    assert_kept(&store, "the refusals");
}

#[tokio::test]
async fn it_nests_and_a_parent_can_be_named_by_its_old_id() {
    let store = store().await;
    store.rename(SYS, rename("M-3", "M-2.1")).await.unwrap();
    // M-6 hangs under M-2.1: M-2.1.1, with the stem by the old id too.
    store.rename(SYS, rename("M-6", "M-3.1")).await.unwrap();
    let t = store.get(SYS, "M-6").await.unwrap();
    assert_eq!((t.id.as_str(), t.parent.as_deref()), ("M-2.1.1", Some("M-2.1")));
    assert!(t.address.ends_with("M-2.1.1"), "{}", t.address);
    assert_eq!(t.blocked_by, ["M-4"]);
    assert_eq!(store.get(SYS, "hypermemetic:M-2.1.1").await.unwrap().id, "M-2.1.1");
    assert_eq!(under(&store, "M-2.1"), 2);
    assert_kept(&store, "the nest");
}

#[tokio::test]
async fn the_file_holds_what_the_cache_holds_and_the_old_id_stays_pinned() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("subtasks.db");
    let store = Store::open(&path).unwrap();
    fill(&store).await;
    store.rename(SYS, rename("M-3", "M-2.1")).await.unwrap();
    store.rename(SYS, rename("M-6", "M-2.1.1")).await.unwrap();
    let rows: Vec<Row> = store.tickets.read().unwrap().order.values().cloned().collect();
    drop(store);
    match Store::verify_derived(&path).unwrap() {
        Verdict::Consistent { .. } => {}
        other => panic!("{other:?}"),
    }
    let again = Store::open(&path).unwrap();
    let reread: Vec<Row> = again.tickets.read().unwrap().order.values().cloned().collect();
    assert_eq!(reread, rows);
    for (text, id) in [("M-3", "M-2.1"), ("M-2.1", "M-2.1"), ("M-6", "M-2.1.1"), ("hypermemetic:M-2.1.1", "M-2.1.1")] {
        assert_eq!(again.get(SYS, text).await.unwrap().id, id, "{text}");
    }
    assert_eq!(again.get(SYS, "M-2.1.1").await.unwrap().moved_from, ["M-6"]);
}
