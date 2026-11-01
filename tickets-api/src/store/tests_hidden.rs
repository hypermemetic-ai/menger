//! TICKETS-27: the `hidden` column — a ticket hidden from the nav's views
//! (`list`, `nav tree`, `nav counts`, `nav groups`) unless the filter names
//! `hidden` or the read is its parent's children, a normal ticket to `get`,
//! `find` and `search`; `update` sets it; the mark is the cell `hidden`
//! (a scan agrees); a database from before it gains the column, every row
//! `false`.

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

/// `H`: H-1 its root, H-2 and H-3 builds under it, H-4 a build under H-2.
async fn fixture(store: &Store) {
    store.create(SYS, new("H", "the root", Kind::Scope, None)).await.unwrap();
    store.create(SYS, new("H", "two zanzibar", Kind::Build, Some("H-1"))).await.unwrap();
    store.create(SYS, new("H", "three", Kind::Build, Some("H-1"))).await.unwrap();
    store.create(SYS, new("H", "four", Kind::Build, Some("H-2"))).await.unwrap();
}

async fn hide(store: &Store, id: &str, hidden: bool) -> Updated {
    store.update(SYS.updating(id), Update { hidden: Some(hidden), ..Update::default() }).await.unwrap()
}

fn listed(store: &Store, filter: &str) -> Vec<String> {
    let q = Filtered { lens: Lens::parse(filter).unwrap(), epic: None, status: None, parent: None, keep: None };
    store.filtered_page(SYS, &q, None, None, None).unwrap().tickets.iter().map(|t| t.id.to_string()).collect()
}

fn count(store: &Store, filter: &str) -> u64 {
    store.counts(SYS, &Lens::parse(filter).unwrap()).into_iter().map(|(_, n)| n).sum()
}

fn plain(store: &Store) -> Vec<String> {
    store.page(SYS, PageQuery::default()).unwrap().tickets.iter().map(|t| t.id.to_string()).collect()
}

fn children_of(store: &Store, parent: &str) -> Vec<String> {
    store.page(SYS, PageQuery { parent: Some(parent), ..PageQuery::default() }).unwrap().tickets.iter().map(|t| t.id.to_string()).collect()
}

fn tree(store: &Store, parent: Option<&str>, epic: Option<&str>, filter: &str, matches_only: bool) -> Vec<String> {
    let q = TreeQuery { parent: parent.map(String::from), epic: epic.map(String::from), lens: Lens::parse(filter).unwrap(), matches_only, keep: None };
    store.tree_page(SYS, &q, None, None).unwrap().nodes.iter().map(|n| n.summary.id.to_string()).collect()
}

fn assert_derived(store: &Store, after: &str) {
    let cache = store.tickets.read().unwrap();
    let scanned = Cache::new(cache.order.values().map(|t| (t.id.to_string(), t.clone())).collect(), cache.inputs.clone());
    assert_eq!(stored::compare(&cache.form(), &scanned.form()), Vec::<String>::new(), "after {after}");
}

#[tokio::test]
async fn a_hidden_ticket_is_in_no_nav_view_unless_the_filter_names_it() {
    let store = Store::new();
    fixture(&store).await;
    assert_eq!(plain(&store), ["H-1", "H-2", "H-3", "H-4"]);
    assert_eq!(count(&store, "in H"), 4);

    let u = hide(&store, "H-3", true).await;
    assert_eq!(u.changed, ["hidden"]);
    assert!(u.ticket.hidden);
    // list, with and without a filter; the counts; the groups.
    assert_eq!(plain(&store), ["H-1", "H-2", "H-4"]);
    assert_eq!(listed(&store, "in H"), ["H-1", "H-2", "H-4"]);
    assert_eq!(count(&store, "in H"), 3);
    assert_eq!(store.count(SYS), 3);
    let group = store.groups(SYS).into_iter().find(|g| g.shortcode == "H").unwrap();
    assert_eq!((group.total, group.docs), (3, 0), "H-3 is counted nowhere");
    // the nav tree at the shortcode's top and at a matches-only parent.
    assert_eq!(tree(&store, None, Some("H"), "in H", false), ["H-1"]);
    assert_eq!(tree(&store, Some("H-2"), None, "in H", false), ["H-4"]);
    assert_eq!(tree(&store, Some("H-1"), None, "in H", true), ["H-2"]);

    // asked for by name: the cell.
    assert_eq!(listed(&store, "hidden"), ["H-3"]);
    assert_eq!(count(&store, "hidden"), 1);
    assert_eq!(listed(&store, "not hidden and in H"), ["H-1", "H-2", "H-4"]);
    assert_eq!(listed(&store, "hidden or not hidden"), ["H-1", "H-2", "H-3", "H-4"]);
    assert_eq!(tree(&store, None, Some("H"), "hidden or in H", true), ["H-1"]);

    // still reached: get, find, search.
    assert!(store.get(SYS, "H-3").await.unwrap().hidden);
    assert!(store.find_page(SYS, "three", None, None).unwrap().tickets.iter().any(|t| t.id == "H-3"), "find");
    hide(&store, "H-2", true).await;
    let hits = store.search(SYS, "zanzibar", None, None, None).await.unwrap();
    assert!(hits.hits.iter().any(|h| h.summary.id == "H-2"), "search reaches a hidden ticket");
    assert!(hits.hits.iter().all(|h| h.summary.hidden == (h.summary.id == "H-2")));
    assert_derived(&store, "two hidden");

    // shown again: back in every view.
    let u = hide(&store, "H-3", false).await;
    assert_eq!(u.changed, ["hidden"]);
    hide(&store, "H-2", false).await;
    assert_eq!(plain(&store), ["H-1", "H-2", "H-3", "H-4"]);
    assert_eq!(count(&store, "in H"), 4);
    assert_eq!(listed(&store, "hidden"), Vec::<String>::new());
    let group = store.groups(SYS).into_iter().find(|g| g.shortcode == "H").unwrap();
    assert_eq!(group.total, 4);
    assert_derived(&store, "shown again");
}

#[tokio::test]
async fn a_hidden_ticket_is_reached_through_its_parents_children() {
    let store = Store::new();
    fixture(&store).await;
    hide(&store, "H-2", true).await;
    hide(&store, "H-4", true).await;
    assert_eq!(plain(&store), ["H-1", "H-3"]);
    // `list --parent`, and the tree's page of a parent's children.
    assert_eq!(children_of(&store, "H-1"), ["H-2", "H-3"]);
    assert_eq!(children_of(&store, "H-2"), ["H-4"]);
    assert_eq!(tree(&store, Some("H-1"), None, "in H", false), ["H-2", "H-3"]);
    assert_eq!(tree(&store, Some("H-2"), None, "in H", false), ["H-4"]);
    // a filtered list of a parent's children, too — but not a list that is no parent's.
    let q = Filtered { lens: Lens::parse("in H").unwrap(), epic: None, status: None, parent: Some("H-1".into()), keep: None };
    let kids: Vec<String> = store.filtered_page(SYS, &q, None, None, None).unwrap().tickets.iter().map(|t| t.id.to_string()).collect();
    assert_eq!(kids, ["H-2", "H-3"]);
    assert_eq!(listed(&store, "in H"), ["H-1", "H-3"]);
}

#[tokio::test]
async fn update_hidden_is_idempotent_and_broadcast_once() {
    let store = Store::new();
    fixture(&store).await;
    let mut rx = store.subscribe();
    let u = hide(&store, "H-3", true).await;
    assert_eq!(u.changed, ["hidden"]);
    let mut heard = Vec::new();
    while let Ok(c) = rx.try_recv() {
        heard.push(c);
    }
    assert!(heard.iter().any(|c| matches!(c, Change::HiddenChanged { ticket } if ticket.id == "H-3" && ticket.hidden)), "{heard:?}");
    let u = hide(&store, "H-3", true).await;
    assert!(u.changed.is_empty(), "already hidden: nothing written");
    assert!(rx.try_recv().is_err(), "and nothing sent");
    let revisions = store.get(SYS, "H-3").await.unwrap();
    assert!(revisions.hidden);
}

#[tokio::test]
async fn a_database_from_before_the_column_gains_it_with_every_row_false() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("before-27.db");
    let store = Store::open(&path).unwrap();
    fixture(&store).await;
    let groups = store.groups(SYS);
    drop(store);
    // Back to a file as form 14 left it: no `hidden` on the tickets or the cells.
    let conn = rusqlite::Connection::open(&path).unwrap();
    conn.execute_batch("ALTER TABLE cells DROP COLUMN hidden; ALTER TABLE tickets DROP COLUMN hidden; UPDATE derived_state SET form = 14;").unwrap();
    drop(conn);
    assert!(matches!(Store::verify_derived(&path).unwrap(), Verdict::NotStored(why) if why.contains("form 14; this build reads 17")));

    let store = Store::open(&path).unwrap();
    assert_eq!(store.groups(SYS), groups, "answers as before, nothing built again");
    for id in ["H-1", "H-2", "H-3", "H-4"] {
        assert!(!store.get(SYS, id).await.unwrap().hidden, "{id}");
    }
    assert_eq!(plain(&store), ["H-1", "H-2", "H-3", "H-4"]);
    hide(&store, "H-3", true).await;
    drop(store);

    let conn = rusqlite::Connection::open(&path).unwrap();
    let (total, visible): (i64, i64) = conn.query_row("SELECT count(*), sum(hidden = 0) FROM tickets", [], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
    assert_eq!((total, visible), (4, 3), "a NOT NULL column; only the one hidden since is true");
    let notnull: i64 = conn.query_row("SELECT \"notnull\" FROM pragma_table_info('tickets') WHERE name = 'hidden'", [], |r| r.get(0)).unwrap();
    assert_eq!(notnull, 1);
    assert_eq!(conn.query_row("SELECT form FROM derived_state", [], |r| r.get::<_, i64>(0)).unwrap(), 17);
    drop(conn);
    assert!(matches!(Store::verify_derived(&path).unwrap(), Verdict::Consistent { .. }));

    // and the hidden mark is durable across an open.
    let store = Store::open(&path).unwrap();
    assert_eq!(plain(&store), ["H-1", "H-2", "H-4"]);
    assert_derived(&store, "reopened");
}

#[test]
fn the_filter_language_reads_hidden_as_a_cell() {
    let f = crate::filter::Filter::parse("hidden and not kind doc").unwrap();
    assert!(f.names_hidden());
    assert_eq!(f.format(), "hidden and not kind doc");
    assert!(!crate::filter::Filter::parse("in H").unwrap().names_hidden());
}
