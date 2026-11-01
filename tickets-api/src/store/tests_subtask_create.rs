//! TICKETS-28: `create_subtask` — the next free `<parent>.N`, hidden, in
//! `create`'s own write: numbered in order from 1, in the parent's
//! shortcode, out of the nav's views and in its parent's children, a number
//! never given out twice (a delete, a rename), refused without a parent,
//! under a permit of the parent's scope, nested, and the file holding what
//! the cache holds.

use super::tests_update::assert_kept;
use super::*;

fn new(title: &str, kind: Kind, parent: Option<&str>) -> NewTicket {
    NewTicket { path: None, epic: "M".into(), title: title.into(), kind, severity: None, blocked_by: Vec::new(), parent: parent.map(Into::into), body: format!("the body of {title}") }
}

/// `M`: M-1 its root, M-2 an area under it, M-3 a build under M-2.
async fn store() -> Store {
    let store = Store::new();
    store.create(SYS, new("root", Kind::Scope, None)).await.unwrap();
    store.create(SYS, new("area", Kind::Execution, Some("M-1"))).await.unwrap();
    store.create(SYS, new("three", Kind::Build, Some("M-2"))).await.unwrap();
    store
}

fn ids(page: Page) -> Vec<String> {
    page.tickets.iter().map(|t| t.id.to_string()).collect()
}

fn children(store: &Store, parent: &str) -> Vec<String> {
    ids(store.page(SYS, PageQuery { parent: Some(parent), ..PageQuery::default() }).unwrap())
}

fn assert_derived(store: &Store, after: &str) {
    let cache = store.tickets.read().unwrap();
    let scanned = Cache::new(cache.order.values().map(|t| (t.id.to_string(), t.clone())).collect(), cache.inputs.clone());
    assert_eq!(stored::compare(&cache.form(), &scanned.form()), Vec::<String>::new(), "after {after}");
}

#[tokio::test]
async fn subtasks_are_numbered_from_one_in_order_and_hidden() {
    let store = store().await;
    let mut made = Vec::new();
    for title in ["one", "two", "three"] {
        made.push(store.create_subtask(SYS, new(title, Kind::Build, Some("M-2"))).await.unwrap());
    }
    assert_eq!(made.iter().map(|t| t.id.to_string()).collect::<Vec<_>>(), ["M-2.1", "M-2.2", "M-2.3"]);
    for t in &made {
        assert!(t.hidden, "{}", t.id);
        assert_eq!((t.parent.as_deref(), t.status, t.kind), (Some("M-2"), Status::Draft, Kind::Build));
        assert!(t.address.ends_with(t.id.as_str()), "{}", t.address);
    }
    // Hidden: in no list or tree, in its parent's children; `get` reaches it.
    assert_eq!(ids(store.page(SYS, PageQuery::default()).unwrap()), ["M-1", "M-2", "M-3"]);
    let q = Filtered { lens: Lens::parse("in M").unwrap(), epic: None, status: None, parent: None, keep: None };
    assert_eq!(ids(store.filtered_page(SYS, &q, None, None, None).unwrap()), ["M-1", "M-2", "M-3"]);
    let tree = TreeQuery { parent: Some("M-2".into()), epic: None, lens: Lens::parse("in M").unwrap(), matches_only: true, keep: None };
    assert_eq!(store.tree_page(SYS, &tree, None, None).unwrap().nodes.iter().map(|n| n.summary.id.to_string()).collect::<Vec<_>>(), ["M-3"]);
    assert_eq!(children(&store, "M-2"), ["M-3", "M-2.1", "M-2.2", "M-2.3"]);
    assert_eq!(store.get(SYS, "m-2.2").await.unwrap().title, "two");
    // The shortcode's numbers are spent, as a rename leaves them.
    assert_eq!(store.create(SYS, new("after", Kind::Build, Some("M-1"))).await.unwrap().id, "M-7");
    assert_kept(&store, "the subtasks");
    assert_derived(&store, "the subtasks");
}

#[tokio::test]
async fn a_number_is_not_reused_after_a_delete_or_a_rename() {
    let store = store().await;
    for t in ["a", "b", "c"] {
        store.create_subtask(SYS, new(t, Kind::Build, Some("M-2"))).await.unwrap();
    }
    // Delete the highest: its number stays out.
    store.delete(SYS, Selection::Ids(vec!["M-2.3".into()]), Deletion::default(), None).await.unwrap();
    assert_eq!(store.create_subtask(SYS, new("d", Kind::Build, Some("M-2"))).await.unwrap().id, "M-2.4");
    // Rename a plain child into .5: the next is past it.
    store.rename(SYS, Rename { id: "M-3".into(), to: "M-2.5".into(), ..Rename::default() }).await.unwrap();
    assert_eq!(store.create_subtask(SYS, new("e", Kind::Build, Some("M-2"))).await.unwrap().id, "M-2.6");
    assert_kept(&store, "the numbers");
}

#[tokio::test]
async fn subtasks_nest_and_take_blockers_and_the_parent_by_any_name() {
    let store = store().await;
    let one = store.create_subtask(SYS, new("one", Kind::Build, Some("m-2"))).await.unwrap();
    assert_eq!(one.id, "M-2.1");
    let deep = store.create_subtask(SYS, NewTicket { blocked_by: vec!["M-3".into()], ..new("deep", Kind::Gap, Some("M-2.1")) }).await.unwrap();
    assert_eq!((deep.id.as_str(), deep.parent.as_deref(), deep.blocked_by.iter().map(|b| b.to_string()).collect::<Vec<_>>(), deep.hidden), ("M-2.1.1", Some("M-2.1"), vec!["M-3".to_string()], true));
    assert_eq!(children(&store, "M-2.1"), ["M-2.1.1"]);
    assert_eq!(store.get(SYS, "M-2.1").await.unwrap().child_count, 1);
    assert_derived(&store, "nested");
}

#[tokio::test]
async fn a_subtask_needs_a_parent_that_exists_and_work_to_be() {
    let store = store().await;
    let e = store.create_subtask(SYS, new("x", Kind::Build, None)).await.unwrap_err().to_string();
    assert!(e.contains("needs its parent"), "{e}");
    let e = store.create_subtask(SYS, new("x", Kind::Build, Some("M-99"))).await.unwrap_err().to_string();
    assert!(e.contains("M-99"), "{e}");
    let e = store.create_subtask(SYS, new("x", Kind::Scope, Some("M-2"))).await.unwrap_err().to_string();
    assert!(e.contains("roots a shortcode"), "{e}");
    // Nothing written, no number spent.
    assert_eq!(children(&store, "M-2"), ["M-3"]);
    assert_eq!(store.create_subtask(SYS, new("ok", Kind::Build, Some("M-2"))).await.unwrap().id, "M-2.1");
    assert_eq!(store.create(SYS, new("next", Kind::Build, Some("M-1"))).await.unwrap().id, "M-5");
}
