//! SHELL.176-3: `· order changed` — a filtered page's matches newest last
//! touch first (FILTER-30's touch), cut by `at` and by `after` alike, the
//! filter's view kept; and the language reading and writing it.

use super::history::Clock;
use super::*;

const DAY: i64 = 86_400;

fn build(title: &str) -> NewTicket {
    NewTicket {
        epic: "T".into(),
        title: title.into(),
        kind: Kind::Build,
        severity: None,
        blocked_by: Vec::new(),
        parent: None,
        path: None,
        body: String::new(),
    }
}

fn page(store: &Store, text: &str, after: Option<&str>, at: Option<u64>, limit: u32) -> Page {
    let lens = Lens::parse(text).unwrap_or_else(|e| panic!("{text}: {e}")).with_clock(Clock::default());
    let q = Filtered { lens, epic: None, status: None, parent: None, keep: None };
    store.filtered_page(SYS, &q, after, at, Some(limit)).unwrap()
}

fn ids(p: &Page) -> Vec<String> {
    p.tickets.iter().map(|t| t.id.to_string()).collect()
}

/// T-1 (a scope) .. T-5, T-n last written n - ½ days ago (T-1 the newest), reopened.
async fn spread(path: &std::path::Path) -> Store {
    let store = Store::open(path).unwrap();
    store.create(SYS, NewTicket { kind: Kind::Scope, ..build("t 1") }).await.unwrap();
    for i in 2..=5 {
        store.create(SYS, build(&format!("t {i}"))).await.unwrap();
    }
    drop(store);
    let now = chrono::Utc::now().timestamp();
    let conn = rusqlite::Connection::open(path).unwrap();
    for i in 1..=5i64 {
        let at = now - i * DAY + DAY / 2;
        conn.execute("UPDATE tickets SET created_unix = ?1 - 1000, updated_unix = ?1 WHERE seq = ?2", [at, i]).unwrap();
    }
    conn.execute("UPDATE transitions SET at_unix = ?1", [now - 30 * DAY]).unwrap();
    drop(conn);
    Store::open(path).unwrap()
}

#[tokio::test]
async fn the_most_recently_changed_come_first() {
    let dir = tempfile::tempdir().unwrap();
    let store = spread(&dir.path().join("t.db")).await;
    let all = page(&store, "in T · order changed", None, None, 100);
    assert_eq!(ids(&all), ["T-1", "T-2", "T-3", "T-4", "T-5"], "newest touch first");
    assert_eq!((all.first, all.remaining, all.next), (Some(0), 0, None));
    // The store's order is untouched without it.
    assert_eq!(ids(&page(&store, "in T", None, None, 100)), ["T-1", "T-2", "T-3", "T-4", "T-5"]);

    // A write moves a ticket to the top.
    store.amend(SYS.permit("T-4"), None, Some("touched now".into())).await.unwrap();
    assert_eq!(ids(&page(&store, "in T · order changed", None, None, 100)), ["T-4", "T-1", "T-2", "T-3", "T-5"]);
    assert_eq!(ids(&page(&store, "in T · order code", None, None, 100)), ["T-1", "T-2", "T-3", "T-4", "T-5"]);

    // By position: the page, its first and what remains.
    let mid = page(&store, "in T · order changed", None, Some(1), 2);
    assert_eq!(ids(&mid), ["T-1", "T-2"]);
    assert_eq!((mid.first, mid.remaining, mid.next.as_deref()), (Some(1), 2, Some("T-2")));
    // After a cursor: its own place in the touch order.
    let rest = page(&store, "in T · order changed", mid.next.as_deref(), None, 10);
    assert_eq!(ids(&rest), ["T-3", "T-5"]);
    assert_eq!((rest.first, rest.remaining, rest.next), (Some(3), 0, None));

    // The filter's view is kept: a window narrows, the order stays.
    let recent = page(&store, "changed within 2d · order changed", None, None, 100);
    assert_eq!(ids(&recent), ["T-4", "T-1", "T-2"]);
    // A cursor the filter does not admit still has its place.
    assert_eq!(ids(&page(&store, "changed within 2d · order changed", Some("T-3"), None, 100)), Vec::<String>::new());
    assert_eq!(ids(&page(&store, "not ticket T-1 · order changed", Some("T-1"), None, 100)), ["T-2", "T-3", "T-5"]);
}

#[test]
fn the_language_reads_and_writes_it() {
    use crate::filter::{Filter, Order};
    let f = Filter::parse("changed within 7d · order changed").unwrap();
    assert_eq!(f.display.order, Some(Order::Changed));
    assert_eq!(f.format(), "changed within 7d · order changed");
    assert_eq!(Filter::parse("· order changed").unwrap().format(), "· order changed");
    let refused = Filter::parse("ready · order title").unwrap_err().to_string();
    assert!(refused.contains("order code") && refused.contains("order changed"), "{refused}");
}
